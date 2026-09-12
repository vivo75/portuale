#!/usr/bin/env python3
"""Align a real-portage `RT_SEL` trace with a portuale `MO_SEL` trace --
the comparator half of the harness described in
`TEST/scripts/mo-trace/README.md`.

Both streams carry one line per `_serialize_tasks`/`select_nodes`
iteration, same field order:

    <PREFIX> iter=N retlist=R alive=A asap=[cpv ...] prefer_asap=0|1
             drop_satisfied=0|1 ig=NAME pick=cat/pkg-ver ...

This walks the two streams in order and reports the first iteration whose
state differs, field by field, so the investigation starts at the exact
selection where the frontier drained differently instead of at the merge
list the binary comparator already flags.

Usage:

    align-traces.py REAL.trace PORTUALE.trace
    align-traces.py --merge-only REAL.trace PORTUALE.trace

`--merge-only` skips the iteration walk (which is sensitive to nomerge
ordering noise) and diffs just the ordered merge picks -- the actual
merge-list sequence.

Exit: 0 aligned, 1 divergence or length mismatch, 2 usage.
"""

import re
import sys

FIELDS = [
    "iter",
    "retlist",
    "alive",
    "asap",
    "prefer_asap",
    "drop_satisfied",
    "ig",
    "pick",
]

LINE_RE = re.compile(
    r"^(?:RT_SEL|MO_SEL)\s+"
    + r"\s+".join(
        [
            r"iter=(\d+)",
            r"retlist=(\d+)",
            r"alive=(\d+)",
            r"asap=\[([^\]]*)\]",
            r"prefer_asap=(\d+)",
            r"drop_satisfied=(\d+)",
            r"ig=(\S+)",
            r"pick=(.*)$",
        ]
    )
)


def parse(path):
    rows = []
    nodes = None
    with open(path, errors="replace") as f:
        for line in f:
            s = line.strip()
            if s.startswith("MO_NODES "):
                if nodes is None:
                    m = re.match(r"MO_NODES count=(\d+)\s*(.*)$", s)
                    if m:
                        nodes = m.group(2).split()
                continue
            m = LINE_RE.match(s)
            if m:
                rows.append(dict(zip(FIELDS, m.groups())))
    return rows, nodes or []


def render(row):
    return (
        f"iter={row['iter']} retlist={row['retlist']} alive={row['alive']} "
        f"asap={row['asap']} prefer_asap={row['prefer_asap']} "
        f"drop_satisfied={row['drop_satisfied']} ig={row['ig']} pick={row['pick']}"
    )


def report_nodes(real_nodes, ptl_nodes):
    """Name a post-prune graph membership difference (the gtk:4
    alive=398 vs 395 gap) before the iteration walk."""
    if not real_nodes and not ptl_nodes:
        return False
    rs, ps = set(real_nodes), set(ptl_nodes)
    print(f"graph nodes: real {len(real_nodes)}  portuale {len(ptl_nodes)}")
    if rs != ps:
        for label, diff in (("only in real", sorted(rs - ps)), ("only in portuale", sorted(ps - rs))):
            if diff:
                print(f"  {label} ({len(diff)}):")
                for n in diff:
                    print(f"    {n}")
        return True
    print("  node sets match")
    return False


def merge_sequence(rows):
    """The ordered merge picks (`m:` items) across the whole trace --
    exactly the order they were appended to `retlist`, i.e. the merge
    list real/portuale are actually compared on. Nomerges are skipped so
    an irrelevant nomerge-ordering difference does not mask the first
    real merge-order divergence."""
    seq = []
    for row in rows:
        for item in row["pick"].split():
            if item.startswith("m:"):
                seq.append(item)
    return seq


def report_merges(real, ptl):
    r, p = merge_sequence(real), merge_sequence(ptl)
    print(f"merge picks: real {len(r)}  portuale {len(p)}")
    for i in range(min(len(r), len(p))):
        if r[i] != p[i]:
            lo = max(0, i - 2)
            print(f"\nfirst merge-order divergence at merge index {i}:")
            for j in range(lo, i):
                print(f"    = {r[j]}")
            print(f"  real     : {r[i]}")
            print(f"  portuale : {p[i]}")
            if len(r) > i + 1 or len(p) > i + 1:
                print(f"  next real     : {r[i+1] if i+1 < len(r) else '<end>'}")
                print(f"  next portuale : {p[i+1] if i+1 < len(p) else '<end>'}")
            return 1
    if len(r) != len(p):
        print(f"\nmerge sequences agree on the common prefix; lengths differ "
              f"(real {len(r)} vs portuale {len(p)})")
        return 1
    print("merge sequences align")
    return 0


def main(argv):
    merge_only = "--merge-only" in argv
    argv = [a for a in argv if a != "--merge-only"]
    if len(argv) != 2:
        print(__doc__)
        return 2
    real, real_nodes = parse(argv[0])
    ptl, ptl_nodes = parse(argv[1])
    node_diff = report_nodes(real_nodes, ptl_nodes)
    if merge_only:
        rc = report_merges(real, ptl)
        return 1 if node_diff or rc else 0
    print(f"real iterations: {len(real)}   portuale iterations: {len(ptl)}")
    n = min(len(real), len(ptl))
    for i in range(n):
        diffs = [f for f in FIELDS if real[i][f] != ptl[i][f]]
        if diffs:
            print(f"\nfirst divergence at trace index {i} (0-based):")
            print(f"  real     : {render(real[i])}")
            print(f"  portuale : {render(ptl[i])}")
            for f in diffs:
                print(f"    {f}: real={real[i][f]!r} portuale={ptl[i][f]!r}")
            return 1
    if len(real) != len(ptl):
        if len(real) > n:
            print(f"\nportuale trace ended first; next real line: {render(real[n])}")
        else:
            print(f"\nreal trace ended first; next portuale line: {render(ptl[n])}")
        return 1
    if node_diff:
        print("\n(nodes differ but the selected sequences align)")
        return 1
    print("traces align through every iteration")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
