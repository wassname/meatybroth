# /// script
# requires-python = ">=3.12"
# dependencies = ["fastembed>=0.7,<1", "numpy>=1.26,<3", "tokenizers>=0.19"]
# ///
"""Batch-embed the retained reader corpus with a local CPU embedding model.

Reads the live reader DB read-only via a SQLite backup-API snapshot, keeps
only the 30-day rolling window (created_at within [now-30d, now]), embeds
full post bodies locally (no text leaves the machine), stores vectors OUTSIDE
the core DB in .local/semantic/.

Chunking uses the MODEL'S OWN tokenizer: windows of <=254 content tokens
(+2 specials <= 256, the sentence-transformers max_seq_length for
all-MiniLM-L6-v2). Windows partition the post's token-id sequence, so every
content token is embedded in some chunk; note decoded chunk text is
tokenizer-normalized, not character-identical to the original. Per-post
vector = renormalized mean of normalized chunk vectors. Every stored chunk
is re-encoded to verify <=256 tokens, so the budget claim is measured.

Ordering: single canonical order (ORDER BY source, source_id) shared by the
single canonical artifact corpus.npz (matrix+ids+manifest+meta) written by one
atomic os.replace. Incremental runs map old rows by canonical_id, never by
position. Reuse requires an exact settings-fingerprint
match (model source+commit+blobs, chunk budget, pool method); else full
re-embed. A no-op run (nothing changed) reuses everything without crashing.

Usage:
  uv run scripts/embed_corpus.py embed [--db PATH] [--out .local/semantic]
  uv run scripts/embed_corpus.py verify [--out .local/semantic]

Regression tests live beside the script (not tests/: the project env has no
numpy/fastembed and this module's env is isolated via inline metadata):
  uv run --with pytest --with fastembed --with numpy python -m pytest scripts/test_embed_corpus.py -q
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sqlite3
import sys
import tempfile
import time
from pathlib import Path

MODEL_NAME = "sentence-transformers/all-MiniLM-L6-v2"  # single baseline, not configurable:
# provenance, tokenizer and chunk budget below hardcode this model, so a --model flag
# would let a wrong model silently reuse old vectors. Change the constant, not a flag.
MODEL_SLUG = "models--qdrant--all-MiniLM-L6-v2-onnx"
CONTENT_TOKENS = 254  # +2 specials <= 256
POOL = "mean-normalized-renorm"
WINDOW_DAYS = 30
PROGRESS_EVERY = 500


def snapshot_db(db_path: Path) -> Path:
    """Consistent read snapshot via the SQLite backup API (WAL-safe)."""
    src = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True, timeout=30)
    tmp = Path(tempfile.mkdtemp(prefix="embed-snap-")) / "snap.db"
    dst = sqlite3.connect(str(tmp), timeout=30)
    try:
        with dst:
            src.backup(dst)
    finally:
        dst.close()
        src.close()
    return tmp


def read_posts(snap: Path, now: int) -> list[tuple[str, str, int]]:
    """Rolling-window rows in stable canonical order. Window enforced here."""
    lo, hi = now - WINDOW_DAYS * 86400, now
    db = sqlite3.connect(str(snap), timeout=30)
    try:
        rows = db.execute(
            "SELECT source || ':' || source_id, text, created_at FROM posts"
            " WHERE created_at >= ? AND created_at <= ?"
            " ORDER BY source, source_id",
            (lo, hi),
        ).fetchall()
    finally:
        db.close()
    return [(c, t or "", ts) for c, t, ts in rows]


def token_chunks(text: str, encode, decode, budget: int = CONTENT_TOKENS) -> list[str]:
    """Split into decoded windows of <=budget content tokens. Windows are
    token-based so long single words are handled. Decode/re-encode is not
    perfectly stable, so oversized windows are halved until they verify."""
    ids = encode(text)
    from collections import deque

    queue = deque(ids[i : i + budget] for i in range(0, max(len(ids), 1), budget))
    out: list[str] = []
    while queue:
        w = queue.popleft()
        s = decode(w)
        if not w or len(encode(s)) <= budget:
            out.append(s)
        elif len(w) <= 1:
            raise ValueError(f"single token decodes to >{budget} tokens; cannot window")
        else:
            mid = len(w) // 2
            queue.appendleft(w[mid:])
            queue.appendleft(w[:mid])
    return out


def check_chunk_budgets(chunks: list[str], encode, limit: int = CONTENT_TOKENS + 2) -> int:
    """Re-encode every chunk with the model tokenizer; return max tokens.
    Raises if any chunk exceeds the model's input limit."""
    mx = 0
    for c in chunks:
        n = len(encode(c))
        mx = max(mx, n)
        if n > limit:
            raise ValueError(f"chunk of {n} tokens exceeds limit {limit}")
    return mx


def load_tokenizer(model, prov: dict):
    from tokenizers import Tokenizer

    tok = Tokenizer.from_file(prov["loaded_files"]["tokenizer.json"])
    tok.no_truncation()
    tok.no_padding()  # padding would pad every encode to 128 ids and corrupt token counts
    encode = lambda text: tok.encode(text).ids  # noqa: E731
    return encode, lambda ids: tok.decode(ids, skip_special_tokens=True)  # noqa: E731


GEN_FILE = "corpus.npz"  # single canonical artifact: one atomic os.replace per
# generation, so a reader sees a complete old or complete new generation, never a mix.


class GenerationError(ValueError):
    pass


def save_generation(out: Path, order: list[str], mat, rows: list[dict], meta: dict) -> None:
    import numpy as np

    tmp = out / f".{GEN_FILE}.staging-{os.getpid()}.npz"  # .npz suffix: savez appends it otherwise
    np.savez(
        tmp,
        embeddings=np.asarray(mat, dtype=np.float32),
        ids=np.asarray(order, dtype="<U256"),
        # plain JSON strings (not object arrays) so allow_pickle=False loads them
        manifest=np.asarray(json.dumps(rows)),
        meta=np.asarray(json.dumps(meta)),
    )
    os.replace(tmp, out / GEN_FILE)


def load_generation(out: Path):
    """Load and validate one complete generation. Raises GenerationError on any
    corruption — never returns a silently mixed or truncated generation."""
    import numpy as np

    p = out / GEN_FILE
    if not p.exists():
        raise FileNotFoundError(f"no generation at {p}")
    try:
        z = np.load(str(p), allow_pickle=False)
        mat = z["embeddings"]
        ids = [str(x) for x in z["ids"].tolist()]
        man_rows = json.loads(str(z["manifest"]))
        meta = json.loads(str(z["meta"]))
    except Exception as e:
        raise GenerationError(f"{p} is corrupt/torn, refusing to read a mixed generation: {e}") from e
    if not (mat.shape[0] == len(ids) == len(man_rows)):
        raise GenerationError(f"{p}: length mismatch rows={mat.shape[0]} ids={len(ids)} manifest={len(man_rows)}")
    if [r["canonical_id"] for r in man_rows] != ids:
        raise GenerationError(f"{p}: manifest order != matrix order")
    if meta.get("n") != len(ids):
        raise GenerationError(f"{p}: meta n={meta.get('n')} != {len(ids)}")
    return ids, {r["canonical_id"]: r for r in man_rows}, meta, mat


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(4 << 20), b""):
            h.update(b)
    return h.hexdigest()


def model_provenance(cache_dir: Path) -> dict:
    """Identity of the actually-downloaded artifact snapshot. file_sha256 are
    hashes computed from the local files; upstream_blobs is registry metadata
    (not a local verification). The snapshot dir below is the same one
    fastembed resolves for this cache_dir + model slug, and load_tokenizer
    opens the tokenizer path recorded here — so these are the loaded files."""
    base = cache_dir / MODEL_SLUG
    try:
        commit = (base / "refs" / "main").read_text().strip()
        snap = base / "snapshots" / commit
        files = json.loads((base / "files_metadata.json").read_text())
        blobs = {
            k.split("/")[-1]: v.get("blob_id")
            for k, v in files.items()
            if isinstance(v, dict) and "blob_id" in v
        }
        paths = {}
        for name in ("model.onnx", "tokenizer.json", "config.json"):
            p = snap / name
            if p.exists():
                paths[name] = str(p)
        if "model.onnx" not in paths or "tokenizer.json" not in paths:
            raise OSError(f"incomplete snapshot at {snap}")
        return {
            "source": "qdrant/all-MiniLM-L6-v2-onnx",
            "commit": commit,
            "snapshot_dir": str(snap),
            "loaded_files": paths,
            "file_sha256": {n: sha256_file(Path(p)) for n, p in paths.items()},
            "upstream_blobs": blobs,
        }
    except (OSError, ValueError, KeyError) as e:
        return {"source": "qdrant/all-MiniLM-L6-v2-onnx", "commit": None, "note": f"unresolved: {e}"}


def fingerprint(prov: dict) -> dict:
    return {
        "model_source": prov.get("source"),
        "model_commit": prov.get("commit"),
        "model_file_sha256": prov.get("file_sha256"),
        "chunk_content_tokens": CONTENT_TOKENS,
        "pool": POOL,
    }


def cmd_embed(args: argparse.Namespace) -> int:
    import numpy as np

    from fastembed import TextEmbedding

    t0 = time.time()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    now = int(time.time())
    snap = snapshot_db(Path(args.db))
    try:
        posts = read_posts(snap, now)
    finally:
        snap.unlink(missing_ok=True)
        snap.parent.rmdir()
    print(f"snapshot posts in [{now - WINDOW_DAYS*86400},{now}]: {len(posts)}", flush=True)

    model = TextEmbedding(model_name=MODEL_NAME, cache_dir=args.cache_dir)
    prov = model_provenance(Path(args.cache_dir))
    fp = fingerprint(prov)
    print(f"model commit={prov.get('commit')} files={list((prov.get('file_sha256') or {}))}", flush=True)
    dim = len(next(iter(model.embed(["dimension probe"]))))
    print(f"dim={dim}", flush=True)

    encode, decode = load_tokenizer(model, prov)

    try:
        old_ids, old_man, old_meta, old_mat = load_generation(out)
    except FileNotFoundError:
        old_ids, old_man, old_meta, old_mat = [], {}, None, None
    reuse_ok = old_meta is not None and old_meta.get("fingerprint") == fp
    if old_meta is not None and not reuse_ok:
        print("settings/model changed: full re-embed, ignoring old rows", flush=True)
        old_man = {}
    old_emb = dict(zip(old_ids, old_mat)) if reuse_ok and old_mat is not None else None
    if reuse_ok and old_emb is None:
        old_man = {}

    todo, reused = [], []
    for cid, text, ts in posts:
        digest = hashlib.sha256(text.encode()).hexdigest()
        prev = old_man.get(cid) if reuse_ok else None
        if prev and prev.get("text_sha256") == digest and old_emb is not None and cid in old_emb:
            reused.append(cid)
        else:
            todo.append((cid, text, ts, digest))
    print(f"reuse={len(reused)} todo={len(todo)} expired_dropped={len(old_man) - len(reused)}", flush=True)

    new_vecs: dict[str, np.ndarray] = {}
    new_rows: dict[str, dict] = {}
    if todo:
        chunk_lists = [token_chunks(t, encode, decode) for _, t, _, _ in todo]
        flat = [c for cl in chunk_lists for c in cl]
        mx = check_chunk_budgets(flat, encode)
        print(f"chunks={len(flat)} max_tokens_per_chunk={mx} (limit {CONTENT_TOKENS + 2})", flush=True)
        vecs = np.asarray(list(model.embed(flat)), dtype=np.float64)
        vecs /= np.linalg.norm(vecs, axis=1, keepdims=True) + 1e-12
        i = 0
        for (cid, text, ts, digest), cl in zip(todo, chunk_lists):
            m = vecs[i : i + len(cl)].mean(axis=0)
            m /= np.linalg.norm(m) + 1e-12
            new_vecs[cid] = m.astype(np.float32)
            new_rows[cid] = {
                "canonical_id": cid,
                "text_sha256": digest,
                "created_at": ts,
                "n_chars": len(text),
                "n_chunks": len(cl),
                "max_chunk_tokens": max(len(encode(c)) for c in cl),
                "fastembed_version": __import__("fastembed").__version__,
                "embedded_at": now,
            }
            i += len(cl)
            if len(new_vecs) % PROGRESS_EVERY == 0:
                print(f"  embedded {len(new_vecs)}/{len(todo)}", flush=True)

    order = [cid for cid, _, _ in posts]
    mat = np.stack(
        [new_vecs[c] if c in new_vecs else old_emb[c] for c in order]
    ).astype(np.float32)
    rows = []
    for cid, text, ts in posts:
        r = new_rows.get(cid) or dict(old_man[cid])
        rows.append(r)
    save_generation(out, order, mat, rows, {"fingerprint": fp, "provenance": prov, "dim": dim, "n": len(order)})

    norms = np.linalg.norm(mat, axis=1)
    multi = sum(1 for r in rows if r["n_chunks"] > 1)
    print(
        f"saved n={len(mat)} dim={mat.shape[1]} norm_min={norms.min():.4f} "
        f"finite={bool(np.isfinite(mat).all())} multi_chunk={multi} dt={time.time()-t0:.1f}s",
        flush=True,
    )
    return 0


PARA_A = "the cat sat on the warm rug in the afternoon sun"
PARA_B = "a feline rested on a cozy mat during the sunny afternoon"
UNRELATED = "quantum chromodynamics gauge fixing on a euclidean lattice"


def cmd_verify(args: argparse.Namespace) -> int:
    import numpy as np

    from fastembed import TextEmbedding

    out = Path(args.out)
    ids, man, meta, mat = load_generation(out)  # validated: raises on torn/corrupt
    norms = np.linalg.norm(mat, axis=1)
    assert np.isfinite(mat).all() and (norms > 1e-6).all()
    print(f"count_join ok: n={len(ids)} dim={mat.shape[1]} norm~1.0", flush=True)

    # Through-the-model semantic check (local only): paraphrases must beat unrelated by margin.
    model = TextEmbedding(model_name=MODEL_NAME, cache_dir=args.cache_dir)
    got = {t: v / np.linalg.norm(v) for t, v in zip(
        [PARA_A, PARA_B, UNRELATED], model.embed([PARA_A, PARA_B, UNRELATED]))}
    s_para = float(got[PARA_A] @ got[PARA_B])
    s_unrel = max(float(got[PARA_A] @ got[UNRELATED]), float(got[PARA_B] @ got[UNRELATED]))
    print(f"through-model: paraphrase_cos={s_para:.3f} unrelated_cos={s_unrel:.3f}", flush=True)
    assert s_para > s_unrel + 0.2, "model fails basic semantic separation"

    sims = mat @ mat[0]
    print(f"self_cos={sims[0]:.4f} corpus_min={sims.min():.4f}", flush=True)
    import random

    for k in random.Random(1).sample(range(len(ids)), min(3, len(ids))):
        top = np.argsort(-(mat @ mat[k]))[1:4]
        print(f"--- anchor {ids[k]} chunks={man[ids[k]]['n_chunks']}", flush=True)
        for j in top:
            print(f"    cos={float(mat[k] @ mat[j]):.3f} {ids[j]}", flush=True)
    print("VERIFY OK", flush=True)
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=".local/meatybroth.db")
    ap.add_argument("--out", default=".local/semantic")
    ap.add_argument("--cache-dir", default=".local/semantic/model-cache")
    sub = ap.add_subparsers(dest="cmd")
    sub.add_parser("embed")
    sub.add_parser("verify")
    args = ap.parse_args(argv)
    if args.cmd == "verify":
        return cmd_verify(args)
    return cmd_embed(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
