#!/usr/bin/env python3
"""Vendors a real Gentoo tree snapshot for the benchmark dataset, per
PROMPT.md ("Benchmark data: a real, vendored Gentoo tree snapshot
(not purely synthetic stress data)").

Walks a local ebuild repository (e.g. /.gentoo/repos/gentoo -- a full
Gentoo tree checkout) and records the set of real version strings that
exist for each package, using the real portage.versions.pkgsplit as the
authority for what a version string is (rather than re-deriving version
parsing here). The output is a plain JSON map of "category/package" to a
sorted list of version strings, committed at gentoo_snapshot.json so the
benchmark dataset doesn't depend on this path existing at CI/benchmark
time -- only at (occasional, manual) re-vendoring time.

Usage:
    python3 bench/extract_snapshot.py /.gentoo/repos/gentoo
    python3 bench/extract_snapshot.py /.gentoo/repos/gentoo --out other.json
"""

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = Path(__file__).resolve().parent / "gentoo_snapshot.json"

sys.path.insert(0, str(REPO_ROOT / "3rdparty" / "portage" / "lib"))
from portage.versions import pkgsplit


def extract(tree: Path) -> dict[str, list[str]]:
    packages: dict[str, set[str]] = {}
    skipped = []
    for ebuild in tree.glob("*/*/*.ebuild"):
        stem = ebuild.stem  # "pn-pv", e.g. "vim-9.1.1652-r2"
        split = pkgsplit(stem)
        if split is None:
            skipped.append(str(ebuild))
            continue
        pn, ver, rev = split
        full_version = ver if rev == "r0" else f"{ver}-{rev}"
        cp = f"{ebuild.parent.parent.name}/{pn}"
        packages.setdefault(cp, set()).add(full_version)

    if skipped:
        print(
            f"warning: {len(skipped)} ebuild(s) had unparseable filenames, skipped:",
            file=sys.stderr,
        )
        for path in skipped[:10]:
            print(f"  {path}", file=sys.stderr)

    return {cp: sorted(versions) for cp, versions in packages.items()}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tree", type=Path, help="path to a Gentoo ebuild repository")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args()

    if not args.tree.is_dir():
        print(f"error: {args.tree} is not a directory", file=sys.stderr)
        return 1

    packages = extract(args.tree)
    total_versions = sum(len(v) for v in packages.values())
    args.out.write_text(
        json.dumps(packages, sort_keys=True, separators=(",", ":")) + "\n"
    )
    print(
        f"wrote {args.out}: {len(packages)} packages, {total_versions} version strings"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
