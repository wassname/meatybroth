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
    sync::Arc,
    time::Duration,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
const ROOT: &str = "60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6";
const MODES: [&str; 6] = [
    "new",
    "relevance",
    "meaning",
    "topics",
    "conversations",
    "discovery",
];

struct App {
    path: PathBuf,
    root: String,
    templates: Environment<'static>,
    embedding: Option<Arc<embed::MiniLm>>,
}

fn local_embedding(path: &std::path::Path) -> Result<Option<Arc<embed::MiniLm>>, Error> {
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
            Ok(Some(Arc::new(embed::MiniLm::open(&cache)?)))
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
            Err("Bedrock transport is disabled until the paid-call approval gate".into())
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
    topic: Option<i64>,
    error: Option<String>,
}
impl Search {
    fn parse(raw: &str, now: i64) -> Self {
        let args: HashMap<String, String> = url::form_urlencoded::parse(raw.as_bytes())
            .into_owned()
            .collect();
        let arg = |key: &str| args.get(key).map(String::as_str).unwrap_or("");
        let number =
            |key: &str, default: i64| arg(key).parse::<i64>().map(|n| n.max(0)).unwrap_or(default);
        let q = arg("q").trim().to_owned();
        let mut mode = arg("mode");
        if mode == "recent" {
            mode = "new";
        }
        if !q.is_empty() && args.contains_key("go") {
            mode = "relevance";
        } else if !MODES.contains(&mode) && mode != "similar" {
            mode = if q.is_empty() { "new" } else { "relevance" };
        }
        if q.is_empty() && mode == "relevance" {
            mode = "conversations";
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
        let parsed = if q.is_empty() || ["meaning", "similar"].contains(&mode) {
            Ok(None)
        } else {
            parse_fts(&q).map(Some)
        };
        let (expression, error) = match parsed {
            Ok(e) => (e, None),
            Err(e) => (None, Some(e)),
        };
        Self {
            q,
            mode: mode.into(),
            page: number("page", 0),
            before,
            reach: match number("reach", 2) {
                n @ 1..=3 => n,
                _ => 2,
            },
            order: if arg("order") == "connections" {
                "connections"
            } else {
                "recent"
            }
            .into(),
            expression,
            similar: args.get("similar").cloned(),
            topic: args.get("topic").and_then(|value| value.parse().ok()),
            error,
        }
    }
    fn query_string(&self) -> String {
        let mut qs = url::form_urlencoded::Serializer::new(String::new());
        if !self.q.is_empty() {
            qs.append_pair("q", &self.q);
        }
        qs.append_pair("mode", &self.mode);
        if let Some(before) = self.before {
            qs.append_pair("before", &before.to_string());
        }
        if self.mode == "discovery" {
            qs.append_pair("reach", &self.reach.to_string())
                .append_pair("order", &self.order);
        }
        if let Some(similar) = &self.similar {
            qs.append_pair("similar", similar);
        }
        if let Some(topic) = self.topic {
            qs.append_pair("topic", &topic.to_string());
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
        return Err("Search query has no searchable terms.".into());
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
    let mut shared = json!({"modes":MODES,"window_days":30,"mode":"new",
    "mode_labels":{"new":"New","relevance":"Relevance","meaning":"Meaning","topics":"Topics","conversations":"Conversations","discovery":"Social","similar":"Similar"},
    "mode_explanations":{
        "new":"Every post from the last 30 days, newest first.",
        "relevance":"Posts matching your terms, best match first (BM25: lower = closer); newer breaks ties.",
        "meaning":"Posts nearest to your text in the local MiniLM vector space.",
        "similar":"Posts nearest to the selected post in the local MiniLM vector space.",
        "topics":"Keyword-labelled clusters from the local MiniLM vector space.",
        "conversations":"Threads with the most distinct recent repliers first; with a query, only matching threads.",
        "discovery":"Recent posts within the selected 1–3 hop reach. Connections favours authors supported by more independent accounts you follow; lower distance score ranks first. Based on the collected, incomplete follow graph."
    }});
    shared
        .as_object_mut()
        .unwrap()
        .extend(data.as_object().unwrap().clone());
    Ok(app.templates.get_template(name)?.render(shared)?)
}

fn event_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn semantic_feed(
    app: &App,
    db: &Connection,
    search: &Search,
    now: i64,
) -> Result<Vec<(queries::Post, f32)>, Error> {
    let model = app
        .embedding
        .as_ref()
        .ok_or("Semantic search is unavailable because no embedding backend is configured")?;
    let (query, exclude) = if search.mode == "meaning" {
        if search.q.is_empty() {
            return Err("Meaning search requires text".into());
        }
        (model.embed_text(&search.q)?.vector, None)
    } else {
        let id = search
            .similar
            .as_deref()
            .ok_or("Similar search requires an event ID")?;
        let event_id = EventId::from_hex(id)?;
        let vector = embed::event_vector(&app.path, model.vector_space(), event_id.as_bytes())?
            .ok_or("This post has not been embedded yet")?;
        (vector, Some(event_id))
    };
    let offset = usize::try_from(search.page)? * queries::PAGE_SIZE;
    let ranked = embed::nearest(
        &app.path,
        model.vector_space(),
        &query,
        exclude
            .as_ref()
            .map(|event_id| event_id.as_bytes().as_slice()),
        now,
        offset + queries::PAGE_SIZE + 1,
    )?;
    let mut rows = Vec::new();
    for (event_id, score) in ranked.into_iter().skip(offset) {
        if let Some(post) = queries::get(db, &event_hex(&event_id), now)? {
            rows.push((post, score));
        }
    }
    Ok(rows)
}

fn topic_feed(
    app: &App,
    db: &Connection,
    search: &Search,
    now: i64,
) -> Result<Vec<(queries::Post, f32)>, Error> {
    let model = app
        .embedding
        .as_ref()
        .ok_or("Topics are unavailable because no embedding backend is configured")?;
    let Some(topic_id) = search.topic else {
        return Ok(Vec::new());
    };
    let offset = usize::try_from(search.page)? * queries::PAGE_SIZE;
    let event_ids = embed::topic_events(
        &app.path,
        model.vector_space(),
        topic_id,
        now,
        queries::PAGE_SIZE + 1,
        offset,
    )?;
    let mut rows = Vec::new();
    for event_id in event_ids {
        if let Some(post) = queries::get(db, &event_hex(&event_id), now)? {
            rows.push((post, f32::NAN));
        }
    }
    Ok(rows)
}

fn feed(app: &App, db: &Connection, raw: &str, now: i64) -> Result<(StatusCode, String), Error> {
    let mut search = Search::parse(raw, now);
    let semantic = ["meaning", "similar"].contains(&search.mode.as_str());
    let topic_mode = search.mode == "topics";
    let topics = if topic_mode {
        match app.embedding.as_ref() {
            Some(model) => embed::topics(&app.path, model.vector_space())?,
            None => {
                search.error = Some(
                    "Topics are unavailable because no embedding backend is configured".into(),
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let mut scored = if search.error.is_none() && semantic {
        match semantic_feed(app, db, &search, now) {
            Ok(rows) => rows,
            Err(error) => {
                search.error = Some(error.to_string());
                Vec::new()
            }
        }
    } else if search.error.is_none() && topic_mode {
        topic_feed(app, db, &search, now)?
    } else if search.error.is_none() {
        queries::feed(db, &search, &app.root, now)?
            .into_iter()
            .map(|post| (post, f32::NAN))
            .collect()
    } else {
        Vec::new()
    };
    let has_next = scored.len() > queries::PAGE_SIZE;
    scored.truncate(queries::PAGE_SIZE);
    let results = scored
        .iter()
        .map(|(post, similarity)| {
            let mut card = render::card(db, post, now, &search.mode, true)?;
            if semantic {
                card["score"] = json!(format!("cosine similarity {similarity:.3}"));
            }
            Ok(card)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let html = page(
        app,
        "feed.html",
        json!({"q":search.q,"mode":search.mode,"page":search.page,"error":search.error,
        "results":results,"has_next":has_next,"qs_base":search.query_string(),"now":now,"before":search.before,
        "topics":topics,"selected_topic":search.topic,
        "reach":if search.mode=="discovery"{Some(search.reach)}else{None},
        "order":if search.mode=="discovery"{Some(&search.order)}else{None},"state_fields":[]}),
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

fn thread(app: &App, db: &Connection, id: &str, now: i64) -> Result<(StatusCode, String), Error> {
    let Some(post) = queries::get(db, id, now)? else {
        return Ok((
            StatusCode::NOT_FOUND,
            "Post not stored in the current window.".into(),
        ));
    };
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
        ancestors.push(render::card(db, &p, now, "", false)?);
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
        let mut view = render::card(db, &p, now, "", false)?;
        view["tree_depth"] = json!(depth.min(6));
        replies.push(view);
    }
    Ok((
        StatusCode::OK,
        page(
            app,
            "context.html",
            json!({"post":render::card(db,&post,now,"",false)?,
        "ancestors":ancestors,"available_reply_count":replies.len(),"replies":replies,"missing_parent_id":missing,"cycle_cut":cycle}),
        )?,
    ))
}

fn status(app: &App, db: &Connection, now: i64) -> Result<String, Error> {
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
    let mut statement=db.prepare("SELECT coalesce(l.source,a.source),coalesce(l.identifier,a.identifier),l.event_id,l.event_created_at,l.checked_at,json_array_length(l.members_json),
        a.attempted_at,a.error FROM moderation_lists l FULL OUTER JOIN moderation_refresh_attempts a USING(source,identifier) ORDER BY 1,2")?;
    let lists=statement.query_map([],|r|Ok(json!({"source":r.get::<_,String>(0)?,"identifier":r.get::<_,String>(1)?,
        "event_id":r.get::<_,Option<String>>(2)?,"signed":r.get::<_,Option<i64>>(3)?.map(render::time),
        "checked":r.get::<_,Option<i64>>(4)?.map(render::time),"members":r.get::<_,Option<i64>>(5)?,
        "attempted":r.get::<_,Option<i64>>(6)?.map(render::time),"error":r.get::<_,Option<String>>(7)?})))?.collect::<Result<Vec<_>,_>>()?;
    let counts = queries::count(db, now, None)?;
    page(
        app,
        "status.html",
        json!({"now":render::time(now),"eligible_posts":counts,"status_rows":rows,"gap_count":gap_count,"gaps":gaps,"lists":lists}),
    )
}

fn handle(app: &App, path: &str, raw: &str, now: i64) -> Result<(StatusCode, String), Error> {
    let db = Connection::open_with_flags(&app.path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(Duration::from_secs(10))?;
    db.execute_batch("BEGIN")?;
    match path {
        "/" => feed(app, &db, raw, now),
        "/about" | "/tos" => Ok((
            StatusCode::OK,
            page(app, &format!("{}.html", &path[1..]), json!({}))?,
        )),
        "/status" => Ok((StatusCode::OK, status(app, &db, now)?)),
        _ => {
            if let Some(id) = path
                .strip_prefix("/context/")
                .and_then(|s| s.split_once('/'))
            {
                let id = format!("{}:{}", id.0, id.1);
                thread(app, &db, &id, now)
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
    let result = tokio::task::spawn_blocking(move || {
        handle(
            &app,
            uri.path(),
            uri.query().unwrap_or(""),
            Utc::now().timestamp(),
        )
    })
    .await;
    match result {
        Ok(Ok((status, html))) => (status, Html(html)).into_response(),
        error => {
            eprintln!("Reader request failed: {error:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Reader request failed; see server log.",
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
#[tokio::main]
async fn main() -> Result<(), Error> {
    let path = PathBuf::from(std::env::var("MEATYBROTH_DB")?);
    let read_only = std::env::var("MEATYBROTH_READ_ONLY").as_deref() == Ok("1");
    let sdk = if read_only {
        Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        None
    } else {
        Some(collect::open(&path, collect::PRIMAL_AUTHOR).await?)
    };
    let root = std::env::var("MEATYBROTH_ROOT").unwrap_or_else(|_| ROOT.into());
    let root_key = PublicKey::from_hex(&root)?;
    let embedding = local_embedding(&path)?;
    if std::env::var("MEATYBROTH_EMBED_ONLY").as_deref() == Ok("1") {
        let model = embedding
            .as_ref()
            .ok_or("MEATYBROTH_EMBED_ONLY requires an explicit embedding backend")?;
        let mut total = 0;
        loop {
            let count = embed::embed_pending(
                &path,
                model.as_ref(),
                embed::Budget {
                    total_nusd: i64::MAX,
                    monthly_nusd: i64::MAX,
                },
                Utc::now().timestamp(),
                100,
            )
            .await?;
            total += count;
            eprintln!("Embedded {count} local posts in this batch; total={total}");
            if count == 0 {
                break;
            }
        }
        let topics = embed::cluster_topics(&path, model.vector_space(), Utc::now().timestamp())?;
        eprintln!("Built {topics} keyword-labelled MiniLM topics");
        return Ok(());
    }
    let app = router(App {
        path: path.clone(),
        root,
        templates: templates()?,
        embedding: embedding.clone(),
    });
    let addr = std::env::var("MEATYBROTH_ADDR").unwrap_or_else(|_| "127.0.0.1:8083".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("Rust reader listening on http://{addr}");
    if let Some(sdk) = sdk {
        let relays = std::env::var("MEATYBROTH_RELAYS")
            .unwrap_or_else(|_| "wss://relay.damus.io,wss://relay.primal.net".into())
            .split(',')
            .map(str::to_owned)
            .collect();
        tokio::spawn(async move {
            if let Err(error) = collect::run(&path, sdk, relays, root_key, embedding).await {
                eprintln!(
                    "Collector stopped with an explicit error: {error}; HTTP reader remains available"
                );
            }
        });
    }
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod sdk_tests;
#[cfg(test)]
mod tests;
