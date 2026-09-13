use crate::{
    queries::{self, Post},
    Error,
};
use ammonia::{Builder, UrlRelative};
use chrono::{DateTime, Utc};
use pulldown_cmark::{html, CowStr, Event, Parser, Tag, TagEnd};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const PREVIEW_CHARACTERS: usize = 280;
const MIN_COLLAPSED_REMAINDER: usize = 150;

pub fn body(text: &str) -> String {
    let mut finder = linkify::LinkFinder::new();
    finder.kinds(&[linkify::LinkKind::Url]);
    let mut events = Vec::new();
    let (mut links, mut code, mut image_depth) = (0, 0, 0);
    let mut image = String::new();
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => {
                image_depth -= 1;
                if image_depth > 0 {
                    continue;
                }
                let alt = std::mem::take(&mut image);
                events.push(Event::Text(
                    format!(
                        "[{}]",
                        if alt.trim().is_empty() {
                            "image.png".into()
                        } else {
                            format!("image: {}", alt.trim())
                        }
                    )
                    .into(),
                ));
            }
            Event::Text(t) | Event::Code(t) if image_depth > 0 => image.push_str(&t),
            _ if image_depth > 0 => {}
            Event::Start(Tag::Link { .. }) => {
                links += 1;
                events.push(event);
            }
            Event::End(TagEnd::Link) => {
                links -= 1;
                events.push(event);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                code += 1;
                events.push(event);
            }
            Event::End(TagEnd::CodeBlock) => {
                code -= 1;
                events.push(event);
            }
            Event::Text(t) if links == 0 && code == 0 => {
                let mut end = 0;
                for link in finder.links(&t).filter(|l| {
                    url::Url::parse(l.as_str())
                        .is_ok_and(|u| ["http", "https"].contains(&u.scheme()))
                }) {
                    events.push(Event::Text(t[end..link.start()].to_owned().into()));
                    events.push(Event::Start(Tag::Link {
                        link_type: pulldown_cmark::LinkType::Autolink,
                        dest_url: link.as_str().to_owned().into(),
                        title: CowStr::from(""),
                        id: CowStr::from(""),
                    }));
                    events.push(Event::Text(link.as_str().to_owned().into()));
                    events.push(Event::End(TagEnd::Link));
                    end = link.end();
                }
                events.push(Event::Text(t[end..].to_owned().into()));
            }
            _ => events.push(event),
        }
    }
    let mut rendered = String::new();
    html::push_html(&mut rendered, events.into_iter());
    Builder::default()
        .tags(
            [
                "p",
                "br",
                "a",
                "code",
                "pre",
                "strong",
                "em",
                "b",
                "i",
                "ul",
                "ol",
                "li",
                "blockquote",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "hr",
                "table",
                "thead",
                "tbody",
                "tr",
                "td",
                "th",
                "span",
                "div",
                "del",
                "sup",
                "sub",
            ]
            .into_iter()
            .collect(),
        )
        .tag_attributes(HashMap::from([("a", HashSet::from(["href", "title"]))]))
        .generic_attributes(HashSet::new())
        .url_schemes(HashSet::from(["http", "https"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .clean(&rendered)
        .to_string()
}

pub fn time(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M UTC")
        .to_string()
}
pub fn age(timestamp: i64, now: i64) -> String {
    let seconds = (now - timestamp).max(0);
    let (unit, size) = [("d", 86400), ("h", 3600), ("m", 60), ("s", 1)]
        .into_iter()
        .find(|(_, size)| seconds >= *size)
        .unwrap_or(("s", 1));
    format!("{}{unit}", seconds / size)
}

fn identity_from_profile(
    p: &Post,
    profile_content: Option<&str>,
) -> Result<(String, String, bool, String), Error> {
    let placeholder = p.author_name.trim();
    let hexish = (8..=64).contains(&placeholder.len())
        && placeholder
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    let mut name = if hexish { "" } else { placeholder }.to_string();
    let mut address = String::new();
    if let Some(profile) = profile_content.and_then(|s| serde_json::from_str::<Value>(s).ok()) {
        if let Some(display) = profile
            .get("display_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                profile
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
        {
            name = display.into();
        }
        if let Some(nip05) = profile
            .get("nip05")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| s.contains('@'))
        {
            address = if nip05.starts_with('_') {
                format!("@{}", nip05.split_once('@').unwrap().1)
            } else {
                nip05.into()
            };
        }
    }
    let bytes = (0..p.author_id.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&p.author_id[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.len() != 32 {
        return Err("invalid author public key".into());
    }
    let npub = bech32::encode::<bech32::Bech32>(bech32::Hrp::parse("npub")?, &bytes)?;
    let named = !name.is_empty();
    if !named {
        name = p.author_id.chars().take(16).collect();
    }
    Ok((name, address, named, format!("https://njump.me/{npub}")))
}

fn identity(db: &Connection, p: &Post) -> Result<(String, String, bool, String), Error> {
    let content: Option<String> = db
        .query_row(
            "SELECT content FROM events WHERE pubkey=unhex(?1) AND kind=0
             ORDER BY created_at DESC,id ASC LIMIT 1",
            [&p.author_id],
            |row| row.get(0),
        )
        .optional()?;
    identity_from_profile(p, content.as_deref())
}

/// Rendered profile identity fields in card-template order.
pub type Identity = (String, String, bool, String);

/// Resolves card identities from one profile scan rather than one query per card.
pub fn identity_map<'a>(
    db: &Connection,
    posts: impl IntoIterator<Item = &'a Post>,
) -> Result<HashMap<String, Identity>, Error> {
    let posts: Vec<_> = posts.into_iter().collect();
    let mut statement = db.prepare(
        "SELECT lower(hex(pubkey)),content
         FROM events
         WHERE kind=0
         ORDER BY created_at DESC,id ASC",
    )?;
    let mut profiles = HashMap::<String, String>::new();
    for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))? {
        let (author, content) = row?;
        profiles.entry(author).or_insert(content);
    }
    posts
        .into_iter()
        .map(|post| {
            Ok((
                post.canonical_id.clone(),
                identity_from_profile(post, profiles.get(&post.author_id).map(String::as_str))?,
            ))
        })
        .collect()
}

pub fn warning_map<'a>(
    db: &Connection,
    posts: impl IntoIterator<Item = &'a Post>,
) -> Result<HashMap<String, Vec<String>>, Error> {
    let posts: Vec<_> = posts.into_iter().collect();
    let object_type: String = db.query_row(
        "SELECT type FROM sqlite_master WHERE name='content_warnings'",
        [],
        |row| row.get(0),
    )?;
    let mut warnings = HashMap::<String, Vec<String>>::new();
    if object_type == "view" {
        let members: Option<String> = db
            .query_row(
                "SELECT members_json FROM moderation_lists WHERE identifier='spam'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let members: HashSet<String> = members
            .map(|members| serde_json::from_str(&members))
            .transpose()?
            .unwrap_or_default();
        for post in &posts {
            if members.contains(&post.author_id) {
                warnings
                    .entry(post.source_id.clone())
                    .or_default()
                    .push("auto-flagged: spam Primal snapshot".into());
            }
        }
    } else {
        let mut statement =
            db.prepare("SELECT event_id,reason FROM content_warnings ORDER BY category")?;
        for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))? {
            let (event_id, reason) = row?;
            warnings.entry(event_id).or_default().push(reason);
        }
    }
    // Batch NIP-36 and duplicate detection; per-card scans dominated HTTP latency. -- Pi/gpt-5.6-sol
    let mut statement = db.prepare(
        "SELECT lower(hex(event.id)),coalesce(json_extract(tag.value,'$[1]'),'')
         FROM events event,json_each(event.tags) tag
         WHERE event.kind=1 AND json_extract(tag.value,'$[0]')='content-warning'",
    )?;
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (event_id, reason) = row?;
        warnings
            .entry(event_id)
            .or_default()
            .push(format!("author: {reason}"));
    }
    let mut statement = db.prepare(
        "SELECT lower(hex(pubkey)),content
         FROM events
         WHERE kind=1 AND created_at>=unixepoch()-?1
         GROUP BY pubkey,content
         HAVING count(*)>1",
    )?;
    let duplicates: HashSet<(String, String)> = statement
        .query_map([queries::WINDOW], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<_, _>>()?;
    for post in posts {
        let reasons = warnings.entry(post.source_id.clone()).or_default();
        for label in crate::policy::text_labels(&post.text) {
            if !reasons.contains(&label) {
                reasons.push(label);
            }
        }
        if duplicates.contains(&(post.author_id.clone(), post.text.clone()))
            && !reasons
                .iter()
                .any(|reason| reason.contains("duplicate-content"))
        {
            reasons.push("auto-flagged: spam duplicate-content".into());
        }
    }
    Ok(warnings)
}

fn warnings(db: &Connection, p: &Post, initial: Option<&[String]>) -> Result<Vec<String>, Error> {
    if let Some(reasons) = initial {
        return Ok(reasons.to_vec());
    }
    let warnings = warning_map(db, std::iter::once(p))?;
    Ok(warnings
        .get(&p.source_id)
        .into_iter()
        .chain(warnings.get(&p.canonical_id))
        .flatten()
        .cloned()
        .collect())
}
fn hides(reasons: &[String]) -> bool {
    reasons.iter().any(|r| {
        ["author:", "auto-flagged: explicit", "curated-nsfw:"]
            .iter()
            .any(|prefix| r.starts_with(prefix))
    })
}

fn parent_excerpt(parent: &Post, author: &str, reasons: &[String]) -> Value {
    let text = if hides(reasons) {
        "content warning".to_string()
    } else {
        let html = body(&parent.text);
        let stripped = regex::Regex::new("<[^>]+>")
            .unwrap()
            .replace_all(&html, " ");
        html_escape::decode_html_entities(&stripped)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let excerpt = if text.chars().count() > 160 {
        format!("{}…", text.chars().take(159).collect::<String>().trim_end())
    } else {
        text
    };
    json!({"source_id":parent.source_id,"author":author,"text":excerpt})
}

/// Resolves feed parent excerpts with bounded batch scans instead of per-card queries.
pub fn parent_excerpt_map<'a>(
    db: &Connection,
    posts: impl IntoIterator<Item = &'a Post>,
    now: i64,
) -> Result<HashMap<String, Value>, Error> {
    let posts: Vec<_> = posts.into_iter().collect();
    let eligible = queries::eligible_map(db, now)?;
    let parents: Vec<_> = posts
        .iter()
        .filter_map(|post| post.parent_id.as_ref())
        .filter_map(|id| eligible.get(id))
        .collect();
    let identities = identity_map(db, parents.iter().copied())?;
    let parent_warnings = warning_map(db, parents.iter().copied())?;
    let values = posts
        .into_iter()
        .map(|post| {
            let value = post
                .parent_id
                .as_ref()
                .and_then(|id| eligible.get(id))
                .map(|parent| {
                    let reasons = parent_warnings
                        .get(&parent.source_id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    parent_excerpt(parent, &identities[&parent.canonical_id].0, reasons)
                })
                .unwrap_or(Value::Null);
            (post.canonical_id.clone(), value)
        })
        .collect();
    Ok(values)
}

/// Optional batch results used by feed cards; context pages use the direct query path.
#[derive(Default)]
pub struct CardCache<'a> {
    pub reply_count: Option<i64>,
    pub warnings: Option<&'a [String]>,
    pub identity: Option<&'a Identity>,
    pub parent: Option<&'a Value>,
}

pub fn card(
    db: &Connection,
    p: &Post,
    now: i64,
    mode: &str,
    with_parent: bool,
    cache: CardCache<'_>,
) -> Result<Value, Error> {
    let started = std::time::Instant::now();
    let (author, address, named, profile_url) = cache
        .identity
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| identity(db, p))?;
    let identity_at = started.elapsed();
    let reasons = warnings(db, p, cache.warnings)?;
    let warnings_at = started.elapsed();
    let mut score = match mode {
        "relevance" => format!("match {:.1} (lower=closer)", p.bm25.unwrap()),
        "conversations" => format!("{} repliers (24h)", p.n_reply_authors),
        "discovery" => {
            if p.hops == 1 {
                "1 hop · you follow".into()
            } else {
                format!("{} hops · connection {:.2}", p.hops, 1.0 / p.mass.sqrt())
            }
        }
        _ => String::new(),
    };
    if ["conversations", "discovery"].contains(&mode) {
        if let Some(bm25) = p.bm25 {
            score.push_str(&format!(" · match {bm25:.1}"));
        }
    }
    let mut parent_excerpt_value = cache.parent.cloned().unwrap_or(Value::Null);
    if with_parent && cache.parent.is_none() {
        if let Some(parent) = p
            .parent_id
            .as_deref()
            .map(|id| queries::get(db, id, now))
            .transpose()?
            .flatten()
        {
            parent_excerpt_value = parent_excerpt(
                &parent,
                &identity(db, &parent)?.0,
                &warnings(db, &parent, None)?,
            );
        }
    }
    let parent_at = started.elapsed();
    let original_url = url::Url::parse(&p.url)
        .ok()
        .filter(|u| ["http", "https"].contains(&u.scheme()))
        .map(|_| p.url.clone());
    let reply_count = match cache.reply_count {
        Some(count) => count,
        None => queries::count(db, now, Some(&p.canonical_id))?,
    };
    let length = p.text.chars().count();
    let collapsed = length.saturating_sub(PREVIEW_CHARACTERS) >= MIN_COLLAPSED_REMAINDER;
    let full_html = body(&p.text);
    let preview_html = if collapsed {
        body(&p.text.chars().take(PREVIEW_CHARACTERS).collect::<String>())
    } else {
        String::new()
    };
    let body_at = started.elapsed();
    if body_at.as_millis() >= 100 {
        eprintln!(
            "Slow card id={} chars={} identity_ms={} warnings_ms={} parent_ms={} body_ms={}",
            p.source_id,
            length,
            identity_at.as_millis(),
            (warnings_at - identity_at).as_millis(),
            (parent_at - warnings_at).as_millis(),
            (body_at - parent_at).as_millis(),
        );
    }
    Ok(
        json!({"canonical_id":p.canonical_id,"source_id":p.source_id,
            "author_id":p.author_id,"author":author,"address":address,"named":named,"profile_url":profile_url,
            "original_url":original_url,"when":time(p.created_at),"age":age(p.created_at,now),
            "full_html":full_html,"preview_html":preview_html,
            "rest_chars":length.saturating_sub(PREVIEW_CHARACTERS),"warning":reasons.join(" · "),"warning_hides":hides(&reasons),
            "reply_count":reply_count,"parent_excerpt":parent_excerpt_value,"score":score,
            "score_title":if mode=="discovery" {"distance score: 1/distance² over each distinct first-hop account that endorses this author; lower ranks first"}else{""},
            "conversation":if mode=="conversations" {json!({"n_reply_authors":p.n_reply_authors,"n_replies":p.n_replies,
                "latest":age(p.latest_activity,now),"root_present":p.root_present,"root_id":p.root_id.as_ref().unwrap_or(&p.canonical_id)})}else{Value::Null}
        }),
    )
}
