#!/usr/bin/env python3
"""Neutral CLI test-harness binary (Python side) for the versions-comparison
pilot -- see PORTING/PROMPT.md, "Test/benchmark harness architecture". Wraps
portage.versions instead of the real product CLI, and exposes the same
argv/output contract as the Rust harness at
PORTING/rust/versions-harness so both can be driven identically by a
black-box test suite.

Usage:
    versions_harness.py vercmp <ver1> <ver2>   -> prints an integer or "None"
    versions_harness.py ververify <ver>        -> prints "True" or "False"
    versions_harness.py batch                  -> reads "<op> <args...>"
                                                   lines from stdin, one
                                                   result per line on
                                                   stdout (benchmark mode:
                                                   avoids per-op
                                                   fork/exec overhead)
"""

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "lib"))

from portage.versions import vercmp, ververify


def _dispatch(op, args):
    if op == "vercmp":
        if len(args) != 2:
            raise ValueError(f"vercmp expects 2 args, got {len(args)}")
        result = vercmp(args[0], args[1])
        return "None" if result is None else str(result)
    if op == "ververify":
        if len(args) != 1:
            raise ValueError(f"ververify expects 1 arg, got {len(args)}")
        return str(ververify(args[0]))
    raise ValueError(f"unknown op {op!r}")


def _run_batch():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        parts = line.split()
        print(_dispatch(parts[0], parts[1:]))


def main(argv):
    if not argv:
        print(
            "usage: versions_harness.py <vercmp v1 v2 | ververify v | batch>",
            file=sys.stderr,
        )
        return 2
    try:
        if argv[0] == "batch":
            _run_batch()
        else:
            print(_dispatch(argv[0], argv[1:]))
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
