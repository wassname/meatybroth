use super::*;
use axum::extract::ws::{Message, WebSocketUpgrade};
use nostr_sdk::prelude::{
    DatabaseEventStatus, Event, EventBuilder, Filter, FinalizeEvent, Keys, Kind, NostrDatabase,
    PublicKey, Tag, Timestamp,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

struct MockEmbedder {
    calls: AtomicUsize,
    space: embed::Space,
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            space: embed::Space::titan_v2(),
        }
    }
}

#[derive(Clone)]
struct FailingEmbedder {
    space: embed::Space,
    calls: Arc<AtomicUsize>,
}

impl embed::Transport for FailingEmbedder {
    fn space(&self) -> &embed::Space {
        &self.space
    }

    fn split(&self, text: &str) -> Result<Vec<String>, Error> {
        Ok(embed::titan_chunks(text))
    }

    fn embed<'a>(
        &'a self,
        _text: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<embed::Output, Error>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err("synthetic dispatch failure".into())
        })
    }

    fn concurrency(&self) -> usize {
        8
    }
}

#[derive(Clone)]
struct SlowEmbedder {
    space: embed::Space,
    calls: Arc<AtomicUsize>,
    release: Arc<tokio::sync::Notify>,
}

impl embed::Transport for SlowEmbedder {
    fn space(&self) -> &embed::Space {
        &self.space
    }

    fn split(&self, text: &str) -> Result<Vec<String>, Error> {
        Ok(embed::titan_chunks(text))
    }

    fn concurrency(&self) -> usize {
        8
    }

    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<embed::Output, Error>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.release.notified().await;
            let mut vector = vec![0.0; self.space.dimensions];
            vector[0] = 1.0;
            Ok(embed::Output {
                vector,
                input_tokens: i64::try_from(text.len()).unwrap(),
            })
        })
    }
}

impl embed::SemanticModel for MockEmbedder {
    fn vector_space(&self) -> &embed::Space {
        &self.space
    }

    fn embed_text(&self, _text: &str) -> Result<embed::Output, Error> {
        let mut vector = vec![0.0; self.space.dimensions];
        vector[0] = 1.0;
        Ok(embed::Output {
            vector,
            input_tokens: 1,
        })
    }
}

impl embed::Transport for MockEmbedder {
    fn space(&self) -> &embed::Space {
        &self.space
    }

    fn split(&self, text: &str) -> Result<Vec<String>, Error> {
        Ok(embed::titan_chunks(text))
    }

    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<embed::Output, Error>> + Send + 'a>,
    > {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let mut vector = vec![0.0; self.space.dimensions];
            vector[call % self.space.dimensions] = 1.0;
            Ok(embed::Output {
                vector,
                input_tokens: i64::try_from(text.len().max(1)).unwrap(),
            })
        })
    }
}

struct DeadlineBoundaryEmbedder {
    inner: MockEmbedder,
}

impl embed::Transport for DeadlineBoundaryEmbedder {
    fn space(&self) -> &embed::Space {
        &self.inner.space
    }

    fn split(&self, _text: &str) -> Result<Vec<String>, Error> {
        Ok(vec!["first".into(), "second".into()])
    }

    fn epoch_seconds(&self) -> u64 {
        if self.inner.calls.load(Ordering::SeqCst) == 0 {
            0
        } else {
            u64::MAX
        }
    }

    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<embed::Output, Error>> + Send + 'a>,
    > {
        self.inner.embed(text)
    }
}

fn signed(keys: &Keys, kind: u16, content: &str, time: u64, tags: Vec<Vec<&str>>) -> Event {
    EventBuilder::new(Kind::from(kind), content)
        .custom_created_at(Timestamp::from(time))
        .tags(tags.into_iter().map(|t| Tag::parse(t).unwrap()))
        .finalize(keys)
        .unwrap()
}
async fn relay_ordered(
    events: Vec<Value>,
    eose_first: Option<bool>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/",
        get(move |upgrade: WebSocketUpgrade| {
            let events = events.clone();
            async move {
                upgrade.on_upgrade(move |mut socket| async move {
                    while let Some(Ok(message)) = socket.recv().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let req: Value = serde_json::from_str(&text).unwrap();
                        if req[0] == "REQ" {
                            if eose_first == Some(true) {
                                socket
                                    .send(Message::Text(json!(["EOSE", req[1]]).to_string().into()))
                                    .await
                                    .unwrap();
                            }
                            for event in &events {
                                socket
                                    .send(Message::Text(
                                        json!(["EVENT", req[1], event]).to_string().into(),
                                    ))
                                    .await
                                    .unwrap();
                            }
                            if eose_first == Some(false) {
                                socket
                                    .send(Message::Text(json!(["EOSE", req[1]]).to_string().into()))
                                    .await
                                    .unwrap();
                            }
                        }
                    }
                })
            }
        }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, task)
}
async fn relay(events: Vec<Value>) -> (String, tokio::task::JoinHandle<()>) {
    relay_ordered(events, Some(false)).await
}
async fn html_with_embedding(
    path: &std::path::Path,
    uri: &str,
    embedding: Option<Arc<dyn embed::SemanticModel>>,
) -> (StatusCode, String) {
    html_with_models(path, uri, embedding, None, None).await
}

async fn html_with_models(
    path: &std::path::Path,
    uri: &str,
    embedding: Option<Arc<dyn embed::SemanticModel>>,
    embedding_transport: Option<Arc<dyn embed::Transport>>,
    embedding_budget: Option<embed::Budget>,
) -> (StatusCode, String) {
    let embedding_queries = if let Some(transport) = embedding_transport {
        let budget = embedding_budget.unwrap();
        let path = path.to_path_buf();
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<collect::EmbeddingQuery>(1);
        tokio::spawn(async move {
            while let Some(request) = receiver.recv().await {
                let result = embed::cache_query(
                    &path,
                    transport.as_ref(),
                    budget,
                    &request.query,
                    Utc::now().timestamp(),
                )
                .await
                .map(|_| ())
                .map_err(|error| error.to_string());
                let _ = request.reply.send(result);
            }
        });
        Some(sender)
    } else {
        None
    };
    let app = router(App {
        path: path.to_path_buf(),
        root: ROOT.into(),
        templates: templates().unwrap(),
        embedding,
        embedding_queries,
        embedding_error: Arc::new(Mutex::new(None)),
        cached_minilm: embed::space_by_backend(path, "minilm").unwrap(),
        default_embedding: "minilm".into(),
        collecting: false,
        reader_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        status_cache: Arc::new(RwLock::new(None)),
    });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    (
        response.status(),
        String::from_utf8(
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap(),
    )
}
async fn html(path: &std::path::Path, uri: &str) -> (StatusCode, String) {
    html_with_embedding(path, uri, None).await
}
fn count(db: &Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[tokio::test]
async fn relay_budget_expires_only_between_drained_policy_scans() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let policy = policy::Policy::new(Default::default(), Default::default());
    let (url, task) = relay(Vec::new()).await;
    let client = collect::client(sdk.clone(), Arc::new(policy.clone()));
    client.add_relay(&url).await.unwrap();
    client.connect().await;
    let result = collect::collect_relay(
        &path,
        &client,
        &policy,
        &sdk,
        &url,
        &mut std::collections::BTreeSet::new(),
        tokio::time::Instant::now(),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(policy.active_request_count(), 0);
    task.abort();
}

#[tokio::test]
async fn timed_out_recent_scan_keeps_arrivals_live_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let note = signed(
        &Keys::generate(),
        1,
        "retained partial recent arrival",
        Timestamp::now().as_secs(),
        vec![],
    );
    let (url, task) =
        relay_ordered(vec![serde_json::from_str(&note.as_json()).unwrap()], None).await;
    let policy = policy::Policy::new(Default::default(), Default::default());
    let client = collect::client(sdk.clone(), Arc::new(policy.clone()));
    client.add_relay(&url).await.unwrap();
    client.connect().await;
    let result = collect::collect_relay(
        &path,
        &client,
        &policy,
        &sdk,
        &url,
        &mut std::collections::BTreeSet::new(),
        tokio::time::Instant::now() + Duration::from_secs(30),
    )
    .await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("partial arrivals retained"));
    assert_eq!(policy.active_request_count(), 0);
    drop(sdk);
    let reopened = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        reopened
            .query_row(
                "SELECT live FROM embedding_admissions WHERE event_id=?1",
                [note.id.as_bytes()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    task.abort();
}

#[tokio::test]
async fn sdk_relay_to_atomic_fts_http_policy_and_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let author = Keys::generate();
    let other = Keys::generate();
    let bad = Keys::generate();
    let nsfw = Keys::generate();
    let primal = Keys::generate();
    let now = Timestamp::now().as_secs();
    let sdk = collect::open(&path, &primal.public_key().to_hex())
        .await
        .unwrap();
    let list = signed(
        &primal,
        30000,
        "",
        now,
        vec![
            vec!["d", "nsfw_list"],
            vec!["p", &nsfw.public_key().to_hex()],
        ],
    );
    let members = collect::list_members(&list, primal.public_key(), "nsfw_list").unwrap();
    sdk.save_event(&list).await.unwrap();
    let policy = Arc::new(policy::Policy::new(
        [bad.public_key().to_hex()].into(),
        members,
    ));
    let parent = signed(&author, 1, "parent anchorword", now - 200, vec![]);
    let note = signed(
        &author,
        1,
        "bridgeword local AI",
        now - 100,
        vec![
            vec!["e", "bad-id", "", "root"],
            vec!["e", &parent.id.to_hex(), "", "root"],
        ],
    );
    let duplicate = signed(&author, 1, "bridgeword local AI", now - 90, vec![]);
    let profile = signed(&author, 0, r#"{"name":"Old"}"#, now - 200, vec![]);
    let replacement = signed(
        &author,
        0,
        r#"{"display_name":"Bridge Name","nip05":"_@bridge.example"}"#,
        now - 100,
        vec![],
    );
    let secret = signed(
        &other,
        1,
        &format!("synthetic nsec1{}", "q".repeat(40)),
        now - 100,
        vec![],
    );
    let secret_profile = signed(
        &other,
        0,
        "{}",
        now - 100,
        vec![vec!["name", &format!("nsec1{}", "q".repeat(40))]],
    );
    let blocked = signed(&bad, 1, "blockedword", now - 100, vec![]);
    let hidden = signed(&nsfw, 1, "same-topic healthword", now - 100, vec![]);
    let benign = signed(&other, 1, "same-topic healthword", now - 90, vec![]);
    let cw = signed(
        &other,
        1,
        "warningword explicit sex scene",
        now - 90,
        vec![vec!["content-warning", "sensitive"]],
    );
    let expired = signed(
        &other,
        1,
        "expiredword",
        now - queries::WINDOW as u64 - 1,
        vec![],
    );
    let boundary = signed(
        &other,
        1,
        "boundaryword",
        now - queries::WINDOW as u64,
        vec![],
    );
    let wrong_filter = signed(&other, 42, "wrongkind", now - 100, vec![]);
    let mut forged: Value = serde_json::from_str(&note.as_json()).unwrap();
    forged["created_at"] = json!(now - queries::WINDOW as u64 - 9);
    forged["content"] = json!("tampered cached ID");
    let mut events: Vec<Value> = [
        &parent,
        &note,
        &note,
        &duplicate,
        &profile,
        &replacement,
        &secret,
        &secret_profile,
        &blocked,
        &hidden,
        &benign,
        &cw,
        &expired,
        &boundary,
        &wrong_filter,
    ]
    .iter()
    .map(|event| serde_json::from_str(&event.as_json()).unwrap())
    .collect();
    events.push(forged);
    let (url, task) = relay(events).await;
    let client = collect::client(sdk.clone(), policy.clone());
    client.add_relay(&url).await.unwrap();
    client.connect().await;
    let observed = collect::scan(
        &client,
        &policy,
        &url,
        Filter::new()
            .kinds([Kind::Metadata, Kind::TextNote])
            .since(Timestamp::from(now - queries::WINDOW as u64 - 10))
            .until(Timestamp::from(now)),
        Duration::from_secs(3),
    )
    .await
    .unwrap();
    assert!(observed.eose);
    assert_eq!(observed.rejected, 4);
    assert!(!observed.ids.contains(&wrong_filter.id));
    assert_eq!(observed.oldest, Some(expired.created_at));
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(count(&db, "SELECT count(*) FROM events WHERE kind=0"), 1);
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM posts WHERE text='bridgeword local AI'"
        ),
        2
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM posts_fts WHERE posts_fts MATCH 'bridgeword'"
        ),
        2
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM events WHERE content LIKE '%nsec1%' OR content='blockedword'"
        ),
        0
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM reader_events"),
        count(&db, "SELECT count(*) FROM events")
    );
    assert_eq!(
        db.query_row(
            "SELECT root_id FROM posts WHERE source_id=?1",
            [note.id.to_hex()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        format!("nostr:{}", parent.id)
    );
    let (code, body) = html(&path, "/?q=bridgeword").await;
    assert_eq!(code, StatusCode::OK);
    for value in ["Bridge Name", "@bridge.example", "duplicate-content"] {
        assert!(body.contains(value), "{value}");
    }
    let (_, body) = html(&path, &format!("/context/nostr/{}", note.id)).await;
    assert!(body.contains("parent anchorword"));
    assert!(!body.contains("bad-id"));
    let (_, body) = html(&path, "/?q=warningword").await;
    assert!(body.contains("class=\"warning\""));
    assert!(body.contains("auto-flagged: explicit"));
    assert_eq!(queries::count(&db, now as i64, None).unwrap(), 6);
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM posts_fts WHERE posts_fts MATCH 'healthword'"
        ),
        1
    );
    collect::prune(&sdk, &policy, now).await.unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM posts_fts WHERE posts_fts MATCH 'expiredword'"
        ),
        0
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM posts_fts WHERE posts_fts MATCH 'boundaryword'"
        ),
        1
    );
    policy.blocked.write().unwrap().insert(note.id.to_hex());
    collect::prune(&sdk, &policy, now).await.unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM posts_fts WHERE posts_fts MATCH 'bridgeword'"
        ),
        1
    );
    let empty = signed(&primal, 30000, "", now + 1, vec![vec!["d", "nsfw_list"]]);
    assert!(
        collect::list_members(&empty, primal.public_key(), "nsfw_list")
            .unwrap()
            .is_empty()
    );
    sdk.save_event(&empty).await.unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT json_array_length(members_json) FROM moderation_lists WHERE identifier='nsfw'"
        ),
        0
    );
    let restrictive = signed(
        &primal,
        30000,
        "",
        now + 2,
        vec![
            vec!["d", "nsfw_list"],
            vec!["p", &author.public_key().to_hex()],
        ],
    );
    let refreshed = collect::list_members(&restrictive, primal.public_key(), "nsfw_list").unwrap();
    sdk.save_event(&restrictive).await.unwrap();
    collect::apply_nsfw(&sdk, &policy, refreshed, now + 2)
        .await
        .unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM posts_fts WHERE posts_fts MATCH 'bridgeword'"
        ),
        0
    );
    assert_eq!(count(&db,"SELECT count(*) FROM reader_events r LEFT JOIN events e ON e.id=r.event_id WHERE e.id IS NULL"),0);
    client.shutdown().await;
    task.abort();
    drop(db);
    drop(sdk);
    let reopened = collect::open(&path, &primal.public_key().to_hex())
        .await
        .unwrap();
    assert_eq!(
        reopened
            .count(Filter::new().kind(Kind::Metadata))
            .await
            .unwrap(),
        1
    );
    assert_eq!(html(&path, "/?q=bridgeword").await.0, StatusCode::OK);
    eprintln!("SDK→events→view/FTS→HTTP verified; forbidden=0, duplicate/replacement stable, policy/expiry delete indexes, restart retained");
}

#[tokio::test]
async fn expired_provider_deadline_makes_no_calls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let now = i64::try_from(Timestamp::now().as_secs()).unwrap();
    sdk.save_event(&signed(
        &Keys::generate(),
        1,
        "already expired",
        now as u64,
        vec![],
    ))
    .await
    .unwrap();
    let model = MockEmbedder::default();
    let budget = embed::Budget {
        total_nusd: i64::MAX,
        monthly_nusd: i64::MAX,
    };
    assert_eq!(
        embed::embed_pending_until(&path, &model, budget, Some(0), now, 1, false)
            .await
            .unwrap(),
        0
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn provider_deadline_stops_before_a_second_chunk_and_resumes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let now = i64::try_from(Timestamp::now().as_secs()).unwrap();
    let event = signed(&Keys::generate(), 1, "two chunks", now as u64, vec![]);
    sdk.save_event(&event).await.unwrap();
    let model = DeadlineBoundaryEmbedder {
        inner: MockEmbedder::default(),
    };
    let deadline = 1;
    let budget = embed::Budget {
        total_nusd: i64::MAX,
        monthly_nusd: i64::MAX,
    };
    assert_eq!(
        embed::embed_pending_until(&path, &model, budget, Some(deadline), now, 1, false)
            .await
            .unwrap(),
        0
    );
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM embedding_chunks"), 1);
    assert_eq!(count(&conn, "SELECT count(*) FROM post_embeddings"), 0);
    drop(conn);
    assert_eq!(
        embed::embed_pending_until(&path, &model, budget, None, now, 1, false)
            .await
            .unwrap(),
        1
    );
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM embedding_chunks"), 2);
    assert_eq!(count(&conn, "SELECT count(*) FROM post_embeddings"), 1);
    assert_eq!(model.inner.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn machine_presence_envelopes_stay_auditable_but_not_reader_or_embedding_eligible() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let now = i64::try_from(Timestamp::now().as_secs()).unwrap();
    let keys = Keys::generate();
    let presence = signed(
        &keys,
        1,
        r#"{"type":"presence","payload":"online"}"#,
        now as u64,
        vec![],
    );
    let zone = signed(
        &keys,
        1,
        r#"{"type":"zone_presence","zone":"zone-a","devicePk":"device","role":"gateway","metrics":{"clients":0},"ts":123,"ttl":120}"#,
        now as u64,
        vec![],
    );
    let price = signed(
        &keys,
        1,
        r#"{"type":"price","title":"XMR $535.76","body":"market update"}"#,
        now as u64,
        vec![],
    );
    let explained = signed(
        &keys,
        1,
        r#"Example payload: {"type":"presence","payload":"online"}"#,
        now as u64,
        vec![],
    );
    for event in [&presence, &zone, &price, &explained] {
        sdk.save_event(event).await.unwrap();
    }
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM events WHERE kind=1"), 4);
    assert_eq!(count(&conn, "SELECT count(*) FROM posts"), 2);
    assert!(
        crate::queries::get(&conn, &canonical_event_id(price.id.as_bytes()), now)
            .unwrap()
            .is_some()
    );
    assert!(
        crate::queries::get(&conn, &canonical_event_id(explained.id.as_bytes()), now)
            .unwrap()
            .is_some()
    );
    drop(conn);

    let mock = Arc::new(MockEmbedder::default());
    assert_eq!(
        embed::embed_pending(
            &path,
            mock.as_ref(),
            embed::Budget {
                total_nusd: i64::MAX,
                monthly_nusd: i64::MAX,
            },
            now,
            10,
        )
        .await
        .unwrap(),
        2
    );
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM post_embeddings"), 2);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM post_embeddings WHERE event_id IN (
               SELECT id FROM events
               WHERE CASE WHEN json_valid(content) THEN json_extract(content,'$.type') END
                 IN ('presence','zone_presence'))"
        ),
        0
    );

    // Reproduce a vector written by a pre-migration process and prove both HTTP and schema cleanup reject it. -- Pi/gpt-5.6-sol
    conn.execute(
        "INSERT INTO post_embeddings(event_id,space_id,dimensions,chunk_count,input_tokens,cost_nusd,vector,embedded_at)
         SELECT ?1,space_id,dimensions,chunk_count,input_tokens,cost_nusd,vector,embedded_at
         FROM post_embeddings WHERE event_id=?2",
        (presence.id.as_bytes(), price.id.as_bytes()),
    )
    .unwrap();
    drop(conn);
    let uri = format!(
        "/?mode=similar&similar=nostr:{}&embedding=minilm",
        presence.id.to_hex()
    );
    let (status, body) = html_with_embedding(&path, &uri, Some(mock.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("post is not available in this view"));

    let conn = Connection::open(&path).unwrap();
    crate::collect::cleanup_ineligible_derived(&conn).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM post_embeddings"), 2);
    assert_eq!(count(&conn, "SELECT count(*) FROM events WHERE kind=1"), 4);
}

#[tokio::test]
async fn reviewed_aepiot_campaign_keeps_canonical_events_and_ledger_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let now = Utc::now().timestamp();
    let target_keys = Keys::generate();
    let targeted = [
        signed(&target_keys, 1, "Visit AEPIOT today", now as u64, vec![]),
        signed(&target_keys, 1, "news from #AllGraph", now as u64, vec![]),
        signed(
            &target_keys,
            1,
            "headlines-world digest",
            now as u64,
            vec![],
        ),
    ];
    let same_author_benign = signed(
        &target_keys,
        1,
        &"A long benign research note without campaign markers. ".repeat(100),
        now as u64,
        vec![],
    );
    let other_author_marker = signed(
        &Keys::generate(),
        1,
        "aepiot allgraph headlines-world",
        now as u64,
        vec![],
    );
    for event in targeted
        .iter()
        .chain([&same_author_benign, &other_author_marker])
    {
        sdk.save_event(event).await.unwrap();
    }
    let mock = MockEmbedder::default();
    assert_eq!(
        embed::embed_pending(
            &path,
            &mock,
            embed::Budget {
                total_nusd: i64::MAX,
                monthly_nusd: i64::MAX,
            },
            now,
            10,
        )
        .await
        .unwrap(),
        5
    );

    let conn = Connection::open(&path).unwrap();
    let reviewed_author = "441d176ae740ef78b4b22129da2aea29aa2caf20dbf53bb8463ddd4fea90cf47";
    for event in targeted.iter().chain([&same_author_benign]) {
        conn.execute(
            "UPDATE events SET pubkey=unhex(?1) WHERE id=?2",
            (reviewed_author, event.id.as_bytes()),
        )
        .unwrap();
    }
    assert_eq!(count(&conn, "SELECT count(*) FROM events WHERE kind=1"), 5);
    assert_eq!(count(&conn, "SELECT count(*) FROM posts"), 2);
    collect::cleanup_ineligible_derived(&conn).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM events WHERE kind=1"), 5);
    assert_eq!(count(&conn, "SELECT count(*) FROM post_embeddings"), 2);
    assert_eq!(
        count(
            &conn,
            "SELECT count(DISTINCT event_id) FROM embedding_requests WHERE status='succeeded'"
        ),
        5
    );
    drop(conn);
    assert_eq!(
        embed::embed_pending(
            &path,
            &mock,
            embed::Budget {
                total_nusd: i64::MAX,
                monthly_nusd: i64::MAX,
            },
            now,
            10,
        )
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn transport_failure_disables_further_paid_calls_until_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let keys = Keys::generate();
    sdk.save_event(&signed(
        &keys,
        1,
        "provider failure fixture",
        Utc::now().timestamp() as u64,
        vec![],
    ))
    .await
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let transport: Arc<dyn embed::Transport> = Arc::new(FailingEmbedder {
        space: embed::Space::titan_v2(),
        calls: calls.clone(),
    });
    let (_sender, queries) = tokio::sync::mpsc::channel(1);
    let error = Arc::new(Mutex::new(None));
    let mut worker = collect::EmbeddingWorker {
        transport,
        budget: embed::Budget {
            total_nusd: 1_000_000,
            monthly_nusd: 1_000_000,
        },
        queries,
        error: error.clone(),
        disabled: false,
        validated: false,
        preflight_only: false,
        shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        topic_rebuild: None,
        topics_disabled: false,
    };
    let mut topics_dirty = false;
    collect::service_embeddings(
        &path,
        &mut worker,
        &mut topics_dirty,
        Duration::from_secs(1),
    )
    .await;
    assert!(worker.disabled);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(error
        .lock()
        .unwrap()
        .as_deref()
        .unwrap()
        .contains("synthetic dispatch failure"));
    collect::service_embeddings(
        &path,
        &mut worker,
        &mut topics_dirty,
        Duration::from_secs(1),
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn preflight_only_makes_exactly_one_successful_provider_call() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    sdk.save_event(&signed(
        &Keys::generate(),
        1,
        "successful preflight fixture",
        Utc::now().timestamp() as u64,
        vec![],
    ))
    .await
    .unwrap();
    let mock = Arc::new(MockEmbedder::default());
    let transport: Arc<dyn embed::Transport> = mock.clone();
    let (_sender, queries) = tokio::sync::mpsc::channel(1);
    let mut worker = collect::EmbeddingWorker {
        transport,
        budget: embed::Budget {
            total_nusd: 1_000_000,
            monthly_nusd: 1_000_000,
        },
        queries,
        error: Arc::new(Mutex::new(None)),
        disabled: false,
        validated: false,
        preflight_only: true,
        shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        topic_rebuild: None,
        topics_disabled: false,
    };
    let mut topics_dirty = false;
    collect::service_embeddings(
        &path,
        &mut worker,
        &mut topics_dirty,
        Duration::from_secs(1),
    )
    .await;
    assert!(worker.disabled);
    assert!(worker.validated);
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
    collect::service_embeddings(
        &path,
        &mut worker,
        &mut topics_dirty,
        Duration::from_secs(1),
    )
    .await;
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn shutdown_drains_eight_inflight_calls_before_returning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    for index in 0..9 {
        sdk.save_event(&signed(
            &Keys::generate(),
            1,
            &format!("slow provider fixture {index}"),
            Utc::now().timestamp() as u64,
            vec![],
        ))
        .await
        .unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Notify::new());
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let transport: Arc<dyn embed::Transport> = Arc::new(SlowEmbedder {
        space: embed::Space::titan_v2(),
        calls: calls.clone(),
        release: release.clone(),
    });
    let (_sender, queries) = tokio::sync::mpsc::channel(1);
    let worker = collect::EmbeddingWorker {
        transport,
        budget: embed::Budget {
            total_nusd: 2_000_000,
            monthly_nusd: 2_000_000,
        },
        queries,
        error: Arc::new(Mutex::new(None)),
        disabled: false,
        validated: true,
        preflight_only: false,
        shutdown: shutdown.clone(),
        topic_rebuild: None,
        topics_disabled: false,
    };
    let path_for_task = path.clone();
    let task = tokio::spawn(async move {
        let mut worker = worker;
        let mut topics_dirty = false;
        collect::service_embeddings(
            &path_for_task,
            &mut worker,
            &mut topics_dirty,
            Duration::from_secs(1),
        )
        .await;
        worker
    });
    let started = tokio::time::timeout(Duration::from_secs(5), async {
        while calls.load(Ordering::SeqCst) < 8 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(
        started.is_ok(),
        "started {} of 8 calls",
        calls.load(Ordering::SeqCst)
    );
    let inflight = Connection::open(&path).unwrap();
    assert_eq!(
        count(
            &inflight,
            "SELECT count(*) FROM embedding_requests WHERE status='reserved'"
        ),
        8
    );
    inflight.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
    drop(inflight);
    shutdown.store(true, Ordering::Release);
    assert!(!task.is_finished());
    release.notify_waiters();
    task.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 8);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM embedding_requests WHERE status='succeeded'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        8
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM embedding_requests WHERE status='reserved'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn eight_call_batch_drains_and_accounts_after_provider_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    for index in 0..8 {
        sdk.save_event(&signed(
            &Keys::generate(),
            1,
            &format!("failed provider fixture {index}"),
            Utc::now().timestamp() as u64,
            vec![],
        ))
        .await
        .unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let model = FailingEmbedder {
        space: embed::Space::titan_v2(),
        calls: calls.clone(),
    };
    let error = embed::embed_pending(
        &path,
        &model,
        embed::Budget {
            total_nusd: i64::MAX,
            monthly_nusd: i64::MAX,
        },
        Utc::now().timestamp(),
        8,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("synthetic dispatch failure"));
    assert_eq!(calls.load(Ordering::SeqCst), 8);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM embedding_requests WHERE status='uncertain'"
        ),
        8
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM embedding_requests WHERE status='reserved'"
        ),
        0
    );
}

#[tokio::test]
async fn continuous_embedding_prioritizes_admission_time_not_event_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let now = Utc::now().timestamp();
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let keys = Keys::generate();
    let newer_event = signed(&keys, 1, "newer signed event", now as u64, vec![]);
    let newly_admitted_old_event = signed(
        &keys,
        1,
        "old event admitted after backlog",
        (now - 10_000) as u64,
        vec![],
    );
    sdk.save_event(&newer_event).await.unwrap();
    sdk.save_event(&newly_admitted_old_event).await.unwrap();
    embed::mark_admissions(
        &path,
        &[newer_event.id.as_bytes().to_vec()],
        now - 100,
        false,
    )
    .unwrap();
    embed::mark_admissions(
        &path,
        &[newly_admitted_old_event.id.as_bytes().to_vec()],
        now,
        true,
    )
    .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT live FROM embedding_admissions WHERE event_id=?1",
            [newly_admitted_old_event.id.as_bytes()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    let mock = MockEmbedder::default();
    let budget = embed::Budget {
        total_nusd: 1_000_000,
        monthly_nusd: 1_000_000,
    };
    assert_eq!(
        embed::embed_recent_pending(&path, &mock, budget, now, 1)
            .await
            .unwrap(),
        1
    );
    assert!(
        embed::event_vector(&path, &mock.space, newly_admitted_old_event.id.as_bytes())
            .unwrap()
            .is_some()
    );
    assert!(
        embed::event_vector(&path, &mock.space, newer_event.id.as_bytes())
            .unwrap()
            .is_none()
    );
    let first_live = signed(&keys, 1, "first live admission", now as u64, vec![]);
    let second_live = signed(&keys, 1, "second live admission", now as u64, vec![]);
    sdk.save_event(&first_live).await.unwrap();
    sdk.save_event(&second_live).await.unwrap();
    embed::mark_admissions(&path, &[first_live.id.as_bytes().to_vec()], now + 1, true).unwrap();
    embed::mark_admissions(&path, &[second_live.id.as_bytes().to_vec()], now + 2, true).unwrap();
    embed::embed_recent_pending(&path, &mock, budget, now, 1)
        .await
        .unwrap();
    assert!(
        embed::event_vector(&path, &mock.space, first_live.id.as_bytes())
            .unwrap()
            .is_some()
    );
    assert!(
        embed::event_vector(&path, &mock.space, second_live.id.as_bytes())
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn dbscan_preserves_noise_cores_and_assigns_new_vectors_to_core_points() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let now = Utc::now().timestamp();
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let keys = Keys::generate();
    for index in 0..6 {
        sdk.save_event(&signed(
            &keys,
            1,
            &format!("DBSCAN fixture {index}"),
            (now - index) as u64,
            vec![],
        ))
        .await
        .unwrap();
    }
    let mock = MockEmbedder::default();
    let budget = embed::Budget {
        total_nusd: 1_000_000,
        monthly_nusd: 1_000_000,
    };
    assert_eq!(
        embed::embed_pending(&path, &mock, budget, now, 10)
            .await
            .unwrap(),
        6
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    let event_ids: Vec<Vec<u8>> = conn
        .prepare("SELECT event_id FROM post_embeddings ORDER BY event_id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (index, event_id) in event_ids.iter().enumerate() {
        let mut vector = vec![0.0_f32; mock.space.dimensions];
        vector[match index {
            0..=3 => 0,
            4 => 1,
            _ => 2,
        }] = 1.0;
        let bytes: Vec<u8> = vector.into_iter().flat_map(f32::to_le_bytes).collect();
        conn.execute(
            "UPDATE post_embeddings SET vector=?1 WHERE event_id=?2",
            rusqlite::params![bytes, event_id],
        )
        .unwrap();
    }
    assert!(embed::cluster_topics(&path, &mock.space, now).unwrap() > 0);
    assert!(embed::cluster_dbscan_topics(&path, &mock.space, now, 0.0, 2).is_err());
    assert_eq!(
        embed::cluster_dbscan_topics(&path, &mock.space, now, 0.01, 2).unwrap(),
        2
    );
    assert!(!embed::topics_due(&path, &mock.space, now + 6 * 3600 - 1).unwrap());
    assert!(embed::topics_due(&path, &mock.space, now + 6 * 3600).unwrap());
    let dbscan_topics = embed::topics(&path, &mock.space, "dbscan").unwrap();
    assert!(dbscan_topics
        .iter()
        .any(|topic| topic.id == -1 && topic.label == "Noise / unmatched"));
    assert!(dbscan_topics
        .iter()
        .any(|topic| topic.id >= 0 && topic.label.contains("dbscan")));
    assert_eq!(
        conn.query_row(
            "SELECT epsilon_cosine,min_samples FROM embedding_dbscan_topics WHERE topic_id!=-1 LIMIT 1",
            [],
            |row| Ok((row.get::<_, f32>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap(),
        (0.01, 2)
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM post_dbscan_topics WHERE is_core=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM post_dbscan_topics WHERE topic_id=-1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    let (core_event, core_topic): (Vec<u8>, i64) = conn
        .query_row(
            "SELECT event_id,topic_id FROM post_dbscan_topics WHERE is_core=1 LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let new_event = signed(
        &keys,
        1,
        "new vector near an existing core",
        now as u64,
        vec![],
    );
    let unmatched_event = signed(
        &keys,
        1,
        "new vector outside every DBSCAN neighborhood",
        now as u64,
        vec![],
    );
    sdk.save_event(&new_event).await.unwrap();
    sdk.save_event(&unmatched_event).await.unwrap();
    conn.execute(
        "INSERT INTO post_embeddings(event_id,space_id,dimensions,chunk_count,input_tokens,cost_nusd,vector,embedded_at)
         SELECT ?1,space_id,dimensions,chunk_count,input_tokens,0,vector,?2
         FROM post_embeddings WHERE event_id=?3",
        rusqlite::params![new_event.id.as_bytes(), now, core_event],
    )
    .unwrap();
    let mut unmatched_vector = vec![0.0_f32; mock.space.dimensions];
    unmatched_vector[3] = 1.0;
    conn.execute(
        "INSERT INTO post_embeddings(event_id,space_id,dimensions,chunk_count,input_tokens,cost_nusd,vector,embedded_at)
         VALUES(?1,?2,?3,1,1,0,?4,?5)",
        rusqlite::params![
            unmatched_event.id.as_bytes(),
            mock.space.id,
            i64::try_from(mock.space.dimensions).unwrap(),
            unmatched_vector
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
            now
        ],
    )
    .unwrap();
    assert_eq!(
        embed::assign_new_topics(&path, &mock.space, now + 1).unwrap(),
        2
    );
    assert_eq!(
        conn.query_row(
            "SELECT topic_id,is_core FROM post_dbscan_topics WHERE event_id=?1",
            [new_event.id.as_bytes()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap(),
        (core_topic, 0)
    );
    assert_eq!(
        conn.query_row(
            "SELECT topic_id,is_core FROM post_dbscan_topics WHERE event_id=?1",
            [unmatched_event.id.as_bytes()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap(),
        (-1, 0)
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM post_topics WHERE event_id IN (?1,?2)",
            rusqlite::params![new_event.id.as_bytes(), unmatched_event.id.as_bytes()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        2
    );
    let selected = embed::topic_events(
        &path,
        &mock.space,
        embed::TopicPage {
            ids: &[core_topic],
            include_unsorted: true,
            algorithm: "dbscan",
            now: now + 1,
            limit: 100,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(selected.len(), 8);
    assert_eq!(
        embed::topic_events(
            &path,
            &mock.space,
            embed::TopicPage {
                ids: &[],
                include_unsorted: true,
                algorithm: "dbscan",
                now: now + 1,
                limit: 100,
                offset: 0,
            },
        )
        .unwrap()
        .len(),
        3
    );
    assert_eq!(
        embed::topic_events(
            &path,
            &mock.space,
            embed::TopicPage {
                ids: &[],
                include_unsorted: false,
                algorithm: "dbscan",
                now: now + 1,
                limit: 100,
                offset: 0,
            },
        )
        .unwrap()
        .len(),
        5
    );

    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let rebuild_path = path.clone();
    let rebuild_space = mock.space.clone();
    let kmeans_rebuild =
        std::thread::spawn(move || embed::cluster_topics(&rebuild_path, &rebuild_space, now + 2));
    std::thread::sleep(Duration::from_millis(50));
    assert!(!kmeans_rebuild.is_finished());
    conn.execute(
        "DELETE FROM post_embeddings WHERE event_id=?1",
        [unmatched_event.id.as_bytes()],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM post_topics WHERE event_id=?1",
        [unmatched_event.id.as_bytes()],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM post_dbscan_topics WHERE event_id=?1",
        [unmatched_event.id.as_bytes()],
    )
    .unwrap();
    conn.execute_batch("COMMIT").unwrap();
    kmeans_rebuild.join().unwrap().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM post_topics WHERE event_id=?1",
            [unmatched_event.id.as_bytes()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT coalesce(sum(post_count),0) FROM embedding_topics",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        conn.query_row("SELECT count(*) FROM post_topics", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    );

    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let rebuild_path = path.clone();
    let rebuild_space = mock.space.clone();
    let dbscan_rebuild = std::thread::spawn(move || {
        embed::cluster_dbscan_topics(&rebuild_path, &rebuild_space, now + 3, 0.01, 2)
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(!dbscan_rebuild.is_finished());
    conn.execute(
        "DELETE FROM post_embeddings WHERE event_id=?1",
        [new_event.id.as_bytes()],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM post_topics WHERE event_id=?1",
        [new_event.id.as_bytes()],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM post_dbscan_topics WHERE event_id=?1",
        [new_event.id.as_bytes()],
    )
    .unwrap();
    conn.execute_batch("COMMIT").unwrap();
    dbscan_rebuild.join().unwrap().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM post_dbscan_topics WHERE event_id=?1",
            [new_event.id.as_bytes()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT coalesce(sum(post_count),0) FROM embedding_dbscan_topics",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        conn.query_row("SELECT count(*) FROM post_dbscan_topics", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    );
    assert_eq!(
        embed::cluster_dbscan_topics(&path, &mock.space, now + 4, 0.01, 100).unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM post_dbscan_topics WHERE is_core=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );

    conn.execute("UPDATE embedding_topics SET created_at=0", [])
        .unwrap();
    conn.execute("UPDATE embedding_dbscan_topics SET created_at=0", [])
        .unwrap();
    let transport: Arc<dyn embed::Transport> = Arc::new(MockEmbedder {
        calls: AtomicUsize::new(0),
        space: mock.space.clone(),
    });
    let (_sender, queries) = tokio::sync::mpsc::channel(1);
    let mut worker = collect::EmbeddingWorker {
        transport,
        budget,
        queries,
        error: Arc::new(Mutex::new(None)),
        disabled: false,
        validated: true,
        preflight_only: false,
        shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        topic_rebuild: None,
        topics_disabled: false,
    };
    let mut topics_dirty = false;
    collect::service_embeddings(&path, &mut worker, &mut topics_dirty, Duration::ZERO).await;
    assert!(worker.topic_rebuild.is_some());
    while !worker.topic_rebuild.as_ref().unwrap().is_finished() {
        tokio::task::yield_now().await;
    }
    collect::service_embeddings(&path, &mut worker, &mut topics_dirty, Duration::ZERO).await;
    assert!(worker.topic_rebuild.is_none());
    assert!(!embed::topics_due(&path, &mock.space, Utc::now().timestamp()).unwrap());
}

#[test]
fn shutdown_does_not_wait_for_topic_rebuild() {
    let mock = MockEmbedder::default();
    let (_sender, queries) = tokio::sync::mpsc::channel(1);
    let mut worker = collect::EmbeddingWorker {
        transport: Arc::new(mock),
        budget: embed::Budget {
            total_nusd: 1,
            monthly_nusd: 1,
        },
        queries,
        error: Arc::new(Mutex::new(None)),
        disabled: false,
        validated: true,
        preflight_only: false,
        shutdown: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        topic_rebuild: Some(std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(250));
            Ok((0, 0))
        })),
        topics_disabled: false,
    };
    let started = std::time::Instant::now();
    collect::detach_topic_rebuild_at_shutdown(&mut worker);
    assert!(worker.topic_rebuild.is_none());
    assert!(started.elapsed() < Duration::from_millis(100));
}

#[tokio::test]
async fn incremental_embeddings_reuse_delete_and_budget_after_sdk_drain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let now = i64::try_from(Timestamp::now().as_secs()).unwrap();
    let keys = Keys::generate();
    let long = signed(&keys, 1, &"a".repeat(9_000), now as u64, vec![]);
    let profile = signed(&keys, 0, r#"{"name":"embedded"}"#, now as u64, vec![]);
    let (url, relay_task) = relay(vec![
        serde_json::from_str(&long.as_json()).unwrap(),
        serde_json::from_str(&profile.as_json()).unwrap(),
    ])
    .await;
    let policy = Arc::new(policy::Policy::new(Default::default(), Default::default()));
    let client = collect::client(sdk.clone(), policy.clone());
    client.add_relay(&url).await.unwrap();
    client.connect().await;
    let observed = collect::scan(
        &client,
        &policy,
        &url,
        Filter::new().kinds([Kind::TextNote, Kind::Metadata]),
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert_eq!(observed.accepted.len(), 2);
    let mock = Arc::new(MockEmbedder::default());
    let budget = embed::Budget {
        total_nusd: 5_000_000_000,
        monthly_nusd: 5_000_000_000,
    };
    assert_eq!(
        embed::embed_pending(&path, mock.as_ref(), budget, now, 10)
            .await
            .unwrap(),
        1
    );
    assert_eq!(mock.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        count(
            &Connection::open(&path).unwrap(),
            "SELECT count(*) FROM post_embeddings"
        ),
        1
    );
    let vector = embed::event_vector(&path, &mock.space, long.id.as_bytes())
        .unwrap()
        .unwrap();
    let ranked = embed::nearest(&path, &mock.space, &vector, None, now, 10).unwrap();
    assert_eq!(ranked[0].0, long.id.as_bytes());
    assert!(crate::queries::get(
        &Connection::open(&path).unwrap(),
        &canonical_event_id(&ranked[0].0),
        now,
    )
    .unwrap()
    .is_some());
    assert_eq!(embed::cluster_topics(&path, &mock.space, now).unwrap(), 1);
    let topics = embed::topics(&path, &mock.space, "kmeans").unwrap();
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].post_count, 1);
    assert_eq!(
        embed::topic_events(
            &path,
            &mock.space,
            embed::TopicPage {
                ids: &[topics[0].id],
                include_unsorted: false,
                algorithm: "kmeans",
                now,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap(),
        vec![long.id.as_bytes().to_vec()]
    );
    assert_eq!(
        embed::embed_pending(&path, mock.as_ref(), budget, now, 10)
            .await
            .unwrap(),
        0
    );
    assert_eq!(mock.calls.load(Ordering::SeqCst), 2);

    let second = signed(&keys, 1, "another semantic card", now as u64, vec![]);
    sdk.save_event(&second).await.unwrap();
    let mut topics_dirty = false;
    let (_query_sender, mut queries) = tokio::sync::mpsc::channel(1);
    let shutdown = std::sync::atomic::AtomicBool::new(false);
    collect::embed_until_next_scan(
        &path,
        mock.as_ref(),
        budget,
        &mut queries,
        &shutdown,
        &mut topics_dirty,
        tokio::time::Instant::now() + Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(
        embed::event_vector(&path, &mock.space, second.id.as_bytes())
            .unwrap()
            .is_some()
    );
    let provider: Arc<dyn embed::Transport> = mock.clone();
    let (status, titan_meaning) = html_with_models(
        &path,
        "/?q=related+concept&mode=meaning&embedding=titan",
        None,
        Some(provider.clone()),
        Some(budget),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(titan_meaning.contains("<article"));
    let calls_after_query = mock.calls.load(Ordering::SeqCst);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let cached_app = router(App {
        path: path.clone(),
        root: ROOT.into(),
        templates: templates().unwrap(),
        embedding: None,
        embedding_queries: Some(sender),
        embedding_error: Arc::new(Mutex::new(None)),
        cached_minilm: embed::space_by_backend(&path, "minilm").unwrap(),
        default_embedding: "minilm".into(),
        collecting: false,
        reader_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        status_cache: Arc::new(RwLock::new(None)),
    });
    let cached_response = tokio::time::timeout(
        Duration::from_secs(1),
        cached_app.oneshot(
            axum::http::Request::builder()
                .uri("/?q=related+concept&mode=meaning&embedding=titan")
                .body(axum::body::Body::empty())
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(cached_response.status(), StatusCode::OK);
    assert!(receiver.try_recv().is_err());
    assert_eq!(mock.calls.load(Ordering::SeqCst), calls_after_query);

    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let awaiting_app = router(App {
        path: path.clone(),
        root: ROOT.into(),
        templates: templates().unwrap(),
        embedding: None,
        embedding_queries: Some(sender),
        embedding_error: Arc::new(Mutex::new(None)),
        cached_minilm: embed::space_by_backend(&path, "minilm").unwrap(),
        default_embedding: "minilm".into(),
        collecting: false,
        reader_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        status_cache: Arc::new(RwLock::new(None)),
    });
    let first_query = tokio::spawn(
        awaiting_app.clone().oneshot(
            axum::http::Request::builder()
                .uri("/?q=uncached-one&mode=meaning&embedding=titan")
                .body(axum::body::Body::empty())
                .unwrap(),
        ),
    );
    let second_query = tokio::spawn(
        awaiting_app.clone().oneshot(
            axum::http::Request::builder()
                .uri("/?q=uncached-two&mode=meaning&embedding=titan")
                .body(axum::body::Body::empty())
                .unwrap(),
        ),
    );
    let first_queued = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    let second_queued = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    let ordinary = tokio::time::timeout(
        Duration::from_secs(1),
        awaiting_app.oneshot(
            axum::http::Request::builder()
                .uri("/about")
                .body(axum::body::Body::empty())
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(ordinary.status(), StatusCode::OK);
    first_query.abort();
    second_query.abort();
    drop(first_queued);
    drop(second_queued);

    let (status, provider_status) =
        html_with_models(&path, "/status", None, Some(provider), Some(budget)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!provider_status.contains("Meaning unavailable"));
    assert!(provider_status.contains(">Titan</option>"));

    let mut cached_minilm_space = mock.space.clone();
    cached_minilm_space.id = "cached-minilm-fixture".into();
    cached_minilm_space.backend = "minilm".into();
    cached_minilm_space.model = embed::MINILM_MODEL.into();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO embedding_spaces
         (id,backend,model,dimensions,normalize,revision,model_sha256,tokenizer_sha256,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        rusqlite::params![
            cached_minilm_space.id,
            cached_minilm_space.backend,
            cached_minilm_space.model,
            i64::try_from(cached_minilm_space.dimensions).unwrap(),
            cached_minilm_space.normalize,
            cached_minilm_space.revision,
            cached_minilm_space.model_sha256,
            cached_minilm_space.tokenizer_sha256,
            now
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO post_embeddings
         (event_id,space_id,dimensions,chunk_count,input_tokens,cost_nusd,vector,embedded_at)
         SELECT event_id,?1,dimensions,chunk_count,input_tokens,0,vector,embedded_at
         FROM post_embeddings WHERE space_id=?2",
        rusqlite::params![cached_minilm_space.id, mock.space.id],
    )
    .unwrap();
    embed::cluster_topics(&path, &cached_minilm_space, now).unwrap();

    let semantic: Arc<dyn embed::SemanticModel> = mock.clone();
    let (status, meaning) =
        html_with_embedding(&path, "/?q=semantic&mode=meaning", Some(semantic.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(meaning.contains("<article"));
    assert!(meaning.contains(&long.id.to_hex()));
    assert!(meaning.contains("embedding=minilm"));
    let similar_uri = format!("/?mode=similar&similar=nostr:{}", long.id.to_hex());
    let (status, similar) = html_with_embedding(&path, &similar_uri, Some(semantic.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(similar.contains("<article"));
    assert!(similar.contains(&second.id.to_hex()));
    assert!(similar.contains("<h2>Similar posts</h2>"));
    assert!(similar.contains("this post and its thread"));
    assert!(similar.contains("Related posts"));
    assert!(!similar.contains("<option value=\"similar\""));
    let topic_id = embed::topics(&path, &mock.space, "kmeans").unwrap()[0].id;
    let topic_uri = format!("/?mode=topics&topic={topic_id}");
    let (status, topic) = html_with_embedding(&path, &topic_uri, Some(semantic)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(topic.contains("<article"));
    assert!(topic.contains("embedding=minilm"));
    let titan_topic_uri = format!("/?mode=topics&topic={topic_id}&embedding=titan");
    let (status, titan_topic) = html_with_embedding(&path, &titan_topic_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(titan_topic.contains("<article"));
    assert!(titan_topic.contains("embedding=titan"));
    assert!(titan_topic.contains("Meaning unavailable"));
    assert!(!titan_topic.contains("cached posts"));
    assert!(!titan_topic.contains("proof-of-work"));
    assert!(titan_topic.contains(">MiniLM</option>"));
    let cached_minilm_uri = format!("/?mode=topics&topic={topic_id}&embedding=minilm");
    let (status, cached_minilm) = html_with_embedding(&path, &cached_minilm_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(cached_minilm.contains("<article"));
    assert!(cached_minilm.contains("value=\"minilm\" selected"));
    let (status, cache_status) = html_with_embedding(&path, "/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(cache_status.contains("Related-post index"));
    assert!(cache_status.contains(embed::TITAN_MODEL));
    assert!(cache_status.contains("Titan: 2 posts ready, 0 waiting"));
    assert!(cache_status.contains("US$0.000180720 recorded, US$0.000180720 this month"));

    let related = signed(&keys, 1, "independent related post", now as u64, vec![]);
    sdk.save_event(&related).await.unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE reader_events SET parent_id=?1,root_id=?1 WHERE event_id=?2",
        (canonical_event_id(long.id.as_bytes()), second.id.as_bytes()),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO post_embeddings
         (event_id,space_id,dimensions,chunk_count,input_tokens,cost_nusd,vector,embedded_at)
         SELECT ?1,space_id,dimensions,chunk_count,input_tokens,0,vector,embedded_at
         FROM post_embeddings WHERE event_id=?2 AND space_id=?3",
        (related.id.as_bytes(), second.id.as_bytes(), &mock.space.id),
    )
    .unwrap();
    drop(conn);
    let root_vector = embed::event_vector(&path, &mock.space, long.id.as_bytes())
        .unwrap()
        .unwrap();
    let ranked = embed::nearest(
        &path,
        &mock.space,
        &root_vector,
        Some(long.id.as_bytes()),
        now,
        10,
    )
    .unwrap();
    assert!(ranked
        .iter()
        .any(|(event_id, _)| event_id == second.id.as_bytes()));
    let calls_before_context = mock.calls.load(Ordering::SeqCst);
    let context_uri = format!("/context/nostr/{}?embedding=titan", long.id.to_hex());
    let (status, context) = html_with_embedding(&path, &context_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(context.contains("<h3>Similar replies:</h3>"));
    assert!(context.contains("<option value=\"titan\" selected>Titan</option>"));
    assert_eq!(context.matches("<article").count(), 3);
    for event in [&long, &second, &related] {
        let article = format!(
            "<article class=\"post\" id=\"nostr:{}\">",
            event.id.to_hex()
        );
        assert_eq!(context.matches(&article).count(), 1);
    }
    let heading = context.find("<h3>Similar replies:</h3>").unwrap();
    assert!(context.find(&second.id.to_hex()).unwrap() < heading);
    assert!(heading < context.find(&related.id.to_hex()).unwrap());
    assert_eq!(mock.calls.load(Ordering::SeqCst), calls_before_context);

    sdk.delete(Filter::new().ids([long.id, second.id, related.id]))
        .await
        .unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM post_embeddings"), 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM embedding_chunks"), 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM post_topics"), 0);
    let too_small = embed::Budget {
        total_nusd: 1,
        monthly_nusd: 1,
    };
    let pending = signed(&keys, 1, "budget guard", now as u64, vec![]);
    sdk.save_event(&pending).await.unwrap();
    let calls = mock.calls.load(Ordering::SeqCst);
    assert!(
        embed::embed_pending(&path, mock.as_ref(), too_small, now, 10)
            .await
            .unwrap_err()
            .to_string()
            .contains("budget exhausted")
    );
    assert_eq!(mock.calls.load(Ordering::SeqCst), calls);
    client.shutdown().await;
    relay_task.abort();
}

#[tokio::test]
async fn moderation_refresh_rejects_future_and_rollback_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let primal = Keys::generate();
    let first_member = Keys::generate().public_key();
    let other_member = Keys::generate().public_key();
    let now = Timestamp::now().as_secs();
    let sdk = collect::open(&path, &primal.public_key().to_hex())
        .await
        .unwrap();
    let snapshot = |identifier: &str, at: u64, member: PublicKey| {
        signed(
            &primal,
            30000,
            "",
            at,
            vec![vec!["d", identifier], vec!["p", &member.to_hex()]],
        )
    };
    let current_nsfw = snapshot("nsfw_list", now - 10, first_member);
    let current_spam = snapshot("spam_list", now - 10, first_member);
    let members = collect::install_primal_snapshots(
        &sdk,
        primal.public_key(),
        vec![
            ("nsfw_list", current_nsfw.clone()),
            ("spam_list", current_spam),
        ],
        now,
    )
    .await
    .unwrap();
    assert_eq!(members, [first_member].into());
    let rollback = snapshot("nsfw_list", now - 20, other_member);
    let error = collect::install_primal_snapshots(
        &sdk,
        primal.public_key(),
        vec![("nsfw_list", rollback)],
        now,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("rolled back"));
    let future = snapshot("nsfw_list", now + 1, other_member);
    let error = collect::install_primal_snapshots(
        &sdk,
        primal.public_key(),
        vec![("nsfw_list", future)],
        now,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("future-dated"));
    let canonical = sdk.query(Filter::new().id(current_nsfw.id)).await.unwrap();
    assert_eq!(canonical.len(), 1);
}

#[tokio::test]
async fn eose_boundary_has_no_late_admitted_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    let note = signed(
        &Keys::generate(),
        1,
        "after eose",
        Timestamp::now().as_secs(),
        vec![],
    );
    let (url, task) = relay_ordered(
        vec![serde_json::from_str(&note.as_json()).unwrap()],
        Some(true),
    )
    .await;
    let policy = Arc::new(policy::Policy::new(Default::default(), Default::default()));
    let client = collect::client(sdk.clone(), policy.clone());
    client.add_relay(&url).await.unwrap();
    client.connect().await;
    let observed = collect::scan(
        &client,
        &policy,
        &url,
        Filter::new().id(note.id),
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    let at_return = sdk.check_id(&note.id).await.unwrap();
    Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO collection_gaps VALUES(?1,0,0,'EOSE boundary checked',unixepoch())",
            [&url],
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(sdk.check_id(&note.id).await.unwrap(), at_return);
    assert_eq!(
        matches!(at_return, DatabaseEventStatus::Saved),
        observed.accepted.contains_key(&note.id)
    );
    client.shutdown().await;
    task.abort();
}

#[tokio::test]
async fn sdk_storage_failure_does_not_stop_http_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let sdk = collect::open(&path, collect::PRIMAL_AUTHOR).await.unwrap();
    Connection::open(&path).unwrap().execute_batch("CREATE TRIGGER fixture_failure BEFORE INSERT ON events WHEN new.kind=1 BEGIN SELECT RAISE(ABORT,'fixture disk failure'); END;").unwrap();
    let note = signed(
        &Keys::generate(),
        1,
        "lostword",
        Timestamp::now().as_secs(),
        vec![],
    );
    let (url, task) = relay(vec![serde_json::from_str(&note.as_json()).unwrap()]).await;
    let policy = Arc::new(policy::Policy::new(Default::default(), Default::default()));
    let client = collect::client(sdk, policy.clone());
    client.add_relay(&url).await.unwrap();
    client.connect().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(App {
        path,
        root: ROOT.into(),
        templates: templates().unwrap(),
        embedding: None,
        embedding_queries: None,
        embedding_error: Arc::new(Mutex::new(None)),
        cached_minilm: None,
        default_embedding: "minilm".into(),
        collecting: false,
        reader_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        status_cache: Arc::new(RwLock::new(None)),
    });
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let error = collect::scan(
        &client,
        &policy,
        &url,
        Filter::new().kind(Kind::TextNote),
        Duration::from_secs(2),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("did not finish admitted event"));
    assert!(tokio::net::TcpStream::connect(address).await.is_ok());
    server.abort();
    client.shutdown().await;
    task.abort();
}
