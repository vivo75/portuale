#!/usr/bin/env python3
"""Re-pin review checklist (`docs/second_python_copy_removal.md` §8).

Given two refs of the `3rdparty/portage` checkout (the old and the new
pin), list every Python function or method that changed under
`lib/_emerge` and `lib/portage`, then find the Rust code that cites it:

* by line number -- `depgraph.py:9461`, `vartree.py:6363-6380`, ... whose
  line falls inside the function's span at the *old* pin;
* by name -- a Rust comment or doc that names the function
  (`_serialize_tasks`, `dep_zapdeps`, ...).

The output is a Markdown checklist of Rust locations to re-read. It is
the main defence against upstream behaviour changes now that no second
copy of the resolver is maintained.

Usage:
  scripts/portage_repin_review.py OLD_REF NEW_REF [--checkout 3rdparty/portage]
"""

from __future__ import annotations

import argparse
import ast
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
RUST = REPO / "rust"
SCOPES = ("lib/_emerge", "lib/portage")
CITATION = re.compile(r"([A-Za-z_][\w/]*\.py):(\d+)(?:-(\d+))?")


def git(checkout: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(checkout), *args], capture_output=True,
                          text=True, check=True).stdout


def changed_ranges(checkout: Path, old: str, new: str) -> dict[str, list[tuple[int, int]]]:
    """Old-side line ranges touched by the diff, per path."""
    out = git(checkout, "diff", "--unified=0", old, new, "--", *SCOPES)
    ranges: dict[str, list[tuple[int, int]]] = defaultdict(list)
    path = None
    for line in out.splitlines():
        if line.startswith("--- "):
            path = line[6:] if line.startswith("--- a/") else None
        elif line.startswith("+++ ") and path is None and line.startswith("+++ b/"):
            path = line[6:]  # added file: no old side, record as whole-file
            ranges[path].append((0, 10**9))
        elif line.startswith("@@") and path and path.endswith(".py"):
            m = re.match(r"@@ -(\d+)(?:,(\d+))?", line)
            assert m is not None, line
            start, count = int(m.group(1)), int(m.group(2) or "1")
            ranges[path].append((start, start + max(count, 1) - 1))
    return {p: r for p, r in ranges.items() if p.endswith(".py")}


def functions(source: str) -> list[tuple[str, int, int]]:
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return []
    found = []

    def visit(node, prefix=""):
        for child in ast.iter_child_nodes(node):
            if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                name = f"{prefix}{child.name}"
                if not isinstance(child, ast.ClassDef):
                    found.append((name, child.lineno, child.end_lineno))
                visit(child, f"{name}.")
    visit(tree)
    return found


def rust_index():
    """Line-number citations per `.py` basename, and comment identifiers."""
    citations: dict[str, list[tuple[int, int, str]]] = defaultdict(list)
    names: dict[str, list[str]] = defaultdict(list)
    for rs in sorted(RUST.glob("*/src/**/*.rs")):
        rel = rs.relative_to(REPO)
        for i, text in enumerate(rs.read_text(errors="replace").splitlines(), 1):
            if "//" not in text:
                continue
            comment = text.split("//", 1)[1]
            for m in CITATION.finditer(comment):
                a = int(m.group(2))
                citations[Path(m.group(1)).name].append(
                    (a, int(m.group(3) or a), f"{rel}:{i} (cites {m.group(0)})"))
            for ident in set(re.findall(r"[A-Za-z_]\w{4,}", comment)):
                names[ident].append(f"{rel}:{i} (names `{ident}`)")
    return citations, names


def main() -> int:
    ap = argparse.ArgumentParser(description="Re-pin review checklist for 3rdparty/portage.")
    ap.add_argument("old")
    ap.add_argument("new")
    ap.add_argument("--checkout", type=Path, default=REPO / "3rdparty" / "portage")
    opts = ap.parse_args()

    touched = changed_ranges(opts.checkout, opts.old, opts.new)
    citations, names = rust_index()
    print(f"# portage re-pin review: {opts.old} -> {opts.new}\n")
    if not touched:
        print("No Python changes under lib/_emerge or lib/portage.")
        return 0
    for path, ranges in sorted(touched.items()):
        try:
            old_src = git(opts.checkout, "show", f"{opts.old}:{path}")
        except subprocess.CalledProcessError:
            old_src = ""
        changed = sorted({
            (name, lo, hi) for name, lo, hi in functions(old_src)
            for a, b in ranges if a <= hi and b >= lo
        }, key=lambda f: f[1])
        base = Path(path).name
        print(f"## {path}\n")
        if not changed:
            print("- (module-level change only)\n")
        for name, lo, hi in changed:
            short = name.rsplit(".", 1)[-1]
            hits = [ref for lo_, hi_, ref in citations.get(base, []) if lo_ <= hi and hi_ >= lo]
            # Name matches only for distinctive identifiers: dunders and
            # plain words (`update`, `match`) would drown the list.
            if "_" in short.strip("_") or short.startswith("_") and not short.endswith("__"):
                hits += names.get(short, [])
            print(f"- [ ] `{name}` (old lines {lo}-{hi})")
            for h in sorted(set(hits)):
                print(f"  - {h}")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
