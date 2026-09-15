#!/usr/bin/env python3
"""Expectation-free output invariants over an L0 run
(`docs/second_python_copy_removal.md` §1/§2).

Reads `<run>/portuale/<slug>.txt` (the `-pv` probe) and, when the run
recorded them, `<run>/portuale-modes/<slug>.{json,tree,quiet}.txt` (+ the `--json` run's
stderr with the unparsed-dependency-token count, §4), and
applies the same checks the contract suite applies to fixtures
(`tests/output_invariants.py`).

The container's vdb and repos are not visible from the host, so the
run snapshots what the checker needs: `vdb-list.txt` ("is this cp
installed", the merge-order exemption) and `dep-classes.tsv` (which
dependency variables of each owner named each child, so the soft-edge
exemption reflects real's `ignore_priority` ladder). Without the
snapshots the corresponding checks degrade to the strict behaviour.

The plain-text checks (`USE` repeats, `Total:` counters) also run over
`<run>/real/` as a control: a violation there is a checker bug, not a
portuale bug.

Usage: check-invariants.py TEST/logs/l0-<stamp>
Exit status 1 when portuale output violates an invariant.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tests"))
import output_invariants as inv  # noqa: E402


def read_dep_classes(run: Path) -> dict:
    """`dep-classes.tsv` -> `{(child_cp, owner_cp): {dep vars}}`."""
    dep_vars: dict = {}
    path = run / "dep-classes.tsv"
    if not path.exists():
        return dep_vars
    for line in path.read_text(errors="replace").splitlines():
        parts = line.split("\t")
        if len(parts) != 3 or "/" not in parts[0] or "/" not in parts[1]:
            continue
        child = tuple(parts[0].split("/", 1))
        owner = tuple(parts[1].split("/", 1))
        dep_vars[(child, owner)] = {v for v in parts[2].split(",") if v}
    return dep_vars


def main(run: Path) -> int:
    violations = 0
    control = 0
    checked = 0
    installed = None
    vdb = run / "vdb-list.txt"
    if vdb.exists():
        installed = inv.parse_vdb_list(vdb.read_text(errors="replace"))
    dep_vars = read_dep_classes(run)
    for plain_path in sorted((run / "portuale").glob("*.txt")):
        slug = plain_path.stem
        plain = plain_path.read_text(errors="replace")
        problems = inv.check_plain_use(plain) + inv.check_summary(plain)
        modes = run / "portuale-modes"
        json_path = modes / f"{slug}.json.txt"
        if json_path.exists():
            doc = inv.parse_json_stdout(json_path.read_text(errors="replace"))
            if doc is not None:
                problems += inv.check_json(doc, installed=installed, dep_vars=dep_vars)
                stderr = modes / f"{slug}.json.stderr"
                if stderr.exists():
                    problems += inv.check_unparsed_dep_tokens(stderr.read_text(errors="replace"))
                tree = (modes / f"{slug}.tree.txt").read_text(errors="replace")
                quiet = (modes / f"{slug}.quiet.txt").read_text(errors="replace")
                problems += inv.check_cross_mode(plain, tree, quiet, doc)
        checked += 1
        for p in problems:
            print(f"portuale {slug}: {p}")
        violations += len(problems)

        real = run / "real" / plain_path.name
        if real.exists():
            text = real.read_text(errors="replace")
            for p in inv.check_plain_use(text) + inv.check_summary(text):
                print(f"real (checker control) {slug}: {p}")
                control += 1

    print(f"invariants: {checked} probes, {violations} portuale violation(s), "
          f"{control} control violation(s) on real output")
    return 1 if violations else 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    sys.exit(main(Path(sys.argv[1])))
