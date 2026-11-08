#!/usr/bin/env python3
"""List relative markdown links whose target does not exist.

Usage: check_links.py PATH... (files or directories; directories are walked
for *.md). URLs, mailto: and pure #anchors are ignored; a trailing #anchor
is stripped. A target listed in LLM/ARCHIVE.manifest.tsv counts as present. Exit status 1 when a link is broken.
"""

import re
import sys
from pathlib import Path

LINK = re.compile(r"\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
SKIP = ("http://", "https://", "mailto:", "#")


def markdown_files(paths):
    for p in map(Path, paths):
        if p.is_dir():
            yield from sorted(p.rglob("*.md"))
        elif p.suffix == ".md":
            yield p


def archived_paths(root):
    """Repo-relative paths listed in LLM/ARCHIVE.manifest.tsv (moved out of the tree)."""
    m = root / "LLM" / "ARCHIVE.manifest.tsv"
    if not m.exists():
        return set()
    return {line.split("\t", 1)[0] for line in m.read_text().splitlines()[1:]}


def broken_links(md, archived=frozenset(), root=Path(".")):
    in_fence = False
    for n, line in enumerate(md.read_text(errors="replace").splitlines(), 1):
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        for target in LINK.findall(line):
            if target.startswith(SKIP):
                continue
            path = target.split("#", 1)[0]
            if not path or (md.parent / path).exists():
                continue
            rel = (md.parent / path).resolve().relative_to(root.resolve()).as_posix() \
                if (md.parent / path).resolve().is_relative_to(root.resolve()) else ""
            if rel not in archived:
                yield n, target


def main(argv):
    bad = 0
    root = Path(__file__).resolve().parents[2]
    archived = archived_paths(root)
    for md in markdown_files(argv[1:]):
        for n, target in broken_links(md, archived, root):
            print(f"{md}:{n}: {target}")
            bad += 1
    print(f"{bad} broken link(s)", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
