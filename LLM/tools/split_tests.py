#!/usr/bin/env python3
"""Split a big `tests/mod.rs` into topic files (#336 Phase 6, step T1).

Usage: split_tests.py TESTS_MOD_RS CRATE_SRC_DIR [--dry-run]

Every `#[test]` function (with its attributes and comments) moves to
`tests/<topic>.rs`, where <topic> is the production module whose
functions the test body calls most (module of each production fn is read
from the crate's own module files). Helpers, imports, consts and nested
`mod`s stay in `tests/mod.rs`; each topic file starts with `use super::*;`,
which sees them (a child module sees its parent's private items). Tests
whose calls point nowhere specific go by the test helper they call
(`--helper-topics` TSV), else to `tests/graph.rs`.
"""

import collections
import os
import re
import sys

sys.path.insert(0, os.path.dirname(__file__))
from rust_items import items  # noqa: E402

IDENT = re.compile(r"(?<![.\w])([a-z_][a-z0-9_]*)\s*\(")


def production_fn_modules(src_dir):
    """Free (column-0) fn name -> module stem, for names defined in exactly
    one production file."""
    seen = collections.defaultdict(set)
    for dirpath, _, files in os.walk(src_dir):
        if os.path.basename(dirpath) == "tests" or "/tests/" in dirpath + "/":
            continue
        for f in files:
            if not f.endswith(".rs") or f in ("lib.rs", "main.rs"):
                continue
            path = os.path.join(dirpath, f)
            stem = os.path.basename(dirpath) if f == "mod.rs" else f[:-3]
            for m in re.finditer(r"(?m)^(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe)\s+)*fn\s+([a-z_][a-z0-9_]*)", open(path).read()):
                seen[m.group(1)].add(stem)
    return {n: next(iter(ms)) for n, ms in seen.items() if len(ms) == 1}


def main(path, src_dir, dry, helper_topics_path=None):
    helper_topics = {}
    if helper_topics_path:
        for line in open(helper_topics_path):
            if line.strip() and not line.startswith("#"):
                h, t = line.split()[:2]
                helper_topics[h] = t
    src = open(path).read()
    lines = src.split("\n")
    fn_mod = production_fn_modules(src_dir)
    helpers = set()
    its = items(src)
    tests, keep = [], []
    for s, e, kind, name in its:
        span = lines[s - 1:e]
        is_test = kind == "fn" and any(l.strip() == "#[test]" for l in span[:15])
        (tests if is_test else keep).append((s, e, kind, name))
        if not is_test and kind == "fn":
            m = re.match(r"(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)", name)
            if m:
                helpers.add(m.group(1))
    topic_of = {}
    for s, e, kind, name in tests:
        body = "\n".join(lines[s - 1:e])
        calls = collections.Counter(
            fn_mod[c] for c in IDENT.findall(body) if c in fn_mod and c not in helpers)
        tname = re.search(r"fn\s+(\w+)", name).group(1)
        if calls:
            best = max(calls.values())
            tied = sorted(m for m, n in calls.items() if n == best)
            named = [m for m in tied if m in tname]
            topic_of[(s, e)] = named[0] if named else tied[0]
        else:
            hits = [helper_topics[c] for c in IDENT.findall(body) if c in helper_topics]
            topic_of[(s, e)] = collections.Counter(hits).most_common(1)[0][0] if hits else "graph"
    files = collections.OrderedDict()
    for s, e, kind, name in tests:
        files.setdefault(topic_of[(s, e)], []).append("\n".join(lines[s - 1:e]))
    kept = "\n\n".join("\n".join(lines[s - 1:e]) for s, e, _, _ in keep)
    decl = "".join(f"mod {t};\n" for t in sorted(files))
    out = {path: kept.rstrip("\n") + "\n\n" + decl}
    d = os.path.dirname(path)
    for t, texts in files.items():
        out[os.path.join(d, f"{t}.rs")] = "use super::*;\n\n" + "\n\n".join(texts) + "\n"
    for p in sorted(out):
        print(f"{out[p].count(chr(10)):6d}  {p}")
    if not dry:
        for p, t in out.items():
            if os.path.exists(p) and p != path:
                sys.exit(f"refusing to overwrite {p}")
        for p, t in out.items():
            open(p, "w").write(t)


if __name__ == "__main__":
    ht = sys.argv[sys.argv.index("--helper-topics") + 1] if "--helper-topics" in sys.argv else None
    main(sys.argv[1], sys.argv[2], "--dry-run" in sys.argv, ht)
