#!/usr/bin/env python3
"""F2b — taxonomy with the shared-vocabulary component removed.

Every portuale document shares the same vocabulary, so raw embeddings put 363
of 376 docs in one cluster. Centring removes that component and the corpus
splits into balanced groups. Names them with the local model.
"""

import json
import os
import sys
from collections import defaultdict

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from f2_taxonomy import ask  # noqa: E402

NAME_SCHEMA = {
    "type": "object",
    "properties": {"clusters": {"type": "array", "items": {
        "type": "object",
        "properties": {"id": {"type": "integer"}, "name": {"type": "string"},
                       "definition": {"type": "string"}, "excludes": {"type": "string"}},
        "required": ["id", "name", "definition", "excludes"],
        "additionalProperties": False}}},
    "required": ["clusters"], "additionalProperties": False,
}

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)

OUT = os.path.join(WORK, "taxonomy")

rows = [json.loads(l) for l in open(os.path.join(OUT, "docs.jsonl"))]
docs = [r["path"] for r in rows]
X = np.array([r["vec"] for r in rows], dtype=np.float32)
X /= np.linalg.norm(X, axis=1, keepdims=True) + 1e-9
X = X - X.mean(axis=0)
X /= np.linalg.norm(X, axis=1, keepdims=True) + 1e-9

cards = defaultdict(list)
for line in open(os.path.join(WORK, "cards_b.jsonl")):
    c = json.loads(line)
    if "_error" not in c:
        cards[c["path"]].append(c)

from sklearn.cluster import KMeans

for k in (7, 9, 11):
    labels = KMeans(n_clusters=k, n_init=20, random_state=0).fit_predict(X)
    groups = defaultdict(list)
    for i, lab in enumerate(labels):
        groups[int(lab)].append(i)

    blocks = []
    for lab in sorted(groups):
        idx = groups[lab]
        cen = X[idx].mean(axis=0)
        cen /= np.linalg.norm(cen) + 1e-9
        order = sorted(idx, key=lambda i: -float(X[i] @ cen))
        lines = []
        for i in order[:9]:
            d = docs[i]
            tops = sorted({t for c in cards[d] for t in c["topics"]})
            lines.append("- " + "; ".join([os.path.basename(d)] + tops[:6]))
        blocks.append(f"CLUSTER {lab} ({len(idx)} docs)\n" + "\n".join(lines))

    prompt = (
        f"Sei un architetto di conoscenza. Ecco {k} cluster di documenti tecnici di `portuale`, "
        "un package manager Gentoo in Rust. Per OGNI cluster: un nome breve (2-4 parole), una "
        "definizione di una frase che dica QUANDO un agente dovrebbe consultarlo, e una frase "
        "che dica cosa NON gli appartiene (il confine con i cluster vicini). Scrivi in inglese.\n\n"
        + "\n\n".join(blocks)
    )
    named = []
    try:
        named = json.loads(ask(prompt, NAME_SCHEMA, 1800))["clusters"]
    except Exception as e:  # noqa: BLE001
        print("naming failed:", e)

    with open(os.path.join(OUT, f"km{k}.md"), "w") as f:
        f.write(f"# Taxonomy (centred embeddings, k-means k={k})\n\n")
        for lab in sorted(groups):
            idx = groups[lab]
            info = next((n for n in named if n["id"] == lab), None)
            f.write(f"## {lab}. {info['name'] if info else '(unnamed)'} — {len(idx)} docs\n\n")
            if info:
                f.write(f"**When:** {info['definition']}\n\n**Excludes:** {info['excludes']}\n\n")
            f.write("\n".join(f"- `{docs[i]}`" for i in sorted(idx)) + "\n\n")
    print(f"k={k} written", flush=True)
