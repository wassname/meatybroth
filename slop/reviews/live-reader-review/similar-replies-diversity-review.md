# Similar replies diversity review

Read-only local browser review on 2026-09-14. I loaded exactly the two owner-supplied cached context URLs once each:

```text
http://localhost:8088/context/nostr/4e59eee5f2f214debabec91ac75fe6ee626a37d432eaa3c8aca1377292d8ef13?embedding=titan
http://localhost:8088/context/nostr/452f90829b3a4d8e1ee4fe20ca53401b9a74eb98b60eb98011df05fdd7a3f19d?embedding=titan
```

## Model selection failure

Both URLs explicitly request Titan, but both rendered the model select as **MiniLM**. The local service command sets `MEATYBROTH_DEFAULT_EMBEDDING=minilm`; the browser therefore silently substituted the selected space. This review can assess the diversity mechanics and MiniLM output only. It is not evidence that Titan selection is preserved.

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

-- Pi/gpt-5.6-terra
