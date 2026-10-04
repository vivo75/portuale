#!/usr/bin/env python3
"""#305 S0.2: list every installed-database access site in rust/ (read-only).

A hit is test code when it lies inside an item annotated `#[cfg(test)]`
(a `mod`, `fn`, `impl`, ... : the span from the attribute to the item's
matching closing brace, braces counted outside strings/comments roughly),
or anywhere under a `tests/` directory.  Everything else is production.

Usage: python3 docs/evidence/305-s0-inventory.py [--all]   (from portuale/)
"""
import re, sys, pathlib

PATTERNS = [
    ("vdb", re.compile(r'var/db/pkg|"db"\)\.join\("pkg"|VDB_DIR|vdb_dir\(')),
    ("world", re.compile(r'var/lib/portage/world(?!_sets)|"world"\)')),
    ("world_sets", re.compile(r'var/lib/portage/world_sets|"world_sets"\)')),
    ("preserved_libs", re.compile(r'preserved_libs_registry')),
    ("config_memory", re.compile(r'var/lib/portage/config')),
    ("counter", re.compile(r'cache/edb/counter')),
]

def test_spans(lines):
    spans = []
    i = 0
    n = len(lines)
    while i < n:
        if re.match(r'\s*#\[cfg\(test\)\]', lines[i]):
            start = i
            depth = 0
            seen = False
            j = i + 1
            while j < n:
                code = re.sub(r'//.*', '', lines[j])
                code = re.sub(r'"(\\.|[^"\\])*"', '""', code)
                code = re.sub(r"'(\\.|[^'\\])'", "''", code)
                for ch in code:
                    if ch == '{':
                        depth += 1; seen = True
                    elif ch == '}':
                        depth -= 1
                if not seen and code.rstrip().endswith(';'):
                    break  # `#[cfg(test)] use ...;` / `mod x;`
                if seen and depth <= 0:
                    break
                j += 1
            spans.append((start + 1, j + 1))
            i = j + 1
        else:
            i += 1
    return spans

def main():
    show_all = "--all" in sys.argv
    root = pathlib.Path("rust")
    per_file = {}
    for p in sorted(root.rglob("*.rs")):
        if "target" in p.parts:
            continue
        lines = p.read_text(errors="replace").splitlines()
        spans = test_spans(lines)
        in_tests_dir = "tests" in p.parts
        for no, line in enumerate(lines, 1):
            for store, rx in PATTERNS:
                if rx.search(line):
                    test = in_tests_dir or any(a <= no <= b for a, b in spans)
                    key = (str(p), "test" if test else "prod")
                    per_file[key] = per_file.get(key, 0) + 1
                    if show_all or not test:
                        print(f"{'TEST' if test else 'PROD'}\t{store}\t{p}:{no}:\t{line.strip()[:160]}")
                    break
    print("\n# per file: prod test")
    files = sorted({f for f, _ in per_file})
    tp = tt = 0
    for f in files:
        a = per_file.get((f, "prod"), 0); b = per_file.get((f, "test"), 0)
        tp += a; tt += b
        print(f"# {a:5d} {b:5d}  {f}")
    print(f"# {tp:5d} {tt:5d}  TOTAL ({tp+tt})")

main()
