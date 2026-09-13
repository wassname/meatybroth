//! Rejects unsafe or out-of-scope Nostr events before the SDK writes them.
//!
//! Each active subscription also records events needed for coverage and storage-drain checks.
//! -- Pi/gpt-5.6-sol

use futures_util::future::BoxFuture;
use nostr_sdk::prelude::*;
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, LazyLock, Mutex, RwLock},
};

static SECRETS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"nsec1[0-9a-z]{30,}|-----BEGIN[A-Z ]*PRIVATE KEY-----|AKIA[0-9A-Z]{16}|sk-ant-[A-Za-z0-9_-]{20,}|\bsk-[A-Za-z0-9]{20,}\b|gh[pousr]_[A-Za-z0-9]{30,}\b").unwrap()
});
static EXPLICIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(hardcore sex|uncensored nudity|explicit sex scene)\b").unwrap()
});
static URLS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+").unwrap());

/// Returns local warning labels without changing admission.
pub fn text_labels(text: &str) -> Vec<String> {
    let mut labels = Vec::new();
    if EXPLICIT.is_match(text) {
        labels.push("auto-flagged: explicit".into());
    }
    if URLS.find_iter(text).count() > 5 && text.split_whitespace().count() < 40 {
        labels.push("auto-flagged: spam link-farm".into());
    }
    labels
}
/// Detects credential-shaped content and tags that must never be stored.
pub fn secret(event: &Event) -> bool {
    SECRETS.is_match(&event.content)
        || event
            .tags
            .iter()
            .any(|tag| tag.as_slice().iter().any(|value| SECRETS.is_match(value)))
}
/// Identity needed to confirm an accepted event reached a canonical SDK state.
#[derive(Debug, Clone)]
pub struct Accepted {
    pub pubkey: PublicKey,
    pub kind: Kind,
    pub created_at: Timestamp,
}

/// Events observed during one bounded relay subscription.
#[derive(Debug, Default, Clone)]
pub struct Observation {
    pub ids: BTreeSet<EventId>,
    pub oldest: Option<Timestamp>,
    pub rejected: usize,
    pub accepted: BTreeMap<EventId, Accepted>,
    pub accepted_notes: BTreeSet<EventId>,
    pub eose: bool,
}
/// Shared admission state and per-subscription observations.
#[derive(Debug, Clone)]
pub struct Policy {
    pub blocked: Arc<RwLock<BTreeSet<String>>>,
    pub nsfw: Arc<RwLock<BTreeSet<PublicKey>>>,
    requests: Arc<Mutex<HashMap<SubscriptionId, (Filter, Observation)>>>,
}
impl Policy {
    /// Creates policy state from the local blocklist and verified NSFW authors.
    pub fn new(blocked: BTreeSet<String>, nsfw: BTreeSet<PublicKey>) -> Self {
        Self {
            blocked: Arc::new(RwLock::new(blocked)),
            nsfw: Arc::new(RwLock::new(nsfw)),
            requests: Arc::default(),
        }
    }
    /// Registers the exact filter before its relay subscription starts.
    pub fn begin(&self, id: SubscriptionId, filter: Filter) {
        assert!(self
            .requests
            .lock()
            .unwrap()
            .insert(id, (filter, Observation::default()))
            .is_none());
    }
    /// Removes and returns one subscription's observation.
    pub fn finish(&self, id: &SubscriptionId) -> Observation {
        self.requests.lock().unwrap().remove(id).unwrap().1
    }
    /// Applies storage exclusions after signature and filter validation.
    pub fn reject(&self, event: &Event) -> bool {
        secret(event)
            || !matches!(event.kind.as_u16(), 0 | 1 | 3 | 10002)
            || self
                .blocked
                .read()
                .unwrap()
                .contains(&event.pubkey.to_hex())
            || self.blocked.read().unwrap().contains(&event.id.to_hex())
            || (event.kind == Kind::TextNote && self.nsfw.read().unwrap().contains(&event.pubkey))
    }
}
impl AdmitPolicy for Policy {
    fn admit_event<'a>(
        &'a self,
        _relay: &'a RelayUrl,
        id: &'a SubscriptionId,
        event: &'a Event,
    ) -> BoxFuture<'a, Result<AdmitStatus, nostr_sdk::error::Error>> {
        Box::pin(async move {
            let mut requests = self.requests.lock().unwrap();
            let Some((filter, observed)) = requests.get_mut(id) else {
                return Ok(AdmitStatus::rejected("inactive request"));
            };
            if !filter.match_event(event, Default::default()) || event.verify().is_err() {
                return Ok(AdmitStatus::rejected("invalid event or filter"));
            }
            // Reverify bytes because the SDK 0.45.2 verification cache is keyed by event ID. -- Pi/gpt-5.6-sol
            // Source: https://docs.rs/nostr-sdk/0.45.2/src/nostr_sdk/shared.rs.html#73-90
            observed.ids.insert(event.id);
            observed.oldest = Some(
                observed
                    .oldest
                    .map_or(event.created_at, |old| old.min(event.created_at)),
            );
            if self.reject(event) {
                observed.rejected += 1;
                Ok(AdmitStatus::rejected("collection policy"))
            } else {
                observed.accepted.insert(
                    event.id,
                    Accepted {
                        pubkey: event.pubkey,
                        kind: event.kind,
                        created_at: event.created_at,
                    },
                );
                if event.kind == Kind::TextNote {
                    observed.accepted_notes.insert(event.id);
                }
                Ok(AdmitStatus::Success)
            }
        })
    }
}
