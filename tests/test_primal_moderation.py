"""Primal categorized-list integration with synthetic signed events."""

import pytest

from meatybroth import ingest
from meatybroth.ingest import collect_once, primal_list_members
from meatybroth.store import Store
from meatybroth.web import _warning_hides, create_app
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


def test_reconcile_primal_membership_replaces_only_primal_categories(store):
    post = sign_event(1, "stored", created_at=NOW - 60, secret=KEY_C)
    store.upsert(ingest.nostr_post(post, "test"), now=NOW)
    store.save_content_warning(post["id"], "spam", "auto-flagged: spam link-farm", now=NOW)
    store.save_content_warning(post["id"], "author-cw", "author: label", now=NOW)
    assert store.reconcile_primal_list("spam_list", {PUB_C}, now=NOW) == 1
    assert "auto-flagged: spam Primal snapshot" in store.content_warnings([post["id"]])[post["id"]]
    assert store.reconcile_primal_list("spam_list", set(), now=NOW + 1) == 0
    assert set(store.content_warnings([post["id"]])[post["id"]]) == {
        "author: label", "auto-flagged: spam link-farm"}


def test_status_renders_primal_provenance(store):
    store.set_status("primal-moderation", {"cache": "wss://cache2.primal.net/v1", "lists": {
        "spam_list": {"event_created_at": 1726079985, "members": 13, "refresh": "verified"}}}, now=NOW)
    response = create_app(store, ROOT).test_client().get("/status")
    text = response.get_data(as_text=True)
    assert response.status_code == 200
    assert "primal-moderation" in text
    assert "1726079985" in text
    assert "wss://cache2.primal.net/v1" in text


def test_primal_nsfw_membership_deletes_stored_posts_and_removal_needs_resync(store):
    post = sign_event(1, "stored", created_at=NOW - 60, secret=KEY_C)
    store.upsert(ingest.nostr_post(post, "test"), now=NOW)
    assert store.reconcile_primal_list("nsfw_list", {PUB_C}, now=NOW) == 1
    assert store.get(f"nostr:{post['id']}", now=NOW) is None
    assert store.count(now=NOW) == 0
    assert store.reconcile_primal_list("nsfw_list", set(), now=NOW + 1) == 0
    assert store.get(f"nostr:{post['id']}", now=NOW) is None  # requires relay re-sync


def test_primal_spam_labels_and_nsfw_excludes_before_storage(store):
    # A synthetic signed post by the synthetic direct follow becomes a member of
    # both verified snapshots; normal collection applies distinct categories.
    store.set_moderation_list("primal", "spam_list", event_id="1" * 64, author=PUB_C,
                              event_created_at=NOW - 10, checked_at=NOW, members={PUB_C})
    store.set_moderation_list("primal", "nsfw_list", event_id="2" * 64, author=PUB_C,
                              event_created_at=NOW - 10, checked_at=NOW, members={PUB_C})
    post = sign_event(1, "synthetic member post", created_at=NOW - 60, secret=KEY_C)
    root_follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]], secret=SECRET)
    report = run(store, {"wss://purplepag.es": ([root_follow, post], True)})
    assert store.get(f"nostr:{post['id']}", now=NOW) is None
    assert report["nostr"]["filter_stats"]["primal_nsfw"] == 1
    assert store.content_warnings([post["id"]]) == {}
