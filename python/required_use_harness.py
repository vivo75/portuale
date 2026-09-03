#!/usr/bin/env python3
"""Neutral CLI test-harness binary (Python side) for the REQUIRED_USE
portuale -- see docs/agent-context.md and
rust/portage-required-use/src/lib.rs for what is and isn't
ported. Wraps the real portage.dep.check_required_use rather than
reimplementing it, always called with eapi="8" -- pinned so both sides
agree on the same EAPI-gated attributes the Rust side hardcodes
unconditionally (see that crate's own doc comment): "??" (at-most-one-of)
groups always recognized (real eapi >= 5), and an empty group NEVER
treated as vacuously satisfied (real eapi <= 6's own special case,
deliberately not replicated -- ordinary per-operator evaluation on an
empty list is used instead, real EAPI 7+ behavior).

Usage:
    required_use_harness.py check <enabled> <iuse> <token...>
        enabled: comma-separated effective USE flags, or "-" for none
        iuse: comma-separated declared IUSE flags, or "-" for none
        token...: the REQUIRED_USE string's whitespace-separated tokens
      -> "true" | "false" | "ERROR"
    required_use_harness.py batch
      -> reads "check <enabled> <iuse> <token...>" lines from stdin, one
         result per line
"""

import os
import sys

sys.path.insert(0, os.path.join(
    os.environ.get("PORTUALE_PORTAGE_CHECKOUT")
    or os.path.join(os.path.dirname(__file__), "..", "3rdparty", "portage"),
    "lib",
))

from portage.dep import check_required_use
from portage.exception import InvalidDependString


def _parse_set(arg):
    return frozenset() if arg == "-" else frozenset(arg.split(","))


def _format_check(enabled_arg, iuse_arg, tokens):
    enabled = _parse_set(enabled_arg)
    iuse = _parse_set(iuse_arg)
    required_use = " ".join(tokens)
    try:
        satisfied = bool(
            check_required_use(required_use, enabled, lambda flag: flag in iuse, eapi="8")
        )
    except InvalidDependString:
        return "ERROR"
    return "true" if satisfied else "false"


def _dispatch(op, args):
    if op == "check":
        if len(args) < 2:
            raise ValueError("check expects at least 2 args (enabled, iuse)")
        return _format_check(args[0], args[1], args[2:])
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
            "usage: required_use_harness.py <check enabled iuse token... | batch>",
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
