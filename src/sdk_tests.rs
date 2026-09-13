use super::*;
use axum::extract::ws::{Message, WebSocketUpgrade};
use nostr_sdk::prelude::{
    Event, EventBuilder, Filter, FinalizeEvent, Keys, Kind, NostrDatabase, Tag, Timestamp,
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn signed(keys: &Keys, kind: u16, content: &str, time: u64, tags: Vec<Vec<&str>>) -> Event {
    EventBuilder::new(Kind::from(kind), content)
        .custom_created_at(Timestamp::from(time))
        .tags(tags.into_iter().map(|t| Tag::parse(t).unwrap()))
        .finalize(keys)
        .unwrap()
}
async fn relay(events: Vec<Value>) -> (String, tokio::task::JoinHandle<()>) {
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
                            for event in &events {
                                socket
                                    .send(Message::Text(
                                        json!(["EVENT", req[1], event]).to_string().into(),
                                    ))
                                    .await
                                    .unwrap();
                            }
                            socket
                                .send(Message::Text(json!(["EOSE", req[1]]).to_string().into()))
                                .await
                                .unwrap();
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
async fn html(path: &std::path::Path, uri: &str) -> (StatusCode, String) {
    let app = router(App {
        path: path.to_path_buf(),
        root: ROOT.into(),
        templates: templates().unwrap(),
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
fn count(db: &Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |r| r.get(0)).unwrap()
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
async fn sdk_storage_failure_reaches_supervisor() {
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
    });
    let error = supervise(listener, app, async {
        collect::scan(
            &client,
            &policy,
            &url,
            Filter::new().kind(Kind::TextNote),
            Duration::from_secs(2),
        )
        .await?;
        Ok(())
    })
    .await
    .unwrap_err();
    assert!(error.to_string().contains("did not persist admitted note"));
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    client.shutdown().await;
    task.abort();
    eprintln!(
        "SDK save failure surfaced, HTTP listener stopped rather than silently remaining stale"
    );
}
