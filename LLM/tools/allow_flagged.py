#!/usr/bin/env python3
"""Add a tracked `#[allow]` to every function clippy flags for a lint.

Usage: cargo clippy --release --all-targets --message-format=json | allow_flagged.py LINT...
The attribute carries `reason = "#336 Phase 5 worklist"` so the remaining
work is greppable; Phase 5 commits remove it as they fix each function.
"""
import json
import sys
from collections import defaultdict

lints = {f"clippy::{l}" for l in sys.argv[1:]}
spots = defaultdict(set)
for line in sys.stdin:
    try:
        msg = json.loads(line).get("message") or {}
    except json.JSONDecodeError:
        continue
    code = (msg.get("code") or {}).get("code")
    if code not in lints:
        continue
    span = next(s for s in msg["spans"] if s["is_primary"])
    spots[span["file_name"]].add((span["line_start"], code))
for path, items in spots.items():
    lines = open(path).read().split("\n")
    for line_no, code in sorted(items, reverse=True):
        i = line_no - 1
        # the primary span starts at the `fn` line (or its first attribute); step
        # back over attributes so the allow sits with them, above the doc-less fn
        indent = lines[i][: len(lines[i]) - len(lines[i].lstrip())]
        lines.insert(i, f'{indent}#[allow({code}, reason = "#336 Phase 5 worklist")]')
    open(path, "w").write("\n".join(lines))
    print(f"{path}: {len(items)}")
