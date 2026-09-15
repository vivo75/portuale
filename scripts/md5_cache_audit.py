#!/usr/bin/env python3
"""Audit committed `metadata/md5-cache` entries against their ebuilds
(backlog #46 S1).

Two report modes:

* default -- validity per repo: for every `metadata/md5-cache/<cat>/<pf>`
  entry, is its `_md5_` the md5 of `<repo>/<cat>/<pn>/<pf>.ebuild`
  (real `cache/template.py::validate_entry`'s `md5_database` rung)? An
  entry without an ebuild is reported separately; dot-dirs (pytest
  junk) are skipped.
* `--against <regen-root>` -- key-level diff of the committed entries
  against a regenerated tree with the same repo layout (`<root>/<repo>`,
  one root per source tree). `_md5_` is excluded from the diff; diffs
  are split into `empty-vs-missing` (one side carries `KEY=`, the other
  omits the key -- real's writer skips empty values) and `substantive`
  (both sides non-empty and different, or one empty and one non-empty).

Exit status: 0 always in report mode; the S1 guard test is the one that
fails a stale tree (it re-implements the md5 check with no dependencies).
"""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path

DEFAULT_REPOS = {
    "fixtures": (
        "repo",
        "overlay",
        "independentoverlay",
        "layoutmasteroverlay",
        "repnamerepo",
    ),
    "TEST/images/overlay": ("porttest",),
}


def parse_entry(path: Path) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in path.read_text(errors="replace").splitlines():
        if "=" in line:
            key, _, value = line.partition("=")
            fields[key] = value
    return fields


def find_ebuild(repo: Path, category: str, pf: str) -> Path | None:
    cat_dir = repo / category
    if not cat_dir.is_dir():
        return None
    for pkg_dir in sorted(cat_dir.iterdir()):
        if not pkg_dir.is_dir() or pkg_dir.name.startswith("."):
            continue
        ebuild = pkg_dir / f"{pf}.ebuild"
        if ebuild.is_file():
            return ebuild
    return None


def repo_entries(repo: Path):
    root = repo / "metadata" / "md5-cache"
    if not root.is_dir():
        return
    for cat_dir in sorted(root.iterdir()):
        if not cat_dir.is_dir() or cat_dir.name.startswith("."):
            continue
        for entry in sorted(cat_dir.iterdir()):
            if not entry.is_file() or entry.name.startswith("."):
                continue
            yield cat_dir.name, entry.name, entry


def audit(repo: Path) -> dict:
    counts = {"valid": 0, "stale": 0, "no-ebuild": 0}
    stale: list[str] = []
    missing: list[str] = []
    for category, pf, entry in repo_entries(repo):
        ebuild = find_ebuild(repo, category, pf)
        if ebuild is None:
            counts["no-ebuild"] += 1
            missing.append(f"{category}/{pf}")
            continue
        fields = parse_entry(entry)
        want = fields.get("_md5_")
        # A missing or malformed `_md5_` (the 31-zero fixture placeholder is
        # both) fails real's validator the same way a mismatched one does.
        if want is None or len(want) != 32:
            counts["stale"] += 1
            stale.append(f"{category}/{pf}")
            continue
        got = hashlib.md5(ebuild.read_bytes()).hexdigest()
        if got == want:
            counts["valid"] += 1
        else:
            counts["stale"] += 1
            stale.append(f"{category}/{pf}")
    return {"counts": counts, "stale": stale, "no-ebuild": missing}


def diff_entries(committed: Path, regenerated: Path) -> dict:
    result = {"identical": 0, "empty-only": [], "substantive": [], "only-committed": [],
              "only-regenerated": []}
    seen: set[tuple[str, str]] = set()
    for category, pf, entry in repo_entries(committed):
        seen.add((category, pf))
        other = regenerated / "metadata" / "md5-cache" / category / pf
        if not other.is_file():
            result["only-committed"].append(f"{category}/{pf}")
            continue
        a = {k: v for k, v in parse_entry(entry).items() if k != "_md5_"}
        b = {k: v for k, v in parse_entry(other).items() if k != "_md5_"}
        if a == b:
            result["identical"] += 1
            continue
        keys = sorted(set(a) | set(b))
        empty_only = True
        details = []
        for key in keys:
            va, vb = a.get(key), b.get(key)
            if va == vb:
                continue
            details.append(f"{key}: committed={va!r} regen={vb!r}")
            # Missing-vs-empty only: real's writer omits empty values, so
            # `KEY=` on one side and no key on the other is the same
            # meaning. A value disappearing (None vs "x") is substantive.
            if va in (None, "") and vb in (None, ""):
                continue
            empty_only = False
        row = f"{category}/{pf} :: " + "; ".join(details)
        (result["empty-only"] if empty_only else result["substantive"]).append(row)
    for category, pf, _ in repo_entries(regenerated):
        if (category, pf) not in seen:
            result["only-regenerated"].append(f"{category}/{pf}")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", action="append", default=[],
                        help="a tree root to audit (default: the repo's fixture roots)")
    parser.add_argument("--against", help="regenerated tree root to diff against")
    args = parser.parse_args()

    if args.root:
        roots = [
            (Path(root), tuple(d.name for d in sorted(Path(root).iterdir()) if d.is_dir()))
            for root in args.root
        ]
    else:
        roots = [(Path.cwd() / key, names) for key, names in DEFAULT_REPOS.items()]
    for root_path, names in roots:
        for name in names:
            repo = root_path / name
            if not (repo / "metadata").is_dir():
                continue
            report = audit(repo)
            c = report["counts"]
            print(f"{repo}: entries valid={c['valid']} stale={c['stale']} "
                  f"no-ebuild={c['no-ebuild']}")
            if report["stale"] and "-v" in sys.argv:
                for row in report["stale"]:
                    print(f"  stale: {row}")
            if report["no-ebuild"] and "-v" in sys.argv:
                for row in report["no-ebuild"]:
                    print(f"  no-ebuild: {row}")
            if args.against:
                regen = Path(args.against) / name
                if not (regen / "metadata").is_dir():
                    print(f"  (no regenerated copy under {regen})")
                    continue
                d = diff_entries(repo, regen)
                print(f"  vs regenerated: identical={d['identical']} "
                      f"empty-only={len(d['empty-only'])} "
                      f"substantive={len(d['substantive'])} "
                      f"only-committed={len(d['only-committed'])} "
                      f"only-regenerated={len(d['only-regenerated'])}")
                for label in ("substantive", "only-committed", "only-regenerated"):
                    for row in d[label]:
                        print(f"  {label}: {row}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
