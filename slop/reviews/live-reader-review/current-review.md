# Live reader acceptance review, before follow-up restart

Reviewed `http://localhost:8088/` on 2026-09-13 15:05–15:17 UTC. The application owner confirmed this was the intended `d1d72fc` release. This is a user review of the running app, with read-only SQLite checks for model separation and moderation.

## Result

The reader is useful enough to explore, but it does not yet meet the continuous collection and embedding result in the plan.

### Blockers

1. The review process is read-only and stale. `MEATYBROTH_READ_ONLY=1`; status said “As of 2026-09-13 15:16 UTC” while both relay reports stopped at 13:25 UTC. The page did not explain that the current process was read-only.
2. A recent visible post is not embedded. [`0d2afb…f196e7` Similar](http://localhost:8088/?mode=similar&similar=nostr:0d2afb8491e5385964aaef3d0945dfbb26b931d292d202b9cc62788259f196e7&embedding=minilm) returned HTTP 400, “This post is absent from the selected embedding cache.” SQLite records receipt at 13:25:41 UTC and no embedding row.
3. [`/status`](http://localhost:8088/status) ended after moderation lists. It did not show embedding eligibility, stored/pending counts, model provenance, tokens, cost, or uncertain requests. The app owner accepted this finding and is preparing one follow-up restart.
4. Similar does not preserve enough context. From the wallet-security post [`d3cca7…e2b8bac`](http://localhost:8088/?mode=similar&similar=nostr:d3cca7e9dcb5f494aafe98705ac647c76e3e6a87e566a38216a10c505e2b8bac&embedding=minilm), the result page shows Feed “New”, no source post, and no “similar to” heading. A reader arriving there cannot tell what the results are similar to.
5. MiniLM Similar is polluted at the top. The wallet-security example starts with two duplicate, auto-flagged replies saying “Consider the security implications of not requiring KYC,” followed by generic “check reviews and security features.” Useful results about hardware-wallet entropy, external signers and Monerujo appear later.
6. Topic labels do not reliably explain membership. MiniLM’s `relays · swarm · clients` cluster is coherent but consists largely of repeated raw `zone_presence` JSON. `trading · bots · sylunara` is coherent duplicate spam. The separate `bitcoin · price · sats` cluster mixes a price bot, a timestamp, block art and market posts. Titan’s [`supply · chain · china`](http://localhost:8088/?mode=topics&topic=5&embedding=titan) sample included AI regulation, relay infrastructure, “Correct.”, Formula 1 and gold. The Titan page only says “incomplete cached one-off corpus”; it does not state that the 119 posts are PoW-biased and unrepresentative.

### Working outcomes

- Paging is real: [`page=0`](http://localhost:8088/?mode=new&page=0) and [`page=1`](http://localhost:8088/?mode=new&page=1) each returned 100 unique posts with zero overlap.
- Long-text behavior matched the latest rule in three 100-post samples. All 68 collapsed posts hid at least 150 Unicode characters; the observed minimum was 151. Shorter remainders were not collapsed in those samples.
- Names and available addresses render together. Each sampled New page showed 100 authors and 9 addresses, including `Johnny · thejohnnycrypto@primal.net` and `Alert · alert@inostr.com`. Most missing profiles fall back to 16 hex characters.
- Thread context works. [`d3cca7…e2b8bac` context](http://localhost:8088/context/nostr/d3cca7e9dcb5f494aafe98705ac647c76e3e6a87e566a38216a10c505e2b8bac) returned HTTP 200 in 2.04 s and rendered seven stored events, including `Farmer J · farmerj@rizful.com`.
- [`Social`](http://localhost:8088/?mode=discovery) is no longer blank. It returned 12 posts in 2.80 s and labelled them `1 hop · you follow`. The control default showed “2 hops”; no two-hop result appeared in this sample.
- Text-only rendering is preserved. Image and video URLs remain links. No inline media appeared. The New feed is nevertheless hard to read because repeated raw JSON occupies the first cards.
- The fixed query [`running language models without internet`, Words](http://localhost:8088/?q=running%20language%20models%20without%20internet&mode=relevance) returned no posts. [Meaning](http://localhost:8088/?q=running%20language%20models%20without%20internet&mode=meaning&embedding=minilm) returned useful local-model posts: CPU-only Hugging Face adaptation, a reader who will use AI when it stays local, and open-source LM Studio alternatives. It also ranked an unrelated developer job ad second.
- My query [`private key`, Words](http://localhost:8088/?q=private%20key&mode=relevance) returned one directly matching post. [Meaning](http://localhost:8088/?q=private%20key&mode=meaning&embedding=minilm) returned the same post plus on-device keys and encrypted messaging, but ranked the one-word post “unlock” first and showed duplicate promotional replies.
- Known Primal moderation members are excluded before storage: the live database had zero kind-1 posts and zero embeddings from all 157 NSFW members and 13 spam members. Heuristic `spam duplicate-content` posts remain visible and labelled.
- MiniLM and Titan are visibly and mechanically separate. The database has 5,274 MiniLM 384-dimensional vectors and 119 Titan 512-dimensional vectors, with zero dimension/byte mismatches. MiniLM results say “MiniLM cosine similarity”; Titan results say “Titan cosine similarity.” An uncached Titan text query returns an explicit HTTP 400 rather than silently using MiniLM. There are currently zero cached query rows, so Titan text Meaning has no usable query in this snapshot. Titan topics total 119 posts.

### Cosmetic and reader-text issues

- The pot and some flag emoji render as square missing-glyph boxes in current and baseline screenshots.
- Adding Vectors makes the selector wrap to a second line on desktop. The search box clips longer query text.
- [`/about`](http://localhost:8088/about) says the corpus is “followed Nostr posts,” which conflicts with the agreed selected-relay ingestion scope.
- Compared with the saved baseline, layout and typography remain recognizable. Current New is less readable because the top cards are duplicate protocol JSON rather than conversations. The current header has more controls but weaker grouping.

## Screenshot reading

I opened each saved PNG after capture.

- `01-home-new.png`: a familiar plain text reader, but the first two cards are near-identical internal `zone_presence` JSON. A duplicate-spam warning appears on the next card.
- `02-meaning-security.png`: the query and MiniLM mode are visible. The first four cards are mostly understandable security matches, including French phishing, private-key backup, identity exposure and multisignature wallets.
- `03-topics-minilm.png`: eleven bare keyword links and counts. There is no prose explaining quality, spam-list overlap, or why a label fits.
- `04-topic-trading-spam.png`: the selected topic is visible and its first cards are obvious repeated trading promotions, each labelled duplicate-content spam.
- `05-similar-wallet-security.png`: results and scores are visible, but the missing source/heading and Feed “New” make the page’s purpose unclear.
- `06-social.png`: a non-empty feed with hop labels, replies and readable thread snippets. Most visible authors lack addresses.
- `07-topics-titan.png`: six weak labels and the incomplete-cache warning. Nothing visible says 119 PoW-biased posts.
- `08-status-before-followup.png`: current “As of” time above relay reports almost two hours old, with no read-only warning or embedding summary.

## Evidence

- Raw browser observations: `browser-observations.jsonl`, `topic-membership.jsonl`, `similar-observations.jsonl`, `similar-security.json`
- HTTP results: `http-timings.tsv`, `paging-timings.tsv`, `private-key-timings.tsv`, `context-timing.tsv`, `similar-newest-timing.tsv`
- Checks: `runtime-state.txt`, `paging-check.txt`, `profile-render-check.txt`, `longtext-check.txt`, `moderation-storage-check.txt`
- Screenshots: `01-home-new.png` through `08-status-before-followup.png`

Remaining uncertainty: I did not open external Nostr/media links, make AWS calls, restart the owner’s service, or infer semantic quality from cosine values. Media-link content was not inspected, so the text-only check does not establish that every linked image is safe.

-- Pi/gpt-5.6-sol
