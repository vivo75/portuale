#!/usr/bin/env python3
"""F3 routing table — which skill each source document feeds.

Deterministic: aggregates the per-chunk `skills` votes from the cards, so the
assignment is reproducible and auditable (no second LLM call).
"""

import json
import os
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)

SKILLS = [
    "slice-delivery", "oracle-probing", "resolver", "ebuild-merge", "tests-harness",
    "code-conventions", "output-contract", "portage-source-map", "docs-and-charter",
]

cards = [json.loads(l) for l in open(os.path.join(WORK, "cards_b.jsonl"))]
cards = [c for c in cards if "_error" not in c]
inv = {json.loads(l)["path"]: json.loads(l) for l in open(os.path.join(WORK, "inventory.jsonl"))}

by_doc = defaultdict(list)
for c in cards:
    by_doc[c["path"]].append(c)

rows = []
for path, cs in by_doc.items():
    votes = Counter()
    for c in cs:
        for s in c["skills"]:
            votes[s] += 1
    primary = votes.most_common(1)[0][0] if votes else "UNROUTED"
    rows.append({
        "path": path,
        "kind": inv.get(path, {}).get("kind", "?"),
        "class": inv.get(path, {}).get("class", "?"),
        "words": inv.get(path, {}).get("words", 0),
        "cards": len(cs),
        "kept": sum(c["_kept"] for c in cs),
        "reusable": any(c["reusable"] for c in cs),
        "primary": primary,
        "votes": dict(votes.most_common()),
    })

rows.sort(key=lambda r: (r["primary"], -r["kept"]))

with open(os.path.join(WORK, "routing.tsv"), "w") as f:
    f.write("path\tkind\tclass\twords\tcards\tkept\tprimary\tvotes\n")
    for r in rows:
        v = ",".join(f"{k}:{n}" for k, n in r["votes"].items())
        f.write(f"{r['path']}\t{r['kind']}\t{r['class']}\t{r['words']}\t{r['cards']}\t{r['kept']}\t{r['primary']}\t{v}\n")

with open(os.path.join(WORK, "routing.md"), "w") as f:
    f.write(f"# Routing: {len(rows)} documents, {sum(r['kept'] for r in rows)} verified facts\n\n")
    by = Counter(r["primary"] for r in rows)
    f.write("| skill | documents | facts | words |\n|---|---|---|---|\n")
    for s in SKILLS + ["UNROUTED"]:
        if not by[s]:
            continue
        sel = [r for r in rows if r["primary"] == s]
        f.write(f"| {s} | {len(sel)} | {sum(r['kept'] for r in sel)} | {sum(r['words'] for r in sel)} |\n")
    un = [r for r in rows if r["kept"] == 0]
    f.write(f"\n## Documents that yielded no verified fact: {len(un)}\n\n")
    for r in un:
        f.write(f"- `{r['path']}` ({r['words']}w)\n")
    for s in SKILLS:
        sel = [r for r in rows if r["primary"] == s]
        if not sel:
            continue
        f.write(f"\n## {s} ({len(sel)} docs)\n\n")
        for r in sel:
            f.write(f"- `{r['path']}` — {r['kept']} facts, {r['words']}w\n")

print(open(os.path.join(WORK, "routing.md")).read()[:2500])
