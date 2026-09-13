//! Collects policy-admitted Nostr events into the SQLite file used by the reader.
//!
//! The SDK owns Nostr event storage; this module owns moderation and collection progress tables.
//! -- Pi/gpt-5.6-sol

use crate::{
    embed::{self, Budget, Transport},
    policy::{Observation, Policy},
    queries::WINDOW,
    Error,
};
use futures_util::{SinkExt, StreamExt};
use nostr_sdk::{
    error::ErrorKind,
    prelude::{
        Client, DatabaseEventStatus, Event, EventId, Filter, Kind, NostrDatabase, PublicKey,
        RelayMessage, RelayNotification, ReqExitPolicy, SubscribeAutoCloseOptions, SubscriptionId,
        SyncOptions, Timestamp,
    },
};
use nostr_sqlite::store::NostrSqlite;
use rusqlite::{Connection, OptionalExtension};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub const PRIMAL_AUTHOR: &str = "5d8282fc89410f1c57681a2c3b8be57afd1566c262fd1deb543999d39d141cb4";
const DERIVED_ELIGIBILITY_CLEANUP: &str = "
DELETE FROM post_topics WHERE NOT EXISTS (
    SELECT 1 FROM reader_post_events reader WHERE reader.event_id=post_topics.event_id
);
DELETE FROM embedding_chunks WHERE NOT EXISTS (
    SELECT 1 FROM reader_post_events reader WHERE reader.event_id=embedding_chunks.event_id
);
DELETE FROM post_embeddings WHERE NOT EXISTS (
    SELECT 1 FROM reader_post_events reader WHERE reader.event_id=post_embeddings.event_id
);";
const PAGE_LIMIT: usize = 500;
const COVERAGE_STEP: i64 = 300;
const MODERATION_REFRESH: u64 = 3600;
const RELAY_WORK_BUDGET: u64 = 120;
const EMBEDDING_INTERLEAVE: u64 = 5;

#[derive(Clone, Copy)]
struct Cursor {
    forward_at: i64,
    backfill_before: i64,
}

pub(crate) fn cleanup_ineligible_derived(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(DERIVED_ELIGIBILITY_CLEANUP)?;
    Ok(())
}

fn admin(path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    Ok(conn)
}

fn recover_interrupted(path: &Path, now: i64) -> Result<(), Error> {
    let mut conn = admin(path)?;
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO collection_gaps(relay,since_at,until_at,reason,checked_at)
         SELECT relay,since_at,until_at,'interrupted before EOSE; partial arrivals retained',?1
         FROM collection_runs WHERE finished_at IS NULL",
        [now],
    )?;
    tx.execute(
        "UPDATE collection_runs SET finished_at=?1,eose=0,accepted=0,rejected=0
         WHERE finished_at IS NULL",
        [now],
    )?;
    tx.commit()?;
    Ok(())
}

fn cursor(path: &Path, relay: &str, now: i64) -> Result<Cursor, Error> {
    let conn = admin(path)?;
    let (first, last): (Option<i64>, Option<i64>) = conn.query_row(
        "SELECT min(e.created_at),max(e.created_at) FROM events e WHERE e.kind=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let first = first.unwrap_or(now - WINDOW);
    let last = last.unwrap_or(first);
    conn.execute(
        "INSERT OR IGNORE INTO collection_cursors(relay,forward_at,backfill_before,updated_at)
         VALUES(?1,?2,?3,?4)",
        (relay, last + 1, first, now),
    )?;
    conn.query_row(
        "SELECT forward_at,backfill_before FROM collection_cursors WHERE relay=?1",
        [relay],
        |row| {
            Ok(Cursor {
                forward_at: row.get(0)?,
                backfill_before: row.get(1)?,
            })
        },
    )
    .map_err(Into::into)
}

fn begin_run(
    path: &Path,
    relay: &str,
    phase: &str,
    since: i64,
    until: i64,
    now: i64,
) -> Result<i64, Error> {
    let conn = admin(path)?;
    conn.execute(
        "INSERT INTO collection_runs(relay,phase,since_at,until_at,started_at)
         VALUES(?1,?2,?3,?4,?5)",
        (relay, phase, since, until, now),
    )?;
    Ok(conn.last_insert_rowid())
}

fn finish_run(
    path: &Path,
    id: i64,
    observed: &Observation,
    cursor_update: (&str, &str, i64),
) -> Result<(), Error> {
    let now = i64::try_from(Timestamp::now().as_secs())?;
    let mut conn = admin(path)?;
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE collection_runs SET finished_at=?1,eose=1,accepted=?2,rejected=?3 WHERE id=?4",
        (
            now,
            i64::try_from(observed.accepted_notes.len())?,
            i64::try_from(observed.rejected)?,
            id,
        ),
    )?;
    let (relay, column, value) = cursor_update;
    let sql = format!("UPDATE collection_cursors SET {column}=?1,updated_at=?2 WHERE relay=?3");
    tx.execute(&sql, (value, now, relay))?;
    tx.commit()?;
    Ok(())
}

fn record_gap(path: &Path, relay: &str, since: i64, until: i64, reason: &str) -> Result<(), Error> {
    admin(path)?.execute(
        "INSERT INTO collection_gaps(relay,since_at,until_at,reason,checked_at)
         VALUES(?1,?2,?3,?4,?5)",
        (
            relay,
            since,
            until,
            reason,
            i64::try_from(Timestamp::now().as_secs())?,
        ),
    )?;
    Ok(())
}

fn finish_unreconciled(
    path: &Path,
    id: i64,
    observed: &Observation,
    cursor_update: (&str, &str, i64),
) -> Result<(), Error> {
    let now = i64::try_from(Timestamp::now().as_secs())?;
    let mut conn = admin(path)?;
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE collection_runs SET finished_at=?1,eose=1,accepted=?2,rejected=?3 WHERE id=?4",
        (
            now,
            i64::try_from(observed.accepted_notes.len())?,
            i64::try_from(observed.rejected)?,
            id,
        ),
    )?;
    tx.execute(
        "INSERT INTO collection_gaps(relay,since_at,until_at,reason,checked_at)
         SELECT relay,since_at,until_at,'EOSE received but relay inventory cannot be reconciled',?1
         FROM collection_runs WHERE id=?2",
        (now, id),
    )?;
    let (relay, column, value) = cursor_update;
    let sql = format!("UPDATE collection_cursors SET {column}=?1,updated_at=?2 WHERE relay=?3");
    tx.execute(&sql, (value, now, relay))?;
    tx.commit()?;
    Ok(())
}

/// Opens an owned SDK store and installs the reader, coverage, and embedding schemas.
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
    let stage_started = std::time::Instant::now();
    let sdk = NostrSqlite::builder().in_file(path).build().await?;
    eprintln!(
        "Startup stage sdk_open_ms={}",
        stage_started.elapsed().as_millis()
    );
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
    for (name, sql) in [
        ("posts_schema", include_str!("posts.sql")),
        ("coverage_schema", include_str!("coverage.sql")),
        ("embedding_schema", include_str!("embed.sql")),
        ("social_schema", include_str!("social.sql")),
    ] {
        let stage_started = std::time::Instant::now();
        conn.execute_batch(sql)?;
        eprintln!(
            "Startup stage {name}_ms={}",
            stage_started.elapsed().as_millis()
        );
    }
    // Remove derived rows excluded by a newer reader policy; canonical events and spend history remain. -- Pi/gpt-5.6-sol
    let stage_started = std::time::Instant::now();
    cleanup_ineligible_derived(&conn)?;
    eprintln!(
        "Startup stage derived_cleanup_ms={}",
        stage_started.elapsed().as_millis()
    );
    // Query-planner cardinality prevents multi-second feed joins after bulk SDK ingestion. -- Pi/gpt-5.6-sol
    let stage_started = std::time::Instant::now();
    conn.execute_batch("ANALYZE events")?;
    eprintln!(
        "Startup stage analyze_events_ms={}",
        stage_started.elapsed().as_millis()
    );
    drop(conn);
    recover_interrupted(path, i64::try_from(Timestamp::now().as_secs())?)?;
    Ok(sdk)
}

/// Verifies a Primal parameterized list and returns its canonical public-key members.
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
/// Validates a full moderation refresh before saving any snapshot.
///
/// Future and rollback snapshots are rejected. The returned NSFW set is read from the SDK's
/// canonical replaceable event rather than from the fetched candidate. -- Pi/gpt-5.6-sol
pub async fn install_primal_snapshots(
    sdk: &NostrSqlite,
    author: PublicKey,
    snapshots: Vec<(&str, Event)>,
    now: u64,
) -> Result<BTreeSet<PublicKey>, Error> {
    for (identifier, event) in &snapshots {
        list_members(event, author, identifier)?;
        let current = sdk
            .query(
                Filter::new()
                    .author(author)
                    .kind(Kind::from(30000))
                    .identifier(*identifier)
                    .limit(1),
            )
            .await?
            .into_iter()
            .next();
        if event.created_at.as_secs() > now
            || current
                .as_ref()
                .is_some_and(|stored| event.created_at < stored.created_at)
        {
            return Err(
                format!("Primal {identifier} snapshot is future-dated or rolled back").into(),
            );
        }
    }
    for (_, event) in snapshots {
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
        .ok_or("No canonical NSFW snapshot stored")?;
    list_members(&current, author, "nsfw_list")
}

/// Fetches and installs the signed Primal NSFW and spam snapshots.
pub async fn bootstrap(sdk: &NostrSqlite) -> Result<BTreeSet<PublicKey>, Error> {
    let author = PublicKey::from_hex(PRIMAL_AUTHOR)?;
    let mut snapshots = Vec::new();
    for identifier in ["nsfw_list", "spam_list"] {
        let event = fetch_list(identifier).await?;
        let members = list_members(&event, author, identifier)?;
        eprintln!(
            "Verified Primal {identifier} snapshot id={} signed_at={} members={}",
            event.id,
            event.created_at,
            members.len()
        );
        snapshots.push((identifier, event));
    }
    install_primal_snapshots(sdk, author, snapshots, Timestamp::now().as_secs()).await
}

/// Builds the relay client with policy admission before SDK persistence.
///
/// Pinned SDK order: <https://docs.rs/nostr-sdk/0.45.2/src/nostr_sdk/relay/inner.rs.html#1270-1291>.
/// -- Pi/gpt-5.6-sol
pub fn client(sdk: NostrSqlite, policy: Arc<Policy>) -> Client {
    Client::builder()
        .database(sdk)
        .admit_policy(policy.as_ref().clone())
        .verify_subscriptions(true)
        .build()
}
/// Runs one auto-closing request and waits for admitted events to reach a stored outcome.
///
/// NIP-01 EOSE ends stored history, not later live delivery, and does not prove relay completeness:
/// <https://github.com/nostr-protocol/nips/blob/master/01.md#from-client-to-relay-sending-events-and-creating-subscriptions>.
/// -- Pi/gpt-5.6-sol
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
    relay
        .subscribe(vec![filter])
        .with_id(id.clone())
        .close_on(
            SubscribeAutoCloseOptions::default()
                .exit_policy(ReqExitPolicy::ExitOnEOSE)
                .timeout(Some(timeout)),
        )
        .await?;
    let eose = tokio::time::timeout(timeout, async {
        while let Some(notification) = messages.next().await {
            if let RelayNotification::Message { message, .. } = notification {
                if matches!(*message, RelayMessage::EndOfStoredEvents(ref received) if received.as_ref() == &id)
                {
                    return true;
                }
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    let mut observed = policy.finish(&id);
    relay.unsubscribe(&id).await?;
    observed.eose = eose;
    for (event_id, accepted) in &observed.accepted {
        tokio::time::timeout(timeout, async {
            loop {
                let complete = if accepted.kind == Kind::TextNote {
                    matches!(
                        client.database().check_id(event_id).await?,
                        DatabaseEventStatus::Saved | DatabaseEventStatus::Deleted
                    )
                } else {
                    client
                        .database()
                        .query(
                            Filter::new()
                                .author(accepted.pubkey)
                                .kind(accepted.kind)
                                .limit(1),
                        )
                        .await?
                        .iter()
                        .any(|stored| stored.created_at >= accepted.created_at)
                };
                if complete {
                    return Ok::<_, Error>(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| format!("SDK did not finish admitted event {event_id}"))??;
    }
    Ok(observed)
}

fn merge(into: &mut Observation, from: Observation) {
    into.ids.extend(from.ids);
    into.accepted.extend(from.accepted);
    into.accepted_notes.extend(from.accepted_notes);
    into.rejected += from.rejected;
    into.oldest = match (into.oldest, from.oldest) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    into.eose &= from.eose;
}

async fn reconcile_window(
    client: &Client,
    policy: &Policy,
    sdk: &NostrSqlite,
    relay_url: &str,
    filter: Filter,
    deadline: tokio::time::Instant,
) -> Result<Option<Observation>, Error> {
    let local_count = sdk.count(filter.clone()).await?;
    let local = sdk.query(filter.clone()).await?;
    if local.len() != local_count {
        return Err(format!(
            "Local reconciliation inventory truncated: query={} count={local_count}",
            local.len()
        )
        .into());
    }
    let relay = client
        .relay(relay_url)
        .await?
        .ok_or("Relay not registered")?;
    let sync = relay
        .sync(filter)
        .items(local.iter().map(|event| (event.id, event.created_at)))
        .opts(SyncOptions::new().dry_run());
    let summary = match tokio::time::timeout_at(deadline, sync).await {
        Err(_) => return Err("Relay reconciliation inventory exceeded its work budget".into()),
        Ok(result) => match result {
            Ok(summary) => summary,
            Err(error) if error.kind() == ErrorKind::Unsupported => return Ok(None),
            Err(error) => return Err(error.into()),
        },
    };
    if summary.remote.len() > 10_000 {
        return Err(format!(
            "Remote reconciliation inventory too large: {}",
            summary.remote.len()
        )
        .into());
    }
    let mut observed = Observation {
        eose: true,
        ..Observation::default()
    };
    let remote: Vec<_> = summary.remote.into_iter().collect();
    for ids in remote.chunks(PAGE_LIMIT) {
        if tokio::time::Instant::now() >= deadline {
            return Err("Relay reconciliation batches reached their work budget".into());
        }
        let expected: BTreeSet<_> = ids.iter().copied().collect();
        let batch = scan(
            client,
            policy,
            relay_url,
            Filter::new().ids(expected.iter().copied()),
            Duration::from_secs(15),
        )
        .await?;
        if !batch.eose || batch.ids != expected {
            return Err(format!(
                "Exact reconciliation fetch incomplete: expected={} observed={} eose={}",
                expected.len(),
                batch.ids.len(),
                batch.eose
            )
            .into());
        }
        merge(&mut observed, batch);
    }
    Ok(Some(observed))
}

struct DurableObservation {
    observed: Observation,
    reconciled: bool,
}

struct CoverageWindow<'a> {
    relay: &'a str,
    phase: &'a str,
    since: i64,
    until: i64,
    cursor_column: &'a str,
    cursor_value: i64,
    reconciliation_unsupported: bool,
}

async fn durable_window(
    path: &Path,
    client: &Client,
    policy: &Policy,
    sdk: &NostrSqlite,
    window: CoverageWindow<'_>,
    deadline: tokio::time::Instant,
) -> Result<DurableObservation, Error> {
    let CoverageWindow {
        relay,
        phase,
        since,
        until,
        cursor_column,
        cursor_value,
        reconciliation_unsupported,
    } = window;
    let id = begin_run(
        path,
        relay,
        phase,
        since,
        until,
        i64::try_from(Timestamp::now().as_secs())?,
    )?;
    let filter = Filter::new()
        .kind(Kind::TextNote)
        .since(Timestamp::from(u64::try_from(since)?))
        .until(Timestamp::from(u64::try_from(until)?));
    if !reconciliation_unsupported {
        if let Some(observed) =
            reconcile_window(client, policy, sdk, relay, filter.clone(), deadline).await?
        {
            finish_run(path, id, &observed, (relay, cursor_column, cursor_value))?;
            return Ok(DurableObservation {
                observed,
                reconciled: true,
            });
        }
    }
    if tokio::time::Instant::now() >= deadline {
        return Err("Relay window reached its work budget between drained scans".into());
    }
    let observed = scan(
        client,
        policy,
        relay,
        filter.limit(PAGE_LIMIT),
        Duration::from_secs(15),
    )
    .await?;
    if !observed.eose {
        return Err(format!(
            "Unreconciled {phase} scan timed out for {relay}; partial arrivals retained"
        )
        .into());
    }
    finish_unreconciled(path, id, &observed, (relay, cursor_column, cursor_value))?;
    Ok(DurableObservation {
        observed,
        reconciled: false,
    })
}

async fn missing_direct_follow_contact_lists(
    sdk: &NostrSqlite,
    root: PublicKey,
) -> Result<BTreeSet<PublicKey>, Error> {
    let root_contacts = sdk
        .query(Filter::new().author(root).kind(Kind::ContactList))
        .await?;
    let Some(latest) = root_contacts.iter().max_by_key(|event| event.created_at) else {
        return Ok(BTreeSet::new());
    };
    let follows: BTreeSet<_> = latest
        .tags
        .iter()
        .filter_map(|tag| {
            let values = tag.as_slice();
            (values.first().is_some_and(|value| value == "p"))
                .then(|| values.get(1))
                .flatten()
                .and_then(|value| PublicKey::from_hex(value).ok())
        })
        .collect();
    if follows.is_empty() {
        return Ok(follows);
    }
    let owners: BTreeSet<_> = sdk
        .query(
            Filter::new()
                .authors(follows.iter().copied())
                .kind(Kind::ContactList),
        )
        .await?
        .iter()
        .map(|event| event.pubkey)
        .collect();
    Ok(follows.difference(&owners).copied().collect())
}

async fn hydrate_notes(
    client: &Client,
    policy: &Policy,
    sdk: &NostrSqlite,
    relay_url: &str,
    note_ids: &BTreeSet<EventId>,
) -> Result<(Observation, Observation), Error> {
    let notes = sdk
        .query(Filter::new().ids(note_ids.iter().copied()))
        .await?;
    let authors: BTreeSet<_> = notes.iter().map(|event| event.pubkey).collect();
    let parent_ids: BTreeSet<_> = notes
        .iter()
        .flat_map(|event| event.tags.iter())
        .filter_map(|tag| {
            let values = tag.as_slice();
            (values.first().is_some_and(|value| value == "e"))
                .then(|| values.get(1))
                .flatten()
                .and_then(|value| EventId::from_hex(value).ok())
        })
        .collect();
    let metadata = if authors.is_empty() {
        Observation {
            eose: true,
            ..Observation::default()
        }
    } else {
        scan(
            client,
            policy,
            relay_url,
            Filter::new()
                .authors(authors)
                .kinds([Kind::Metadata, Kind::ContactList, Kind::RelayList])
                .limit(500),
            Duration::from_secs(15),
        )
        .await?
    };
    if !metadata.eose {
        return Err(
            format!("Metadata scan timed out for {relay_url}; partial arrivals retained").into(),
        );
    }
    let parents = if parent_ids.is_empty() {
        Observation {
            eose: true,
            ..Observation::default()
        }
    } else {
        scan(
            client,
            policy,
            relay_url,
            Filter::new()
                .ids(parent_ids)
                .kind(Kind::TextNote)
                .limit(500),
            Duration::from_secs(15),
        )
        .await?
    };
    if !parents.eose {
        return Err(
            format!("Parent scan timed out for {relay_url}; partial arrivals retained").into(),
        );
    }
    Ok((metadata, parents))
}

fn missing_profile_authors(path: &Path, relay: &str, now: i64) -> Result<Vec<PublicKey>, Error> {
    let conn = admin(path)?;
    let mut statement = conn.prepare(
        "SELECT DISTINCT notes.pubkey
         FROM events notes
         WHERE notes.kind=1
           AND NOT EXISTS (SELECT 1 FROM events profile WHERE profile.kind=0 AND profile.pubkey=notes.pubkey)
           AND NOT EXISTS (
             SELECT 1 FROM profile_hydration_attempts attempt
             WHERE attempt.pubkey=notes.pubkey AND attempt.relay=?1 AND attempt.checked_at>=?2)
         ORDER BY notes.pubkey LIMIT 500",
    )?;
    let authors = statement
        .query_map((relay, now - 86400), |row| row.get::<_, Vec<u8>>(0))?
        .map(|bytes| Ok(PublicKey::from_slice(&bytes?)?))
        .collect();
    authors
}

async fn hydrate_missing_profiles(
    path: &Path,
    client: &Client,
    policy: &Policy,
    relay: &str,
    now: i64,
) -> Result<Observation, Error> {
    let authors = missing_profile_authors(path, relay, now)?;
    if authors.is_empty() {
        return Ok(Observation {
            eose: true,
            ..Observation::default()
        });
    }
    let observed = scan(
        client,
        policy,
        relay,
        Filter::new()
            .authors(authors.iter().copied())
            .kind(Kind::Metadata)
            .limit(500),
        Duration::from_secs(15),
    )
    .await?;
    let mut conn = admin(path)?;
    let tx = conn.transaction()?;
    for author in authors {
        tx.execute(
            "INSERT INTO profile_hydration_attempts(pubkey,relay,checked_at) VALUES(?1,?2,?3)
             ON CONFLICT(pubkey,relay) DO UPDATE SET checked_at=excluded.checked_at",
            (author.as_bytes(), relay, now),
        )?;
    }
    tx.commit()?;
    Ok(observed)
}

/// Replaces admission state, then removes newly excluded stored notes.
pub async fn apply_nsfw(
    sdk: &NostrSqlite,
    policy: &Policy,
    nsfw: BTreeSet<PublicKey>,
    now: u64,
) -> Result<(), Error> {
    *policy.nsfw.write().unwrap() = nsfw;
    prune(sdk, policy, now).await
}

/// Deletes expired, NSFW-author, and locally blocked events through the SDK.
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
pub(crate) async fn collect_relay(
    path: &Path,
    client: &Client,
    policy: &Policy,
    sdk: &NostrSqlite,
    relay: &str,
    unsupported_reconciliation: &mut BTreeSet<String>,
    deadline: tokio::time::Instant,
) -> Result<(), Error> {
    let now_u64 = Timestamp::now().as_secs();
    let now = i64::try_from(now_u64)?;
    let mut admitted = Observation {
        eose: true,
        ..Observation::default()
    };
    let recent = scan(
        client,
        policy,
        relay,
        Filter::new()
            .kind(Kind::TextNote)
            .since(Timestamp::from(u64::try_from(now - COVERAGE_STEP)?))
            .until(Timestamp::from(now_u64))
            .limit(PAGE_LIMIT),
        Duration::from_secs(15),
    )
    .await?;
    if !recent.eose {
        return Err("Recent scan timed out; partial arrivals retained".into());
    }
    merge(&mut admitted, recent);

    let state = cursor(path, relay, now)?;
    let mut reconciliation_supported = !unsupported_reconciliation.contains(relay);
    if state.forward_at <= now {
        let until = (state.forward_at + COVERAGE_STEP - 1).min(now);
        let result = durable_window(
            path,
            client,
            policy,
            sdk,
            CoverageWindow {
                relay,
                phase: "forward",
                since: state.forward_at,
                until,
                cursor_column: "forward_at",
                cursor_value: until + 1,
                reconciliation_unsupported: !reconciliation_supported,
            },
            deadline,
        )
        .await?;
        reconciliation_supported &= result.reconciled;
        if !result.reconciled {
            unsupported_reconciliation.insert(relay.to_owned());
        }
        merge(&mut admitted, result.observed);
    }
    let cutoff = now - WINDOW;
    if state.backfill_before > cutoff {
        let since = state
            .backfill_before
            .saturating_sub(COVERAGE_STEP)
            .max(cutoff);
        let until = state.backfill_before - 1;
        let result = durable_window(
            path,
            client,
            policy,
            sdk,
            CoverageWindow {
                relay,
                phase: "backfill",
                since,
                until,
                cursor_column: "backfill_before",
                cursor_value: since,
                reconciliation_unsupported: !reconciliation_supported,
            },
            deadline,
        )
        .await?;
        reconciliation_supported &= result.reconciled;
        if !result.reconciled {
            unsupported_reconciliation.insert(relay.to_owned());
        }
        merge(&mut admitted, result.observed);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err("Relay work budget reached between fully drained scans".into());
    }
    let (metadata, parents) =
        hydrate_notes(client, policy, sdk, relay, &admitted.accepted_notes).await?;
    let missing_profiles = hydrate_missing_profiles(path, client, policy, relay, now).await?;
    eprintln!(
        "SDK notes={} rejected={}; metadata={} metadata_eose={}; missing_profiles={} missing_profiles_eose={}; parents={} parents_eose={}; reconciliation_supported={}; relay={relay}; completed reconciliation windows are durable, full retention coverage not established",
        admitted.accepted_notes.len(),
        admitted.rejected,
        metadata.ids.len(),
        metadata.eose,
        missing_profiles.ids.len(),
        missing_profiles.eose,
        parents.accepted_notes.len(),
        parents.eose,
        reconciliation_supported,
    );
    Ok(())
}

pub(crate) struct EmbeddingQuery {
    pub query: String,
    pub reply: oneshot::Sender<Result<(), String>>,
}

async fn cache_embedding_query(
    path: &Path,
    transport: &dyn Transport,
    budget: Budget,
    request: EmbeddingQuery,
    now: i64,
) -> Result<(), Error> {
    let result = embed::cache_query(path, transport, budget, &request.query, now)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string());
    let failure = result.as_ref().err().cloned();
    let _ = request.reply.send(result);
    match failure {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

pub(crate) async fn embed_until_next_scan(
    path: &Path,
    transport: &dyn Transport,
    budget: Budget,
    queries: &mut mpsc::Receiver<EmbeddingQuery>,
    topics_dirty: &mut bool,
    next_scan_at: tokio::time::Instant,
) -> Result<(), Error> {
    loop {
        if tokio::time::Instant::now() >= next_scan_at {
            return Ok(());
        }
        let now = i64::try_from(Timestamp::now().as_secs())?;
        if let Ok(request) = queries.try_recv() {
            cache_embedding_query(path, transport, budget, request, now).await?;
            continue;
        }
        let count =
            embed::embed_recent_pending(path, transport, budget, now, transport.concurrency())
                .await?;
        if count == 0 {
            if *topics_dirty {
                let count = embed::cluster_topics(path, transport.space(), now)?;
                *topics_dirty = false;
                eprintln!(
                    "Built {count} keyword-labelled {} topics",
                    transport.space().backend
                );
            }
            tokio::select! {
                request = queries.recv() => {
                    if let Some(request) = request {
                        cache_embedding_query(path, transport, budget, request, now).await?;
                        continue;
                    }
                }
                _ = tokio::time::sleep_until(next_scan_at) => {}
            }
            return Ok(());
        }
        *topics_dirty = true;
        eprintln!(
            "Embedded {count} {} posts between SDK scans",
            transport.space().backend
        );
        tokio::task::yield_now().await;
    }
}

/// Runs moderation refresh, bounded collection, hydration, and cleanup in one writer sequence.
pub struct EmbeddingWorker {
    pub transport: Arc<dyn Transport>,
    pub budget: Budget,
    pub queries: mpsc::Receiver<EmbeddingQuery>,
    pub error: Arc<Mutex<Option<String>>>,
    pub disabled: bool,
    pub validated: bool,
}

pub(crate) async fn service_embeddings(
    path: &Path,
    worker: &mut EmbeddingWorker,
    topics_dirty: &mut bool,
    duration: Duration,
) {
    if worker.disabled {
        return;
    }
    if !worker.validated {
        let now = i64::try_from(Timestamp::now().as_secs()).unwrap();
        let preflight = if let Ok(request) = worker.queries.try_recv() {
            cache_embedding_query(path, worker.transport.as_ref(), worker.budget, request, now)
                .await
                .map(|()| 1)
        } else {
            embed::embed_recent_pending(path, worker.transport.as_ref(), worker.budget, now, 1)
                .await
        };
        match preflight {
            Ok(0) => return,
            Ok(_) => {
                worker.validated = true;
                *topics_dirty = true;
            }
            Err(error) => {
                let message = format!(
                    "{} embedding preflight failed: {error}",
                    worker.transport.space().backend
                );
                *worker.error.lock().unwrap() = Some(message.clone());
                worker.disabled = true;
                eprintln!("{message}; paid embedding is disabled until process restart");
                return;
            }
        }
    }
    match embed_until_next_scan(
        path,
        worker.transport.as_ref(),
        worker.budget,
        &mut worker.queries,
        topics_dirty,
        tokio::time::Instant::now() + duration,
    )
    .await
    {
        Ok(()) => *worker.error.lock().unwrap() = None,
        Err(error) => {
            let message = format!(
                "{} embedding between completed SDK work failed: {error}",
                worker.transport.space().backend
            );
            *worker.error.lock().unwrap() = Some(message.clone());
            worker.disabled = true;
            eprintln!("{message}; paid embedding is disabled until process restart");
        }
    }
}

pub async fn run(
    path: &Path,
    sdk: NostrSqlite,
    relays: Vec<String>,
    profile_relays: Vec<String>,
    root: PublicKey,
    mut embedding: Option<EmbeddingWorker>,
) -> Result<(), Error> {
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
    let mut moderation_refresh_at = Timestamp::now().as_secs() + MODERATION_REFRESH;
    let client = client(sdk.clone(), policy.clone());
    for relay in relays.iter().chain(&profile_relays) {
        client.add_relay(relay).await?;
    }
    for relay in &relays {
        cursor(path, relay, i64::try_from(Timestamp::now().as_secs())?)?;
    }
    client.connect().await;
    for relay in relays.iter().chain(&profile_relays) {
        let result = scan(
            &client,
            &policy,
            relay,
            Filter::new()
                .author(root)
                .kinds([Kind::Metadata, Kind::ContactList, Kind::RelayList]),
            Duration::from_secs(15),
        )
        .await;
        match result {
            Ok(observed) if observed.eose => eprintln!(
                "Hydrated root profile and follow list; events={}; relay={relay}",
                observed.ids.len()
            ),
            Ok(_) => {
                let reason = "root profile/follow hydration timed out; Social graph may be empty";
                record_gap(path, relay, 0, 0, reason)?;
                eprintln!("{reason}; relay={relay}; collector continues");
            }
            Err(error) => {
                let reason = format!("root profile/follow hydration failed: {error}");
                record_gap(path, relay, 0, 0, &reason)?;
                eprintln!("{reason}; relay={relay}; collector continues");
            }
        }
    }
    let mut missing_contacts = missing_direct_follow_contact_lists(&sdk, root).await?;
    for relay in relays.iter().chain(&profile_relays) {
        if missing_contacts.is_empty() {
            break;
        }
        let requested = missing_contacts.len();
        let result = scan(
            &client,
            &policy,
            relay,
            Filter::new()
                .authors(missing_contacts.iter().copied())
                .kind(Kind::ContactList)
                .limit(requested),
            Duration::from_secs(15),
        )
        .await;
        match result {
            Ok(observed) if observed.eose => {
                missing_contacts = missing_direct_follow_contact_lists(&sdk, root).await?;
                eprintln!(
                    "Hydrated direct-follow contact lists; requested={requested} missing={}; relay={relay}",
                    missing_contacts.len()
                );
            }
            Ok(_) => eprintln!(
                "Direct-follow contact-list hydration timed out; requested={requested}; relay={relay}; collector continues"
            ),
            Err(error) => eprintln!(
                "Direct-follow contact-list hydration failed: {error}; requested={requested}; relay={relay}; collector continues"
            ),
        }
    }
    let mut unsupported_reconciliation = BTreeSet::new();
    let mut topics_dirty = false;
    loop {
        *policy.blocked.write().unwrap() = load_blocks()?;
        let now_u64 = Timestamp::now().as_secs();
        let now = i64::try_from(now_u64)?;
        if now_u64 >= moderation_refresh_at {
            let nsfw = bootstrap(&sdk).await?;
            apply_nsfw(&sdk, &policy, nsfw, now_u64).await?;
            moderation_refresh_at = now_u64 + MODERATION_REFRESH;
            eprintln!("Verified Primal moderation snapshots refreshed before further admission");
        } else {
            prune(&sdk, &policy, now_u64).await?;
        }
        for relay in &relays {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(RELAY_WORK_BUDGET);
            let error = collect_relay(
                path,
                &client,
                &policy,
                &sdk,
                relay,
                &mut unsupported_reconciliation,
                deadline,
            )
            .await
            .err()
            .map(|error| error.to_string());
            if let Some(error) = error {
                let reason = format!("recoverable relay cycle failure: {error}");
                record_gap(path, relay, now - COVERAGE_STEP, now, &reason)?;
                eprintln!("{reason}; relay={relay}; HTTP reader remains available");
            }
            if let Some(worker) = &mut embedding {
                service_embeddings(
                    path,
                    worker,
                    &mut topics_dirty,
                    Duration::from_secs(EMBEDDING_INTERLEAVE),
                )
                .await;
            }
        }
        let next_scan_at = tokio::time::Instant::now() + Duration::from_secs(30);
        if let Some(worker) = &mut embedding {
            service_embeddings(
                path,
                worker,
                &mut topics_dirty,
                next_scan_at.saturating_duration_since(tokio::time::Instant::now()),
            )
            .await;
        }
        tokio::time::sleep_until(next_scan_at).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn coverage_cursor_survives_restart_and_interruption_becomes_gap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.sqlite");
        let now = i64::try_from(Timestamp::now().as_secs()).unwrap();
        let sdk = open(&path, PRIMAL_AUTHOR).await.unwrap();
        let initial = cursor(&path, "wss://relay.example", now).unwrap();
        let complete = Observation {
            eose: true,
            ..Observation::default()
        };
        let forward = begin_run(
            &path,
            "wss://relay.example",
            "forward",
            initial.forward_at,
            initial.forward_at + COVERAGE_STEP - 1,
            now,
        )
        .unwrap();
        finish_run(
            &path,
            forward,
            &complete,
            (
                "wss://relay.example",
                "forward_at",
                initial.forward_at + COVERAGE_STEP,
            ),
        )
        .unwrap();
        let advanced = cursor(&path, "wss://relay.example", now + 1).unwrap();
        assert_eq!(advanced.forward_at, initial.forward_at + COVERAGE_STEP);
        begin_run(
            &path,
            "wss://relay.example",
            "backfill",
            initial.backfill_before - COVERAGE_STEP,
            initial.backfill_before - 1,
            now,
        )
        .unwrap();
        drop(sdk);
        let _reopened = open(&path, PRIMAL_AUTHOR).await.unwrap();
        let conn = admin(&path).unwrap();
        let gap: String = conn
            .query_row(
                "SELECT reason FROM collection_gaps ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(gap, "interrupted before EOSE; partial arrivals retained");
        assert_eq!(
            cursor(&path, "wss://relay.example", now + 2)
                .unwrap()
                .backfill_before,
            initial.backfill_before
        );
    }
}
