#!/usr/bin/env python3
"""Run `cargo dupes` over crates/ only and print a readable summary.

Usage: dupes.py [--exclude-tests] [--json OUT]
The JSON stream cargo-dupes emits holds five values: the summary, then the
exact-function, near-function, sub-function-exact and sub-function-near
group arrays. Sub-function members report the *enclosing* function's span,
not the duplicated block's, so their line numbers locate, not measure.
"""
import json
import subprocess
import sys

args = ["cargo", "dupes", "-p", "crates", "--sub-function", "--min-lines", "50", "--format", "json"]
if "--exclude-tests" in sys.argv:
    args.append("--exclude-tests")
raw = subprocess.run(args, capture_output=True, text=True, check=True).stdout
if "--json" in sys.argv:
    open(sys.argv[sys.argv.index("--json") + 1], "w").write(raw)
dec, i, vals = json.JSONDecoder(), 0, []
while raw[i:].strip():
    rest = raw[i:].lstrip()
    v, j = dec.raw_decode(rest)
    vals.append(v)
    i = len(raw) - len(rest) + j
summary, exact, near, sub_exact, sub_near = vals
print(json.dumps(summary, indent=1))
for label, groups in (("EXACT", exact), ("NEAR", near)):
    for g in groups:
        members = ", ".join(f"{m['name']} {m['file']}:{m['line_start']}-{m['line_end']}" for m in g["members"])
        print(f"{label} {g.get('similarity', 1):.3f}: {members}")
by_fn = {}
for label, groups in (("sub-exact", sub_exact), ("sub-near", sub_near)):
    for g in groups:
        for m in g["members"]:
            key = (m["file"], m.get("parent_name") or m["name"])
            by_fn[key] = by_fn.get(key, 0) + 1
print("\nsub-function duplicate members by enclosing unit (top 25):")
for (f, n), c in sorted(by_fn.items(), key=lambda x: -x[1])[:25]:
    print(f"  {c:4d}  {f}  {n}")
