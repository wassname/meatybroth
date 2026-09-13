"""Primal categorized-list integration. Synthetic signed events only. -- Terra"""

import pytest

from meatybroth import ingest
from meatybroth.ingest import collect_once, primal_list_members
from meatybroth.store import Store
from meatybroth.web import _warning_hides
from test_ingest import KEY_C, NOW, PUB_C, ROOT, SECRET, run, sign_event


@pytest.fixture
def store(tmp_path):
    return Store(tmp_path / "reader.db")


def primal_event(identifier, members):
    return sign_event(30000, "", created_at=NOW, secret=KEY_C,
                      tags=[["d", identifier], *[["p", member] for member in members]])


def test_valid_signed_list_requires_author_kind_and_d_tag():
    event = primal_event("spam_list", ["a" * 64])
    assert primal_list_members(event, "spam_list", author=PUB_C) == {"a" * 64}
    with pytest.raises(ValueError, match="d tag"):
        primal_list_members(event, "nsfw_list", author=PUB_C)
    with pytest.raises(ValueError, match="author"):
        primal_list_members(event, "spam_list")


def test_snapshot_replacement_removes_members(store):
    store.set_moderation_list("primal", "spam_list", event_id="1" * 64, author=PUB_C,
                              event_created_at=NOW - 10, checked_at=NOW, members={"a" * 64, "b" * 64})
    store.set_moderation_list("primal", "spam_list", event_id="2" * 64, author=PUB_C,
                              event_created_at=NOW, checked_at=NOW + 1, members={"b" * 64})
    saved = store.moderation_list("primal", "spam_list")
    assert saved["event_id"] == "2" * 64
    assert saved["members"] == {"b" * 64}


def test_primal_spam_labels_and_nsfw_hides(store):
    # A synthetic signed post by the synthetic direct follow becomes a member of
    # both verified snapshots; normal collection applies distinct categories.
    store.set_moderation_list("primal", "spam_list", event_id="1" * 64, author=PUB_C,
                              event_created_at=NOW - 10, checked_at=NOW, members={PUB_C})
    store.set_moderation_list("primal", "nsfw_list", event_id="2" * 64, author=PUB_C,
                              event_created_at=NOW - 10, checked_at=NOW, members={PUB_C})
    post = sign_event(1, "synthetic member post", created_at=NOW - 60, secret=KEY_C)
    root_follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]], secret=SECRET)
    report = run(store, {"wss://purplepag.es": ([root_follow, post], True)})
    warnings = store.content_warnings([post["id"]])
    assert set(warnings[post["id"]]) == {
        "auto-flagged: spam Primal spam snapshot", "curated-nsfw: Primal snapshot"}
    assert report["nostr"]["filter_stats"]["spam"] == 1
    assert not _warning_hides(["auto-flagged: spam Primal spam snapshot"])
    assert _warning_hides(["curated-nsfw: Primal snapshot"])
