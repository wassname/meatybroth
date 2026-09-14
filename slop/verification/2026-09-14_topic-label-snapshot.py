#!/usr/bin/env python3
# Frozen topic-label evidence from stored memberships and centroids. -- Pi/gpt-5.6-sol
import argparse
import heapq
import json
import math
import re
import sqlite3
from pathlib import Path

import numpy as np

URL = re.compile(r"(?i)(?:https?://|www\.)\S+")
WORD = re.compile(r"[\w-]{3,}", re.UNICODE)
STOP = set((Path(__file__).parents[2] / "src/data/english-stopwords.txt").read_text().splitlines())


def tokens(text):
    terms = set()
    for raw in WORD.findall(URL.sub(" ", text)):
        word = raw.lower()
        if len(word) > 40 or not word[0].isalnum() or not any(char.isalpha() for char in word):
            continue
        if word.isascii() and word in STOP:
            continue
        terms.add(word)
    return terms


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("database")
    parser.add_argument("--backend", default="minilm")
    parser.add_argument("--clustering", choices=["kmeans", "dbscan"], required=True)
    args = parser.parse_args()
    db = sqlite3.connect(args.database)
    space_id = db.execute(
        "SELECT id FROM embedding_spaces WHERE backend=? ORDER BY created_at DESC LIMIT 1",
        (args.backend,),
    ).fetchone()[0]
    if args.clustering == "kmeans":
        topic_table, membership_table = "embedding_topics", "post_topics"
    else:
        topic_table, membership_table = "embedding_dbscan_topics", "post_dbscan_topics"
    corpus = {}
    for event_id, content, embedding in db.execute(
        f"SELECT member.event_id,event.content,embedding.vector FROM {membership_table} member "
        "JOIN events event ON event.id=member.event_id "
        "JOIN post_embeddings embedding ON embedding.event_id=member.event_id AND embedding.space_id=member.space_id "
        "WHERE member.space_id=?",
        (space_id,),
    ):
        corpus[event_id] = (content, tokens(content), np.frombuffer(embedding, dtype="<f4"))
    corpus_frequencies = {}
    for _, terms, _ in corpus.values():
        for term in terms:
            corpus_frequencies[term] = corpus_frequencies.get(term, 0) + 1
    topics = []
    for topic_id, label, stored_count, centroid_blob in db.execute(
        f"SELECT topic_id,label,post_count,centroid FROM {topic_table} "
        "WHERE space_id=? ORDER BY post_count DESC,topic_id",
        (space_id,),
    ):
        member_ids = [row[0] for row in db.execute(
            f"SELECT event_id FROM {membership_table} WHERE space_id=? AND topic_id=?",
            (space_id, topic_id),
        )]
        representatives = []
        if centroid_blob is not None:
            centroid = np.frombuffer(centroid_blob, dtype="<f4")
            matrix = np.stack([corpus[event_id][2] for event_id in member_ids])
            scores = matrix @ centroid
            scored = zip(scores.tolist(), member_ids)
            for score, event_id in heapq.nlargest(5, scored):
                representatives.append({
                    "event_id": event_id.hex(),
                    "cosine": round(score, 6),
                    "content": corpus[event_id][0],
                })
        terms = [] if label in {"mixed", "Unlabelled topic", "Noise / unmatched"} else label.split(" · ")
        representative_terms = [tokens(rep["content"]) for rep in representatives]
        evidence = []
        candidates = []
        cluster_frequencies = {}
        for event_id in member_ids:
            for term in corpus[event_id][1]:
                cluster_frequencies[term] = cluster_frequencies.get(term, 0) + 1
        minimum_support = max(2, math.ceil(len(member_ids) / 10))
        minimum_representatives = min(2, len(representative_terms))
        for term, cluster_frequency in cluster_frequencies.items():
            corpus_frequency = corpus_frequencies[term]
            cluster_rate = cluster_frequency / len(member_ids) if member_ids else 0
            corpus_rate = corpus_frequency / len(corpus) if corpus else 0
            nearest_presence = [term in representative_term_set for representative_term_set in representative_terms]
            row = {
                "term": term,
                "cluster_document_frequency": cluster_frequency,
                "cluster_document_percent": round(100 * cluster_rate, 1),
                "corpus_document_frequency": corpus_frequency,
                "outside_document_frequency": corpus_frequency - cluster_frequency,
                "nearest_five_presence": nearest_presence,
            }
            if cluster_frequency >= minimum_support and cluster_rate >= 1.5 * corpus_rate and sum(nearest_presence) >= minimum_representatives:
                row["score"] = cluster_rate * math.log(1 / corpus_rate)
                candidates.append(row)
        candidates.sort(key=lambda row: (-row["score"], -row["cluster_document_frequency"], row["term"]))
        by_term = {row["term"]: row for row in candidates}
        for term in terms:
            evidence.append(by_term.get(term, {"term": term, "missing_from_candidates": True}))
        count = len(member_ids)
        topics.append({
            "topic_id": topic_id,
            "label": label,
            "stored_count": stored_count,
            "membership_count": count,
            "percent_of_clustered_population": round(100 * count / len(corpus), 1) if corpus else 0,
            "label_evidence": evidence,
            "candidate_terms": candidates,
            "representatives": representatives,
        })
    print(json.dumps({
        "database": args.database,
        "backend": args.backend,
        "clustering": args.clustering,
        "space_id": space_id,
        "clustered_population": len(corpus),
        "topics": topics,
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
