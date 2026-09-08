#!/usr/bin/env python3
"""Structured filesystem + VDB diff of two normalised snapshots.

SKELETON -- implemented in slice 2 (L1). Will emit typed findings
(MISSING / MODE / OWNER / XATTR / SIZE / CONTENT / SYMLINK / VDB:<field>
/ CONTENTS / MTIME) per docs/real-world-testing.md §4.4, apply
`known-divergences.yaml`, and exit non-zero on any unexplained hard
finding.

    diff.py <snap-a-prefix> <snap-b-prefix> [allowlist.yaml]

L0 does not use this file (see resolve-compare.py).
"""
import sys


def main(argv: list[str]) -> int:
    raise SystemExit(
        "diff.py is a slice-2 (L1) skeleton -- not yet implemented. "
        "See docs/real-world-testing.md §4.4 and TEST/compare/normalize.md"
    )


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
