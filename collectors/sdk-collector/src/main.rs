// sdk-collector M1 scaffold — policy-driven loop over verified SDK APIs.
// Method names/shapes pinned to nostrdevkit/nostr master @ 2026-09-11 sources.
// NOT compiled yet (M1 = scaffold only, HOLD on broader changes respected).

// NOTE: BoxedFuture is pub(crate) in the SDK, so external impls name the
// identical Pin<Box<dyn Future...>> type explicitly (verified in future.rs).
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use nostr_database::prelude::*;
use nostr_sdk::prelude::*;
use nostr_sqlite::builder::NostrSqliteBuilder;

const STORE_PATH: &str = ".local/collector-sdk/nostr.sqlite";
const RETENTION_DAYS: u64 = 30;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // ONE writer owns this file; the Python reader/exporter opens it read-only.
    let db = NostrSqliteBuilder::default()
        .in_file(STORE_PATH)
        .build()
        .await?;
    // Pre-store moderation gate: secret scan + blocklist + kind allowlist run
    // BEFORE save on live AND sync-download paths (AdmitPolicy::admit_event
    // receives the full verified event; see report §7b). Reads the same
    // file-based lists the current reader uses; no new tables in v1.
    let client: Client = ClientBuilder::default()
        .database(db.clone())
        .admit_policy(CollectionPolicy::from_files())
        .build();

    // Relay set comes from follow-graph policy (custom lane), capped; READ only.
    for url in policy_relays().await {
        client.add_relay(url).await?;
    }
    client.connect().await;

    // Live tail: persistent subscription, SDK auto-reconnect on drop.
    client.subscribe(policy_live_filters().await).await?;

    // History per work unit: negentropy where supported, paged fetch elsewhere.
    for unit in policy_history_work_units().await {
        match client
            .sync(unit.filter.clone())
            .opts(SyncOptions::default().direction(SyncDirection::Down))
            .await
        {
            Ok(out) => {
                record_gap(&unit, discovered(out.remote.len()), received(out.received.len()));
                // Acceptance rule: compare `received` against local store, never
                // treat successful return as completeness.
            }
            Err(e) if negentropy_unsupported(&e) => {
                // Declared-gap path for NEG-disabled relays (e.g. momostr.pink):
                // bounded paged fetch with (created_at, id) paging, else gap row.
                let events = client
                    .fetch_events(unit.filter.clone())
                    .timeout(Duration::from_secs(10))
                    .await?;
                record_fallback(&unit, events.len());
            }
            Err(e) => return Err(e.into()),
        }
    }

    // Retention stays explicit (backend has no auto-purge: event_expiration=false).
    let cutoff = Timestamp::now().as_secs().saturating_sub(RETENTION_DAYS * 86400);
    db.delete(Filter::new().until(Timestamp::from(cutoff))).await?;

    Ok(())
}

// ---- Pre-store admission policy (secrets MUST die here, never post-hoc) ----
#[derive(Debug)]
struct CollectionPolicy {
    // blocklist pubkeys, secret-pattern set, kind allowlist — file-loaded
}

impl CollectionPolicy {
    fn from_files() -> Self {
        todo!("load operator blocklist + Primal snapshots + secret patterns")
    }
}

impl AdmitPolicy for CollectionPolicy {
    fn admit_event<'a>(
        &'a self,
        _relay_url: &'a RelayUrl,
        _subscription_id: &'a SubscriptionId,
        event: &'a Event,
    ) -> Pin<Box<dyn Future<Output = Result<AdmitStatus, nostr_sdk::error::Error>> + Send + 'a>> {
        Box::pin(async move {
            // 1. synthetic-secret scan over content AND tag values → Rejected
            // 2. author blocklist / Primal-list membership → Rejected
            // 3. kind allowlist (0/1/3/10002 + policy kinds) → Rejected otherwise
            // M2 must prove a secret-bearing fixture event never reaches the DB.
            let _ = event;
            todo!("admission checks")
        })
    }
}

// ---- Collection-policy stubs (custom lane; follow-graph, cursors, gap store) ----
async fn policy_relays() -> Vec<String> {
    todo!("follow-graph policy: capped READ relay set")
}
async fn policy_live_filters() -> Filter {
    todo!("live-tail filters")
}
async fn policy_history_work_units() -> Vec<HistoryUnit> {
    todo!("per-(relay, filter) newest/history work units")
}
struct HistoryUnit {
    filter: Filter,
}
fn discovered(n: usize) -> usize {
    n
}
fn received(n: usize) -> usize {
    n
}
fn record_gap(_unit: &HistoryUnit, _discovered: usize, _received: usize) {
    todo!("durable gap record")
}
fn record_fallback(_unit: &HistoryUnit, _n: usize) {
    todo!("fallback fetch record")
}
fn negentropy_unsupported(_e: &nostr_sdk::error::Error) -> bool {
    todo!("match SDK negentropy_not_supported / UnsupportedProtocolVersion branches")
}
