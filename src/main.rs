mod collect;
mod embed;
mod policy;
mod queries;
mod render;

use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use chrono::{NaiveDate, Utc};
use minijinja::Environment;
use nostr_sdk::prelude::{EventId, PublicKey};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
const ROOT: &str = "60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6";
static CONVERSATION_REFRESHING: AtomicBool = AtomicBool::new(false);
const MODES: [&str; 6] = [
    "new",
    "relevance",
    "meaning",
    "topics",
    "conversations",
    "discovery",
];
const FEED_MODES: [&str; 4] = ["new", "conversations", "discovery", "topics"];

struct StatusSnapshot {
    generated_at: i64,
    html: String,
    refresh_error: bool,
}

#[derive(Clone)]
struct App {
    path: PathBuf,
    root: String,
    templates: Environment<'static>,
    embedding: Option<Arc<dyn embed::SemanticModel>>,
    embedding_queries: Option<tokio::sync::mpsc::Sender<collect::EmbeddingQuery>>,
    embedding_error: Arc<Mutex<Option<String>>>,
    cached_minilm: Option<embed::Space>,
    default_embedding: String,
    collecting: bool,
    reader_slots: Arc<tokio::sync::Semaphore>,
    status_cache: Arc<RwLock<Option<StatusSnapshot>>>,
}

fn local_embedding(
    path: &std::path::Path,
    intra_threads: usize,
) -> Result<Option<Arc<embed::MiniLm>>, Error> {
    let Ok(backend) = std::env::var("MEATYBROTH_EMBED_BACKEND") else {
        return Ok(None);
    };
    match backend.as_str() {
        "minilm" => {
            if std::env::var("MEATYBROTH_EMBED_MODEL")? != embed::MINILM_MODEL
                || std::env::var("MEATYBROTH_EMBED_DIMENSIONS")?.parse::<usize>()?
                    != embed::MINILM_DIMENSIONS
                || std::env::var("MEATYBROTH_EMBED_NORMALIZE")? != "true"
            {
                return Err(
                    "MiniLM model, dimensions, or normalization do not match its vector space"
                        .into(),
                );
            }
            let cache = path.parent().unwrap().join("models/minilm");
            std::fs::create_dir_all(&cache)?;
            Ok(Some(Arc::new(embed::MiniLm::open(&cache, intra_threads)?)))
        }
        "bedrock" => {
            if std::env::var("MEATYBROTH_EMBED_MODEL")? != embed::TITAN_MODEL
                || std::env::var("MEATYBROTH_EMBED_DIMENSIONS")?.parse::<usize>()?
                    != embed::TITAN_DIMENSIONS
                || std::env::var("MEATYBROTH_EMBED_NORMALIZE")? != "true"
            {
                return Err(
                    "Titan model, dimensions, or normalization do not match its vector space"
                        .into(),
                );
            }
            Ok(None)
        }
        other => Err(format!("Unknown embedding backend: {other}").into()),
    }
}

struct Search {
    q: String,
    mode: String,
    page: i64,
    before: Option<i64>,
    reach: i64,
    order: String,
    expression: Option<String>,
    similar: Option<String>,
    topics: Vec<i64>,
    include_unsorted: bool,
    hide_flagged_spam: bool,
    embedding: String,
    clustering: String,
    error: Option<String>,
}
impl Search {
    fn parse(raw: &str, now: i64, default_embedding: &str) -> Self {
        let pairs: Vec<(String, String)> = url::form_urlencoded::parse(raw.as_bytes())
            .into_owned()
            .collect();
        let args: HashMap<String, String> = pairs.iter().cloned().collect();
        let arg = |key: &str| args.get(key).map(String::as_str).unwrap_or("");
        let number =
            |key: &str, default: i64| arg(key).parse::<i64>().map(|n| n.max(0)).unwrap_or(default);
        let q = arg("q").trim().to_owned();
        let mut mode = arg("mode");
        if mode == "recent" {
            mode = "new";
        }
        if !q.is_empty() && (arg("go") == "1" || matches!(arg("search"), "relevance" | "meaning")) {
            mode = match arg("search") {
                "meaning" => "meaning",
                _ => "relevance",
            };
        } else if !MODES.contains(&mode) && mode != "similar" {
            mode = if q.is_empty() { "topics" } else { "relevance" };
        }
        if q.is_empty() && mode == "relevance" {
            mode = "new";
        }
        let timestamp = match number("before", 0) {
            0 => NaiveDate::parse_from_str(arg("date"), "%Y-%m-%d")
                .ok()
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
                .unwrap_or(0),
            n => n,
        };
        let before = if timestamp > 0 && timestamp < now {
            Some(timestamp)
        } else {
            None
        };
        let parsed = if q.is_empty() || ["meaning", "similar", "topics"].contains(&mode) {
            Ok(None)
        } else {
            parse_fts(&q).map(Some)
        };
        let (expression, error) = match parsed {
            Ok(e) => (e, None),
            Err(e) => (None, Some(e)),
        };
        let clustering = match arg("clustering") {
            "dbscan" => "dbscan",
            _ => "kmeans",
        };
        let embedding = match arg("embedding") {
            "titan" => "titan",
            "minilm" => "minilm",
            _ => default_embedding,
        };
        let mut topics = pairs
            .iter()
            .filter(|(key, _)| key == "topic" || key == "topics")
            .filter_map(|(_, value)| value.parse().ok())
            .collect::<Vec<_>>();
        topics.sort_unstable();
        topics.dedup();
        let include_unsorted = arg("unsorted") == "true" || topics.contains(&-1);
        topics.retain(|topic| *topic != -1);
        Self {
            q,
            mode: mode.into(),
            page: number("page", 0),
            before,
            reach: match number("reach", 2) {
                n @ 1..=3 => n,
                _ => 2,
            },
            order: match (mode, arg("order")) {
                ("discovery", "connections") => "connections",
                ("discovery", _) => "recent",
                ("relevance", "newest") => "newest",
                ("relevance", _) => "relevance",
                _ => "recent",
            }
            .into(),
            expression,
            similar: args.get("similar").cloned(),
            topics,
            include_unsorted,
            hide_flagged_spam: arg("hide_spam") != "false"
                && arg("show_spam") != "true"
                && arg("show_flagged_spam") != "true",
            embedding: embedding.into(),
            clustering: clustering.into(),
            error,
        }
    }
    fn query_string(&self) -> String {
        self.query_string_with_before(true, true)
    }
    fn latest_query_string(&self) -> String {
        self.query_string_with_before(false, true)
    }
    fn hide_flagged_query_string(&self) -> String {
        let mut query = self.query_string_with_before(true, false);
        if self.page > 0 {
            query.push_str(&format!("page={}&", self.page));
        }
        query.push_str("hide_spam=true&");
        query
    }
    fn show_flagged_query_string(&self) -> String {
        let mut query = self.query_string_with_before(true, false);
        if self.page > 0 {
            query.push_str(&format!("page={}&", self.page));
        }
        query.push_str("show_spam=true&");
        query
    }
    fn query_string_with_before(&self, include_before: bool, include_hide_spam: bool) -> String {
        let mut qs = url::form_urlencoded::Serializer::new(String::new());
        if !self.q.is_empty() {
            qs.append_pair("q", &self.q);
        }
        qs.append_pair("mode", &self.mode);
        if include_before {
            if let Some(before) = self.before {
                qs.append_pair("before", &before.to_string());
            }
        }
        if self.mode == "discovery" {
            qs.append_pair("reach", &self.reach.to_string())
                .append_pair("order", &self.order);
        } else if self.mode == "relevance" {
            qs.append_pair("order", &self.order);
        }
        if let Some(similar) = &self.similar {
            qs.append_pair("similar", similar);
        }
        if self.mode == "topics" {
            qs.append_pair("clustering", &self.clustering);
        }
        for topic in &self.topics {
            qs.append_pair("topics", &topic.to_string());
        }
        if self.include_unsorted {
            qs.append_pair("unsorted", "true");
        }
        if include_hide_spam {
            if self.hide_flagged_spam {
                qs.append_pair("hide_spam", "true");
            } else {
                qs.append_pair("show_spam", "true");
            }
        }
        if self.embedding != "minilm" {
            qs.append_pair("embedding", &self.embedding);
        }
        format!("{}&", qs.finish())
    }
}
fn parse_fts(q: &str) -> Result<String, String> {
    if !q.matches('"').count().is_multiple_of(2) {
        return Err("Unmatched quotation mark in search query.".into());
    }
    let words = regex::Regex::new(r"[\p{L}\p{N}_]+").unwrap();
    let parts: Vec<_> = q.split('"').collect();
    let mut clauses: Vec<String> = parts
        .iter()
        .skip(1)
        .step_by(2)
        .map(|p| {
            words
                .find_iter(p)
                .map(|m| m.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|s| !s.is_empty())
        .map(|s| format!("\"{s}\""))
        .collect();
    for part in parts.iter().step_by(2) {
        clauses.extend(words.find_iter(part).map(|m| format!("\"{}\"", m.as_str())));
    }
    if clauses.is_empty() {
        return Err("Enter a word or phrase to search.".into());
    }
    Ok(clauses.join(" AND "))
}

fn templates() -> Result<Environment<'static>, Error> {
    let mut env = Environment::new();
    for (name, source) in [
        ("base.html", include_str!("../templates/base.html")),
        ("feed.html", include_str!("../templates/feed.html")),
        ("_post.html", include_str!("../templates/_post.html")),
        ("context.html", include_str!("../templates/context.html")),
        ("about.html", include_str!("../templates/about.html")),
        ("tos.html", include_str!("../templates/tos.html")),
        ("status.html", include_str!("../templates/status.html")),
    ] {
        env.add_template(name, source)?;
    }
    Ok(env)
}
fn page(app: &App, name: &str, data: Value) -> Result<String, Error> {
    let mut shared = json!({"modes":MODES,"feed_modes":FEED_MODES,"window_days":30,"mode":"topics","collecting":app.collecting,
    "meaning_available":app.embedding.is_some() || app.embedding_queries.is_some(),
    "minilm_available":app.embedding.is_some() || app.cached_minilm.is_some(),"embedding":app.default_embedding,
    "mode_labels":{"new":"Latest","relevance":"Words","meaning":"Meaning","topics":"Topics","conversations":"With replies","discovery":"Network","similar":"Similar posts"},
    "mode_explanations":{
        "new":"Recent posts, newest first.",
        "relevance":"Posts containing these words. Sort by relevance or newest.",
        "meaning":"Posts related to your query. It can find posts without the same words.",
        "similar":"Posts related to this post.",
        "topics":"Groups of related posts. Choose one or more topics, or browse all topic posts.",
        "conversations":"Threads with recent replies. A search limits this view to matching threads.",
        "discovery":"Posts within the selected number of follow hops. Stored follow lists are incomplete."
    }});
    shared
        .as_object_mut()
        .unwrap()
        .extend(data.as_object().unwrap().clone());
    Ok(app.templates.get_template(name)?.render(shared)?)
}

fn canonical_event_id(bytes: &[u8]) -> String {
    format!(
        "nostr:{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn selected_space(app: &App, search: &Search) -> Result<embed::Space, Error> {
    if search.embedding == "titan" {
        embed::space_by_backend(&app.path, "bedrock")?
            .ok_or_else(|| "Titan search is not ready yet.".into())
    } else {
        app.embedding
            .as_ref()
            .map(|model| model.vector_space().clone())
            .or_else(|| app.cached_minilm.clone())
            .ok_or_else(|| "MiniLM search is not available.".into())
    }
}

fn semantic_feed(
    app: &App,
    db: &Connection,
    search: &Search,
    space: &embed::Space,
    now: i64,
) -> Result<Vec<(queries::Post, f32)>, Error> {
    let started = std::time::Instant::now();
    let (query, exclude) = if search.mode == "meaning" {
        if search.q.is_empty() {
            return Err("Enter text for Meaning search.".into());
        }
        let query = if search.embedding == "titan" {
            embed::cached_query(&app.path, space, &search.q)?
                .ok_or("This Meaning search is not available yet.")?
        } else {
            app.embedding
                .as_ref()
                .ok_or("MiniLM search is not available.")?
                .embed_text(&search.q)?
                .vector
        };
        (query, None)
    } else {
        let id = search.similar.as_deref().ok_or("Choose a post first.")?;
        let event_id = EventId::from_hex(id.strip_prefix("nostr:").unwrap_or(id))
            .map_err(|_| "Choose a post first.")?;
        if queries::get(db, &canonical_event_id(event_id.as_bytes()), now)?.is_none() {
            return Err("This post is not available in this view.".into());
        }
        let vector = embed::event_vector(&app.path, space, event_id.as_bytes())?
            .ok_or("Similar posts are not available for this post.")?;
        (vector, Some(event_id))
    };
    let query_at = started.elapsed();
    let offset = usize::try_from(search.page)? * queries::PAGE_SIZE;
    let ranked = embed::nearest(
        &app.path,
        space,
        &query,
        exclude
            .as_ref()
            .map(|event_id| event_id.as_bytes().as_slice()),
        now,
        offset + queries::PAGE_SIZE + 1,
    )?;
    let nearest_at = started.elapsed();
    let ranked_ids = ranked
        .iter()
        .skip(offset)
        .map(|(event_id, _)| canonical_event_id(event_id))
        .collect::<Vec<_>>();
    let mut eligible = queries::eligible_map_for(db, now, Some(&ranked_ids))?;
    let mut rows = Vec::new();
    for (event_id, score) in ranked.into_iter().skip(offset) {
        if let Some(post) = eligible.remove(&canonical_event_id(&event_id)) {
            rows.push((post, score));
        }
    }
    let cards_at = started.elapsed();
    if cards_at.as_secs() >= 1 {
        eprintln!(
            "Slow semantic backend={} query_ms={} nearest_ms={} lookup_ms={}",
            search.embedding,
            query_at.as_millis(),
            (nearest_at - query_at).as_millis(),
            (cards_at - nearest_at).as_millis(),
        );
    }
    Ok(rows)
}

fn topic_feed(
    app: &App,
    db: &Connection,
    search: &Search,
    space: &embed::Space,
    now: i64,
) -> Result<Vec<(queries::Post, f32)>, Error> {
    let offset = usize::try_from(search.page)? * queries::PAGE_SIZE;
    let event_ids = embed::topic_events(
        &app.path,
        space,
        embed::TopicPage {
            ids: &search.topics,
            include_unsorted: search.include_unsorted,
            algorithm: &search.clustering,
            now,
            limit: queries::PAGE_SIZE + 1,
            offset,
        },
    )?;
    let canonical_ids = event_ids
        .iter()
        .map(|id| canonical_event_id(id))
        .collect::<Vec<_>>();
    let mut eligible = queries::eligible_map_for(db, now, Some(&canonical_ids))?;
    let mut rows = Vec::new();
    for event_id in event_ids {
        if let Some(post) = eligible.remove(&canonical_event_id(&event_id)) {
            rows.push((post, f32::NAN));
        }
    }
    Ok(rows)
}

fn cached_conversations(
    app: &App,
    db: &Connection,
    now: i64,
) -> Result<Option<Vec<queries::Post>>, Error> {
    let Some(cache) = queries::conversation_cache(db)? else {
        return Ok(None);
    };
    if now - cache.generated_at > 300 && !CONVERSATION_REFRESHING.swap(true, Ordering::AcqRel) {
        let (path, root, default_embedding) = (
            app.path.clone(),
            app.root.clone(),
            app.default_embedding.clone(),
        );
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let result = (|| {
                let db = open_reader(&path)?;
                let search = Search::parse("mode=conversations", now, &default_embedding);
                let rows = queries::feed(&db, &search, &root, now)?;
                let rank_ms = started.elapsed().as_millis();
                db.execute_batch("COMMIT")?;
                queries::store_conversation_cache(&path, now, &rows, None)?;
                Ok::<_, Error>((rows.len(), rank_ms))
            })();
            match result {
                Ok((rows, rank_ms)) => eprintln!(
                    "Slow reader mode=conversations rows={rows} rank_ms={rank_ms} total_ms={}",
                    started.elapsed().as_millis()
                ),
                Err(error) => {
                    if let Err(store_error) =
                        queries::store_conversation_cache(&path, now, &[], Some(&error.to_string()))
                    {
                        eprintln!("Conversation cache refresh failed: {error}; could not record it: {store_error}");
                    }
                }
            }
            CONVERSATION_REFRESHING.store(false, Ordering::Release);
        });
    }
    let ids = cache
        .rows
        .iter()
        .map(|row| row.canonical_id.clone())
        .collect::<Vec<_>>();
    let mut current = queries::eligible_map_for(db, now, Some(&ids))?;
    Ok(Some(
        cache
            .rows
            .into_iter()
            .filter_map(|ranked| {
                current.remove(&ranked.canonical_id).map(|mut post| {
                    post.n_reply_authors = ranked.n_reply_authors;
                    post.n_replies = ranked.n_replies;
                    post.latest_activity = ranked.latest_activity;
                    post.root_present = ranked.root_present;
                    post
                })
            })
            .collect(),
    ))
}

fn feed(app: &App, db: &Connection, raw: &str, now: i64) -> Result<(StatusCode, String), Error> {
    let started = std::time::Instant::now();
    let mut search = Search::parse(raw, now, &app.default_embedding);
    let similar_context = search.similar.as_deref().and_then(|id| {
        id.split_once(':')
            .map(|(source, source_id)| format!("/context/{source}/{source_id}"))
    });
    let semantic = ["meaning", "similar"].contains(&search.mode.as_str());
    let topic_mode = search.mode == "topics";
    let selected_space = match selected_space(app, &search) {
        Ok(space) => Some(space),
        Err(error) if semantic || topic_mode => {
            search.error = Some(error.to_string());
            None
        }
        Err(_) => None,
    };
    let configured_topic_settings = topic_mode
        .then(embed::TopicSettings::from_env)
        .transpose()?;
    let topic_settings = if topic_mode {
        selected_space
            .as_ref()
            .map(|space| embed::stored_topic_settings(&app.path, space))
            .transpose()?
            .flatten()
            .or(configured_topic_settings)
    } else {
        None
    };
    let topic_epsilon = topic_settings
        .as_ref()
        .map(|settings| format!("{:.2}", settings.dbscan_epsilon_cosine));
    let mut topics = if topic_mode {
        selected_space
            .as_ref()
            .map(|space| embed::topics(&app.path, space, &search.clustering))
            .transpose()?
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let unsorted_count = topics
        .iter()
        .find(|topic| topic.id == -1)
        .map_or(0, |topic| topic.post_count);
    let unsorted_percent = topics
        .iter()
        .find(|topic| topic.id == -1)
        .map_or_else(|| "0.0".to_string(), |topic| topic.percent.clone());
    topics.retain(|topic| topic.id != -1);
    if topic_mode
        && search
            .topics
            .iter()
            .any(|selected| !topics.iter().any(|topic| topic.id == *selected))
    {
        search.error = Some("One or more selected topics are not available here.".into());
    }
    let mut initial_conversation_cache = None;
    let mut scored = if search.error.is_none() && semantic {
        match semantic_feed(
            app,
            db,
            &search,
            selected_space.as_ref().expect("space resolved above"),
            now,
        ) {
            Ok(rows) => rows,
            Err(error) => {
                search.error = Some(error.to_string());
                Vec::new()
            }
        }
    } else if search.error.is_none() && topic_mode {
        topic_feed(
            app,
            db,
            &search,
            selected_space.as_ref().expect("space resolved above"),
            now,
        )?
    } else if search.error.is_none() {
        let cacheable = search.mode == "conversations"
            && search.q.is_empty()
            && search.page == 0
            && search.before.is_none();
        let rows = if cacheable {
            match cached_conversations(app, db, now)? {
                Some(rows) => rows,
                None => {
                    let rows = queries::feed(db, &search, &app.root, now)?;
                    initial_conversation_cache = Some(rows.clone());
                    rows
                }
            }
        } else {
            queries::feed(db, &search, &app.root, now)?
        };
        rows.into_iter().map(|post| (post, f32::NAN)).collect()
    } else {
        Vec::new()
    };
    let mut feed_warnings = None;
    if search.hide_flagged_spam && !scored.is_empty() {
        let warnings = render::warning_map(db, scored.iter().map(|(post, _)| post))?;
        scored.retain(|(post, _)| {
            !render::has_flagged_spam(&render::mapped_warnings(&warnings, post))
        });
        feed_warnings = Some(warnings);
    }
    let ranked_at = started.elapsed();
    let has_next = scored.len() > queries::PAGE_SIZE;
    scored.truncate(queries::PAGE_SIZE);
    let scored_event_ids = scored
        .iter()
        .map(|(post, _)| EventId::from_hex(&post.source_id).map(|id| id.as_bytes().to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    let similar_available = selected_space
        .as_ref()
        .map(|space| embed::vector_event_ids(&app.path, space, &scored_event_ids))
        .transpose()?
        .unwrap_or_default();
    let results = if scored.is_empty() {
        Vec::new()
    } else {
        let card_ids = scored
            .iter()
            .map(|(post, _)| post.canonical_id.clone())
            .collect::<Vec<_>>();
        let reply_counts = queries::reply_counts_for(db, now, Some(&card_ids))?;
        let warning_map = match feed_warnings {
            Some(warnings) => warnings,
            None => render::warning_map(db, scored.iter().map(|(post, _)| post))?,
        };
        let identity_map = render::identity_map(db, scored.iter().map(|(post, _)| post))?;
        let parent_excerpt_map =
            render::parent_excerpt_map(db, scored.iter().map(|(post, _)| post), now)?;
        scored
            .iter()
            .map(|(post, similarity)| {
                let warnings: Vec<_> = warning_map
                    .get(&post.source_id)
                    .into_iter()
                    .chain(warning_map.get(&post.canonical_id))
                    .flatten()
                    .cloned()
                    .collect();
                let mut card = render::card(
                    db,
                    post,
                    now,
                    &search.mode,
                    true,
                    render::CardCache {
                        reply_count: Some(*reply_counts.get(&post.canonical_id).unwrap_or(&0)),
                        warnings: Some(&warnings),
                        identity: identity_map.get(&post.canonical_id),
                        parent: parent_excerpt_map.get(&post.canonical_id),
                    },
                )?;
                card["embedding"] = json!(&search.embedding);
                card["hide_flagged_spam"] = json!(search.hide_flagged_spam);
                card["similar_available"] = json!(EventId::from_hex(&post.source_id)
                    .map(|id| similar_available.contains(id.as_bytes().as_slice()))?);
                if semantic {
                    card["score"] = json!(format!(
                        "{} cosine similarity {similarity:.3}",
                        if search.embedding == "titan" {
                            "Titan"
                        } else {
                            "MiniLM"
                        }
                    ));
                }
                Ok(card)
            })
            .collect::<Result<Vec<_>, Error>>()?
    };
    let cards_at = started.elapsed();
    if cards_at.as_secs() >= 1 {
        eprintln!(
            "Slow reader mode={} rows={} rank_ms={} total_ms={}",
            search.mode,
            results.len(),
            ranked_at.as_millis(),
            cards_at.as_millis(),
        );
    }
    if let Some(rows) = initial_conversation_cache {
        db.execute_batch("COMMIT")?;
        queries::store_conversation_cache(&app.path, now, &rows, None)?;
    }
    let html = page(
        app,
        "feed.html",
        json!({"q":search.q,"mode":search.mode,"page":search.page,"error":search.error,
        "embedding":search.embedding,"clustering":search.clustering,"results":results,"has_next":has_next,
        "qs_base":search.query_string(),"qs_latest":search.latest_query_string(),
        "hide_flagged_url":format!("/?{}",search.hide_flagged_query_string()),
        "show_flagged_url":format!("/?{}",search.show_flagged_query_string()),
        "spam_filter_available":true,"hide_flagged_spam":search.hide_flagged_spam,
        "now":now,"before":search.before,"similar":search.similar,
        "similar_context":similar_context,"topics":topics,"selected_topics":search.topics,
        "selected_topic_labels":topics.iter().filter(|topic| search.topics.contains(&topic.id)).map(|topic| topic.label.clone()).collect::<Vec<_>>(),
        "include_unsorted":search.include_unsorted,"unsorted_count":unsorted_count,"unsorted_percent":unsorted_percent,
        "topic_settings":topic_settings,"topic_epsilon":topic_epsilon,
        "meaning_available":app.embedding.is_some() || app.embedding_queries.is_some(),"minilm_available":app.embedding.is_some() || app.cached_minilm.is_some(),
        "reach":if search.mode=="discovery"{Some(search.reach)}else{None},
        "order":&search.order,
        "state_fields":[["embedding",search.embedding.as_str()],["clustering",search.clustering.as_str()]]}),
    )?;
    Ok((
        if search.error.is_some() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        },
        html,
    ))
}

fn diverse_related_ids(candidates: Vec<(String, Option<String>)>, limit: usize) -> Vec<String> {
    let mut relations = HashSet::new();
    candidates
        .into_iter()
        .filter_map(|(id, parent_id)| {
            relations
                .insert(parent_id.unwrap_or_else(|| id.clone()))
                .then_some(id)
        })
        .take(limit)
        .collect()
}

fn thread(
    app: &App,
    db: &Connection,
    id: &str,
    raw: &str,
    now: i64,
) -> Result<(StatusCode, String), Error> {
    let Some(post) = queries::get(db, id, now)? else {
        return Ok((StatusCode::NOT_FOUND, "This post is not available.".into()));
    };
    let search = Search::parse(raw, now, &app.default_embedding);
    let space = selected_space(app, &search).ok();
    let mut seen = HashSet::from([id.to_string()]);
    let (mut ancestors, mut replies) = (Vec::new(), Vec::new());
    let (mut missing, mut cycle) = (None, false);
    let mut parent = post.parent_id.clone();
    while let Some(id) = parent {
        if !seen.insert(id.clone()) {
            cycle = true;
            break;
        }
        let Some(p) = queries::get(db, &id, now)? else {
            missing = Some(id);
            break;
        };
        parent = p.parent_id.clone();
        let mut view = render::card(db, &p, now, "", false, render::CardCache::default())?;
        view["embedding"] = json!(&search.embedding);
        view["hide_flagged_spam"] = json!(search.hide_flagged_spam);
        ancestors.push(view);
    }
    ancestors.reverse();
    let mut pending: Vec<_> = queries::children(db, &post.canonical_id, now)?
        .into_iter()
        .rev()
        .map(|p| (p, 1))
        .collect();
    while let Some((p, depth)) = pending.pop() {
        if !seen.insert(p.canonical_id.clone()) {
            cycle = true;
            continue;
        }
        pending.extend(
            queries::children(db, &p.canonical_id, now)?
                .into_iter()
                .rev()
                .map(|p| (p, depth + 1)),
        );
        let mut view = render::card(db, &p, now, "", false, render::CardCache::default())?;
        view["embedding"] = json!(&search.embedding);
        view["hide_flagged_spam"] = json!(search.hide_flagged_spam);
        view["tree_depth"] = json!(depth.min(6));
        if !search.hide_flagged_spam || !view["flagged_spam"].as_bool().unwrap() {
            replies.push(view);
        }
    }
    let mut similar_replies = Vec::new();
    if let Some(space) = space {
        let event_id = EventId::from_hex(&post.source_id)?;
        if let Some(vector) = embed::event_vector(&app.path, &space, event_id.as_bytes())? {
            let ranked = embed::nearest(
                &app.path,
                &space,
                &vector,
                Some(event_id.as_bytes()),
                now,
                seen.len().saturating_add(64),
            )?;
            let ranked_ids = ranked
                .into_iter()
                .map(|(event_id, _)| canonical_event_id(&event_id))
                .filter(|id| !seen.contains(id))
                .take(64)
                .collect::<Vec<_>>();
            let eligible = queries::eligible_map_for(db, now, Some(&ranked_ids))?;
            let candidate_warnings = render::warning_map(db, eligible.values())?;
            let candidates = ranked_ids
                .into_iter()
                .filter_map(|id| {
                    let post = eligible.get(&id)?;
                    let reasons = render::mapped_warnings(&candidate_warnings, post);
                    (!search.hide_flagged_spam || !render::has_flagged_spam(&reasons))
                        .then(|| (id, post.parent_id.clone()))
                })
                .collect();
            for id in diverse_related_ids(candidates, 5) {
                let Some(related) = eligible.get(&id) else {
                    continue;
                };
                let mut view =
                    render::card(db, related, now, "", true, render::CardCache::default())?;
                view["embedding"] = json!(&search.embedding);
                view["hide_flagged_spam"] = json!(search.hide_flagged_spam);
                view["similar_available"] = json!(true);
                similar_replies.push(view);
            }
        }
    }
    let mut current = render::card(db, &post, now, "", false, render::CardCache::default())?;
    current["embedding"] = json!(&search.embedding);
    current["hide_flagged_spam"] = json!(search.hide_flagged_spam);
    let context_url = format!(
        "/context/nostr/{}?embedding={}",
        post.source_id, search.embedding
    );
    Ok((
        StatusCode::OK,
        page(
            app,
            "context.html",
            json!({"post":current,"ancestors":ancestors,"available_reply_count":replies.len(),
        "replies":replies,"similar_replies":similar_replies,"embedding":search.embedding,
        "spam_filter_available":true,"hide_flagged_spam":search.hide_flagged_spam,
        "hide_flagged_url":format!("{context_url}&hide_spam=true"),
        "show_flagged_url":context_url,"missing_parent_id":missing,"cycle_cut":cycle}),
        )?,
    ))
}

fn status_uncached(app: &App, db: &Connection, now: i64) -> Result<String, Error> {
    let mut rows = Vec::new();
    let mut statement =
        db.prepare("SELECT source,updated_at,detail FROM source_status ORDER BY source")?;
    let records = statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in records {
        let (source, updated, detail) = row?;
        rows.push(json!({"source":source,"updated":render::time(updated),"detail":serde_json::from_str::<Value>(&detail)?}));
    }
    let gap_count: i64 = db.query_row("SELECT count(*) FROM collection_gaps", [], |r| r.get(0))?;
    let mut statement=db.prepare("SELECT relay,since_at,until_at,reason,checked_at FROM collection_gaps ORDER BY checked_at DESC,relay LIMIT 100")?;
    let gaps=statement.query_map([],|r|Ok(json!({"relay":r.get::<_,String>(0)?,"since":render::time(r.get(1)?),
        "until":render::time(r.get(2)?),"reason":r.get::<_,String>(3)?,"checked":render::time(r.get(4)?)})))?.collect::<Result<Vec<_>,_>>()?;
    let mut statement=db.prepare("SELECT coalesce(l.source,a.source),coalesce(l.identifier,a.identifier),l.event_id,l.event_created_at,l.checked_at,
        (SELECT count(DISTINCT value) FROM json_each(l.members_json)),a.attempted_at,a.error
        FROM moderation_lists l FULL OUTER JOIN moderation_refresh_attempts a USING(source,identifier) ORDER BY 1,2")?;
    let lists=statement.query_map([],|r|Ok(json!({"source":r.get::<_,String>(0)?,"identifier":r.get::<_,String>(1)?,
        "event_id":r.get::<_,Option<String>>(2)?,"signed":r.get::<_,Option<i64>>(3)?.map(render::time),
        "checked":r.get::<_,Option<i64>>(4)?.map(render::time),"members":r.get::<_,Option<i64>>(5)?,
        "attempted":r.get::<_,Option<i64>>(6)?.map(render::time),"error":r.get::<_,Option<String>>(7)?})))?.collect::<Result<Vec<_>,_>>()?;
    let counts = queries::count(db, now, None)?;
    let (direct_follows, missing_contact_lists): (i64, i64) = db.query_row(
        "WITH direct AS (
           SELECT DISTINCT followee FROM social_edges WHERE follower=?1
         ), owners AS (
           SELECT DISTINCT lower(hex(pubkey)) owner FROM events WHERE kind=3
         )
         SELECT count(*),coalesce(sum(owner IS NULL),0)
         FROM direct LEFT JOIN owners ON owner=followee",
        [&app.root],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let conversation_cache = queries::conversation_cache(db)?.map(
        |cache| json!({"age":now.saturating_sub(cache.generated_at),"attempt_age":now.saturating_sub(cache.last_attempt),"error":cache.last_error}),
    );
    let embedding_spaces: Vec<_> = embed::cache_status(db, now)?
        .into_iter()
        .map(|status| {
            let cost_usd = format!("{:.9}", status.cost_nusd as f64 / 1_000_000_000.0);
            let monthly_cost_usd =
                format!("{:.9}", status.monthly_cost_nusd as f64 / 1_000_000_000.0);
            json!({"backend":status.backend,"model":status.model,"dimensions":status.dimensions,
                "revision":status.revision,"model_sha256":status.model_sha256,
                "tokenizer_sha256":status.tokenizer_sha256,"vectors":status.vectors,
                "eligible_vectors":status.eligible_vectors,"pending":status.pending,"rejected":status.rejected,
                "requests":status.requests,"tokens":status.tokens,"cost_usd":cost_usd,
                "monthly_cost_usd":monthly_cost_usd,"uncertain":status.uncertain,
                "reserved":status.reserved,
                "newest_embedded":status.newest_embedded_at.map(render::time),
                "oldest_pending":status.oldest_pending_at.map(render::time)})
        })
        .collect();
    page(
        app,
        "status.html",
        json!({"now":render::time(now),"eligible_posts":counts,"status_rows":rows,"gap_count":gap_count,
            "gaps":gaps,"lists":lists,"embedding_spaces":embedding_spaces,
            "embedding_error":app.embedding_error.lock().unwrap().clone(),"conversation_cache":conversation_cache,
            "direct_follows":direct_follows,"missing_contact_lists":missing_contact_lists}),
    )
}

fn open_reader(path: &std::path::Path) -> Result<Connection, Error> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(Duration::from_secs(10))?;
    db.pragma_update(None, "cache_size", -16_384)?;
    db.pragma_update(None, "mmap_size", 67_108_864)?;
    db.pragma_update(None, "temp_store", "FILE")?;
    db.execute_batch("BEGIN")?;
    Ok(db)
}

fn refresh_status_cache(app: &App, now: i64) -> Result<(), Error> {
    let db = open_reader(&app.path)?;
    let html = status_uncached(app, &db, now)?;
    *app.status_cache.write().unwrap() = Some(StatusSnapshot {
        generated_at: now,
        html,
        refresh_error: false,
    });
    Ok(())
}

fn cached_status_html(app: &App, now: i64) -> Option<String> {
    let cache = app.status_cache.read().unwrap();
    let snapshot = cache.as_ref()?;
    let generated =
        chrono::DateTime::from_timestamp(snapshot.generated_at, 0)?.format("%Y-%m-%d %H:%M:%S UTC");
    let age = now.saturating_sub(snapshot.generated_at);
    let warning = if snapshot.refresh_error || age > 300 {
        " It may be out of date."
    } else {
        ""
    };
    let notice =
        format!("<p class=\"status-line\">Status updated {generated} ({age}s ago).{warning}</p>");
    Some(
        snapshot
            .html
            .replacen("<div id=\"status-snapshot-notice\"></div>", &notice, 1),
    )
}

fn cached_status(app: &App, db: &Connection, now: i64) -> Result<String, Error> {
    if let Some(html) = cached_status_html(app, now) {
        return Ok(html);
    }
    let html = status_uncached(app, db, now)?;
    *app.status_cache.write().unwrap() = Some(StatusSnapshot {
        generated_at: now,
        html,
        refresh_error: false,
    });
    cached_status_html(app, now).ok_or_else(|| "status snapshot was not stored".into())
}

fn handle(app: &App, path: &str, raw: &str, now: i64) -> Result<(StatusCode, String), Error> {
    if path == "/status" {
        if let Some(html) = cached_status_html(app, now) {
            return Ok((StatusCode::OK, html));
        }
    }
    let db = open_reader(&app.path)?;
    match path {
        "/" => feed(app, &db, raw, now),
        "/about" | "/tos" => Ok((
            StatusCode::OK,
            page(app, &format!("{}.html", &path[1..]), json!({}))?,
        )),
        "/status" => Ok((StatusCode::OK, cached_status(app, &db, now)?)),
        _ => {
            if let Some(id) = path
                .strip_prefix("/context/")
                .and_then(|s| s.split_once('/'))
            {
                let id = format!("{}:{}", id.0, id.1);
                thread(app, &db, &id, raw, now)
            } else {
                Ok((StatusCode::NOT_FOUND, "Not found".into()))
            }
        }
    }
}
async fn request(State(app): State<Arc<App>>, OriginalUri(uri): OriginalUri) -> Response {
    if uri.path() == "/search" {
        return (
            StatusCode::MOVED_PERMANENTLY,
            [("location", format!("/?{}", uri.query().unwrap_or("")))],
        )
            .into_response();
    }
    if uri.path() == "/status" {
        if let Some(html) = cached_status_html(&app, Utc::now().timestamp()) {
            return Html(html).into_response();
        }
    }
    let now = Utc::now().timestamp();
    let path = uri.path().to_owned();
    let raw = uri.query().unwrap_or("").to_owned();
    let search = Search::parse(&raw, now, &app.default_embedding);
    if path == "/"
        && search.mode == "meaning"
        && search.embedding == "titan"
        && !search.q.is_empty()
    {
        let cache_path = app.path.clone();
        let query = search.q.clone();
        let cached = tokio::task::spawn_blocking(move || {
            let Some(space) = embed::space_by_backend(&cache_path, "bedrock")? else {
                return Ok::<bool, Error>(false);
            };
            Ok(embed::cached_query(&cache_path, &space, &query)?.is_some())
        })
        .await;
        let cached = match cached {
            Ok(Ok(cached)) => cached,
            error => {
                eprintln!("Titan query cache lookup failed: {error:?}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Meaning search is temporarily unavailable.",
                )
                    .into_response();
            }
        };
        if !cached {
            let Some(sender) = &app.embedding_queries else {
                return (
                    StatusCode::BAD_REQUEST,
                    "Meaning search is unavailable right now.",
                )
                    .into_response();
            };
            let (reply, result) = tokio::sync::oneshot::channel();
            if sender
                .send(collect::EmbeddingQuery {
                    query: search.q.clone(),
                    reply,
                })
                .await
                .is_err()
            {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Meaning search is temporarily unavailable.",
                )
                    .into_response();
            }
            let result = tokio::time::timeout(Duration::from_secs(300), result).await;
            match result {
                Ok(Ok(Ok(()))) => *app.embedding_error.lock().unwrap() = None,
                Ok(Ok(Err(error))) => {
                    let message = format!("Titan semantic search failed: {error}");
                    *app.embedding_error.lock().unwrap() = Some(message.clone());
                    eprintln!("{message}");
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Meaning search is temporarily unavailable.",
                    )
                        .into_response();
                }
                Ok(Err(_)) => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Meaning search is temporarily unavailable.",
                    )
                        .into_response();
                }
                Err(_) => {
                    return (
                        StatusCode::GATEWAY_TIMEOUT,
                        "Meaning search took too long. Try again later.",
                    )
                        .into_response();
                }
            }
        }
    }
    let permit = match app.reader_slots.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "The reader cannot accept this request right now.",
            )
                .into_response();
        }
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        handle(&app, &path, &raw, now)
    })
    .await;
    match result {
        Ok(Ok((status, html))) => (status, Html(html)).into_response(),
        error => {
            eprintln!("Reader request failed: {error:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "The reader could not load this page. Try again shortly.",
            )
                .into_response()
        }
    }
}
fn router(app: App) -> Router {
    Router::new()
        .route(
            "/static/style.css",
            get(|| async {
                (
                    [("content-type", "text/css")],
                    include_str!("../static/style.css"),
                )
            }),
        )
        .fallback(get(request))
        .with_state(Arc::new(app))
}
fn usd_nusd(value: &str) -> Result<i64, Error> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 9
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !fraction.chars().all(|c| c.is_ascii_digit())
    {
        return Err(format!("Invalid USD budget: {value}").into());
    }
    let padded = format!("{fraction:0<9}");
    Ok(whole.parse::<i64>()? * 1_000_000_000 + padded.parse::<i64>()?)
}

fn continuous_budget(backend: Option<&str>) -> Result<Option<embed::Budget>, Error> {
    match backend {
        Some("bedrock") => {
            let monthly_nusd = usd_nusd(&std::env::var("MEATYBROTH_EMBED_MONTHLY_BUDGET_USD")?)?;
            if monthly_nusd > 5_000_000_000 {
                return Err("Titan monthly safety ceiling exceeds US$5".into());
            }
            Ok(Some(embed::Budget {
                total_nusd: i64::MAX,
                monthly_nusd,
            }))
        }
        Some("minilm") => Ok(Some(embed::Budget {
            total_nusd: i64::MAX,
            monthly_nusd: i64::MAX,
        })),
        None => Ok(None),
        Some(other) => Err(format!("Unknown embedding backend: {other}").into()),
    }
}

async fn backfill_embeddings(
    path: &std::path::Path,
    transport: &(impl embed::Transport + ?Sized),
    budget: embed::Budget,
) -> Result<(), Error> {
    let mut total = 0;
    let max_posts = match std::env::var("MEATYBROTH_EMBED_MAX_POSTS") {
        Ok(value) => value.parse()?,
        Err(std::env::VarError::NotPresent) => usize::MAX,
        Err(error) => return Err(error.into()),
    };
    loop {
        let remaining = max_posts.saturating_sub(total);
        if remaining == 0 {
            break;
        }
        let count = embed::embed_pending(
            path,
            transport,
            budget,
            Utc::now().timestamp(),
            remaining.min(100),
        )
        .await?;
        total += count;
        eprintln!("Embedded {count} posts in this batch; total={total}");
        if count == 0 {
            break;
        }
    }
    if let Ok(queries) = std::env::var("MEATYBROTH_EMBED_QUERY_CACHE") {
        for query in queries.split('|').filter(|query| !query.is_empty()) {
            let added =
                embed::cache_query(path, transport, budget, query, Utc::now().timestamp()).await?;
            eprintln!("Cached query added={added}: {query}");
        }
    }
    let (kmeans, dbscan) = embed::rebuild_topics(path, transport.space(), Utc::now().timestamp())?;
    eprintln!("Built {kmeans} fixed-k and {dbscan} DBSCAN topics");
    Ok(())
}

async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler must install");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| "A process-level rustls CryptoProvider was already installed")?;
    let path = PathBuf::from(std::env::var("MEATYBROTH_DB")?);
    let read_only = std::env::var("MEATYBROTH_READ_ONLY").as_deref() == Ok("1");
    let embed_only = std::env::var("MEATYBROTH_EMBED_ONLY").as_deref() == Ok("1");
    let sdk = if embed_only {
        None
    } else if read_only {
        Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        None
    } else {
        Some(collect::open(&path, collect::PRIMAL_AUTHOR).await?)
    };
    let root = std::env::var("MEATYBROTH_ROOT").unwrap_or_else(|_| ROOT.into());
    let root_key = PublicKey::from_hex(&root)?;
    if let Ok(backend) = std::env::var("MEATYBROTH_RECLUSTER_ONLY") {
        if !["minilm", "bedrock"].contains(&backend.as_str()) {
            return Err("MEATYBROTH_RECLUSTER_ONLY must be minilm or bedrock".into());
        }
        let space = embed::space_by_backend(&path, &backend)?
            .ok_or("Selected embedding cache is not available")?;
        let (kmeans, dbscan) = embed::rebuild_topics(&path, &space, Utc::now().timestamp())?;
        eprintln!(
            "Built {kmeans} fixed-k and {dbscan} DBSCAN topics from cached {backend} vectors"
        );
        return Ok(());
    }
    let stage_started = std::time::Instant::now();
    let backend = std::env::var("MEATYBROTH_EMBED_BACKEND").ok();
    let embedding = local_embedding(&path, 8)?;
    let bedrock = if backend.as_deref() == Some("bedrock") {
        Some(Arc::new(
            embed::Bedrock::new(&std::env::var("AWS_REGION")?).await,
        ))
    } else {
        None
    };
    let embedding_transport: Option<Arc<dyn embed::Transport>> = match (&embedding, &bedrock) {
        (Some(model), None) => Some(model.clone()),
        (None, Some(model)) => Some(model.clone()),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!(),
    };
    let embedding_budget = continuous_budget(backend.as_deref())?;
    eprintln!(
        "Startup stage embedding_setup_ms={}",
        stage_started.elapsed().as_millis()
    );
    if embed_only {
        let transport = embedding_transport
            .as_ref()
            .ok_or("MEATYBROTH_EMBED_ONLY requires an explicit embedding backend")?;
        let budget = if backend.as_deref() == Some("bedrock") {
            let total_nusd = usd_nusd(&std::env::var("MEATYBROTH_EMBED_TOTAL_BUDGET_USD")?)?;
            let monthly_nusd = usd_nusd(&std::env::var("MEATYBROTH_EMBED_MONTHLY_BUDGET_USD")?)?;
            if total_nusd > 5_000_000_000 || monthly_nusd > 5_000_000_000 {
                return Err("Approved Titan ceiling is US$5 total and US$5/month".into());
            }
            embed::Budget {
                total_nusd,
                monthly_nusd,
            }
        } else {
            embedding_budget.expect("an embedding transport requires a budget")
        };
        backfill_embeddings(&path, transport.as_ref(), budget).await?;
        return Ok(());
    }
    let default_embedding =
        std::env::var("MEATYBROTH_DEFAULT_EMBEDDING").unwrap_or_else(|_| "minilm".into());
    if !["minilm", "titan"].contains(&default_embedding.as_str()) {
        return Err("MEATYBROTH_DEFAULT_EMBEDDING must be minilm or titan".into());
    }
    let embedding_error = Arc::new(Mutex::new(None));
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let (collector_embedding, embedding_queries) = if let Some(transport) = embedding_transport {
        let (sender, queries) = tokio::sync::mpsc::channel(32);
        let query_sender = (transport.space().backend == "bedrock").then_some(sender);
        (
            Some(collect::EmbeddingWorker {
                transport,
                budget: embedding_budget.expect("an embedding transport requires a budget"),
                queries,
                error: embedding_error.clone(),
                disabled: false,
                validated: false,
                preflight_only: std::env::var("MEATYBROTH_EMBED_PREFLIGHT_ONLY").as_deref()
                    == Ok("1"),
                shutdown: shutdown_requested.clone(),
                topic_rebuild: None,
                topics_disabled: false,
            }),
            query_sender,
        )
    } else {
        (None, None)
    };
    let cached_minilm = embed::space_by_backend(&path, "minilm")?;
    let app_state = App {
        path: path.clone(),
        root,
        templates: templates()?,
        embedding: embedding
            .clone()
            .map(|model| model as Arc<dyn embed::SemanticModel>),
        embedding_queries,
        embedding_error: embedding_error.clone(),
        cached_minilm,
        default_embedding,
        collecting: !read_only,
        reader_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        status_cache: Arc::new(RwLock::new(None)),
    };
    refresh_status_cache(&app_state, Utc::now().timestamp())?;
    let status_app = app_state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(120)).await;
            let app = status_app.clone();
            let permit = app
                .reader_slots
                .clone()
                .acquire_owned()
                .await
                .expect("reader semaphore remains open");
            match tokio::task::spawn_blocking(move || {
                let _permit = permit;
                refresh_status_cache(&app, Utc::now().timestamp())
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    if let Some(snapshot) = status_app.status_cache.write().unwrap().as_mut() {
                        snapshot.refresh_error = true;
                    }
                    eprintln!("Status snapshot refresh failed: {error}");
                }
                Err(error) => eprintln!("Status snapshot task failed: {error}"),
            }
        }
    });
    let app = router(app_state);
    let addr = std::env::var("MEATYBROTH_ADDR").unwrap_or_else(|_| "127.0.0.1:8083".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("Rust reader listening on http://{addr}");
    let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
    let mut collector = None;
    if let Some(sdk) = sdk {
        let relays = std::env::var("MEATYBROTH_RELAYS")
            .unwrap_or_else(|_| "wss://relay.damus.io,wss://relay.primal.net".into())
            .split(',')
            .map(str::to_owned)
            .collect();
        let profile_relays = std::env::var("MEATYBROTH_PROFILE_RELAYS")
            .unwrap_or_else(|_| "wss://purplepag.es".into())
            .split(',')
            .map(str::to_owned)
            .collect();
        collector = Some(tokio::spawn(async move {
            if let Err(error) = collect::run(
                &path,
                sdk,
                relays,
                profile_relays,
                root_key,
                collector_embedding,
                shutdown_receiver,
            )
            .await
            {
                eprintln!(
                    "Collector stopped with an explicit error: {error}; HTTP reader remains available"
                );
            }
        }));
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_requested.store(true, Ordering::Release);
            let _ = shutdown_sender.send(true);
        })
        .await?;
    if let Some(collector) = collector {
        collector.await?;
    }
    Ok(())
}

#[cfg(test)]
mod sdk_tests;
#[cfg(test)]
mod tests;
