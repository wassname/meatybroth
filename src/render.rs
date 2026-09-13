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

fn identity(db: &Connection, p: &Post) -> Result<(String, String, bool, String), Error> {
    let placeholder = p.author_name.trim();
    let hexish = (8..=64).contains(&placeholder.len())
        && placeholder
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    let mut name = if hexish { "" } else { placeholder }.to_string();
    let mut address = String::new();
    let content: Option<String> = db
        .query_row(
            "SELECT content FROM events WHERE pubkey=unhex(?1) AND kind=0
        ORDER BY created_at DESC,id ASC LIMIT 1",
            [&p.author_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(profile) = content.and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
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

fn warnings(db: &Connection, p: &Post) -> Result<Vec<String>, Error> {
    let mut stmt = db.prepare(
        "SELECT reason FROM content_warnings WHERE event_id IN (?1,?2) ORDER BY category",
    )?;
    let mut reasons = stmt
        .query_map([&p.source_id, &p.canonical_id], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    // NIP-36 remains effective even when the old projection omitted its warning row. -- Pi/gpt-6-astra
    let mut stmt = db.prepare(
        "SELECT coalesce(json_extract(t.value,'$[1]'),'') FROM events e,json_each(e.tags) t
        WHERE e.id=unhex(?1) AND json_extract(t.value,'$[0]')='content-warning'",
    )?;
    for reason in stmt.query_map([&p.source_id], |r| r.get::<_, String>(0))? {
        let label = format!("author: {}", reason?);
        if !reasons.iter().any(|r| r.starts_with("author:")) {
            reasons.push(label);
        }
    }
    Ok(reasons)
}
fn hides(reasons: &[String]) -> bool {
    reasons.iter().any(|r| {
        ["author:", "auto-flagged: explicit", "curated-nsfw:"]
            .iter()
            .any(|prefix| r.starts_with(prefix))
    })
}

pub fn card(
    db: &Connection,
    p: &Post,
    now: i64,
    mode: &str,
    with_parent: bool,
) -> Result<Value, Error> {
    let (author, address, named, profile_url) = identity(db, p)?;
    let reasons = warnings(db, p)?;
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
    let mut parent_excerpt = Value::Null;
    if with_parent {
        if let Some(parent) = p
            .parent_id
            .as_deref()
            .map(|id| queries::get(db, id, now))
            .transpose()?
            .flatten()
        {
            let text = if hides(&warnings(db, &parent)?) {
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
            parent_excerpt = json!({"source":parent.source,"source_id":parent.source_id,
                "author":identity(db,&parent)?.0,"text":excerpt});
        }
    }
    let original_url = url::Url::parse(&p.url)
        .ok()
        .filter(|u| ["http", "https"].contains(&u.scheme()))
        .map(|_| p.url.clone());
    let length = p.text.chars().count();
    Ok(
        json!({"canonical_id":p.canonical_id,"source":p.source,"source_id":p.source_id,
            "author_id":p.author_id,"author":author,"address":address,"named":named,"profile_url":profile_url,
            "original_url":original_url,"when":time(p.created_at),"age":age(p.created_at,now),
            "full_html":body(&p.text),"preview_html":if length>280 {body(&p.text.chars().take(280).collect::<String>())}else{String::new()},
            "rest_chars":length.saturating_sub(280),"warning":reasons.join(" · "),"warning_hides":hides(&reasons),
            "reply_count":queries::count(db,now,Some(&p.canonical_id))?,"parent_excerpt":parent_excerpt,"score":score,
            "score_title":if mode=="discovery" {"distance score: 1/distance² over each distinct first-hop account that endorses this author; lower ranks first"}else{""},
            "conversation":if mode=="conversations" {json!({"n_reply_authors":p.n_reply_authors,"n_replies":p.n_replies,
                "latest":age(p.latest_activity,now),"root_present":p.root_present,"root_id":p.root_id.as_ref().unwrap_or(&p.canonical_id)})}else{Value::Null}
        }),
    )
}
