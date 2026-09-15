"""Review-flag corpus of `emerge` outputs harvested while the Python
reference (`python/emerge_pretend_reference.py`) still existed.

`docs/second_python_copy_removal.md` §11: before the reference was
deleted (commit `2738e9c` holds the harvest tooling), every
contract-suite invocation and an expanded grid of fixture atoms x option
combinations was run through *both* implementations, and the outputs
they agreed on were stored here. The Python copy is gone; what remains
is its last known agreement with Rust.

A Rust output that no longer matches its stored entry is **flagged for
review, not auto-failed**: a `CorpusDrift` warning plus a line in the
session summary. Set `PORTUALE_CORPUS_STRICT=1` to turn drift into test
failures, or `PORTUALE_CORPUS_BLESS=1` to accept the current Rust output
as the new stored value (review the `git diff` of the corpus file).

Entries are normalised so they survive a different checkout path or
pytest base temp: the fixture root becomes `<FIXTURES>`, the pytest base
temp becomes `<TMP>`, the repo root becomes `<REPO>`.
"""

from __future__ import annotations

import contextvars
import hashlib
import json
import lzma
import os
import threading
import warnings
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
FIXTURES_ROOT = REPO_ROOT / "fixtures"
CORPUS_DIR = Path(__file__).resolve().parent / "corpus"
CONTRACT_CORPUS = CORPUS_DIR / "contract.json.xz"
EXPANDED_CORPUS = CORPUS_DIR / "expanded.json.xz"

STRICT = os.environ.get("PORTUALE_CORPUS_STRICT") == "1"
BLESS = os.environ.get("PORTUALE_CORPUS_BLESS") == "1"

current_nodeid: contextvars.ContextVar[str | None] = contextvars.ContextVar(
    "current_nodeid", default=None
)
_basetemp: list[str] = []
_lock = threading.Lock()


class CorpusDrift(UserWarning):
    """A Rust output differs from its harvested Rust==Python agreement."""


def set_basetemp(path: str) -> None:
    _basetemp[:] = [str(path)]


def denormalize(text: str) -> str:
    text = text.replace("<FIXTURES>", str(FIXTURES_ROOT)).replace("<REPO>", str(REPO_ROOT))
    return text.replace("<TMP>", _basetemp[0]) if _basetemp else text


def trim_padding(text: str) -> str:
    """Drop per-line trailing whitespace. Conflict-block caret markers pad
    to a width that encodes the checkout path length (`installed in
    '<root>'` lines), so an entry harvested at a different path length can
    never match byte-for-byte; trailing whitespace carries no semantics."""
    return "\n".join(line.rstrip() for line in text.split("\n"))


def normalize(text: str) -> str:
    # Longest prefixes first: the fixtures root lives under the repo root.
    for base in _basetemp:
        text = text.replace(base, "<TMP>")
    text = text.replace(str(FIXTURES_ROOT), "<FIXTURES>")
    text = text.replace(str(REPO_ROOT), "<REPO>")
    return trim_padding(text)


# The host environment at import time; a case's identity is only what the
# test changed on top of it (fixture roots, overrides, CLEAN_DELAY, ...).
_HOST_ENV = dict(os.environ)
_IGNORED_ENV = {"PYTEST_CURRENT_TEST", "PORTUALE_CORPUS_STRICT",
                "PORTUALE_CORPUS_BLESS"}


def env_key(env: dict[str, str]) -> dict[str, str]:
    """The case-identifying part of an environment, normalised."""
    return {
        k: normalize(v)
        for k, v in sorted(env.items())
        if k not in _IGNORED_ENV and _HOST_ENV.get(k) != v
    }


def case_identity(args: list[str], env: dict[str, str]) -> dict:
    return {"args": [normalize(a) for a in args], "env": env_key(env)}


def result_record(rc: int, stdout: str, stderr: str) -> dict:
    return {"rc": rc, "stdout": normalize(stdout), "stderr": normalize(stderr)}


# --------------------------------------------------------------------------
# Storage: {"version": 1, "blobs": {sha: text}, "cases": {key: case}}
# A case is {"args", "env", "rc", "stdout": sha, "stderr": sha}.


def load(path: Path) -> dict[str, dict]:
    if not path.exists():
        return {}
    with lzma.open(path, "rt", encoding="utf-8") as fh:
        data = json.load(fh)
    blobs = data["blobs"]
    cases = {}
    for key, case in data["cases"].items():
        case = dict(case)
        case["stdout"] = blobs[case["stdout"]]
        case["stderr"] = blobs[case["stderr"]]
        cases[key] = case
    return cases


def save(path: Path, cases: dict[str, dict]) -> None:
    blobs: dict[str, str] = {}
    packed = {}
    for key in sorted(cases):
        case = dict(cases[key])
        for stream in ("stdout", "stderr"):
            text = case[stream]
            sha = hashlib.sha256(text.encode()).hexdigest()[:20]
            blobs[sha] = text
            case[stream] = sha
        packed[key] = case
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(
        {"version": 1, "blobs": dict(sorted(blobs.items())), "cases": packed},
        indent=0, sort_keys=True, ensure_ascii=False,
    )
    with lzma.open(path, "wt", encoding="utf-8", preset=9) as fh:
        fh.write(payload)


# --------------------------------------------------------------------------
# Drift checking (after deletion).

_contract_cases: dict[str, dict] | None = None
_rust_ordinals: dict[str, int] = {}
drifted: list[str] = []
blessed: dict[str, dict] = {}
blessed_expanded: dict[str, dict] = {}


def contract_key(nodeid: str, ordinal: int) -> str:
    return f"{nodeid}#{ordinal}"


def _contract() -> dict[str, dict]:
    global _contract_cases
    if _contract_cases is None:
        _contract_cases = load(CONTRACT_CORPUS)
    return _contract_cases


_nodeid_index: dict[str, list[tuple[str, dict]]] | None = None


def _by_nodeid() -> dict[str, list[tuple[str, dict]]]:
    global _nodeid_index
    if _nodeid_index is None:
        _nodeid_index = {}
        for k, c in sorted(_contract().items()):
            _nodeid_index.setdefault(k.rsplit("#", 1)[0], []).append((k, c))
    return _nodeid_index


def compare(key: str, stored: dict, args, env, result) -> str | None:
    """Return a drift description, or None when the output still matches.
    A case whose identity (args/env) changed is stale, not drifted."""
    ident = case_identity(list(args), env)
    if ident["args"] != stored["args"] or ident["env"] != stored["env"]:
        return None
    now = result_record(result.returncode, result.stdout, result.stderr)
    if now["rc"] != stored["rc"]:
        return f"{key}: rc changed"
    changed = [
        s for s in ("stdout", "stderr")
        if trim_padding(now[s]) != trim_padding(stored[s])
    ]
    if not changed:
        return None
    return f"{key}: {', '.join(changed)} changed"


def flag(key: str, message: str, stored: dict, args, env, result, sink: dict) -> None:
    with _lock:
        drifted.append(message)
        if BLESS:
            sink[key] = {**stored, **case_identity(list(args), env),
                         **result_record(result.returncode, result.stdout, result.stderr)}
    if STRICT and not BLESS:
        raise AssertionError(f"corpus drift (PORTUALE_CORPUS_STRICT=1): {message}")
    warnings.warn(message, CorpusDrift, stacklevel=3)


def check_contract_call(args, env, result) -> tuple[str | None, str | None]:
    """Called for every Rust `_run` in the contract suite. Returns the
    corpus key (None when this call has no harvested entry, or its args or
    env changed since) and the drift message (None when it still matches)."""
    nodeid = current_nodeid.get()
    if nodeid is None:
        return None, None
    with _lock:
        n = _rust_ordinals.get(nodeid, 0)
        _rust_ordinals[nodeid] = n + 1
    key = contract_key(nodeid, n)
    stored = _contract().get(key)
    ident = case_identity(list(args), env)
    if stored is None or ident["args"] != stored["args"] or ident["env"] != stored["env"]:
        # The harvest paired calls by args+env, so a test that runs the same
        # command twice may have its entry under the other ordinal.
        key, stored = next(
            ((k, c) for k, c in _by_nodeid().get(nodeid, [])
             if c["args"] == ident["args"] and c["env"] == ident["env"]),
            (None, None),
        )
        if stored is None:
            return None, None
    message = compare(key, stored, args, env, result)
    if message:
        flag(key, message, stored, args, env, result, blessed)
    return key, message


def write_blessed() -> None:
    if not BLESS:
        return
    if blessed:
        cases = dict(_contract())
        cases.update(blessed)
        save(CONTRACT_CORPUS, cases)
    if blessed_expanded:
        cases = load(EXPANDED_CORPUS)
        cases.update(blessed_expanded)
        save(EXPANDED_CORPUS, cases)
