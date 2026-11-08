#!/usr/bin/env python3
"""Build work/manifest.tsv: the one table that proves nothing was lost.

Destination vocabulary:
  keep              stays untouched (live plan, or tooling reads it)
  keep+index        stays, and a skill points at it (code-notes)
  archive:<skill>   removed from the tree; its knowledge lives in that skill

`work/to-archive.txt` lists exactly the files that leave the tree.
"""

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


STATUS = re.compile(r"^\s*Status:\s*\**\s*(done|proposed|in progress|superseded)", re.I)

# Manual calls where the card routing is wrong or a process decision wins.
OVERRIDES = {
    "LLM/lessons-of-backlog-ops-2026-10-03.md": "archive:portuale-slice-delivery",
    "LLM/recap-of-backlog-ops-2026-10-03.md": "archive:portuale-slice-delivery",
    "LLM/recurring-issues.md": "keep",
    "LLM/scope-cuts.md": "keep",
    "LLM/scope-backlog.md": "keep",
    "LLM/ideas.md": "archive:portuale-slice-delivery",
    "LLM/glep-compliance-review.md": "archive:portuale-ebuild-merge",
    "LLM/Paragone_solver_portage.md": "archive:portuale-resolver",
    "LLM/feat-157-authoritative-vdb-database.md": "archive:portuale-ebuild-merge",
    "LLM/portage-quirks.md": "keep",
}


def plan_status(path):
    try:
        with open(os.path.join(REPO, path), errors="ignore") as f:
            for line in f:
                m = STATUS.match(line)
                if m:
                    return m.group(1).lower()
    except OSError:
        pass
    return ""


def main():
    inv = [json.loads(l) for l in open(os.path.join(WORK, "inventory.jsonl"))]
    route = {}
    with open(os.path.join(WORK, "routing.tsv")) as f:
        head = f.readline().rstrip("\n").split("\t")
        for line in f:
            d = dict(zip(head, line.rstrip("\n").split("\t")))
            route[d["path"]] = d

    rows = []
    for r in inv:
        p, cls = r["path"], r["class"]
        prim = route.get(p, {}).get("primary", "UNROUTED")
        kept = int(route.get(p, {}).get("kept", 0) or 0)

        if p in OVERRIDES:
            dest, note = OVERRIDES[p], "manual override"
        elif cls == "A":
            dest = "keep+index" if p.startswith("LLM/code-notes/") else "keep"
            note = "tooling or live state reads it"
        elif cls == "C" and not p.endswith(".md"):
            dest, note = "archive:raw", "raw capture, cited by path"
        elif not p.endswith(".md"):
            dest, note = "archive:raw", "baseline artefact, not prose"
        elif prim == "UNROUTED" or kept == 0:
            dest, note = "keep", "kept: cited by the backlog, no fact extracted"
        else:
            dest, note = "keep", f"{kept} verifiable facts also distilled into portuale-{prim}"

        rows.append(
            {
                "path": p,
                "class": cls,
                "kind": r["kind"],
                "words": r.get("words", 0),
                "destination": dest,
                "skill": f"portuale-{prim}" if prim != "UNROUTED" else "",
                "refs": r.get("refs", 0),
                "note": note,
            }
        )

    rows.sort(key=lambda x: (x["destination"], x["path"]))
    out = os.path.join(WORK, "manifest.tsv")
    with open(out, "w") as f:
        f.write("path\tclass\tkind\twords\tdestination\tskill\trefs\tnote\n")
        for r in rows:
            f.write(
                f"{r['path']}\t{r['class']}\t{r['kind']}\t{r['words']}\t{r['destination']}\t"
                f"{r['skill']}\t{r['refs']}\t{r['note']}\n"
            )

    with open(os.path.join(WORK, "to-archive.txt"), "w") as f:
        for r in rows:
            if r["destination"].startswith("archive:"):
                f.write(r["path"] + "\n")
    with open(os.path.join(WORK, "archived-paths.txt"), "w") as f:
        for r in rows:
            if r["destination"].startswith("archive:"):
                f.write(r["path"] + "\n")

    by = defaultdict(int)
    for r in rows:
        by[r["destination"]] += 1
    print("manifest written:", out)
    for d, n in sorted(by.items(), key=lambda kv: -kv[1]):
        print(f"  {d:34s} {n}")


if __name__ == "__main__":
    main()
