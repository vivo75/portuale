#!/usr/bin/env python3
"""Structured filesystem + VDB diff of two normalised L1 snapshots.

    diff.py <prefix-a> <prefix-b> [known-divergences.yaml]

`prefix-a` is the reference (Portage), `prefix-b` the candidate
(portuale). Both must have been through normalize.py first
(`<prefix>.files.norm.tsv` + `<prefix>.vdb/`).

Emits typed findings (docs/real-world-testing.md §4.4):
  MISSING   path present for one PM, absent for the other
  MODE / OWNER / XATTR / SIZE / CONTENT / SYMLINK   path in both, attr differs
  VDB:<file>   a VDB metadata file differs
  CONTENTS     the VDB CONTENTS file differs (line-typed)
  MTIME        reported separately, non-fatal by default

A finding is *explained* when it matches an entry in the allowlist; the
run is GREEN iff every hard finding is explained. Exit: 0 green, 1
unexplained, 2 usage/IO.

stdlib + PyYAML.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

try:
    import yaml
except ModuleNotFoundError:  # pragma: no cover
    sys.exit("diff.py needs PyYAML (dev-python/pyyaml)")

HARD = {"MISSING", "MODE", "OWNER", "XATTR", "SIZE", "CONTENT", "SYMLINK", "VDB", "CONTENTS"}


def load_files(prefix: Path) -> dict[str, tuple]:
    f = prefix.parent / (prefix.name + ".files.norm.tsv")
    out: dict[str, tuple] = {}
    if not f.exists():
        return out
    for line in f.read_text().splitlines():
        p = line.split("\t")
        if len(p) != 9:
            continue
        out[p[0]] = tuple(p[1:])  # type,mode,uid,gid,size,sha,link,xattr
    return out


def load_mtimes(prefix: Path) -> dict[str, str]:
    f = prefix.parent / (prefix.name + ".mtimes.tsv")
    out: dict[str, str] = {}
    if f.exists():
        for line in f.read_text().splitlines():
            k, _, v = line.partition("\t")
            out[k] = v
    return out


def load_vdb(prefix: Path) -> dict[str, str]:
    """cat/pf/file -> file text, for every regular file in the vdb tree."""
    root = prefix.parent / (prefix.name + ".vdb") / "pkg"
    out: dict[str, str] = {}
    if not root.is_dir():
        return out
    for f in root.rglob("*"):
        if f.is_file():
            out[str(f.relative_to(root))] = f.read_text(errors="replace")
    return out


class Report:
    def __init__(self) -> None:
        self.findings: list[dict] = []

    def add(self, cat: str, path: str, detail: str) -> None:
        self.findings.append({"category": cat, "path": path, "detail": detail})


def diff_files(a: dict, b: dict, rep: Report) -> None:
    fields = ["type", "mode", "uid", "gid", "size", "sha", "link", "xattr"]
    for p in sorted(a.keys() - b.keys()):
        rep.add("MISSING", p, "present for portage, absent for portuale")
    for p in sorted(b.keys() - a.keys()):
        rep.add("MISSING", p, "present for portuale, absent for portage")
    for p in sorted(a.keys() & b.keys()):
        av, bv = a[p], b[p]
        if av == bv:
            continue
        d = {fields[i]: (av[i], bv[i]) for i in range(8) if av[i] != bv[i]}
        if "type" in d:
            rep.add("MISSING", p, f"type differs: portage {d['type'][0]} vs portuale {d['type'][1]}")
            continue
        if "link" in d:
            rep.add("SYMLINK", p, f"portage -> {d['link'][0]!r}  portuale -> {d['link'][1]!r}")
        if "mode" in d:
            rep.add("MODE", p, f"portage {d['mode'][0]} vs portuale {d['mode'][1]}")
        if "uid" in d or "gid" in d:
            rep.add("OWNER", p, f"portage {av[2]}:{av[3]} vs portuale {bv[2]}:{bv[3]}")
        if "xattr" in d:
            rep.add("XATTR", p, f"portage {d['xattr'][0]} vs portuale {d['xattr'][1]}")
        if "size" in d:
            rep.add("SIZE", p, f"portage {d['size'][0]} vs portuale {d['size'][1]}")
        if "sha" in d and "size" not in d:
            rep.add("CONTENT", p, f"sha256 differs ({d['sha'][0][:12]} vs {d['sha'][1][:12]})")


def diff_vdb(a: dict, b: dict, rep: Report) -> None:
    for k in sorted(a.keys() - b.keys()):
        rep.add("VDB", k, "vdb file present for portage, absent for portuale")
    for k in sorted(b.keys() - a.keys()):
        rep.add("VDB", k, "vdb file present for portuale, absent for portage")
    for k in sorted(a.keys() & b.keys()):
        if a[k] == b[k]:
            continue
        pf = k.rsplit("/", 1)[-1]
        if pf == "CONTENTS":
            la, lb = set(a[k].splitlines()), set(b[k].splitlines())
            for ln in sorted(la - lb)[:8]:
                rep.add("CONTENTS", k, f"portage-only line: {ln}")
            for ln in sorted(lb - la)[:8]:
                rep.add("CONTENTS", k, f"portuale-only line: {ln}")
            extra = (len(la - lb) + len(lb - la)) - min(8, len(la - lb)) - min(8, len(lb - la))
            if extra > 0:
                rep.add("CONTENTS", k, f"... +{extra} more line diffs")
        else:
            va = a[k].strip().replace("\n", " | ")[:200]
            vb = b[k].strip().replace("\n", " | ")[:200]
            rep.add("VDB", k, f"portage={va!r} portuale={vb!r}")


def diff_mtimes(a: dict, b: dict, rep: Report) -> int:
    n = 0
    for p in a.keys() & b.keys():
        if a[p] != b[p]:
            n += 1
    return n


# --- allowlist ------------------------------------------------------
def load_allowlist(path: Path | None) -> list[dict]:
    if path is None or not path.is_file():
        return []
    data = yaml.safe_load(path.read_text()) or []
    return data if isinstance(data, list) else []


def glob_match(pat: str, s: str) -> bool:
    return re.fullmatch(re.escape(pat).replace(r"\*", ".*"), s) is not None


def explained(f: dict, allow: list[dict]) -> str | None:
    for e in allow:
        if e.get("layer") not in (None, "l1"):
            continue
        cats = e.get("categories") or ([e["category"]] if "category" in e else [])
        if cats and f["category"] not in cats:
            continue
        globs = e.get("path_glob")
        globs = [globs] if isinstance(globs, str) else (globs or [])
        if globs and not any(glob_match(g, f["path"]) for g in globs):
            continue
        sub = e.get("match")
        if sub and sub not in f["detail"]:
            continue
        if not (cats or globs or sub):
            continue
        return e.get("id", "<unnamed>")
    return None


def main(argv: list[str]) -> int:
    if not 2 <= len(argv) <= 3:
        print(__doc__)
        return 2
    a, b = Path(argv[0]), Path(argv[1])
    allow = load_allowlist(Path(argv[2]) if len(argv) == 3 else None)

    rep = Report()
    diff_files(load_files(a), load_files(b), rep)
    diff_vdb(load_vdb(a), load_vdb(b), rep)
    mtime_diffs = diff_mtimes(load_mtimes(a), load_mtimes(b), rep)

    unexplained: list[dict] = []
    explained_hits: list[tuple[dict, str]] = []
    for f in rep.findings:
        if f["category"] not in HARD:
            continue
        eid = explained(f, allow)
        if eid:
            f["explained_by"] = eid
            explained_hits.append((f, eid))
        else:
            unexplained.append(f)

    by_cat: dict[str, int] = {}
    for f in unexplained:
        by_cat[f["category"]] = by_cat.get(f["category"], 0) + 1

    def meta(prefix: Path) -> str:
        m = prefix.parent / (prefix.name + ".meta.tsv")
        return m.read_text() if m.exists() else "(none)\n"

    lines = [
        "# L1 merge-parity report",
        "",
        "## portage (reference)",
        *("  " + x for x in meta(a).splitlines()),
        "## portuale (candidate)",
        *("  " + x for x in meta(b).splitlines()),
        "",
        "## summary",
        f"  hard findings     : {len(unexplained) + len(explained_hits)}",
        f"  explained         : {len(explained_hits)}",
        f"  UNEXPLAINED       : {len(unexplained)}",
        *(f"    {c:10s} : {n}" for c, n in sorted(by_cat.items())),
        f"  mtime-only diffs  : {mtime_diffs}  (non-fatal)",
        "",
    ]
    if unexplained:
        lines.append("## unexplained findings")
        for f in unexplained[:400]:
            lines.append(f"  [{f['category']}] {f['path']}")
            lines.append(f"      {f['detail']}")
        if len(unexplained) > 400:
            lines.append(f"  ... +{len(unexplained) - 400} more")
        lines.append("")
    if explained_hits:
        lines.append("## explained (allowlisted)")
        for f, eid in explained_hits[:200]:
            lines.append(f"  [{f['category']}] ({eid}) {f['path']}")
        lines.append("")

    print("\n".join(lines))
    (a.parent / "l1-report.json").write_text(
        json.dumps(
            {
                "summary": {
                    "unexplained": len(unexplained),
                    "explained": len(explained_hits),
                    "by_category": by_cat,
                    "mtime_diffs": mtime_diffs,
                },
                "findings": rep.findings,
            },
            indent=2,
        )
    )
    return 1 if unexplained else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
