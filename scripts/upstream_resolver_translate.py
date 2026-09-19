#!/usr/bin/env python3
"""Capture upstream resolver tests as data for portuale fixture translation
(backlog #50 / `docs/history/second_python_copy_removal.md` §7).

Loads an upstream `lib/portage/tests/resolver/test_*.py` module, runs its
real `ResolverPlayground` against the vendored portage (pinned in
`3rdparty/repos.toml`), and records, per resolution:

* the playground's inputs (`ebuilds`, `installed`, `world`,
  `user_config`, `binpkgs`, `repo_configs`, `sets`, `profile`);
* the request atoms / options / action;
* the **actual** result (success, mergelist, use_changes, unstable
  keywords, slot-collision solutions, ...) -- the genuine oracle, not
  the `mergelist=[...]` literal in the test source. Some files rebind
  `test_cases` before consuming it, so the source literal is not always
  the executed oracle (`docs/history/023-oracle.md`).

The output is JSON; `--emit-fixtures DIR` additionally writes a
`fixtures/`-shaped repo + config + vdb + world per playground (ebuilds
and md5-cache from `ebuilds{}`, installed entries from `installed{}`)
plus a `cases.json` holding the oracle pins. Fixture emission is
best-effort for the simple shapes; a file whose shapes need hand
fixups is reported, not silently mis-emitted.

Usage:
    scripts/upstream_resolver_translate.py --all --stats
    scripts/upstream_resolver_translate.py test_disjunctive_depend_order
    scripts/upstream_resolver_translate.py --file 3rdparty/.../test_x.py --json /tmp/x.json

Determinism: `PYTHONHASHSEED=0` and `CLEAN_DELAY=0` are pinned for the
run (the same pins `TEST/layers/l0/in-container.sh` uses).
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
PORTAGE_LIB = REPO_ROOT / "3rdparty" / "portage" / "lib"
RESOLVER_TESTS = PORTAGE_LIB / "portage" / "tests" / "resolver"

INPUT_KEYS = (
    "ebuilds", "installed", "world", "user_config", "binpkgs",
    "repo_configs", "sets", "profile", "debug", "eroot", "targetroot",
)

_RESULT_LISTS = (
    "mergelist", "cleanlist", "graph_order", "unstable_keywords",
    "needed_p_mask_changes", "unsatisfied_deps", "forced_rebuilds",
    "required_use_unsatisfied", "circular_dependency_solutions",
)


def _bootstrap() -> None:
    sys.path.insert(0, str(PORTAGE_LIB))
    os.environ["PYTHONHASHSEED"] = "0"
    os.environ["CLEAN_DELAY"] = "0"
    import portage

    portage._internal_caller = True
    portage._disable_legacy_globals()
    gpg = tempfile.mkdtemp(prefix="portuale-upstream-gpg-")
    shutil.copytree(PORTAGE_LIB / "portage" / "tests" / ".gnupg", gpg, dirs_exist_ok=True)
    os.chmod(gpg, 0o700)
    os.environ["PORTAGE_GNUPGHOME"] = gpg
    os.environ["PATH"] = portage.const.PORTAGE_BIN_PATH + ":" + os.environ.get("PATH", "")  # type: ignore[attr-defined]


def _jsonable(value):
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, dict):
        return {str(k): _jsonable(v) for k, v in sorted(value.items(), key=lambda kv: str(kv[0]))}
    if isinstance(value, (list, tuple, set, frozenset)):
        return [_jsonable(v) for v in sorted(value, key=str)]
    return str(value)


def _result_record(result) -> dict:
    record: dict = {"success": bool(getattr(result, "success", True))}
    for field in _RESULT_LISTS:
        value = getattr(result, field, None)
        if value:
            record[field] = [_jsonable(x) for x in value]
    changes = getattr(result, "use_changes", None)
    if changes:
        record["use_changes"] = {
            str(cpv): sorted(str(flag) for flag in flags)
            for cpv, flags in sorted(changes.items(), key=lambda kv: str(kv[0]))
        }
    slots = getattr(result, "slot_collision_solutions", None)
    if slots:
        record["slot_collision_solutions"] = _jsonable(slots)
    if hasattr(result, "req_pkg_count"):
        record["req_pkg_count"] = result.req_pkg_count
    return record


class Capture:
    """Wrap `ResolverPlayground` so both constructor inputs and every
    `run()` result are recorded. Tests that call `run()` directly (not
    through `run_TestCase`) are captured too."""

    def __init__(self) -> None:
        self.playgrounds: list[dict] = []
        self.cases: list[dict] = []
        self._current: dict | None = None

    def begin_playground(self, kwargs: dict) -> None:
        self._current = {key: _jsonable(kwargs.get(key)) for key in INPUT_KEYS}
        self._current["_index"] = len(self.playgrounds)
        self.playgrounds.append(self._current)

    def record(self, args: tuple, kwargs: dict, result) -> None:
        atoms = args[0] if args else kwargs.get("atoms")
        options = args[1] if len(args) > 1 else kwargs.get("options")
        action = args[2] if len(args) > 2 else kwargs.get("action")
        self.cases.append({
            "playground": len(self.playgrounds) - 1,
            "request": list(atoms or []),
            "options": _jsonable(options or {}),
            "action": action,
            "result": _result_record(result),
        })

    def install(self) -> None:
        from portage.tests.resolver import ResolverPlayground as RP

        capture = self
        orig_init = RP.ResolverPlayground.__init__
        orig_run = RP.ResolverPlayground.run

        def init(inner_self, **kwargs):
            capture.begin_playground(kwargs)
            orig_init(inner_self, **kwargs)

        def run(inner_self, *args, **kwargs):
            result = orig_run(inner_self, *args, **kwargs)
            capture.record(args, kwargs, result)
            return result

        setattr(RP.ResolverPlayground, "__init__", init)
        setattr(RP.ResolverPlayground, "run", run)


def module_name_for(path: Path) -> str:
    return f"portage.tests.resolver.{path.stem}"


def capture_module(module: str) -> dict:
    capture = Capture()
    capture.install()
    started = time.time()
    status: dict = {"module": module, "cases": 0, "playgrounds": 0, "errors": [], "failures": 0}
    try:
        mod = importlib.import_module(module)
    except Exception as exc:  # noqa: BLE001 -- report, don't crash the sweep
        status["import_error"] = f"{type(exc).__name__}: {exc}"
        status["captured"] = capture
        return status
    suite = unittest.TestLoader().loadTestsFromModule(mod)
    stream = open(os.devnull, "w")
    try:
        result = unittest.TextTestRunner(verbosity=0, stream=stream).run(suite)
    finally:
        stream.close()
    status["tests"] = result.testsRun
    status["failures"] = len(result.failures)
    status["skipped"] = len(result.skipped)
    status["errors"] = [f"{case.id()}: {tb.strip().splitlines()[-1][:200]}"
                        for case, tb in result.errors]
    status["playgrounds"] = len(capture.playgrounds)
    status["cases"] = len(capture.cases)
    status["seconds"] = round(time.time() - started, 1)
    status["captured"] = capture
    return status


# -- fixture emission ----------------------------------------------------


def _ebuild_text(cpv: str, meta: dict) -> str:
    lines = [f'EAPI={meta.get("EAPI", "8")}']
    if "DESCRIPTION" in meta:
        lines.append(f'DESCRIPTION="{meta["DESCRIPTION"]}"')
    lines.append(f'SLOT="{meta.get("SLOT", "0")}"')
    if meta.get("KEYWORDS") is not None:
        lines.append(f'KEYWORDS="{meta["KEYWORDS"]}"')
    if "IUSE" in meta:
        lines.append(f'IUSE="{meta["IUSE"]}"')
    if "REQUIRED_USE" in meta:
        lines.append(f'REQUIRED_USE="{meta["REQUIRED_USE"]}"')
    for var in ("DEPEND", "RDEPEND", "BDEPEND", "IDEPEND", "PDEPEND",
                "PROPERTIES", "RESTRICT", "LICENSE"):
        if var in meta:
            lines.append(f'{var}="{meta[var]}"')
    return "\n".join(lines) + "\n"


def _cache_text(meta: dict, ebuild_text: str) -> str:
    keys = ("EAPI", "DEFINED_PHASES", "DESCRIPTION", "IUSE", "KEYWORDS",
            "SLOT", "DEPEND", "RDEPEND", "BDEPEND", "IDEPEND", "PDEPEND",
            "PROPERTIES", "RESTRICT", "LICENSE")
    lines = []
    for key in keys:
        if key in meta:
            lines.append(f"{key}={meta[key]}")
    if "DEFINED_PHASES" not in meta:
        lines.append("DEFINED_PHASES=-")
    # The real md5 of the emitted ebuild, not a placeholder: the committed
    # fixture guard (`test_committed_fixture_md5_cache_entries_match_their_
    # ebuilds`) and the reader validation (#46 S3) both check it.
    lines.append(f"_md5_={hashlib.md5(ebuild_text.encode()).hexdigest()}")
    return "\n".join(lines) + "\n"


def emit_fixtures(capture: Capture, out: Path) -> dict:
    """Best-effort `fixtures/`-shaped tree from a capture's playgrounds.
    Returns a report of what was emitted / skipped."""
    report: dict = {"playgrounds": [], "cases": capture.cases}
    out.mkdir(parents=True, exist_ok=True)
    for index, playground in enumerate(capture.playgrounds):
        pg_dir = out / f"pg{index}"
        notes: list[str] = []
        ebuilds = playground.get("ebuilds") or {}
        for cpv, meta in sorted(ebuilds.items()):
            if not isinstance(meta, dict):
                notes.append(f"ebuild {cpv}: non-dict meta, skipped")
                continue
            cat, _, pkgver = cpv.partition("/")
            pkg, _, version = pkgver.rpartition("-")
            pkgdir = pg_dir / "repo" / cat / pkg
            pkgdir.mkdir(parents=True, exist_ok=True)
            ebuild_text = _ebuild_text(cpv, meta)
            (pkgdir / f"{pkgver}.ebuild").write_text(ebuild_text)
            cache_dir = pg_dir / "repo" / "metadata" / "md5-cache" / cat
            cache_dir.mkdir(parents=True, exist_ok=True)
            (cache_dir / pkgver).write_text(_cache_text(meta, ebuild_text))
        installed = playground.get("installed") or {}
        for cpv, meta in sorted(installed.items()):
            meta = meta if isinstance(meta, dict) else {}
            cat, _, pkgver = cpv.partition("/")
            vdb = pg_dir / "var" / "db" / "pkg" / cat / pkgver
            vdb.mkdir(parents=True, exist_ok=True)
            (vdb / "SLOT").write_text(meta.get("SLOT", "0") + "\n")
            for var in ("EAPI", "IUSE", "KEYWORDS", "DEPEND", "RDEPEND",
                        "BDEPEND", "IDEPEND", "PDEPEND", "PROPERTIES",
                        "RESTRICT", "LICENSE", "repository"):
                if var in meta:
                    (vdb / var).write_text(str(meta[var]) + "\n")
        world = playground.get("world") or []
        if world:
            world_file = pg_dir / "var" / "lib" / "portage" / "world"
            world_file.parent.mkdir(parents=True, exist_ok=True)
            world_file.write_text("".join(f"{atom}\n" for atom in world))
        for key in ("user_config", "repo_configs", "sets", "profile", "binpkgs"):
            if playground.get(key):
                notes.append(f"{key}: not emitted (needs hand translation)")
        report["playgrounds"].append({
            "index": index,
            "ebuilds": len(ebuilds),
            "installed": len(installed),
            "world": len(world),
            "notes": notes,
        })
    (out / "cases.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    return report


def run_all(stats: bool) -> dict:
    files = sorted(RESOLVER_TESTS.glob("test_*.py"))
    summary: dict = {"files": len(files), "captured": 0, "with_cases": 0,
                     "errors": 0, "import_errors": 0, "cases": 0, "crashes": 0,
                     "per_file": {}}
    with tempfile.TemporaryDirectory(prefix="portuale-upstream-") as tmp:
        for path in files:
            out = Path(tmp) / f"{path.stem}.json"
            proc = subprocess.run(
                [sys.executable, __file__, "--file", str(path), "--json", str(out)],
                capture_output=True, text=True, timeout=1800,
            )
            if out.exists():
                entry = json.loads(out.read_text())
                status = entry.get("status", entry)
            else:
                entry = {"fatal": True}
                status = {"module": module_name_for(path), "fatal": True,
                          "stderr_tail": proc.stderr.strip().splitlines()[-1:] or [""]}
            summary["per_file"][path.name] = {
                k: v for k, v in status.items()
                if k in ("cases", "playgrounds", "tests", "failures", "skipped",
                         "errors", "import_error", "seconds", "fatal")
            }
            if status.get("fatal") or status.get("import_error"):
                summary["crashes" if status.get("fatal") else "import_errors"] += 1
            else:
                summary["captured"] += 1
                if status.get("cases"):
                    summary["with_cases"] += 1
                    summary["cases"] += status["cases"]
                summary["errors"] += len(status.get("errors", []))
    if stats:
        print(f"{summary['files']} upstream files: {summary['captured']} captured, "
              f"{summary['with_cases']} with cases, {summary['cases']} cases, "
              f"{summary['errors']} upstream test errors, "
              f"{summary['import_errors']} import errors, {summary['crashes']} crashes")
        for name, entry in sorted(summary["per_file"].items()):
            print(f"  {name}: " + json.dumps(entry, sort_keys=True))
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("modules", nargs="*", help="upstream module names")
    parser.add_argument("--file", help="path to one upstream test_*.py")
    parser.add_argument("--all", action="store_true", help="every resolver test file")
    parser.add_argument("--json", help="write the capture to this path")
    parser.add_argument("--emit-fixtures", metavar="DIR",
                        help="also emit a fixtures-shaped tree + cases.json")
    parser.add_argument("--stats", action="store_true", help="print a per-file summary")
    args = parser.parse_args()

    if args.all:
        summary = run_all(args.stats)
        if args.json:
            Path(args.json).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
        return 0 if summary["captured"] else 1

    _bootstrap()
    if args.file:
        if not Path(args.file).is_file():
            parser.error(f"no such upstream test file: {args.file}")
        modules = [module_name_for(Path(args.file))]
    else:
        modules = args.modules
    if not modules:
        parser.error("give one or more module names, --file, or --all")

    documents = []
    failed = False
    for module in modules:
        status = capture_module(module)
        capture = status.pop("captured")
        document = {"module": module, "status": status,
                    "playgrounds": capture.playgrounds, "cases": capture.cases}
        if args.emit_fixtures:
            document["fixtures"] = emit_fixtures(
                capture, Path(args.emit_fixtures) / module.split(".")[-1])
        documents.append(document)
        if status.get("import_error") or status.get("errors"):
            failed = True
        if args.stats:
            print(f"{module}: {json.dumps(status, sort_keys=True)}")
    if args.json:
        payload = documents[0] if len(documents) == 1 else documents
        Path(args.json).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
