#!/usr/bin/env python3
"""One-shot harvest of the review-flag corpus (`tests/corpus.py`) while
`python/emerge_pretend_reference.py` still exists
(`docs/second_python_copy_removal.md` §11).

Two inputs:

* `contract` -- the JSONL call logs written by the contract suite run
  with `PORTUALE_HARVEST=<dir>`; each Python call is paired with the Rust
  call of the same test, args and env, and agreeing pairs are stored
  under `<nodeid>#<rust ordinal>`.
* `expanded` -- every fixture package (and the fixture sets) x a grid of
  option combinations, run through both implementations here.

Usage:
  PORTUALE_HARVEST=/tmp/h python3 -m pytest tests/test_emerge_pretend_contract.py
  scripts/harvest_corpus.py contract /tmp/h
  scripts/harvest_corpus.py expanded
"""

from __future__ import annotations

import collections
import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "tests"))
import corpus  # noqa: E402

FIXTURES = REPO / "fixtures"
RUST = REPO / "rust" / "target" / "release" / "portuale"
PYTHON_REF = REPO / "python" / "emerge_pretend_reference.py"

GRID = [
    ["--pretend"],
    ["--pretend", "--verbose"],
    ["--pretend", "--quiet"],
    ["--pretend", "--tree"],
    ["--pretend", "--json"],
    ["--pretend", "--update"],
    ["--pretend", "--update", "--deep"],
    ["--pretend", "--update", "--deep", "--newuse"],
    ["--pretend", "--emptytree"],
    ["--pretend", "--nodeps"],
]

SETS = ["@world", "@system", "@selected", "@dualslotset", "@nestedtestset",
        "@innernestedset"]


def fixture_env() -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if k not in _config_vars()}
    env.update(
        PORTAGE_CONFIGROOT=str(FIXTURES),
        ROOT=str(FIXTURES),
        PORTAGE_RUNNING_ROOT=str(FIXTURES),
        DISTDIR=str(FIXTURES / "distfiles"),
        CLEAN_DELAY="0",
    )
    return env


def _config_vars() -> set[str]:
    import conftest  # the suite's own list of stripped config vars

    return set(conftest._ENV_CONFIG_VARS)


def fixture_atoms() -> list[str]:
    atoms = set()
    for repo in ("repo", "overlay", "independentoverlay", "layoutmasteroverlay",
                 "repnamerepo"):
        root = FIXTURES / repo
        for ebuild in root.glob("*/*/*.ebuild"):
            atoms.add(f"{ebuild.parent.parent.name}/{ebuild.parent.name}")
    return sorted(atoms)


def run(cmd: list[str], env: dict[str, str]) -> dict:
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=env,
                           timeout=300, check=False)
    except subprocess.TimeoutExpired:
        return {"rc": "timeout", "stdout": "", "stderr": ""}
    return {"rc": p.returncode, "stdout": p.stdout, "stderr": p.stderr}


def harvest_expanded() -> None:
    env = fixture_env()
    cases = [(opts, atom) for atom in fixture_atoms() + SETS for opts in GRID]

    def one(case):
        opts, atom = case
        args = [*opts, atom]
        rust = run([str(RUST), "emerge", *args], env)
        py = run([sys.executable, str(PYTHON_REF), *args], env)
        return args, rust, py

    agreed: dict[str, dict] = {}
    disagreed: list[str] = []
    with ThreadPoolExecutor(max_workers=max(1, (os.cpu_count() or 2) - 2)) as pool:
        for i, (args, rust, py) in enumerate(pool.map(one, cases)):
            key = " ".join(args)
            if rust["rc"] == "timeout" or py["rc"] == "timeout":
                disagreed.append(f"{key}  (timeout)")
            elif rust == py:
                agreed[key] = {**corpus.case_identity(args, env),
                               **corpus.result_record(rust["rc"], rust["stdout"],
                                                      rust["stderr"])}
            else:
                which = [s for s in ("rc", "stdout", "stderr") if rust[s] != py[s]]
                disagreed.append(f"{key}  ({', '.join(which)})")
            if i % 500 == 0:
                print(f"{i}/{len(cases)}", file=sys.stderr)
    corpus.save(corpus.EXPANDED_CORPUS, agreed)
    report = corpus.CORPUS_DIR / "expanded-disagreements.txt"
    report.write_text(
        "# Grid cases where Rust and the Python reference disagreed at harvest\n"
        "# time; not stored in expanded.json.xz. See tests/corpus.py.\n"
        "# Every row names a cache-less fixture (an ebuild with no md5-cache entry,\n"
        "# used by the real-execution tests): Rust runs the depend phase for it\n"
        "# (#41 C2), the Python reference never did (owner decision D2), so either\n"
        "# the package resolves only in Rust or the \"similar names\" list differs.\n"
        + "".join(f"{line}\n" for line in sorted(disagreed))
    )
    print(f"expanded: {len(agreed)} agreed, {len(disagreed)} disagreed", file=sys.stderr)


def harvest_contract(log_dir: Path) -> None:
    calls = collections.defaultdict(lambda: {"rust": [], "python": []})
    for log in sorted(log_dir.glob("calls-*.jsonl")):
        for line in log.read_text(encoding="utf-8").splitlines():
            c = json.loads(line)
            calls[c["nodeid"]][c["impl"]].append(c)

    agreed: dict[str, dict] = {}
    unpaired = disagreed = 0
    for nodeid, by_impl in calls.items():
        rust_calls = sorted(by_impl["rust"], key=lambda c: c["ordinal"])
        used: set[int] = set()
        for py in sorted(by_impl["python"], key=lambda c: c["ordinal"]):
            match = next(
                (r for r in rust_calls
                 if r["ordinal"] not in used and r["args"] == py["args"]
                 and r["env"] == py["env"]),
                None,
            )
            if match is None:
                unpaired += 1
                continue
            used.add(match["ordinal"])
            fields = ("rc", "stdout", "stderr")
            if all(match[f] == py[f] for f in fields):
                agreed[corpus.contract_key(nodeid, match["ordinal"])] = {
                    "args": match["args"], "env": match["env"],
                    **{f: match[f] for f in fields},
                }
            else:
                disagreed += 1
    corpus.save(corpus.CONTRACT_CORPUS, agreed)
    print(f"contract: {len(agreed)} agreed, {disagreed} disagreed, "
          f"{unpaired} python calls without a matching rust call", file=sys.stderr)


if __name__ == "__main__":
    if sys.argv[1:2] == ["expanded"]:
        harvest_expanded()
    elif sys.argv[1:2] == ["contract"] and len(sys.argv) == 3:
        harvest_contract(Path(sys.argv[2]))
    else:
        sys.exit(__doc__)
