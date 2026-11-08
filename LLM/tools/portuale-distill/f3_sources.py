#!/usr/bin/env python3
"""Emit LLM/skills/<name>/references/sources.md from the fact dumps.

Every document that contributed a verified fact to a skill, with how many.
This is the traceability the SKILL.md itself does not carry: the skill states
the procedure, this file says which document each part was distilled from.
"""

import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)
FACTS = os.path.join(WORK, "facts")

HEAD = re.compile(r"^## `(.+)`\s*$")
BULLET = re.compile(r"^- ")


def parse(path):
    counts, order = {}, []
    cur = None
    for line in open(path):
        m = HEAD.match(line)
        if m:
            cur = m.group(1)
            continue
        if cur and BULLET.match(line):
            if cur not in counts:
                counts[cur] = 0
                order.append(cur)
            counts[cur] += 1
    return counts, order


def kind_of(p):
    n = os.path.basename(p)
    if p.startswith("LLM/code-notes/"):
        return "code-note"
    if n.endswith(".review.md") or ".review" in n:
        return "review"
    if n.endswith(".muse.md"):
        return "plan-alt"
    if n.endswith(".opus.md"):
        return "plan"
    if p.startswith("LLM/evidence/"):
        return "evidence report"
    return "doc"


def main():
    total = 0
    for skill in sorted(os.listdir(FACTS)):
        if skill == "INDEX.md" or not skill.endswith(".md"):
            continue
        name = "portuale-" + skill[:-3] if not skill.startswith("port") else skill[:-3]
        if skill == "portage-source-map.md":
            name = "portage-source-map"
        counts, order = parse(os.path.join(FACTS, skill))
        if not counts:
            continue
        outdir = os.path.join(REPO, "LLM", "skills", name, "references")
        os.makedirs(outdir, exist_ok=True)
        order.sort(key=lambda p: (-counts[p], p))
        plans = sum(counts[p] for p in order if kind_of(p) in ("plan", "plan-alt", "review"))
        allf = sum(counts.values())
        with open(os.path.join(outdir, "sources.md"), "w") as f:
            f.write(f"# Where `{name}` came from\n\n")
            f.write(
                f"{allf} verified facts from {len(order)} documents; "
                f"{plans} of them ({100*plans//max(allf,1)}%) from per-item plans.\n\n"
            )
            f.write(
                "A *verified fact* is a statement whose supporting quote was found\n"
                "verbatim in the source by the extraction pass. Statements that could\n"
                "not be quoted were discarded, so this table is a lower bound.\n\n"
            )
            f.write("| document | kind | facts |\n|---|---|---|\n")
            for p in order:
                f.write(f"| `{p}` | {kind_of(p)} | {counts[p]} |\n")
            f.write(
                "\nThe plans are input, not text: what survives here is the procedure\n"
                "and the invariant, never the per-slice task list, commit order or\n"
                "evidence path — those belong to the record in `LLM/` and to git.\n"
            )
        total += 1
        print(f"{name:28s} {allf:4d} facts / {len(order):3d} docs ({plans} from plans)")
    print("written:", total)


if __name__ == "__main__":
    main()
