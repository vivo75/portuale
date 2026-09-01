#!/usr/bin/env python3
"""Neutral CLI test-harness binary (Python side) for the
use_reduce(flat=True) pilot -- see PROMPT.md and
rust/use-reduce-harness/src/use_reduce.rs for what is and isn't
ported. Wraps the real portage.dep.use_reduce rather than reimplementing
it, always called with flat=True and no masklist/excludeall/is_src_uri/
opconvert/subset/token_class -- see the Rust side's doc comment for why
those are out of v1 scope.

Usage:
    use_reduce_harness.py reduce <mode> <uselist> <token...>
        mode: "normal" | "matchall" | "matchnone"
        uselist: comma-separated enabled flags, or "-" for none
        token...: the dep string's whitespace-separated tokens
      -> comma-joined flattened tokens (possibly empty), or "ERROR"
    use_reduce_harness.py batch
      -> reads "reduce <mode> <uselist> <token...>" lines from stdin, one
         result per line
"""

import os
import sys

sys.path.insert(0, os.path.join(
    os.environ.get("PORTUALE_PORTAGE_CHECKOUT")
    or os.path.join(os.path.dirname(__file__), "..", "3rdparty", "portage"),
    "lib",
))

from portage.dep import use_reduce
from portage.exception import InvalidDependString


def _format_reduce(mode, uselist_arg, tokens):
    uselist = frozenset() if uselist_arg == "-" else frozenset(uselist_arg.split(","))
    depstr = " ".join(tokens)
    if mode == "normal":
        kwargs = {"uselist": uselist}
    elif mode == "matchall":
        kwargs = {"matchall": True}
    elif mode == "matchnone":
        kwargs = {"matchnone": True}
    else:
        raise ValueError(f"unknown mode {mode!r}")
    try:
        result = use_reduce(depstr, flat=True, **kwargs)
    except InvalidDependString:
        return "ERROR"
    return ",".join(result)


def _dispatch(op, args):
    if op == "reduce":
        if len(args) < 2:
            raise ValueError("reduce expects at least 2 args (mode, uselist)")
        return _format_reduce(args[0], args[1], args[2:])
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
            "usage: use_reduce_harness.py <reduce mode uselist token... | batch>",
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
