#!/usr/bin/env python3
"""Rewrite relative markdown links after files were moved.

Usage: relink.py MOVES.tsv ROOT PATH...

MOVES.tsv holds `old<TAB>new` repo-relative paths, as already applied with
`git mv` (a directory row moves everything below it). Every *.md file under
PATH (given at its *new* location) has each relative link resolved from the
file's *old* location; if the target moved, or the file itself moved, the
link is recomputed so it points at the target's new location. Links whose
target did not exist before are left alone. Anchors are kept.
"""

import os
import re
import sys
from pathlib import Path

LINK = re.compile(r"(\]\()([^)\s#]*)((?:#[^)\s]*)?)((?:\s+\"[^\"]*\")?\))")
SKIP = ("http://", "https://", "mailto:")


def load_moves(tsv):
    moves = []
    for line in Path(tsv).read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            old, new = line.split("\t")[:2]
            moves.append((old.rstrip("/"), new.rstrip("/")))
    return moves


def remap(path, moves, forward=True):
    """Map a repo-relative path through the moves (old->new or new->old)."""
    for old, new in moves:
        src, dst = (old, new) if forward else (new, old)
        if path == src or path.startswith(src + "/"):
            return dst + path[len(src):]
    return path


def main(argv):
    moves = load_moves(argv[1])
    root = Path(argv[2]).resolve()
    files = []
    for p in map(Path, argv[3:]):
        files += sorted(p.rglob("*.md")) if p.is_dir() else [p]
    changed = 0
    for md in files:
        new_rel = md.resolve().relative_to(root).as_posix()
        old_dir = Path(remap(new_rel, moves, forward=False)).parent

        def fix(m):
            target = m.group(2)
            if not target or target.startswith(SKIP) or target.startswith("/"):
                return m.group(0)
            old_target = os.path.normpath(old_dir / target)
            if old_target.startswith(".."):
                return m.group(0)
            new_target = remap(old_target, moves)
            if not (root / new_target).exists():
                return m.group(0)
            rel = os.path.relpath(new_target, Path(new_rel).parent)
            if target.endswith("/") and not rel.endswith("/"):
                rel += "/"
            return m.group(1) + rel + m.group(3) + m.group(4)

        text = md.read_text()
        out = LINK.sub(fix, text)
        if out != text:
            md.write_text(out)
            changed += 1
    print(f"relinked {changed} file(s)", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv)
