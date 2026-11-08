#!/usr/bin/env python3
"""F3 prep — per-skill fact dumps, deduplicated, ordered by document weight."""

import json
import os
import re
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)

OUT = os.path.join(WORK, "facts")
SKILLS = [
    "slice-delivery", "oracle-probing", "resolver", "ebuild-merge", "tests-harness",
    "code-conventions", "output-contract", "portage-source-map", "docs-and-charter",
]


def norm(s):
    return re.sub(r"[^a-z0-9 ]+", " ", s.lower()).strip()


route = {}
with open(os.path.join(WORK, "routing.tsv")) as f:
    head = f.readline().rstrip("\n").split("\t")
    for line in f:
        d = dict(zip(head, line.rstrip("\n").split("\t")))
        route[d["path"]] = d

cards = defaultdict(list)
for line in open(os.path.join(WORK, "cards_b.jsonl")):
    c = json.loads(line)
    if "_error" in c:
        continue
    cards[c["path"]].append(c)

os.makedirs(OUT, exist_ok=True)
index = open(os.path.join(OUT, "INDEX.md"), "w")
index.write("# Facts per skill\n\n| skill | documents | facts | file |\n|---|---|---|---|\n")

for skill in SKILLS:
    paths = [p for p, r in route.items() if r["primary"] == skill]
    paths.sort(key=lambda p: (-int(route[p]["kept"]), p))
    seen = {}
    out = []
    for p in paths:
        for c in cards.get(p, []):
            items = [(i["text"], i["quote"]) for i in c["claims"]]
            if c.get("procedure"):
                items.append(("PROCEDURE: " + c["procedure"]["name"], c["procedure"]["quote"]))
            for text, quote in items:
                k = norm(text)[:90]
                if k in seen:
                    seen[k]["repeat"] += 1
                    continue
                rec = {"path": p, "heading": c["heading"], "text": text, "quote": quote, "repeat": 1}
                seen[k] = rec
                out.append(rec)
    fn = os.path.join(OUT, f"{skill}.md")
    with open(fn, "w") as f:
        f.write(f"# Facts routed to {skill}\n\n")
        f.write(f"{len(out)} facts from {len(paths)} documents\n\n")
        for p in paths:
            sel = [r for r in out if r["path"] == p]
            if not sel:
                continue
            f.write(f"\n## `{p}`\n\n")
            for r in sel:
                rep = f" (x{r['repeat']})" if r["repeat"] > 1 else ""
                f.write(f"- {r['text']}{rep}\n  > {r['quote'][:220]}\n")
    index.write(f"| {skill} | {len(paths)} | {len(out)} | `{fn}` |\n")
    print(f"{skill}: {len(out)} facts / {len(paths)} docs")
index.close()
