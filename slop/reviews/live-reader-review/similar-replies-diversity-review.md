# Similar replies diversity review

Read-only local browser review on 2026-09-14. I loaded exactly the two owner-supplied cached context URLs once each:

```text
http://localhost:8088/context/nostr/4e59eee5f2f214debabec91ac75fe6ee626a37d432eaa3c8aca1377292d8ef13?embedding=titan
http://localhost:8088/context/nostr/452f90829b3a4d8e1ee4fe20ca53401b9a74eb98b60eb98011df05fdd7a3f19d?embedding=titan
```

## Model selection failure

An earlier pre-release review found that URLs explicitly requesting Titan rendered MiniLM because shared template data omitted the selected embedding. That review therefore assessed MiniLM output only. The final verification below supersedes this display failure.

## Mechanics

| source event | actual thread articles before heading | recommendations after heading | unique article IDs | recommendation parent/source keys |
|:---|---:|---:|---:|:---|
| `4e59eee5f2f214de…` | 30 | 5 | 35/35 | 5 distinct |
| `452f90829b3a4d8e…` | 23 | 5 | 28/28 | 5 distinct |

Raw tags from the live local database confirm distinct recommendation keys. For the saved source they are `db76…`, top-level own ID `000023…`, `2820…`, `0000fcc…`, and `7e94…`; for the fresh source they are `b4e6…`, `e57d…`, `ea1e…`, top-level own ID `6d41…`, and `f5f7…`. This supports that the one-per-known-direct-parent selection removed the former repeated `db76…` group. It does not assess all possible Nostr reply-tag shapes.

The browser requests had no Meaning query text. Context/Similar reads cached vectors and stored records, so these browser actions initiated no provider call. This does not claim that unrelated writer activity made no provider call.

## Quality examples

The saved source is a weak result after diversity: only `8ab990…` is directly relevant to the source's cashless-Dubai discussion; the other four include a Tehran Times travel link, Liquid debate, Portuguese sovereignty text, and an unrelated top-level post. Diversity reduced the repeated conversation but did not make the five recommendations useful as a set.

The fresh source is a post about trimming tech longs before CPI. `325c89…`, `39847b…`, and `792e41…` are useful related examples: CPI risk, holding cash, and position sizing. `6d69ee…` is a crypto signal promotion and `6d4185…` is a chess/late-night post; these are non-useful. The fresh screenshot makes this distinction visible.

Fresh screenshots opened locally: `41-local-similar-replies-diverse-saved.png` and `42-local-similar-replies-diverse-fresh.png`. They remain uncommitted image evidence for parent inspection.

## Final Titan verification

After the owner added the selected embedding to shared template data and an SDK assertion, I loaded exactly the same two URLs once more. Both now visibly select **Titan**. The saved page has 31 actual thread articles, then `Similar replies:`, then five recommendations (36 unique article IDs). The fresh page has 26 actual articles, then the heading, then five recommendations (31 unique IDs). The change from the earlier counts is consistent with the persistent local writer receiving extra actual replies; it is not evidence of a duplicate because each final page's article IDs are unique.

Read-only raw-tag checks on the final recommendation IDs again show five distinct direct-parent/source keys per page. The parent-grouping mechanism therefore remains effective after the display fix. The final fresh screenshot [`44-local-final-similar-fresh.png`](44-local-final-similar-fresh.png) was opened: Titan is selected; three recommendations concern CPI/tech exposure and are plausibly useful, while the crypto promotion and chess post remain clear misses. The final browser requests contained no Meaning query text and used cached context/Similar data; they initiated no provider call, without claiming anything about unrelated writer work.

Final screenshots `43-local-final-similar-saved.png` and `44-local-final-similar-fresh.png` remain uncommitted image evidence for parent inspection.

## Screenshot 44 visual-label audit

The apparent `e77b84fba220ed4c…` collision in screenshot 44 is an **author-label** collision, not evidence that the same canonical event appears above and below the heading. In the rendered card template, the visible header is `p.author`; the actual canonical event ID is only the `<article id="{{ p.canonical_id }}">` DOM attribute and the `Post details` event value. The screenshot does not show the latter.

Read-only current-database inspection finds 31 distinct events authored by `e77b84fba220ed4c…`. Their canonical IDs differ while many replies repeat finance/CPI language. For example, `f530cd9904d69ea…` and `140aaa5eca93f35…` reply directly to the supplied source `452f…`; `325c89fa5cf4c01…`, the final recommendation candidate, instead has direct parent `b4e6858cd0df5cdd…`. Thus an `e77…` header can occur on both sides with distinct canonical IDs and different parent keys.

The earlier final browser DOM count was collected in a temporary CDP result file that is no longer present, so this later audit cannot enumerate its exact before/after DOM IDs from that response. It should not be treated as preserved raw-HTML proof. The retained screenshot plus source/template and database evidence resolve the visual-label interpretation, while the exact-ID mechanics evidence remains limited to the contemporaneous documented DOM check. The live listener was timing out when this audit was requested; I made no fresh browser request.

The author’s dense repeated replies are a separate quality/moderation observation. Parent-key diversity prevents repeat *parent conversations* in recommendations; it does not limit repeated authors in the actual thread or prove useful retrieval.

-- Pi/gpt-5.6-terra
