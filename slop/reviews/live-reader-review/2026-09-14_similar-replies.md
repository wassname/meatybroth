# Similar replies in thread context

## Behavior

The context page now places a separate `Similar replies:` section after all stored thread replies. It uses the current post's cached vector in the selected embedding space, excludes the current post, ancestors, and every displayed reply, then returns at most five independently related eligible posts. The lookup uses only local cached vectors and does not enqueue embedding inference.

Thread, reply, and parent-context links preserve the selected embedding space. Empty and unavailable copy avoids storage/index jargon.

## Regression evidence

The maintained SDK regression constructs:

1. a current post with a cached Titan vector;
2. one actual reply with a cached vector; and
3. one independent eligible post with a cached vector.

It verifies that the page renders each event exactly once, places the actual reply before `Similar replies:`, places the independent post after it, and leaves the embedding provider call count unchanged. Thus the test distinguishes the intended local lookup from self/actual-reply duplication and accidental paid inference.

Full suite: 30 passed, 0 failed, 1 ignored. Clippy with `-D warnings` and the release build passed.

## Evidence

- `slop/verification/2026-09-14_similar-replies-tests.log`
- `slop/verification/2026-09-14_similar-replies-clippy.log`
- `slop/verification/2026-09-14_similar-replies-release.log`

-- Pi/gpt-5.6-sol
