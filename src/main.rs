use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Router,
};
use minijinja::{context, Environment};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Deserialize, Default)]
struct Search {
    #[serde(default)]
    q: String,
    #[serde(default)]
    page: u32,
    mode: Option<String>,
}

fn templates() -> Result<Environment<'static>, Error> {
    let mut env = Environment::new();
    for (name, source) in [
        ("base.html", include_str!("../templates/base.html")),
        ("feed.html", include_str!("../templates/feed.html")),
        ("_post.html", include_str!("../templates/_post.html")),
        (
            "text.html",
            "<p style=\"white-space:pre-wrap\">{{ text }}</p>",
        ),
    ] {
        env.add_template(name, source)?;
    }
    Ok(env)
}

fn render(path: &PathBuf, search: Search) -> Result<String, Error> {
    let mode = search.mode.as_deref().unwrap_or(if search.q.is_empty() {
        "recent"
    } else {
        "relevance"
    });
    if !["recent", "relevance"].contains(&mode) {
        return Err("This first Rust slice supports recent and lexical search only".into());
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let env = templates()?;
    let select = "SELECT p.canonical_id,p.author_name,p.author_id,p.text,p.created_at,
        (SELECT json_group_array(reason) FROM content_warnings WHERE event_id=p.canonical_id)";
    let from = if search.q.is_empty() {
        "FROM posts p WHERE p.created_at>=?1"
    } else {
        "FROM posts_fts JOIN posts p ON p.rowid=posts_fts.rowid WHERE p.created_at>=?1 AND posts_fts MATCH ?3"
    };
    let order = if !search.q.is_empty() && mode == "relevance" {
        "bm25(posts_fts),p.created_at DESC,p.canonical_id"
    } else {
        "p.created_at DESC,p.canonical_id"
    };
    let mut statement = db.prepare(&format!(
        "{select} {from} ORDER BY {order} LIMIT 51 OFFSET ?2"
    ))?;
    let offset = i64::from(search.page) * 50;
    let since = now - 30 * 86400;
    let params: Vec<&dyn rusqlite::ToSql> = if search.q.is_empty() {
        vec![&since, &offset]
    } else {
        vec![&since, &offset, &search.q]
    };
    let mut rows = statement.query(params.as_slice())?;
    let mut posts = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let author: String = row.get(1)?;
        let author_id: String = row.get(2)?;
        let text: String = row.get(3)?;
        let created: i64 = row.get(4)?;
        let warnings: Vec<String> = serde_json::from_str(&row.get::<_, String>(5)?)?;
        let full_html = env
            .get_template("text.html")?
            .render(context!(text => text))?;
        let preview: String = text.chars().take(280).collect();
        let rest_chars = text.chars().count().saturating_sub(280);
        let preview_html = if rest_chars > 0 {
            Some(
                env.get_template("text.html")?
                    .render(context!(text => preview))?,
            )
        } else {
            None
        };
        posts.push(json!({
            "canonical_id": id, "source": "nostr", "source_id": id,
            "author": author, "author_id": author_id,
            "profile_url": format!("https://njump.me/{author_id}"),
            "original_url": format!("https://njump.me/{id}"),
            "age": format!("{}h", (now-created).max(0)/3600), "when": created.to_string(),
            "named": author != author_id.chars().take(16).collect::<String>(),
            "full_html": full_html, "preview_html": preview_html, "rest_chars": rest_chars,
            "warning": warnings.join(", "), "warning_hides": !warnings.is_empty()
        }));
    }
    let has_next = posts.len() > 50;
    posts.truncate(50);
    let qs = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", &search.q)
        .append_pair("mode", mode)
        .finish();
    Ok(env.get_template("feed.html")?.render(context!(
        q => search.q, mode => mode, page => search.page, results => posts,
        modes => ["recent", "relevance"],
        mode_labels => json!({"recent":"Recent", "relevance":"Words"}),
        mode_explanations => json!({"recent":"Newest posts first", "relevance":"SQLite FTS5 BM25"}),
        now => now, has_next => has_next, qs_base => format!("{qs}&"),
        state_fields => Vec::<Value>::new(),
        error => "Rust-only reader prototype: collection, full ranking, Markdown and context routes are not connected yet. Database opened read-only."
    ))?)
}

async fn feed(
    State(path): State<Arc<PathBuf>>,
    Query(search): Query<Search>,
) -> Result<Html<String>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || render(&path, search))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Html)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let path = PathBuf::from(std::env::var("MEATYBROTH_DB")?);
    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let app = Router::new()
        .route("/", get(feed))
        .route(
            "/static/style.css",
            get(|| async {
                (
                    [("content-type", "text/css")],
                    include_str!("../static/style.css"),
                )
            }),
        )
        .fallback(|| async {
            (
                StatusCode::NOT_IMPLEMENTED,
                "Not implemented in the first Rust reader slice",
            )
        })
        .with_state(Arc::new(path));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8083").await?;
    eprintln!("Rust reader listening on http://localhost:8083");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_text_cannot_insert_html_or_remote_media() {
        let env = templates().unwrap();
        let html = env
            .get_template("text.html")
            .unwrap()
            .render(context!(
                text => "<script>alert(1)</script><img src=https://tracker.invalid/pixel>"
            ))
            .unwrap();
        assert!(!html.contains("<script"));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("tracker.invalid"));
    }
}
