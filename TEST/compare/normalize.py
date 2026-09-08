#!/usr/bin/env python3
"""Normalise a snapshot.sh manifest + VDB tar in place for diff.py.

SKELETON -- implemented in slice 2 (L1). The ruleset it must apply is
`TEST/compare/normalize.md`. Left as a stub so slice 1 (L0, which uses
`resolve-compare.py` and needs no filesystem normalisation) can ship
with the directory layout in place.

    normalize.py <out-prefix>     # rewrites <out-prefix>.files.tsv etc.
"""
import sys

RULESET = "TEST/compare/normalize.md"


def main(argv: list[str]) -> int:
    raise SystemExit(
        f"normalize.py is a slice-2 (L1) skeleton -- not yet implemented. "
        f"Spec: {RULESET}"
    )


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
