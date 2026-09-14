"""Replay the fixture-atom x option grid harvested before the Python
reference was removed (`docs/second_python_copy_removal.md` §11,
`tests/corpus.py`). A changed Rust output is flagged for review
(`CorpusDrift` warning + session summary), not failed, unless
`PORTUALE_CORPUS_STRICT=1`.

The per-test contract corpus is checked inline by the contract suite's
own `_run`; this file covers the grid, which no test pins otherwise.
"""

from __future__ import annotations

import os
import subprocess
from concurrent.futures import ThreadPoolExecutor

import corpus
from conftest import _ENV_CONFIG_VARS


def test_expanded_corpus_replays(emerge_binary):
    cases = corpus.load(corpus.EXPANDED_CORPUS)
    assert cases, "tests/corpus/expanded.json.xz is missing or empty"
    base_env = {k: v for k, v in os.environ.items() if k not in _ENV_CONFIG_VARS}

    def one(item):
        key, stored = item
        env = dict(base_env)
        env.update({k: corpus.denormalize(v) for k, v in stored["env"].items()})
        args = [corpus.denormalize(a) for a in stored["args"]]
        result = subprocess.run([str(emerge_binary), *args], capture_output=True,
                                text=True, env=env, check=False)
        return key, stored, args, env, result

    workers = max(2, min(16, os.cpu_count() or 2))
    with ThreadPoolExecutor(workers) as pool:
        results = list(pool.map(one, sorted(cases.items())))
    for key, stored, args, env, result in results:
        message = corpus.compare(f"expanded: {key}", stored, args, env, result)
        if message:
            corpus.flag(key, message, stored, args, env, result, corpus.blessed_expanded)
