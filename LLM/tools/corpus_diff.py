#!/usr/bin/env python3
"""Diff two pmtest corpus files (contract.json.xz) record by record.

Usage: corpus_diff.py BEFORE.json.xz AFTER.json.xz

Prints, for every record whose fields differ, the unified diff of each
changed text field, then a count. Use it to review a drift before blessing
it: a refactor's bless should show only the intended text change.
"""

import difflib
import json
import lzma
import sys


def load(path):
    with lzma.open(path, "rt") as f:
        return json.load(f)


def records(corpus):
    """Cases with their stdout/stderr blob ids resolved to text."""
    blobs = corpus["blobs"]
    out = {}
    for key, case in corpus["cases"].items():
        case = dict(case)
        for field in ("stdout", "stderr"):
            if case.get(field) in blobs:
                case[field] = blobs[case[field]]
        out[key] = case
    return out


def main(before, after):
    a, b = records(load(before)), records(load(after))
    changed = 0
    for key in sorted(set(a) | set(b)):
        ra, rb = a.get(key), b.get(key)
        if ra == rb:
            continue
        changed += 1
        print(f"=== {key}")
        if ra is None or rb is None:
            print("  only in", "after" if ra is None else "before")
            continue
        for field in sorted(set(ra) | set(rb)):
            va, vb = ra.get(field), rb.get(field)
            if va == vb:
                continue
            if isinstance(va, str) and isinstance(vb, str):
                for line in difflib.unified_diff(va.splitlines(), vb.splitlines(), lineterm="", n=0):
                    if not line.startswith(("---", "+++", "@@")):
                        print(f"  {field}: {line}")
            else:
                print(f"  {field}: {va!r} -> {vb!r}")
    print(f"{changed} changed record(s)", file=sys.stderr)


if __name__ == "__main__":
    main(*sys.argv[1:3])
