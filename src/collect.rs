use crate::{
    policy::{Observation, Policy},
    queries::WINDOW,
    Error,
};
use futures_util::{SinkExt, StreamExt};
use nostr_sdk::prelude::{
    Client, DatabaseEventStatus, Event, EventId, Filter, Kind, NostrDatabase, PublicKey,
    RelayMessage, RelayNotification, SubscriptionId, Timestamp,
};
use nostr_sqlite::store::NostrSqlite;
use rusqlite::{Connection, OptionalExtension};
use std::{collections::BTreeSet, path::Path, sync::Arc, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub const PRIMAL_AUTHOR: &str = "5d8282fc89410f1c57681a2c3b8be57afd1566c262fd1deb543999d39d141cb4";

pub async fn open(path: &Path, primal_author: &str) -> Result<NostrSqlite, Error> {
    if path.exists() {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let legacy: Option<String> = conn
            .query_row(
                "SELECT type FROM sqlite_master WHERE name='posts'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if legacy.as_deref() == Some("table") {
            return Err("Refusing to modify the reference projection; choose a new database or MEATYBROTH_READ_ONLY=1".into());
        }
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    let sdk = NostrSqlite::builder().in_file(path).build().await?;
    let conn = Connection::open(path)?;
    let initialized: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='reader_events')",
        [],
        |r| r.get(0),
    )?;
    if !initialized {
        let events: i64 = conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
        if events != 0 {
            return Err("New reader schema requires an empty SDK store; migration must be verified separately".into());
        }
        PublicKey::from_hex(primal_author)?;
        conn.execute_batch(&include_str!("schema.sql").replace("{primal_author}", primal_author))?;
    }
    Ok(sdk)
}

pub fn list_members(
    event: &Event,
    author: PublicKey,
    identifier: &str,
) -> Result<BTreeSet<PublicKey>, Error> {
    event.verify()?;
    if crate::policy::secret(event) {
        return Err("Secret pattern in moderation list".into());
    }
    let identifiers: Vec<_> = event
        .tags
        .iter()
        .filter(|t| t.as_slice().first().is_some_and(|s| s == "d"))
        .collect();
    if event.pubkey != author
        || event.kind.as_u16() != 30000
        || identifiers.len() != 1
        || identifiers[0].as_slice().get(1).map(String::as_str) != Some(identifier)
    {
        return Err("Wrong moderation list author, kind or identifier".into());
    }
    Ok(event
        .tags
        .iter()
        .filter_map(|t| {
            let values = t.as_slice();
            if values.first().is_some_and(|s| s == "p") {
                values
                    .get(1)
                    .and_then(|s| PublicKey::from_hex(s).ok())
                    .filter(|p| p.to_hex() == values[1])
            } else {
                None
            }
        })
        .collect())
}
async fn fetch_list(identifier: &str) -> Result<Event, Error> {
    let fetch = async {
        let (mut socket, _) = connect_async("wss://cache2.primal.net/v1").await?;
        let id = format!("meatybroth-{identifier}");
        socket
            .send(Message::Text(
                serde_json::json!(["REQ",id,{"cache":["parameterized_replaceable_list",{
            "pubkey":PRIMAL_AUTHOR,"identifier":identifier}]}])
                .to_string()
                .into(),
            ))
            .await?;
        while let Some(message) = socket.next().await {
            if let Message::Text(text) = message? {
                let value: serde_json::Value = serde_json::from_str(&text)?;
                if value[0] == "EVENT" && value[1] == id {
                    let event = Event::from_json(value[2].to_string())?;
                    list_members(&event, PublicKey::from_hex(PRIMAL_AUTHOR)?, identifier)?;
                    socket.close(None).await?;
                    return Ok(event);
                }
                if value[0] == "EOSE" && value[1] == id {
                    return Err("Moderation list missing".into());
                }
            }
        }
        Err::<Event, Error>("Moderation connection closed before its signed list".into())
    };
    tokio::time::timeout(Duration::from_secs(20), fetch).await?
}
pub async fn bootstrap(sdk: &NostrSqlite) -> Result<BTreeSet<PublicKey>, Error> {
    let author = PublicKey::from_hex(PRIMAL_AUTHOR)?;
    for id in ["nsfw_list", "spam_list"] {
        let event = fetch_list(id).await?;
        sdk.save_event(&event).await?;
    }
    let current = sdk
        .query(
            Filter::new()
                .author(author)
                .kind(Kind::from(30000))
                .identifier("nsfw_list")
                .limit(1),
        )
        .await?
        .into_iter()
        .next()
        .ok_or("No verified NSFW list stored")?;
    list_members(&current, author, "nsfw_list")
}

pub fn client(sdk: NostrSqlite, policy: Arc<Policy>) -> Client {
    Client::builder()
        .database(sdk)
        .admit_policy(policy.as_ref().clone())
        .verify_subscriptions(true)
        .build()
}
pub async fn scan(
    client: &Client,
    policy: &Policy,
    relay_url: &str,
    filter: Filter,
    timeout: Duration,
) -> Result<Observation, Error> {
    let relay = client
        .relay(relay_url)
        .await?
        .ok_or("Relay not registered")?;
    let id = SubscriptionId::generate();
    policy.begin(id.clone(), filter.clone());
    let mut messages = relay.notifications();
    relay.subscribe(vec![filter]).with_id(id.clone()).await?;
    let eose=tokio::time::timeout(timeout,async {
        while let Some(notification)=messages.next().await {
            if let RelayNotification::Message{message,..}=notification {
                if matches!(*message,RelayMessage::EndOfStoredEvents(ref received) if received.as_ref()==&id){return true;}
            }
        }
        false
    }).await.unwrap_or(false);
    let mut observed = policy.finish(&id);
    relay.unsubscribe(&id).await?;
    observed.eose = eose;
    for event_id in observed.accepted_notes.iter().filter(|_| eose) {
        if client.database().check_id(event_id).await? == DatabaseEventStatus::NotExistent {
            return Err(format!("SDK did not persist admitted note {event_id}").into());
        }
    }
    Ok(observed)
}
pub async fn prune(sdk: &NostrSqlite, policy: &Policy, now: u64) -> Result<(), Error> {
    sdk.delete(
        Filter::new()
            .kind(Kind::TextNote)
            .until(Timestamp::from(now - WINDOW as u64 - 1)),
    )
    .await?;
    let nsfw: Vec<_> = policy.nsfw.read().unwrap().iter().copied().collect();
    for authors in nsfw.chunks(500) {
        sdk.delete(
            Filter::new()
                .kind(Kind::TextNote)
                .authors(authors.iter().copied()),
        )
        .await?;
    }
    let blocked: Vec<_> = policy.blocked.read().unwrap().iter().cloned().collect();
    for keys in blocked.chunks(500) {
        let ids = keys
            .iter()
            .map(|s| EventId::from_hex(s))
            .collect::<Result<Vec<_>, _>>()?;
        let authors = keys
            .iter()
            .map(|s| PublicKey::from_hex(s))
            .collect::<Result<Vec<_>, _>>()?;
        sdk.delete(Filter::new().ids(ids)).await?;
        sdk.delete(Filter::new().authors(authors)).await?;
    }
    Ok(())
}
pub async fn run(path: &Path, sdk: NostrSqlite, relays: Vec<String>) -> Result<(), Error> {
    let blocks = path.with_file_name("blocklist.txt");
    let load_blocks = || -> Result<BTreeSet<String>, Error> {
        let text = std::fs::read_to_string(&blocks)?;
        let values: BTreeSet<_> = text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .map(str::to_owned)
            .collect();
        for value in &values {
            EventId::from_hex(value)?;
        }
        Ok(values)
    };
    let policy = Arc::new(Policy::new(load_blocks()?, bootstrap(&sdk).await?));
    let client = client(sdk.clone(), policy.clone());
    for relay in &relays {
        client.add_relay(relay).await?;
    }
    client.connect().await;
    loop {
        *policy.blocked.write().unwrap() = load_blocks()?;
        prune(&sdk, &policy, Timestamp::now().as_secs()).await?;
        for relay in &relays {
            let now = Timestamp::now();
            let filter = Filter::new()
                .kind(Kind::TextNote)
                .since(Timestamp::from(now.as_secs() - WINDOW as u64))
                .until(now)
                .limit(500);
            let observed = scan(&client, &policy, relay, filter, Duration::from_secs(15)).await?;
            eprintln!(
                "SDK received={} rejected={} eose={} relay={relay}; coverage not established",
                observed.ids.len(),
                observed.rejected,
                observed.eose
            );
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
