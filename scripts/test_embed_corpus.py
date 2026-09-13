"""Production-behavior tests for scripts/embed_corpus.py.

A fake embedder (deterministic per text) drives the real cmd_embed, checking
incremental reuse, expiry removal, id alignment under shuffled insertion
order, no-op reruns, and the 30-day window. No model download.
Regression runner (project env has no numpy/fastembed; do NOT move back under tests/):
  uv run --with pytest --with fastembed --with numpy python -m pytest scripts/test_embed_corpus.py -q
"""

import hashlib
import importlib.util
import json
import sqlite3
import sys
import time
from pathlib import Path

import numpy as np
import pytest

MOD_PATH = Path(__file__).resolve().parent / "embed_corpus.py"
spec = importlib.util.spec_from_file_location("embed_corpus", MOD_PATH)
mod = importlib.util.module_from_spec(spec)
sys.modules["embed_corpus"] = mod
spec.loader.exec_module(mod)

DIM = 8


class FakeEmbedding:
    def __init__(self, model_name=None, cache_dir=None):
        pass

    def embed(self, texts):
        for t in texts:
            seed = int.from_bytes(hashlib.sha256(t.encode()).digest()[:8], "little")
            rng = np.random.default_rng(seed)
            v = rng.normal(size=DIM)
            yield v / np.linalg.norm(v)


def fake_tokenizer(model, prov):
    return (lambda text: text.split(), lambda ids: " ".join(ids))


@pytest.fixture
def faked(monkeypatch, tmp_path):
    import fastembed

    monkeypatch.setattr(fastembed, "TextEmbedding", FakeEmbedding)
    monkeypatch.setattr(mod, "load_tokenizer", fake_tokenizer)
    return tmp_path


def make_db(path: Path, posts: list[tuple[str, str, str, int]]):
    path.unlink(missing_ok=True)
    db = sqlite3.connect(str(path))
    db.execute("CREATE TABLE posts (source TEXT, source_id TEXT, text TEXT, created_at INTEGER)")
    db.executemany("INSERT INTO posts VALUES (?,?,?,?)", posts)
    db.commit()
    db.close()


def run_embed(db: Path, out: Path):
    assert mod.main(["--db", str(db), "--out", str(out), "--cache-dir", str(out / "mc")]) == 0


def load_result(out: Path):
    ids, man, meta, mat = mod.load_generation(out)
    man_rows = [man[c] for c in ids]
    assert mat.shape[0] == len(ids) == len(man_rows)
    return mat, ids, man_rows


def test_incremental_expiry_alignment(faked):
    now = int(time.time())
    db, out = faked / "t.db", faked / "sem"
    posts = [
        ("s", "a", "alpha post about cats", now - 100),
        ("s", "b", "beta post about dogs", now - 200),
        ("s", "c", "gamma post about birds", now - 300),
        ("s", "long", " ".join(f"w{i}" for i in range(300)), now - 400),
    ]
    make_db(db, posts)
    run_embed(db, out)
    mat1, ids1, man1 = load_result(out)
    assert ids1 == ["s:a", "s:b", "s:c", "s:long"], ids1
    assert [r["canonical_id"] for r in man1] == ids1  # manifest order == matrix order
    assert mat1.shape == (4, DIM)
    by_id1 = dict(zip(ids1, mat1))
    assert man1[3]["n_chunks"] > 1  # long post actually chunked

    make_db(db, [
        ("s", "a", "alpha post about cats", now - 100),  # unchanged
        ("s", "b", "beta post about ferrets", now - 200),  # changed
        ("s", "d", "delta post about fish", now - 50),  # new
        ("s", "long", " ".join(f"w{i}" for i in range(300)), now - 400),
    ])
    run_embed(db, out)
    mat2, ids2, man2 = load_result(out)
    assert ids2 == ["s:a", "s:b", "s:d", "s:long"]
    assert [r["canonical_id"] for r in man2] == ids2
    assert mat2.shape == (4, DIM)
    by_id2 = dict(zip(ids2, mat2))
    np.testing.assert_array_equal(by_id2["s:a"], by_id1["s:a"])  # reuse bit-exact
    assert not np.allclose(by_id2["s:b"], by_id1["s:b"])  # changed text re-embedded
    assert "s:c" not in by_id2  # expired row dropped
    # every matrix row matches a fresh embed of its row's text (alignment)
    for (src_sid, text, _ts) in [("s:a", "alpha post about cats", 0), ("s:d", "delta post about fish", 0)]:
        want = next(iter(FakeEmbedding().embed([text])))
        np.testing.assert_allclose(by_id2[src_sid], want, rtol=1e-5)


def test_shuffled_insertion_order_same_vectors(faked):
    now = int(time.time())
    db1, db2, out1, out2 = faked / "a.db", faked / "b.db", faked / "o1", faked / "o2"
    posts = [("s", f"p{i}", f"post number {i} about topic{i}", now - i) for i in range(10)]
    make_db(db1, posts)
    make_db(db2, list(reversed(posts)))
    run_embed(db1, out1)
    run_embed(db2, out2)
    mat1, ids1, _ = load_result(out1)
    mat2, ids2, _ = load_result(out2)
    assert ids1 == ids2  # canonical order regardless of insertion
    np.testing.assert_array_equal(mat1, mat2)


def test_noop_rerun(faked):
    now = int(time.time())
    db, out = faked / "t.db", faked / "sem"
    make_db(db, [("s", "a", "some text here", now - 10)])
    run_embed(db, out)
    mat1, _, _ = load_result(out)
    run_embed(db, out)  # must not crash on empty todo
    mat2, ids2, man2 = load_result(out)
    np.testing.assert_array_equal(mat1, mat2)
    assert ids2 == ["s:a"] and len(man2) == 1


def test_unstable_roundtrip_halves_to_budget():
    # decode that expands by one token per window forces halving; must terminate within budget
    encode = lambda text: text.split()  # noqa: E731
    decode = lambda ids: "pad " + " ".join(ids)  # noqa: E731
    text = " ".join(f"w{i}" for i in range(600))
    chunks = mod.token_chunks(text, encode, decode)
    assert len(chunks) > 1
    for c in chunks:
        assert len(encode(c)) <= mod.CONTENT_TOKENS
    recovered = " ".join(c.replace("pad ", "") for c in chunks).split()
    assert recovered == text.split()


def test_same_ids_changed_text_no_misassign(faked):
    # adversarial: same count, same ID set, one text changed — changed row must
    # move, unchanged rows must be bit-identical (no old/new mixing)
    now = int(time.time())
    db, out = faked / "t.db", faked / "sem"
    make_db(db, [
        ("s", "a", "alpha post about cats", now - 100),
        ("s", "b", "beta post about dogs", now - 200),
    ])
    run_embed(db, out)
    mat1, ids1, _ = load_result(out)
    make_db(db, [
        ("s", "a", "alpha post about cats", now - 100),
        ("s", "b", "beta post about airplanes", now - 200),
    ])
    run_embed(db, out)
    mat2, ids2, _ = load_result(out)
    assert ids1 == ids2 == ["s:a", "s:b"]
    np.testing.assert_array_equal(mat2[0], mat1[0])
    assert not np.allclose(mat2[1], mat1[1])


def test_torn_file_raises_never_mixed(faked):
    now = int(time.time())
    db, out = faked / "t.db", faked / "sem"
    make_db(db, [("s", "a", "some text here", now - 10)])
    run_embed(db, out)
    good = (out / mod.GEN_FILE).read_bytes()
    # truncated write (torn generation) must raise, not return mixed rows
    (out / mod.GEN_FILE).write_bytes(good[: len(good) // 2])
    with pytest.raises(mod.GenerationError):
        mod.load_generation(out)
    (out / mod.GEN_FILE).write_bytes(b"not a zip file at all")
    with pytest.raises(mod.GenerationError):
        mod.load_generation(out)
    # restoring the complete file reads the complete generation
    (out / mod.GEN_FILE).write_bytes(good)
    mat, ids, _ = load_result(out)
    assert ids == ["s:a"] and mat.shape == (1, DIM)


def test_window_excludes_old_and_future(faked):
    now = int(time.time())
    db, out = faked / "t.db", faked / "sem"
    make_db(db, [
        ("s", "old", "too old", now - 31 * 86400),
        ("s", "future", "from future", now + 3600),
        ("s", "edge", "just inside", now - 29 * 86400),
        ("s", "now", "current", now),
    ])
    run_embed(db, out)
    _, ids, _ = load_result(out)
    assert ids == ["s:edge", "s:now"], ids
