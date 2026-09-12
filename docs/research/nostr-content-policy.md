# Nostr content-policy research

This note records the source basis for Meaty Broth's minimal controls. It is a
public, read-only, text-only caching reader, not a relay and not an upload
service. The research was checked on 2026-09-12. It is not legal advice.

## Protocol and operator patterns

[strfry's plugin documentation](https://github.com/hoytech/strfry/blob/master/docs/plugins.md)
describes operator policy as a separate decision layer:

> plugins can be used for the following: * White/black-lists (particular
> pubkeys can/can't post events) * Rate-limits * Spam filtering

Meaty Broth uses the same simple shape inside its collector: accept, flag, or
reject with a reason. It does not implement strfry's plugin protocol.

[NIP-36](https://nips.nostr.com/36) defines the standard author-supplied
sensitivity label:

> The `content-warning` tag enables users to specify if the event's content
> needs to be approved by readers to be shown. Clients can hide the content
> until the user acts on it.

[NIP-56](https://nips.nostr.com/56) treats reports as signals rather than an
automatic verdict:

> A report is a `kind 1984` event that signals to users and relays that some
> referenced content is objectionable. The definition of objectionable is
> obviously subjective and all agents on the network (users, apps, relays,
> etc.) may consume and take action on them as they see fit.

> It is not recommended that relays perform automatic moderation using reports,
> as they can be easily gamed.

Operator documents show a recurring practical combination: state the limits,
provide a report route, and remove or block material when the operator decides
that action is required. Examples include
[nostr.wine's terms](https://nostr.wine/terms) and
[DMCA page](https://nostr.wine/dmca). These are operator self-descriptions, not
independent evidence that every stated process is used.

## Implemented controls

Meaty Broth currently applies these controls:

1. Common private-key and API-key patterns are rejected before persistence.
   Logs retain a rule name and event ID, not the matched content.
2. The operator blocklist accepts event IDs and pubkeys. Matching stored posts
   are removed, and later collection cannot restore them while blocked.
3. NIP-36 content warnings and narrowly detected explicit text require a user
   action before the post body is shown. Health, sexuality, and identity terms
   alone are not explicit-content matches.
4. Duplicate-content and link-heavy spam signals are labelled or demoted rather
   than automatically deleted.
5. Imported HTML is sanitized. Remote images, media, embeds, and link previews
   are not loaded.
6. The Terms page links to a GitHub issue form for reports. It asks for the Nostr
   event ID and a short reason, not copied sensitive content, screenshots, or
   credentials.

The site cannot delete signed events from Nostr relays. A local block disables
rendering and re-ingestion only for this reader. Thirty-day expiry limits served
post age but is not a secure-erasure claim.

## Legal scope

Liability and reporting duties depend on the operator's jurisdiction and the
service's legal classification. Official examples include
[17 U.S.C. § 512](https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title17-section512&num=0&edition=prelim),
the EU [Digital Services Act](https://eur-lex.europa.eu/legal-content/EN/TXT/HTML/?uri=CELEX%3A32022R2065),
and Australia's [Online Safety Act 2021](https://www.legislation.gov.au/C2021A00076/latest).
The project does not claim a safe harbour or legal immunity. The operational
commitment is narrower: maintain a working report route and blocklist, inspect
reports, and act according to applicable law.
