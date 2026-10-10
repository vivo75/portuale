#!/usr/bin/env python3
"""Apply identifier renames from a TSV to every .rs file under crates/.

Usage: rename_idents.py INVENTORY.tsv [--dry-run]

Rows with a non-empty `new` column are applied as whole-word replacements
(`\\bold\\b`), longest `old` first so a name never clobbers a longer one that
contains it. Prints per-identifier hit counts; refuses to run if a `new`
name already occurs in the tree (a collision).
"""

import re
import sys
from pathlib import Path


def main(argv):
    tsv, dry = argv[1], "--dry-run" in argv
    rows = [line.split("\t") for line in Path(tsv).read_text().splitlines()[1:]]
    pairs = sorted(((r[0], r[2]) for r in rows if len(r) > 2 and r[2]),
                   key=lambda p: -len(p[0]))
    files = sorted(Path("crates").rglob("*.rs"))
    texts = {f: f.read_text() for f in files}
    blob = "\n".join(texts.values())
    clashes = [new for _, new in pairs if re.search(rf"\b{re.escape(new)}\b", blob)]
    if clashes:
        sys.exit(f"collision, new names already present: {clashes}")
    total = 0
    for old, new in pairs:
        pat = re.compile(rf"\b{re.escape(old)}\b")
        hits = 0
        for f, t in texts.items():
            t2, n = pat.subn(new, t)
            if n:
                texts[f], hits = t2, hits + n
        total += hits
        print(f"{hits:5d}  {old} -> {new}")
    if not dry:
        for f in files:
            if texts[f] != f.read_text():
                f.write_text(texts[f])
    print(f"{total} replacements in {len(pairs)} identifiers", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv)
