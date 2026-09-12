# Nostr ingestion findings

These findings came from bounded, read-only relay checks. They support the
collector choices in `meatybroth/ingest.py`; they do not establish complete
relay or month-long coverage.

## Identity and relay discovery

The configured public identity returned different state from different relays:
bootstrap relays returned older or no follow-list metadata, while the identity's
advertised write relays returned newer state. EOSE with no match is not evidence
that an account or event is absent from Nostr.

The collector therefore fetches identity kinds without the 30-day post cutoff,
follows the author's advertised write relays within a connection budget, and
keeps the greatest `created_at` value for each `(pubkey, kind)`. Equal timestamps
use the lowest event ID so arrival order cannot change the result.

[NIP-02](https://github.com/nostr-protocol/nips/blob/master/02.md) defines the
follow list:

> A special event with kind `3`, meaning "follow list" is defined as having a
> list of `p` tags, one for each of the followed/known profiles one is following.

> Every new following list that gets published overwrites the past ones, so it
> should contain all entries.

[NIP-65](https://github.com/nostr-protocol/nips/blob/master/65.md) says:

> When downloading events **from** a user, clients SHOULD use the **write**
> relays of that user.

The older relay-list event was needed to locate the newer follow list. Applying
the post-retention cutoff to identity state would have hidden that route.

## Posts and reply context

All events in the bounded sample passed canonical event-ID hashing and BIP-340
signature verification. This checks the sample, not all future relay traffic.

One sampled reply and its root were available on multiple relays, while its
direct parent was missing from one relay that served the reply. The collector
therefore treats relay views as partial, fetches exact parent IDs within the
shared request budget, and keeps useful replies visible when context remains
missing.

EOSE only says that a request reached the end of the relay's stored results for
that filter. A limited or saturated response does not prove that a month of
history was retrieved. Backfill pages need explicit time cursors, overlap at
boundaries, deduplication by event ID, and visible incomplete-coverage status.

## Bounds

Thirty days bounds post age, not incoming volume, retries, graph size, or relay
message size. Current code separately limits request count, response bytes,
individual stored text, relay count, second- and third-hop discovery, and
backfill pages. These values are starting assumptions and should change only
from observed collection rates and misses.

A public key identifies a signing key. It does not prove one human, endorsement,
or control of an account on another network. Bridged posts retain their Nostr
signer and original link; the reader does not infer cross-network identity.
