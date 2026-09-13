#!/usr/bin/env python3
"""Structured filesystem + VDB diff of two normalised L1/L2 snapshots.

    diff.py [--layer l0|l1|l2|...] [--tolerate-payload] \
        <prefix-a> <prefix-b> [known-divergences.yaml]

`prefix-a` is the reference (Portage), `prefix-b` the candidate
(portuale). Both must have been through normalize.py first
(`<prefix>.files.norm.tsv` + `<prefix>.vdb/`).

Emits typed findings (docs/real-world-testing.md §4.4):
  MISSING   path present for one PM, absent for the other
  MODE / OWNER / XATTR / SIZE / CONTENT / SYMLINK   path in both, attr differs
  VDB:<file>   a VDB metadata file differs
  CONTENTS     the VDB CONTENTS file differs (line-typed)
  MTIME        reported separately, non-fatal by default
  PAYLOAD      `--tolerate-payload` only: a regular-file size/content
               difference. That is the L2 cross-install mode: a portuale
               archive and a portage archive of the same package may
               legitimately carry different compiled bytes
               (compiler/build nondeterminism), but path/type/mode/owner/
               xattr/symlink differences stay hard.

A finding is *explained* when it matches an entry in the allowlist (the
entry's `layer`, if set, must equal `--layer`); the run is GREEN iff
every hard finding is explained. Exit: 0 green, 1 unexplained, 2
usage/IO.

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
PAYLOAD = "PAYLOAD"


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


def diff_files(a: dict, b: dict, rep: Report, tolerate_payload: bool = False) -> None:
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
        # Regular-file size/bytes: hard by default (L1); under
        # --tolerate-payload (L2 cross-install) they are compiler/build
        # nondeterminism on compiled artefacts and reported as PAYLOAD.
        if "size" in d:
            cat = PAYLOAD if tolerate_payload else "SIZE"
            rep.add(cat, p, f"size portage {d['size'][0]} vs portuale {d['size'][1]}")
        if "sha" in d and "size" not in d:
            cat = PAYLOAD if tolerate_payload else "CONTENT"
            rep.add(cat, p, f"sha256 differs ({d['sha'][0][:12]} vs {d['sha'][1][:12]})")


def _contents_map(text: str) -> dict:
    out: dict[tuple, str] = {}
    for ln in text.splitlines():
        f = ln.split()
        if not f:
            continue
        if f[0] == "obj" and len(f) >= 4:
            out[("obj", f[1])] = f[2]
        elif f[0] == "sym" and len(f) >= 4:
            out[("sym", f[1])] = " ".join(f[2:])
        else:
            out[(f[0], " ".join(f[1:]))] = ""
    return out


def diff_contents(k: str, ta: str, tb: str, rep: Report, tolerate_payload: bool) -> None:
    if not tolerate_payload:
        la, lb = set(ta.splitlines()), set(tb.splitlines())
        for ln in sorted(la - lb):
            rep.add("CONTENTS", k, f"portage-only line: {ln}")
        for ln in sorted(lb - la):
            rep.add("CONTENTS", k, f"portuale-only line: {ln}")
        return
    # tolerated mode: entry sets must match (hard); an `obj` md5 is a
    # payload difference (compiled file), a `sym` target/dir change is
    # structural and stays hard.
    ma, mb = _contents_map(ta), _contents_map(tb)
    for key in sorted(ma.keys() - mb.keys()):
        rep.add("CONTENTS", k, f"portage-only entry: {key[0]} {key[1]}")
    for key in sorted(mb.keys() - ma.keys()):
        rep.add("CONTENTS", k, f"portuale-only entry: {key[0]} {key[1]}")
    for key in sorted(ma.keys() & mb.keys()):
        if ma[key] == mb[key]:
            continue
        if key[0] == "obj":
            rep.add("PAYLOAD", k, f"{key[1]}: md5 portage {ma[key][:12]} vs portuale {mb[key][:12]}")
        else:
            rep.add("CONTENTS", k, f"{key[0]} {key[1]}: portage {ma[key]!r} portuale {mb[key]!r}")


def diff_vdb(a: dict, b: dict, rep: Report, tolerate_payload: bool = False) -> None:
    for k in sorted(a.keys() - b.keys()):
        rep.add("VDB", k, "vdb file present for portage, absent for portuale")
    for k in sorted(b.keys() - a.keys()):
        rep.add("VDB", k, "vdb file present for portuale, absent for portage")
    for k in sorted(a.keys() & b.keys()):
        if a[k] == b[k]:
            continue
        pf = k.rsplit("/", 1)[-1]
        if pf == "CONTENTS":
            diff_contents(k, a[k], b[k], rep, tolerate_payload)
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


def explained(f: dict, allow: list[dict], layer: str) -> str | None:
    for e in allow:
        if e.get("layer") not in (None, layer):
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
    layer = "l1"
    tolerate_payload = False
    pos: list[str] = []
    i = 0
    while i < len(argv):
        if argv[i] == "--layer":
            i += 1
            if i >= len(argv):
                print(__doc__)
                return 2
            layer = argv[i]
        elif argv[i] == "--tolerate-payload":
            tolerate_payload = True
        elif argv[i] in ("-h", "--help"):
            print(__doc__)
            return 0
        else:
            pos.append(argv[i])
        i += 1
    if not 2 <= len(pos) <= 3:
        print(__doc__)
        return 2
    a, b = Path(pos[0]), Path(pos[1])
    allow = load_allowlist(Path(pos[2]) if len(pos) == 3 else None)

    rep = Report()
    diff_files(load_files(a), load_files(b), rep, tolerate_payload)
    diff_vdb(load_vdb(a), load_vdb(b), rep, tolerate_payload)
    mtime_diffs = diff_mtimes(load_mtimes(a), load_mtimes(b), rep)

    unexplained: list[dict] = []
    explained_hits: list[tuple[dict, str]] = []
    payload_hits: list[dict] = []
    for f in rep.findings:
        if f["category"] == PAYLOAD:
            payload_hits.append(f)
            continue
        if f["category"] not in HARD:
            continue
        eid = explained(f, allow, layer)
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
        f"# {layer.upper()} merge-parity report",
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
        f"  payload diffs     : {len(payload_hits)}  (tolerated, non-fatal)",
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
    if payload_hits:
        lines.append("## expected payload (compiled-artefact nondeterminism)")
        for f in payload_hits[:200]:
            lines.append(f"  [PAYLOAD] {f['path']}")
            lines.append(f"      {f['detail']}")
        if len(payload_hits) > 200:
            lines.append(f"  ... +{len(payload_hits) - 200} more")
        lines.append("")

    print("\n".join(lines))
    (a.parent / f"{layer}-report.json").write_text(
        json.dumps(
            {
                "layer": layer,
                "summary": {
                    "unexplained": len(unexplained),
                    "explained": len(explained_hits),
                    "by_category": by_cat,
                    "payload": len(payload_hits),
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
