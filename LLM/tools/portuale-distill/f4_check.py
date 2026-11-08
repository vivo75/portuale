#!/usr/bin/env python3
"""F4 — acceptance checks for the LLM/ distillation.

Exit 0 only when every source document has a destination, every destination
exists, and the skill tree carries no dangling reference.

Usage: f4_check.py [--repo PATH]
"""

import argparse
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)

SKILLS = [
    "portuale-slice-delivery", "portuale-oracle-probing", "portuale-resolver",
    "portuale-ebuild-merge", "portuale-tests-harness", "portuale-code-conventions",
    "portuale-output-contract", "portage-source-map", "portuale-docs-and-charter",
]
DEST = {"keep", "keep+index"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=REPO)
    ap.add_argument("--work", default=WORK)
    ap.add_argument("--manifest", default=None)
    args = ap.parse_args()
    repo = args.repo
    work = args.work
    manifest = args.manifest or os.path.join(work, "manifest.tsv")

    problems = []
    inv = [json.loads(l) for l in open(os.path.join(work, "inventory.jsonl"))]
    rows = {}
    with open(manifest) as f:
        header = f.readline().rstrip("\n").split("\t")
        for line in f:
            parts = line.rstrip("\n").split("\t")
            d = dict(zip(header, parts))
            rows[d["path"]] = d

    # 1. every inventoried file has a manifest row
    missing = [r["path"] for r in inv if r["path"] not in rows]
    if missing:
        problems.append(f"{len(missing)} inventoried files have no manifest row (e.g. {missing[:3]})")

    # 2. every prose file (class B, and class C .md) has a real destination
    for r in inv:
        if r["path"] not in rows:
            continue
        d = rows[r["path"]]
        dest = d.get("destination", "")
        if r["class"] == "C" and not r["path"].endswith(".md"):
            continue
        if not dest:
            problems.append(f"no destination: {r['path']}")
        elif dest not in DEST and not dest.startswith("archive:"):
            problems.append(f"bad destination {dest!r}: {r['path']}")
        elif dest.startswith("archive:") and dest.split(":", 1)[1] != "raw" and dest.split(":", 1)[1] not in SKILLS:
            problems.append(f"unknown skill in destination {dest!r}: {r['path']}")

    # 3. every skill named by a destination exists on disk
    for s in SKILLS:
        p = os.path.join(repo, "LLM", "skills", s, "SKILL.md")
        if not os.path.exists(p):
            problems.append(f"missing skill: LLM/skills/{s}/SKILL.md")

    # 4. markdown links inside the skill tree must resolve
    skroot = os.path.join(repo, "LLM", "skills")
    link = re.compile(r"\]\(([^)#]+)")
    for dirpath, _, files in os.walk(skroot):
        for fn in files:
            if not fn.endswith(".md"):
                continue
            p = os.path.join(dirpath, fn)
            txt = open(p, errors="ignore").read()
            for target in link.findall(txt):
                if target.startswith(("http://", "https://", "mailto:")):
                    continue
                if target.startswith("#"):
                    continue
                if not os.path.exists(os.path.normpath(os.path.join(dirpath, target))):
                    problems.append(f"dangling link in {os.path.relpath(p, repo)}: {target}")

    # 5. no skill claims a source path that exists neither in tree nor in the archive list
    arch = os.path.join(work, "archived-paths.txt")
    archived = set()
    if os.path.exists(arch):
        archived = {l.strip() for l in open(arch)}
    cite = re.compile(r"`(LLM/[^`\s]+?\.(?:md|html|tsv|txt))(?::\d+(?:-\d+)?)?`")
    for dirpath, _, files in os.walk(skroot):
        for fn in files:
            if not fn.endswith(".md"):
                continue
            p = os.path.join(dirpath, fn)
            for c in cite.findall(open(p, errors="ignore").read()):
                if c.startswith("LLM/skills/") or c.startswith("LLM/archive/"):
                    continue
                if "*" in c or "<" in c:
                    continue
                if not os.path.exists(os.path.join(repo, c)) and c not in archived:
                    problems.append(f"citation to a vanished path in {os.path.relpath(p, repo)}: {c}")

    # 6. size budget
    total = files = 0
    for dirpath, dirnames, fns in os.walk(os.path.join(repo, "LLM")):
        for fn in fns:
            fp = os.path.join(dirpath, fn)
            if os.path.islink(fp) and not os.path.exists(fp):
                continue
            total += os.path.getsize(fp)
            files += 1
    print(f"LLM/ = {total/1e6:.1f} MB, {files} files")
    if total > 15e6:
        problems.append(f"size budget exceeded: {total/1e6:.1f} MB > 15 MB")
    if files > 1000:
        problems.append(f"file budget exceeded: {files} > 1000")

    if problems:
        print(f"\n{len(problems)} problem(s):")
        for p in problems[:60]:
            print("  -", p)
        return 1
    print("all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
