use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use tower::ServiceExt;

const SCHEMA:&str="
CREATE TABLE events(id BLOB PRIMARY KEY,pubkey BLOB,kind INTEGER,created_at INTEGER,content TEXT,tags TEXT,sig BLOB);
CREATE INDEX idx_events_created_at ON events(created_at DESC);
CREATE TABLE event_tags(event_id BLOB,tag_name TEXT,tag_value TEXT,PRIMARY KEY(event_id,tag_name,tag_value));
CREATE TABLE social_edges(event_id BLOB,follower TEXT,followee TEXT,PRIMARY KEY(event_id,followee));
CREATE INDEX social_edges_follower ON social_edges(follower,followee);
CREATE INDEX social_edges_followee ON social_edges(followee,follower);
CREATE TRIGGER social_edge_insert AFTER INSERT ON event_tags WHEN new.tag_name='p' BEGIN
 INSERT OR IGNORE INTO social_edges SELECT e.id,lower(hex(e.pubkey)),new.tag_value FROM events e WHERE e.id=new.event_id AND e.kind=3;
END;
CREATE TABLE content_warnings(event_id TEXT,category TEXT,reason TEXT);
CREATE TABLE policy_exclusions(event_id TEXT,reason TEXT);
CREATE TABLE moderation_lists(source TEXT,identifier TEXT,event_id TEXT,event_created_at INTEGER,checked_at INTEGER,members_json TEXT);
CREATE TABLE moderation_refresh_attempts(source TEXT,identifier TEXT,attempted_at INTEGER,error TEXT);
CREATE TABLE post_store(canonical_id TEXT PRIMARY KEY,source_id TEXT,author_id TEXT,author_name TEXT,text TEXT,created_at INTEGER,url TEXT,parent_id TEXT,root_id TEXT);
CREATE INDEX posts_parent ON post_store(parent_id);
CREATE VIEW posts AS SELECT rowid,post.* FROM post_store post
WHERE NOT EXISTS(SELECT 1 FROM policy_exclusions exclusion WHERE exclusion.event_id=post.source_id)
AND post.author_id NOT IN(SELECT member.value FROM moderation_lists list,json_each(list.members_json) member WHERE list.identifier='nsfw' AND member.type='text');
CREATE VIEW reader_post_events AS SELECT post.rowid AS id,unhex(post.source_id) AS event_id,post.root_id,post.parent_id FROM post_store post
WHERE NOT EXISTS(SELECT 1 FROM policy_exclusions exclusion WHERE exclusion.event_id=post.source_id)
AND post.author_id NOT IN(SELECT member.value FROM moderation_lists list,json_each(list.members_json) member WHERE list.identifier='nsfw' AND member.type='text');
CREATE VIRTUAL TABLE posts_fts USING fts5(text,content='post_store',content_rowid='rowid',tokenize='porter unicode61');
CREATE TRIGGER posts_insert AFTER INSERT ON post_store BEGIN INSERT INTO posts_fts(rowid,text) VALUES(new.rowid,new.text); END;
CREATE TABLE source_status(source TEXT,updated_at INTEGER,detail TEXT);
CREATE TABLE collection_gaps(relay TEXT,since_at INTEGER,until_at INTEGER,reason TEXT,checked_at INTEGER);
";
struct Fixture {
    dir: tempfile::TempDir,
    db: Connection,
    now: i64,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("test.sqlite")).unwrap();
        db.execute_batch(SCHEMA).unwrap();
        Self {
            dir,
            db,
            now: Utc::now().timestamp(),
        }
    }
    fn app(&self) -> App {
        App {
            path: self.dir.path().join("test.sqlite"),
            root: key(1),
            templates: templates().unwrap(),
            embedding: None,
            embedding_queries: None,
            embedding_error: Arc::new(Mutex::new(None)),
            cached_minilm: None,
            default_embedding: "minilm".into(),
            collecting: false,
            reader_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            status_cache: Arc::new(RwLock::new(None)),
        }
    }
    fn post(
        &self,
        id: u32,
        author: u32,
        text: &str,
        age: i64,
        parent: Option<u32>,
        root: Option<u32>,
    ) {
        self.db
            .execute(
                "INSERT INTO post_store VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![
                    cid(id),
                    key(id),
                    key(author),
                    &key(author)[..16],
                    text,
                    self.now - age,
                    format!("https://njump.me/{}", key(id)),
                    parent.map(cid),
                    root.map(cid)
                ],
            )
            .unwrap();
    }
    fn sync_post_events(&self) {
        self.db
            .execute(
                "INSERT OR REPLACE INTO events
                 SELECT unhex(source_id),unhex(author_id),1,created_at,text,'[]',zeroblob(64)
                 FROM post_store",
                [],
            )
            .unwrap();
    }
    fn event(&self, author: u32, kind: i64, content: &str, tags: Value) {
        let event_id = key(author + kind as u32 + 10000);
        self.db.execute("INSERT OR REPLACE INTO events VALUES(unhex(?1),unhex(?2),?3,?4,?5,?6,zeroblob(64))",
            rusqlite::params![&event_id,key(author),kind,self.now-600,content,tags.to_string()]).unwrap();
        for tag in tags.as_array().unwrap() {
            if let (Some(name), Some(value)) = (tag[0].as_str(), tag[1].as_str()) {
                self.db
                    .execute(
                        "INSERT OR REPLACE INTO event_tags VALUES(unhex(?1),?2,?3)",
                        (&event_id, name, value),
                    )
                    .unwrap();
            }
        }
    }
    fn follows(&self, author: u32, followees: &[u32]) {
        self.event(
            author,
            3,
            "",
            json!(followees
                .iter()
                .map(|n| json!(["p", key(*n)]))
                .collect::<Vec<_>>()),
        );
    }
    async fn request(&self, path: &str) -> (StatusCode, String) {
        let response = router(self.app())
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        (
            response.status(),
            String::from_utf8(
                to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap(),
        )
    }
}
fn key(n: u32) -> String {
    format!("{n:064x}")
}
fn cid(n: u32) -> String {
    format!("nostr:{}", key(n))
}
fn ids(html: &str) -> Vec<String> {
    regex::Regex::new(r#"<article class="post" id="([^"]+)""#)
        .unwrap()
        .captures_iter(html)
        .map(|c| c[1].to_string())
        .collect()
}

#[test]
fn bounded_card_queries_match_full_window_results() {
    let fixture = Fixture::new();
    fixture.post(1, 11, "root excerpt text", 20, None, None);
    fixture.post(2, 12, "first reply", 10, Some(1), Some(1));
    fixture.post(3, 13, "unrelated", 5, None, None);
    let ids = vec![cid(1), cid(2)];
    let full = queries::eligible_map_for(&fixture.db, fixture.now, None).unwrap();
    let bounded = queries::eligible_map_for(&fixture.db, fixture.now, Some(&ids)).unwrap();
    assert_eq!(bounded.len(), 2);
    for id in &ids {
        assert_eq!(bounded[id].text, full[id].text);
    }
    let full_counts = queries::reply_counts_for(&fixture.db, fixture.now, None).unwrap();
    let bounded_counts = queries::reply_counts_for(&fixture.db, fixture.now, Some(&ids)).unwrap();
    assert_eq!(bounded_counts.get(&cid(1)), full_counts.get(&cid(1)));
    let excerpts =
        render::parent_excerpt_map(&fixture.db, [bounded.get(&cid(2)).unwrap()], fixture.now)
            .unwrap();
    assert!(excerpts[&cid(2)].to_string().contains("root excerpt text"));
}

#[test]
fn topic_selection_preserves_repeated_and_noise_ids() {
    let search = Search::parse(
        "mode=topics&clustering=dbscan&topic=2&topic=-1&topic=2",
        1_000,
        "minilm",
    );
    assert_eq!(search.topics, vec![2]);
    assert!(search.include_unsorted);
    let query = search.query_string();
    assert!(query.contains("clustering=dbscan"));
    assert!(query.contains("unsorted=true"));
    assert!(query.contains("topics=2"));

    let repeated = Search::parse(
        "mode=topics&embedding=titan&clustering=dbscan&topics=3&topics=1&topics=3&unsorted=true&page=2&hide_spam=true",
        1_000,
        "minilm",
    );
    assert_eq!(repeated.topics, vec![1, 3]);
    assert!(repeated.include_unsorted);
    assert!(repeated.hide_flagged_spam);
    assert_eq!(repeated.page, 2);
    let query = repeated.query_string();
    assert!(query.contains("embedding=titan"));
    assert!(query.contains("clustering=dbscan"));
    assert_eq!(query.matches("topics=").count(), 2);
    assert!(query.contains("topics=1") && query.contains("topics=3"));
    assert!(query.contains("unsorted=true"));
    assert!(query.contains("hide_spam=true"));
    assert!(repeated.show_flagged_query_string().contains("page=2"));
    assert!(repeated
        .show_flagged_query_string()
        .contains("show_spam=true"));
    assert!(!repeated.show_flagged_query_string().contains("hide_spam"));
    assert!(Search::parse("mode=new", 1_000, "minilm").hide_flagged_spam);
    let shown = Search::parse("mode=new&show_spam=true", 1_000, "minilm");
    assert!(!shown.hide_flagged_spam);
    assert!(shown.query_string().contains("show_spam=true"));
    assert!(!Search::parse("mode=new&hide_spam=false", 1_000, "minilm").hide_flagged_spam);
    assert_eq!(
        Search::parse("q=vector&search=meaning", 1_000, "minilm").mode,
        "meaning"
    );
    assert_eq!(
        Search::parse("q=vector&search=relevance", 1_000, "minilm").mode,
        "relevance"
    );
}

#[test]
fn similar_replies_keep_one_per_parent_and_keep_top_level_posts_distinct() {
    let same_parent = Some("nostr:quoted".to_string());
    let candidates = vec![
        ("same-1".into(), same_parent.clone()),
        ("same-2".into(), same_parent.clone()),
        ("same-3".into(), same_parent.clone()),
        ("same-4".into(), same_parent.clone()),
        ("same-5".into(), same_parent),
        ("other-parent".into(), Some("nostr:other".into())),
        ("top-level-1".into(), None),
        ("top-level-2".into(), None),
    ];
    assert_eq!(
        diverse_related_ids(candidates, 5),
        vec!["same-1", "other-parent", "top-level-1", "top-level-2"]
    );
}

#[tokio::test]
async fn pages_query_state_and_invalid_requests_use_real_handlers() {
    let f = Fixture::new();
    for i in 0..105 {
        f.post(
            100 + i,
            2,
            "literal NOT activation steering",
            100 + i as i64,
            None,
            None,
        );
    }
    f.post(300, 2, "expired", queries::WINDOW + 2, None, None);
    f.post(301, 2, "future", -86400, None, None);
    let (code, html) = f.request("/?mode=new").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(ids(&html).len(), 100);
    assert_eq!(ids(&html)[0], cid(100));
    assert_eq!(ids(&f.request("/?mode=relevance").await.1), ids(&html));
    assert!(html.contains("<legend>View</legend>"));
    assert!(html.contains("<select id=\"mode-select\" name=\"mode\" data-submit>"));
    assert!(!html.contains("type=\"radio\" name=\"mode\""));
    assert!(html.contains("<legend>Search</legend>"));
    assert!(html.contains("placeholder='words or"));
    assert!(html.contains("aria-label=\"Search posts\""));
    assert!(html.contains("name=\"search\" value=\"relevance\" aria-pressed=\"true\""));
    assert!(html.contains("name=\"search\" value=\"meaning\" aria-pressed=\"false\" disabled"));
    assert!(!html.contains("search-method"));
    assert!(html.contains("id=\"loading\" role=\"status\""));
    assert!(html.contains("<a class=\"help\" href=\"/about#reader-controls\">Help</a>"));
    assert!(!html.contains("<button type=\"button\" class=\"tipbtn\""));
    let (_, topic_controls) = f.request("/?mode=topics").await;
    let embedding = topic_controls.find("id=\"embedding-select\"").unwrap();
    let grouping = topic_controls.find("id=\"clustering-select\"").unwrap();
    let picker = topic_controls.find("<strong>Topics</strong>").unwrap();
    assert!(embedding < grouping && grouping < picker);
    assert!(topic_controls.contains("Topics</strong> — All topics"));
    let (_, second) = f.request("/?mode=new&page=1").await;
    assert_eq!(ids(&second), (200..205).map(cid).collect::<Vec<_>>());
    assert!(!second.contains("older &rarr;"));
    assert_eq!(ids(&f.request("/?mode=new&page=-2").await.1), ids(&html));
    assert_eq!(
        ids(&f.request("/?mode=new&page=invalid").await.1),
        ids(&html)
    );
    for q in ["%22unclosed", "!!!", "%22%22"] {
        let (code, html) = f.request(&format!("/?q={q}")).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(html.contains("Search:"));
        assert!(ids(&html).is_empty());
        assert!(!html.contains("aria-label=\"pagination\""));
    }
    assert_eq!(ids(&f.request("/?q=NOT").await.1).len(), 100);
    for mode in ["new", "relevance", "conversations", "discovery"] {
        assert!(ids(&f.request(&format!("/?q=expired&mode={mode}")).await.1).is_empty());
    }
    assert_eq!(
        f.request(&format!("/context/nostr/{}", key(300))).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(ids(&f.request("/?q=activation+OR+steering").await.1).is_empty());
    let (_, html) = f
        .request("/?q=activation&mode=discovery&go=1&reach=3&order=connections")
        .await;
    assert!(html.contains("value=\"relevance\"") && html.contains("match "));
    assert!(!html.contains("name=\"reach\"") && !html.contains("go=1&amp;"));
    let (_, old) = f
        .request(&format!("/?mode=new&before={}", f.now - 150))
        .await;
    assert_eq!(ids(&old), (150..205).map(cid).collect::<Vec<_>>());
    assert_eq!(
        f.request("/context/nostr/missing").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(f.request("/not-a-route").await.0, StatusCode::NOT_FOUND);
    let redirect = router(f.app())
        .oneshot(
            Request::builder()
                .uri("/search?q=one&page=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(redirect.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(redirect.headers()["location"], "/?q=one&page=2");
}

#[tokio::test]
async fn conversation_context_counts_cycles_and_warning_excerpts() {
    let f = Fixture::new();
    f.post(100, 2, "root", 2000, None, None);
    f.post(101, 3, "reply alpha", 300, Some(100), Some(100));
    f.post(102, 3, "repeated author", 200, Some(101), Some(100));
    f.post(103, 4, "reply beta", 100, Some(101), Some(100));
    f.post(
        104,
        2,
        "root author does not inflate score",
        50,
        Some(100),
        Some(100),
    );
    f.post(200, 3, "missing root reply", 40, Some(999), Some(999));
    f.post(
        300,
        2,
        "root belongs to another thread",
        500,
        None,
        Some(998),
    );
    f.post(
        301,
        2,
        "provisional same-author reply",
        300,
        Some(300),
        Some(300),
    );
    f.sync_post_events();
    let (_, html) = f.request("/?mode=conversations").await;
    assert_eq!(ids(&html)[0], cid(104));
    assert!(html.contains("2 repliers (24h)"));
    assert!(html.contains("first post in this thread is not stored"));
    let (_, provisional) = f.request("/?mode=conversations&q=provisional").await;
    assert!(provisional.contains("1 repliers (24h)"));
    assert!(provisional.contains("first post in this thread is not stored"));
    let (_, html) = f.request("/?mode=conversations&q=alpha").await;
    assert_eq!(ids(&html), vec![cid(101)]);
    assert!(html.contains("reply alpha"));
    assert!(html.contains("2 repliers (24h)"));
    let (_, html) = f.request(&format!("/context/nostr/{}", key(101))).await;
    assert_eq!(ids(&html), [100, 101, 102, 103].map(cid));
    assert!(html.contains("2 replies"));
    f.db.execute(
        "INSERT INTO content_warnings VALUES(?1,'spam','auto-flagged: spam duplicate-content')",
        [key(103)],
    )
    .unwrap();
    let (_, filtered) = f
        .request(&format!(
            "/context/nostr/{}?embedding=minilm&hide_spam=true",
            key(101)
        ))
        .await;
    assert_eq!(ids(&filtered), [100, 101, 102].map(cid));
    assert!(filtered.contains("Show flagged spam"));
    assert!(filtered.contains("hide_spam=true"));
    let (_, html) = f.request(&format!("/context/nostr/{}", key(200))).await;
    assert!(html.contains("parent post is not available"));
    f.db.execute(
        "INSERT INTO content_warnings VALUES(?1,'cw','author: sensitive')",
        [key(100)],
    )
    .unwrap();
    let (_, html) = f.request("/?q=alpha").await;
    assert!(html.contains(": content warning</a>"));
    f.post(500, 2, "cycle one", 100, Some(501), None);
    f.post(501, 3, "cycle two", 90, Some(500), None);
    let (_, html) = f.request(&format!("/context/nostr/{}", key(500))).await;
    assert!(html.contains("reply loop was left out"));
    assert_eq!(ids(&html).len(), 2);
}

#[tokio::test]
async fn social_uses_canonical_graph_shortest_independent_paths_and_reach() {
    let f = Fixture::new();
    f.follows(1, &[2, 3, 4, 5, 6, 7]);
    for n in 2..=7 {
        f.follows(n, &[20, 20]);
    }
    f.follows(20, &[21, 2, 1, 20]);
    f.event(30, 3, "", json!([]));
    f.post(100, 2, "direct query", 400, None, None);
    f.post(101, 20, "popular query", 300, None, None);
    f.post(102, 21, "three hop query", 200, None, None);
    f.post(103, 99, "outsider query", 100, None, None);
    let (_, one) = f.request("/?mode=discovery&reach=1").await;
    assert_eq!(ids(&one), vec![cid(100)]);
    let (_, two) = f.request("/?mode=discovery&reach=2").await;
    assert_eq!(ids(&two), [101, 100].map(cid));
    let (_, three) = f.request("/?mode=discovery&reach=3").await;
    assert_eq!(ids(&three), [102, 101, 100].map(cid));
    let (_, connections) = f
        .request("/?mode=discovery&reach=3&order=connections&q=query")
        .await;
    assert_eq!(ids(&connections), [100, 101, 102].map(cid));
    assert!(connections.contains("connection 0.82"));
    assert!(connections.contains("name=\"reach\" value=\"3\""));
    assert!(connections.contains("name=\"order\" value=\"connections\""));
    assert!(ids(&f.request("/?mode=discovery&q=absent").await.1).is_empty());
    // Direct path to 21 replaces that endorser's longer path, not an extra vote. -- Pi/gpt-6-astra
    f.follows(2, &[20, 21, 2, 1]);
    let search = Search::parse("mode=discovery&reach=3&order=connections", f.now, "minilm");
    let rows = queries::feed(&f.db, &search, &key(1), f.now).unwrap();
    assert!(
        (rows.iter().find(|p| p.author_id == key(21)).unwrap().mass - (0.25 + 5.0 / 9.0)).abs()
            < 1e-12
    );
}

#[tokio::test]
async fn rendering_preserves_safe_text_profiles_warnings_and_exclusions() {
    let f = Fixture::new();
    let text=format!("#hashtag\n\n**strong** https://example.com/path.\n\n![description](https://tracker.invalid/pixel)\n\n<script>alert(1)</script><img src=https://tracker.invalid/raw>\n\n[bad](javascript:alert(2))\n\n`https://code.invalid`\n\n{} tailneedle","x".repeat(400));
    let text = format!(
        "{text}\n\n![outer ![inner](https://tracker.invalid/inner)](https://tracker.invalid/outer)"
    );
    f.post(
        99,
        3,
        "Alert: nostr:quoted AI-operated account",
        200,
        None,
        None,
    );
    f.post(100, 2, &text, 100, Some(99), None);
    f.event(
        2,
        0,
        r#"{"display_name":"<b>Named</b>","nip05":"_@example.com"}"#,
        json!([]),
    );
    let (_, html) = f.request("/?q=tailneedle").await;
    assert_eq!(ids(&html), vec![cid(100)]);
    let post_start = html.find(&format!("id=\"{}\"", cid(100))).unwrap();
    let post_html = &html[post_start..html[post_start..].find("</article>").unwrap() + post_start];
    assert!(post_html.contains("&lt;b&gt;Named&lt;&#x2f;b&gt;"));
    assert!(post_html.contains("@example.com"));
    assert!(html_escape::decode_html_entities(post_html).contains("href=\"https://njump.me/npub1"));
    assert!(post_html.contains("<strong>strong</strong>"));
    assert!(post_html.contains("href=\"https://example.com/path\""));
    assert!(post_html.contains("[image: description]"));
    for forbidden in [
        "<script",
        "<img",
        "tracker.invalid",
        "javascript:",
        "alert(1)",
        "<h1>hashtag",
    ] {
        assert!(!post_html.contains(forbidden), "{forbidden}");
    }
    assert!(post_html.contains("<code>https://code.invalid</code>"));
    assert!(post_html.contains("class=\"rest\""));

    let boundary = |marker: &str, length: usize| {
        format!(
            "{marker} é{}",
            "x".repeat(length - marker.chars().count() - 2)
        )
    };
    f.post(103, 3, &boundary("boundary149", 429), 100, None, None);
    let (_, short_html) = f.request("/?q=boundary149").await;
    assert!(!short_html.contains("class=\"rest\""));
    assert!(html_escape::decode_html_entities(&short_html).contains('é'));
    f.post(104, 3, &boundary("boundary150", 430), 100, None, None);
    let (_, collapsed_html) = f.request("/?q=boundary150").await;
    assert!(collapsed_html.contains("class=\"rest\""));
    assert!(collapsed_html.contains("150 more characters"));

    f.post(105, 3, "tailneedle ordinary post", 50, None, None);
    f.db.execute(
        "INSERT INTO content_warnings VALUES(?1,'spam','auto-flagged: spam link-farm')",
        [key(100)],
    )
    .unwrap();
    let (_, hidden_by_default) = f.request("/?q=tailneedle").await;
    assert!(!hidden_by_default.contains(&format!("id=\"{}\"", cid(100))));
    assert!(hidden_by_default.contains(&format!("id=\"{}\"", cid(105))));
    assert!(hidden_by_default.contains("Show flagged spam"));
    let (_, html) = f.request("/?q=tailneedle&show_spam=true").await;
    let flagged_start = html.find(&format!("id=\"{}\"", cid(100))).unwrap();
    let flagged =
        &html[flagged_start..html[flagged_start..].find("</article>").unwrap() + flagged_start];
    assert!(flagged.contains("<aside class=\"spam-flag\">Flagged spam: link farm</aside>"));
    assert!(flagged.contains("<span class=\"relation-label\">In reply to:</span>"));
    assert!(flagged.contains("Alert: nostr:quoted AI-operated account"));
    assert!(
        flagged.find("Flagged spam:").unwrap() < flagged.find("<strong>strong</strong>").unwrap()
    );
    assert!(!flagged.contains("auto-flagged"));
    assert!(!html.contains("class=\"warning\""));
    let flagged_post_before =
        f.db.query_row(
            "SELECT canonical_id,source_id,text FROM post_store WHERE canonical_id=?1",
            [cid(100)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    let stored_before =
        f.db.query_row("SELECT count(*) FROM post_store", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    let events_before =
        f.db.query_row("SELECT count(*) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    let (_, filtered) = f.request("/?q=tailneedle&hide_spam=true").await;
    assert!(!filtered.contains(&format!("id=\"{}\"", cid(100))));
    assert!(filtered.contains(&format!("id=\"{}\"", cid(105))));
    assert!(filtered.contains("Show flagged spam"));
    let (_, paged) = f
        .request("/?q=tailneedle&mode=new&page=2&hide_spam=true")
        .await;
    assert!(paged.contains("q=tailneedle&amp;mode=new&amp;page=2&amp;"));
    assert!(paged.contains("Show flagged spam"));
    assert_eq!(
        f.db.query_row("SELECT count(*) FROM post_store", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        stored_before
    );
    assert_eq!(
        f.db.query_row("SELECT count(*) FROM events", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        events_before
    );
    assert_eq!(
        f.db.query_row(
            "SELECT canonical_id,source_id,text FROM post_store WHERE canonical_id=?1",
            [cid(100)],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?
            )),
        )
        .unwrap(),
        flagged_post_before
    );
    f.db.execute(
        "INSERT INTO events VALUES(unhex(?1),unhex(?2),1,?3,'',?4,zeroblob(64))",
        rusqlite::params![
            key(100),
            key(2),
            f.now - 100,
            json!([["content-warning", "sensitive"]]).to_string()
        ],
    )
    .unwrap();
    assert!(f
        .request("/?q=tailneedle&show_spam=true")
        .await
        .1
        .contains("class=\"warning\""));
    f.db.execute(
        "INSERT INTO moderation_lists VALUES('primal','nsfw','list',?1,?1,?2)",
        rusqlite::params![f.now, json!([key(2)]).to_string()],
    )
    .unwrap();
    f.post(102, 3, "benign same-topic tailneedle", 100, None, None);
    assert_eq!(
        ids(&f.request("/?q=tailneedle").await.1),
        [105, 102].map(cid)
    );
    assert_eq!(
        f.request(&format!("/context/nostr/{}", key(100))).await.0,
        StatusCode::NOT_FOUND
    );
    f.post(101, 3, "blocked", 100, None, None);
    f.db.execute(
        "INSERT INTO policy_exclusions VALUES(?1,'blocked')",
        [key(101)],
    )
    .unwrap();
    assert!(ids(&f.request("/?q=blocked").await.1).is_empty());
}

#[tokio::test]
async fn bounded_card_hydration_preserves_off_page_duplicates_and_parent_warnings() {
    let f = Fixture::new();
    f.post(100, 2, "needle duplicate", 0, None, None);
    for index in 0..100 {
        f.post(
            101 + index,
            3,
            &format!("needle filler {index}"),
            1 + i64::from(index),
            None,
            None,
        );
    }
    f.post(300, 2, "needle duplicate", 1_000, None, None);
    for (id, age) in [(100, 0), (300, 1_000)] {
        f.db.execute(
            "INSERT INTO events VALUES(unhex(?1),unhex(?2),1,?3,'needle duplicate','[]',zeroblob(64))",
            rusqlite::params![key(id), key(2), f.now - age],
        )
        .unwrap();
    }
    let (_, html) = f.request("/?mode=new&show_spam=true").await;
    assert_eq!(ids(&html).len(), 100);
    assert!(ids(&html).contains(&cid(100)));
    assert!(!ids(&html).contains(&cid(300)));
    assert!(html.contains("Flagged spam: duplicate content"));

    f.post(400, 4, "parent body", 100, None, None);
    f.db.execute(
        "INSERT INTO events VALUES(unhex(?1),unhex(?2),1,?3,'parent body',?4,zeroblob(64))",
        rusqlite::params![
            key(400),
            key(4),
            f.now - 100,
            json!([["content-warning", "sensitive"]]).to_string()
        ],
    )
    .unwrap();
    f.post(401, 5, "parentneedle child", 0, Some(400), Some(400));
    let (_, html) = f.request("/?q=parentneedle").await;
    assert!(html.contains("content warning"));
    assert!(!html.contains("parent body"));
}

#[tokio::test]
async fn status_exposes_gaps_signed_list_age_and_reader_scope() {
    let f = Fixture::new();
    f.post(100, 2, "eligible", 100, None, None);
    f.db.execute(
        "INSERT INTO source_status VALUES('wss://relay',?1,'{\"admitted\":1}')",
        [f.now],
    )
    .unwrap();
    f.db.execute(
        "INSERT INTO collection_gaps VALUES('wss://relay',?1,?2,'same-second cap',?2)",
        [f.now - 600, f.now],
    )
    .unwrap();
    f.db.execute(
        "INSERT INTO moderation_lists VALUES('primal','nsfw','signed-id',?1,?2,'[]')",
        [f.now - 86400, f.now],
    )
    .unwrap();
    f.db.execute(
        "INSERT INTO moderation_refresh_attempts VALUES('primal','nsfw',?1,'timeout')",
        [f.now],
    )
    .unwrap();
    f.db.execute(
        "INSERT INTO moderation_refresh_attempts VALUES('primal','spam',?1,'first refresh failed')",
        [f.now],
    )
    .unwrap();
    let (code, html) = f.request("/status").await;
    assert!(html.contains("first refresh failed"));
    assert!(html.contains("no signed list stored"));
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("1 post is available"));
    for text in [
        "same-second cap",
        "signed-id",
        "timeout",
        &render::time(f.now - 86400),
    ] {
        assert!(html.contains(text), "{text}");
    }
    assert_eq!(f.request("/about").await.0, StatusCode::OK);
    assert_eq!(f.request("/tos").await.0, StatusCode::OK);
}

#[tokio::test]
async fn status_snapshot_avoids_recomputing_on_request() {
    let f = Fixture::new();
    f.post(100, 2, "eligible", 100, None, None);
    let app = f.app();
    refresh_status_cache(&app, f.now).unwrap();
    f.db.execute(
        "INSERT INTO source_status VALUES('after-snapshot',?1,'{}')",
        [f.now],
    )
    .unwrap();
    app.status_cache
        .write()
        .unwrap()
        .as_mut()
        .unwrap()
        .refresh_error = true;
    let response = router(app)
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let html = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("1 post is available"));
    assert!(html.contains("Status updated"));
    assert!(html.contains("It may be out of date"));
    assert!(!html.contains("after-snapshot"));
}

#[tokio::test]
async fn reader_work_waits_for_one_of_two_slots() {
    let f = Fixture::new();
    let app = f.app();
    refresh_status_cache(&app, f.now).unwrap();
    let slots = app.reader_slots.clone();
    let first = slots.clone().acquire_owned().await.unwrap();
    let second = slots.clone().acquire_owned().await.unwrap();
    let mut response = Box::pin(
        router(app.clone()).oneshot(
            Request::builder()
                .uri("/about")
                .body(Body::empty())
                .unwrap(),
        ),
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut response)
            .await
            .is_err()
    );
    let status = tokio::time::timeout(
        Duration::from_millis(20),
        router(app).oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    drop(first);
    drop(second);
    assert_eq!(response.await.unwrap().status(), StatusCode::OK);
}

#[test]
fn conversation_page_cache_keeps_order_and_hydrates_policy() {
    let fixture = Fixture::new();
    fixture.post(1, 1, "root", 10, None, None);
    fixture.post(2, 2, "reply", 5, Some(1), Some(1));
    let search = Search::parse("mode=conversations", fixture.now, "minilm");
    let direct = queries::feed(&fixture.db, &search, &fixture.app().root, fixture.now).unwrap();
    queries::store_conversation_cache(
        &fixture.dir.path().join("test.sqlite"),
        fixture.now,
        &direct,
        None,
    )
    .unwrap();
    let cache = queries::conversation_cache(&fixture.db).unwrap().unwrap();
    assert_eq!(
        cache
            .rows
            .iter()
            .map(|row| &row.canonical_id)
            .collect::<Vec<_>>(),
        direct
            .iter()
            .map(|row| &row.canonical_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        serde_json::to_value(&cache.rows).unwrap()[0]
            .as_object()
            .unwrap()
            .len(),
        5
    );
    queries::store_conversation_cache(
        &fixture.dir.path().join("test.sqlite"),
        fixture.now + 1,
        &[],
        Some("test refresh failed"),
    )
    .unwrap();
    let failed = queries::conversation_cache(&fixture.db).unwrap().unwrap();
    assert_eq!(failed.generated_at, cache.generated_at);
    assert_eq!(failed.rows[0].canonical_id, cache.rows[0].canonical_id);
    assert_eq!(failed.last_error.as_deref(), Some("test refresh failed"));
    fixture
        .db
        .execute(
            "INSERT INTO policy_exclusions VALUES(?1,'test')",
            [&direct[0].source_id],
        )
        .unwrap();
    let hydrated = cached_conversations(&fixture.app(), &fixture.db, fixture.now)
        .unwrap()
        .unwrap();
    assert!(hydrated.is_empty());
}

#[test]
#[ignore = "requires saved matched-corpus fixtures"]
fn matched_reference_reader_outputs() {
    let path = PathBuf::from(std::env::var("MEATYBROTH_PARITY").unwrap());
    let expected: Value =
        serde_json::from_slice(&std::fs::read(path.join("expected.json")).unwrap()).unwrap();
    let app = App {
        path: path.join("canonical.sqlite"),
        root: expected["root"].as_str().unwrap().into(),
        templates: templates().unwrap(),
        embedding: None,
        embedding_queries: None,
        embedding_error: Arc::new(Mutex::new(None)),
        cached_minilm: None,
        default_embedding: "minilm".into(),
        collecting: false,
        reader_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        status_cache: Arc::new(RwLock::new(None)),
    };
    let mut observed = Vec::new();
    for case in expected["cases"].as_array().unwrap() {
        let (status, html) = handle(
            &app,
            case["path"].as_str().unwrap(),
            case["query"].as_str().unwrap(),
            expected["now"].as_i64().unwrap(),
        )
        .unwrap();
        let actual = ids(&html);
        let want: Vec<String> = serde_json::from_value(case["ids"].clone()).unwrap();
        observed.push(json!({"path":case["path"],"query":case["query"],"ids":actual,"expected":want,"equal":actual==want,"status":status.as_u16()}));
    }
    std::fs::write(
        path.join("observed.json"),
        serde_json::to_string_pretty(&observed).unwrap(),
    )
    .unwrap();
    for case in observed {
        assert_eq!(case["equal"], true, "{case}");
        assert_eq!(case["status"], 200);
    }
}
