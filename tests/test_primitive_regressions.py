"""Replay the inputs on which the tree-wide primitive differential
(`scripts/primitive_tree_differential.py`,
`docs/second_python_copy_removal.md` §3) once found Rust and real
Portage disagreeing. Each `tests/primitive_regressions/<harness>.txt`
line is one batch operation; both harnesses must now answer it the same.
"""

from pathlib import Path

import pytest

REGRESSIONS = Path(__file__).resolve().parent / "primitive_regressions"
FILES = sorted(REGRESSIONS.glob("*.txt")) if REGRESSIONS.is_dir() else []


def _batch(cmd, lines):
    import subprocess

    p = subprocess.run([*cmd, "batch"], input="".join(f"{l}\n" for l in lines),
                       capture_output=True, text=True, check=True)
    return p.stdout.splitlines()


@pytest.mark.skipif(not FILES, reason="no recorded primitive regressions")
@pytest.mark.parametrize("path", FILES, ids=[f.stem for f in FILES])
def test_primitive_regressions_agree_with_real_portage(path, request):
    harness = path.stem  # atom | use_reduce | required_use
    rust = request.getfixturevalue(f"{harness}_harness_rust")
    python = request.getfixturevalue(f"{harness}_harness_python")
    lines = [l for l in path.read_text().splitlines() if l.strip()]
    assert _batch([str(rust)], lines) == _batch(python, lines)
