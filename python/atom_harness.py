#!/usr/bin/env python3
"""Neutral CLI test-harness binary (Python side) for the atom-matching
pilot -- see PORTING/PROMPT.md and PORTING/rust/atom-harness/src/atom.rs
for the deliberately narrowed v1 grammar this exercises. Wraps the real
portage.dep.Atom / portage.dep.match_from_list rather than reimplementing
them, and rejects any atom that uses a feature outside the v1 subset (USE
deps, extended/wildcard syntax, build-ids, repo constraints, slot
operators, the "=*" glob operator) so both harnesses agree on the same
input language rather than Python silently accepting a wider one.

Usage:
    atom_harness.py parse <atom>              -> tab-separated fields, or "INVALID"
    atom_harness.py match <atom> <cand...>    -> comma-joined matches
                                                  (possibly empty), or "INVALID"
    atom_harness.py batch                     -> reads "parse <atom>" or
                                                  "match <atom> <cand...>" lines
                                                  from stdin, one result per line
"""

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "lib"))

from portage.dep import Atom, match_from_list
from portage.exception import InvalidAtom
from portage.versions import catpkgsplit

_SUPPORTED_OPERATORS = {None, "=", ">", ">=", "<", "<=", "~"}


def _parse_v1_atom(s):
    """Returns a real Atom if `s` parses under the v1 grammar subset, else
    None (either it's not a valid atom at all, or it uses a feature this
    pilot's Rust side doesn't implement)."""
    try:
        a = Atom(s, allow_wildcard=True)
    except InvalidAtom:
        return None
    if (
        a.use is not None
        or a.extended_syntax
        or a.build_id is not None
        or a.repo is not None
        or a.slot_operator is not None
        or a.operator not in _SUPPORTED_OPERATORS
    ):
        return None
    return a


def _format_parse(s):
    a = _parse_v1_atom(s)
    if a is None:
        return "INVALID"

    blocker = ""
    if a.blocker:
        blocker = "!!" if a.blocker.overlap.forbid else "!"

    if a.version is None:
        version = ""
        revision = ""
    else:
        _, _, version, rev = catpkgsplit(a.cpv)
        revision = "" if rev == "r0" else rev[1:]

    category, package = a.cp.split("/", 1)
    fields = [
        blocker,
        a.operator or "",
        category,
        package,
        version,
        revision,
        a.slot or "",
        a.sub_slot or "",
    ]
    return "\t".join(fields)


def _format_match(atom_str, candidates):
    a = _parse_v1_atom(atom_str)
    if a is None:
        return "INVALID"
    matches = match_from_list(a, list(candidates))
    return ",".join(matches)


def _dispatch(op, args):
    if op == "parse":
        if len(args) != 1:
            raise ValueError(f"parse expects 1 arg, got {len(args)}")
        return _format_parse(args[0])
    if op == "match":
        if len(args) < 1:
            raise ValueError("match expects at least 1 arg (the atom)")
        return _format_match(args[0], args[1:])
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
            "usage: atom_harness.py <parse atom | match atom cand... | batch>",
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
