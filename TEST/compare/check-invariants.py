#!/usr/bin/env python3
"""Expectation-free output invariants over an L0 run
(`docs/second_python_copy_removal.md` §1/§2).

Reads `<run>/portuale/<slug>.txt` (the `-pv` probe) and, when the run
recorded them, `<run>/portuale-modes/<slug>.{json,tree,quiet}.txt` (+ the `--json` run's
stderr with the unparsed-dependency-token count, §4), and
applies the same checks the contract suite applies to fixtures
(`tests/output_invariants.py`). The container's vdb is not visible from
the host, so "owner is installed" is not checked here.

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


def main(run: Path) -> int:
    violations = 0
    control = 0
    checked = 0
    for plain_path in sorted((run / "portuale").glob("*.txt")):
        slug = plain_path.stem
        plain = plain_path.read_text(errors="replace")
        problems = inv.check_plain_use(plain) + inv.check_summary(plain)
        modes = run / "portuale-modes"
        json_path = modes / f"{slug}.json.txt"
        if json_path.exists():
            doc = inv.parse_json_stdout(json_path.read_text(errors="replace"))
            if doc is not None:
                problems += inv.check_json(doc)
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
