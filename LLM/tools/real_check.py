#!/usr/bin/env python3
"""#336 Phase 3 end check: every identifier under crates/ that still contains
"real" must be classified as kept (ACTUAL/FROZEN) in the inventory TSV.
Prints unclassified identifiers; exit 1 if any.
Usage: real_check.py LLM/plans/2026-10-10-real-inventory.tsv
"""
import re
import sys
from pathlib import Path

rows = [l.split("\t") for l in Path(sys.argv[1]).read_text().splitlines()[1:]]
kept = {r[0] for r in rows if not r[2]} | {r[2] for r in rows if r[2]}
ident = re.compile(r"\b[A-Za-z0-9_]*([Rr]eal_|_[Rr]eal|Real|REAL_|_REAL)[A-Za-z0-9_]*\b")
found = set()
for f in Path("crates").rglob("*.rs"):
    found |= {m.group(0) for m in ident.finditer(f.read_text())}
bad = sorted(found - kept)
print("\n".join(bad))
print(f"{len(found)} identifiers with 'real', {len(bad)} unclassified", file=sys.stderr)
sys.exit(1 if bad else 0)
