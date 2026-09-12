#!/usr/bin/env python3
"""Align a real-portage `RT_SEL` trace with a portuale `MO_SEL` trace --
the comparator half of the harness described in
`TEST/scripts/mo-trace/README.md`.

Both streams carry one line per `_serialize_tasks`/`select_nodes`
iteration, same field order:

    <PREFIX> iter=N retlist=R alive=A asap=S prefer_asap=0|1
             drop_satisfied=0|1 ig=NAME pick=cat/pkg-ver ...

This walks the two streams in order and reports the first iteration whose
state differs, field by field, so the investigation starts at the exact
selection where the frontier drained differently instead of at the merge
list the binary comparator already flags.

Usage:

    align-traces.py REAL.trace PORTUALE.trace

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
            r"asap=(\d+)",
            r"prefer_asap=(\d+)",
            r"drop_satisfied=(\d+)",
            r"ig=(\S+)",
            r"pick=(.*)$",
        ]
    )
)


def parse(path):
    rows = []
    with open(path, errors="replace") as f:
        for line in f:
            m = LINE_RE.match(line.strip())
            if m:
                rows.append(dict(zip(FIELDS, m.groups())))
    return rows


def render(row):
    return (
        f"iter={row['iter']} retlist={row['retlist']} alive={row['alive']} "
        f"asap={row['asap']} prefer_asap={row['prefer_asap']} "
        f"drop_satisfied={row['drop_satisfied']} ig={row['ig']} pick={row['pick']}"
    )


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    real, ptl = parse(argv[0]), parse(argv[1])
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
    print("traces align through every iteration")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
