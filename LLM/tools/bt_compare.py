#!/usr/bin/env python3
"""Line up Portage's backtrack masks against portuale's BT trace.

Reads two inputs and prints one table, one row per object:

* a Portage debug capture (``portage-debug-masks.txt``): ``runtime_pkg_mask:``
  snapshot lines (``<Package ('type', '/', 'cat/pkg-ver', ...)>: {reason: ...}``)
  and ``- cat/pkg-ver::repo (masked by: ...)`` messages;
* a portuale trace (``PORTUALE_BT_TRACE=1`` stderr): ``BT try=N kind=K`` headers
  with ``BT   mask``, ``BT   conflict``, ``BT   pullers`` and
  ``BT   missing_dep_trigger`` lines.

Portage snapshots are numbered ``P1``, ``P2``, ... in file order; portuale
passes keep their ``BT try=`` number. Stdlib only.

Usage: bt_compare.py [--portage FILE] [--portuale FILE]
"""

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DEFAULT_PORTAGE = (
    REPO / "LLM" / "evidence" / "2026-10-22-world-divergence" / "portage-debug-masks.txt"
)

# `<Package ('binary', '/', 'cat/pkg-ver', 'merge', ...)>: {'reason': ...}`
# The reason is read separately: captures can be cut mid-line (the 2026-10-22
# evidence is), so a complete `<Package (...)>: {...}` cannot be required.
PKG_RE = re.compile(r"<Package \('(\w+)', '[^']*', '([^']+)'")
REASON_RE = re.compile(r">:\s*\{([^}]*)\}?")
# `- cat/pkg-ver::repo (masked by: ...)`
# Possibly prefixed by a path and colon (`pd.err:- cat/pkg::repo (masked by: ...)`).
MASKED_BY_RE = re.compile(r"(?:^|:)\s*-\s+(\S+)::(\S+)\s+\(masked by: (.*)\)\s*$")
# `BT try=3 kind=Feedback:MissingDep`
BT_TRY_RE = re.compile(r"^BT try=(\d+) kind=(\S+)")
# `BT   mask cat/pkg neg=!=cat/pkg-1.0 reason=SlotConflict { ... }`
BT_MASK_RE = re.compile(r"^BT   mask (\S+) neg=(\S+) reason=(.*)$")
# `BT   conflict cat/pkg parents=[...]`
BT_CONFLICT_RE = re.compile(r"^BT   conflict (\S+) parents=(.*)$")
# `BT   pullers cat/pkg -> [...]`
BT_PULLERS_RE = re.compile(r"^BT   pullers (\S+) -> (.*)$")
# `BT   missing_dep_trigger owner=cat/pkg neg=... atom=...`
BT_TRIGGER_RE = re.compile(r"^BT   missing_dep_trigger owner=(\S+) neg=(.*)$")


def portage_rows(path):
    """Rows for the Portage side: one per runtime mask entry, one per message."""
    rows = []
    snapshot = 0
    text = path.read_text(encoding="utf-8", errors="replace").splitlines()
    for line in text:
        if "runtime_pkg_mask:" in line:
            snapshot += 1
            for chunk in line.split("<Package ")[1:]:
                m = PKG_RE.match("<Package " + chunk)
                if not m:
                    continue
                kind_type, cpv = m.groups()
                r = REASON_RE.search(chunk)
                reason = r.group(1).strip() if r else "(cut off)"
                rows.append(
                    ("portage", f"P{snapshot}", "runtime_pkg_mask", f"{kind_type} {cpv}", reason)
                )
            continue
        m = MASKED_BY_RE.search(line)
        if m:
            cpv, repo, why = m.groups()
            rows.append(("portage", "-", "masked by", f"ebuild {cpv}::{repo}", why))
    return rows


def portuale_rows(path):
    """Rows for the portuale side, from `BT` trace lines (empty if no file)."""
    rows = []
    if path is None or not path.exists():
        return rows
    try_no, kind = "-", "-"
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        m = BT_TRY_RE.match(line)
        if m:
            try_no, kind = m.group(1), m.group(2)
            continue
        m = BT_MASK_RE.match(line)
        if m:
            rows.append(("portuale", try_no, kind, f"mask {m.group(1)}", m.group(2) + " " + m.group(3)))
            continue
        m = BT_CONFLICT_RE.match(line)
        if m:
            rows.append(("portuale", try_no, kind, f"conflict {m.group(1)}", m.group(2)))
            continue
        m = BT_PULLERS_RE.match(line)
        if m:
            rows.append(("portuale", try_no, kind, f"pullers {m.group(1)}", m.group(2)))
            continue
        m = BT_TRIGGER_RE.match(line)
        if m:
            rows.append(("portuale", try_no, kind, f"missing_dep_trigger {m.group(1)}", m.group(2)))
    return rows


def print_table(rows):
    header = ("side", "try", "kind", "object (type cpv)", "reason / detail")
    widths = [len(h) for h in header]
    for row in rows:
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(cell))
    fmt = " | ".join("{:<" + str(w) + "}" for w in widths)
    print(fmt.format(*header))
    print("-+-".join("-" * w for w in widths))
    for row in rows:
        print(fmt.format(*row))


def main(argv=None):
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--portage", type=Path, default=DEFAULT_PORTAGE,
                        help="Portage debug capture (default: the 2026-10-22 evidence file)")
    parser.add_argument("--portuale", type=Path, default=None,
                        help="portuale stderr with BT lines (optional)")
    args = parser.parse_args(argv)
    if not args.portage.exists():
        print(f"no such file: {args.portage}", file=sys.stderr)
        return 2
    rows = portage_rows(args.portage) + portuale_rows(args.portuale)
    if not rows:
        print("no mask, conflict or BT lines found", file=sys.stderr)
        return 1
    print_table(rows)
    return 0


if __name__ == "__main__":
    sys.exit(main())
