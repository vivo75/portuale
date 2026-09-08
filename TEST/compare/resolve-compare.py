#!/usr/bin/env python3
"""L0 -- resolver parity comparison (host half).

Reads a directory produced by ``TEST/layers/l0/in-container.sh``::

    <dir>/real/<slug>.txt        raw `emerge -pv` output, real portage
    <dir>/portuale/<slug>.txt    raw `emerge -pv` output, portuale
    <dir>/meta.tsv               slug \\t kind \\t real_rc \\t ptl_rc
    <dir>/fingerprint.tsv        environment fingerprint

For every probe it normalises both merge lists and reports typed
findings (missing / extra / version / flags / use / order / error /
exit / totals). A finding is *explained* when it matches an entry in the
allowlist (``known-divergences.yaml``); the run is GREEN iff every
finding is explained.

Outputs ``<dir>/l0-report.txt`` (human) and ``<dir>/l0-report.json``
(machine). Exit status: 0 green, 1 unexplained findings, 2 usage/IO.

stdlib + PyYAML only.
"""
from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass, field, asdict
from pathlib import Path

try:
    import yaml
except ModuleNotFoundError:  # pragma: no cover
    sys.exit("resolve-compare.py needs PyYAML (dev-python/pyyaml)")

ANSI = re.compile(r"\x1b\[[0-9;]*m")
# [ebuild   N     ] cat/pkg-1.2-r3::repo  USE="..."   -- the merge-list line
MERGE = re.compile(r"^\[(?P<type>[a-z]+)(?P<flags>[^\]]*)\]\s+(?P<rest>\S.*?)\s*$")
TOTAL = re.compile(r"^Total:\s*(\d+)\s*package")
ERRLINE = re.compile(r"^(emerge:|!!!|\s*\*\s*(ERROR|The following)|.*\bREQUIRED_USE\b)")
KV = re.compile(r'([A-Z0-9_]+)="([^"]*)"')
# version = first hyphen-component that starts with a digit (portage rule)
CPV = re.compile(r"^(?P<cp>.+?)-(?P<ver>\d[^-]*(?:-r\d+)?)(?:::(?P<repo>\S+))?$")

MERGE_TYPES = {"ebuild", "binary", "nomerge", "blocks", "uninstall"}


@dataclass
class Pkg:
    type: str
    cp: str
    ver: str
    repo: str
    flags: str
    use: dict[str, list[str]]
    raw: str


@dataclass
class Probe:
    slug: str
    kind: str
    real_rc: int
    ptl_rc: int
    findings: list[dict] = field(default_factory=list)


def strip_ansi(s: str) -> str:
    return ANSI.sub("", s)


def split_cpv(s: str) -> tuple[str, str, str]:
    m = CPV.match(s)
    if not m:
        return s, "", ""
    return m["cp"], m["ver"], m["repo"] or ""


ADVICE_ATOM = re.compile(r"^[<>=~]*[a-z0-9][a-z0-9+._-]*/\S+(?:\s+\S+)*\s*$")
MASKED = re.compile(r'have been masked|is required to complete your request')
REQUSE = re.compile(r'REQUIRED_USE (?:flag constraints are unsatisfied|not satisfied)')


def parse(path: Path) -> tuple[list[Pkg], list[str], int | None, set[str]]:
    """Return (merge-list, error lines, Total-or-None, advice set).

    The *advice set* is the actionable "you must change something"
    diagnostics -- needed USE-flag changes, masked-package requirements,
    unsatisfied REQUIRED_USE -- normalised so real and portuale can be
    compared even when one truncates its merge list and the other does
    not (the autounmask exit-code divergence).
    """
    pkgs: list[Pkg] = []
    errs: list[str] = []
    total: int | None = None
    advice: set[str] = set()
    if not path.exists():
        return pkgs, ["<no output file>"], None, advice
    in_use_block = False
    for line in path.read_text(errors="replace").splitlines():
        line = strip_ansi(line).rstrip()
        if not line:
            continue
        m = MERGE.match(line)
        if m and m["type"] in MERGE_TYPES:
            rest = m["rest"]
            tok = rest.split()[0]
            if m["type"] in ("blocks", "uninstall"):
                # the token is an atom / cpv, not necessarily splittable
                cp, ver, repo = tok, "", ""
            else:
                cp, ver, repo = split_cpv(tok)
            use = {k: sorted(v.split()) for k, v in KV.findall(rest)}
            pkgs.append(
                Pkg(
                    type=m["type"],
                    cp=cp,
                    ver=ver,
                    repo=repo,
                    flags=" ".join(m["flags"].split()),
                    use=use,
                    raw=line,
                )
            )
            continue
        mt = TOTAL.match(line)
        if mt:
            total = int(mt.group(1))
            continue
        stripped = line.strip()
        if stripped.startswith("The following USE changes are necessary"):
            in_use_block = True
            continue
        if in_use_block:
            if not stripped or stripped.startswith("#") or stripped.startswith("("):
                if not stripped:
                    in_use_block = False
                continue
            if ADVICE_ATOM.match(stripped):
                advice.add("use-change: " + re.sub(r"\s+", " ", stripped))
                continue
            in_use_block = False
        if MASKED.search(line):
            advice.add("masked: " + re.sub(r"\s+", " ", stripped))
        if REQUSE.search(line):
            advice.add("required-use: " + re.sub(r"\s+", " ", stripped))
        m2 = re.search(r'"([^"]+)" have been masked', line)
        if m2:
            advice.add(f"masked-atom: {m2.group(1)}")

        if ERRLINE.match(line):
            # normalise volatile bits out of error/message lines
            n = re.sub(r"/var/tmp/portage/\S+", "<builddir>", line)
            n = re.sub(r"\b\d{4}-\d\d-\d\d\b", "<date>", n)
            n = re.sub(r"\s+", " ", n).strip()
            errs.append(n)
    return pkgs, errs, total, advice


def compare(slug: str, kind: str, rrc: int, prc: int, rp: Path, pp: Path) -> Probe:
    pr = Probe(slug=slug, kind=kind, real_rc=rrc, ptl_rc=prc)
    rpk, rerr, rtot, radv = parse(rp)
    ppk, perr, ptot, padv = parse(pp)

    def add(cat: str, detail: str) -> None:
        pr.findings.append({"category": cat, "detail": detail})

    # -- actionable advice (needed USE changes / masks / REQUIRED_USE) ----
    # compared even when exit codes differ (autounmask truncates one side)
    for a in sorted(radv - padv):
        add("advice", f"real-only: {a}")
    for a in sorted(padv - radv):
        add("advice", f"portuale-only: {a}")

    if rrc != prc:
        add("exit", f"real rc={rrc} portuale rc={prc}")
        # the merge lists are a complete-vs-truncated comparison here --
        # every missing/extra/order/totals finding would be downstream
        # noise. Report the shapes and stop.
        add(
            "exit",
            f"merge-list comparison suppressed: real {len(rpk)} lines / "
            f"Total {rtot}, portuale {len(ppk)} lines / Total {ptot}",
        )
        rset, pset = set(rerr), set(perr)
        for e in sorted(rset - pset):
            add("error", f"real-only message: {e}")
        for e in sorted(pset - rset):
            add("error", f"portuale-only message: {e}")
        return pr

    # -- package identity keyed by (type, cp) -------------------------------
    rmap = {(p.type, p.cp): p for p in rpk}
    pmap = {(p.type, p.cp): p for p in ppk}
    def ident(p: Pkg) -> str:
        return f"{p.type} {p.cp}-{p.ver}" if p.ver else f"{p.type} {p.cp}"

    for key in rmap.keys() - pmap.keys():
        add("missing", f"{ident(rmap[key])} present for real, absent for portuale")
    for key in pmap.keys() - rmap.keys():
        add("extra", f"{ident(pmap[key])} present for portuale, absent for real")

    for key in rmap.keys() & pmap.keys():
        r, p = rmap[key], pmap[key]
        if r.ver != p.ver:
            add("version", f"{r.cp}: real {r.ver} vs portuale {p.ver}")
        if r.flags != p.flags:
            add("flags", f"{r.cp}: real flags [{r.flags}] vs portuale [{p.flags}]")
        for uk in r.use.keys() | p.use.keys():
            rv, pv = r.use.get(uk, []), p.use.get(uk, [])
            if rv != pv:
                add("use", f"{r.cp} {uk}: real {rv} vs portuale {pv}")

    # -- merge order (over the common set) --------------------------------
    common = [k for k in ((x.type, x.cp) for x in rpk) if k in pmap]
    pcommon = [k for k in ((x.type, x.cp) for x in ppk) if k in rmap]
    if common != pcommon and sorted(common) == sorted(pcommon):
        # first divergent position, for a readable detail
        for i, (a, b) in enumerate(zip(common, pcommon)):
            if a != b:
                add(
                    "order",
                    f"merge order diverges at #{i}: real {a[1]} vs portuale {b[1]}",
                )
                break

    # -- Total: count ----------------------------------------------------
    if rtot is not None and ptot is not None and rtot != ptot:
        add("totals", f"real Total {rtot} vs portuale Total {ptot}")

    # -- error / message lines -----------------------------------------
    rset, pset = set(rerr), set(perr)
    for e in sorted(rset - pset):
        add("error", f"real-only message: {e}")
    for e in sorted(pset - rset):
        add("error", f"portuale-only message: {e}")

    return pr


# --------------------------------------------------------------------------
# allowlist
# --------------------------------------------------------------------------
def load_allowlist(path: Path) -> list[dict]:
    if not path.exists():
        return []
    data = yaml.safe_load(path.read_text()) or []
    if not isinstance(data, list):
        sys.exit(f"{path}: expected a top-level YAML list")
    return data


def glob_match(pat: str, s: str) -> bool:
    return re.fullmatch(re.escape(pat).replace(r"\*", ".*"), s) is not None


def explained(finding: dict, slug: str, kind: str, allow: list[dict]) -> dict | None:
    for e in allow:
        if e.get("layer", "l0") != "l0":
            continue
        cats = e.get("categories") or ([e["category"]] if "category" in e else [])
        if cats and finding["category"] not in cats:
            continue
        slugs = e.get("slugs") or []
        if slugs and not any(glob_match(g, slug) for g in slugs):
            continue
        sub = e.get("match")
        if sub and sub not in finding["detail"]:
            continue
        if not (cats or slugs or sub):
            continue  # an entry that constrains nothing matches nothing
        return e
    return None


# --------------------------------------------------------------------------
def main(argv: list[str]) -> int:
    if not 1 <= len(argv) <= 2:
        print(__doc__)
        return 2
    d = Path(argv[0])
    allow = load_allowlist(
        Path(argv[1]) if len(argv) == 2 else Path(__file__).with_name("known-divergences.yaml")
    )
    meta = d / "meta.tsv"
    if not meta.exists():
        sys.exit(f"{meta} not found -- run the in-container half first")

    probes: list[Probe] = []
    for row in meta.read_text().splitlines():
        if not row.strip():
            continue
        slug, kind, rrc, prc = row.split("\t")
        probes.append(
            compare(slug, kind, int(rrc), int(prc), d / "real" / f"{slug}.txt", d / "portuale" / f"{slug}.txt")
        )

    # classify
    unexplained: list[tuple[str, dict]] = []
    explained_hits: list[tuple[str, dict, str]] = []
    for pr in probes:
        for f in pr.findings:
            e = explained(f, pr.slug, pr.kind, allow)
            if e:
                f["explained_by"] = e.get("id", "<unnamed>")
                explained_hits.append((pr.slug, f, e.get("id", "<unnamed>")))
            else:
                unexplained.append((pr.slug, f))

    by_cat: dict[str, int] = {}
    for _, f in unexplained:
        by_cat[f["category"]] = by_cat.get(f["category"], 0) + 1

    n_probes = len(probes)
    n_clean = sum(1 for p in probes if not p.findings)
    parity = n_clean / n_probes if n_probes else 1.0

    # -- text report ---------------------------------------------------
    fp = (d / "fingerprint.tsv").read_text() if (d / "fingerprint.tsv").exists() else "(none)\n"
    lines = [
        "# L0 resolver-parity report",
        "",
        "## environment",
        *("  " + x for x in fp.splitlines()),
        "",
        "## summary",
        f"  probes           : {n_probes}",
        f"  clean            : {n_clean}",
        f"  parity_rate      : {parity:.3f}",
        f"  explained        : {len(explained_hits)}",
        f"  UNEXPLAINED      : {len(unexplained)}",
        *(f"    {c:12s} : {n}" for c, n in sorted(by_cat.items())),
        "",
    ]
    if unexplained:
        lines.append("## unexplained findings")
        cur = None
        for slug, f in unexplained:
            if slug != cur:
                lines.append(f"\n### {slug}")
                cur = slug
            lines.append(f"  [{f['category']}] {f['detail']}")
        lines.append("")
    if explained_hits:
        lines.append("## explained (allowlisted) findings")
        cur = None
        for slug, f, eid in explained_hits:
            if slug != cur:
                lines.append(f"\n### {slug}")
                cur = slug
            lines.append(f"  [{f['category']}] ({eid}) {f['detail']}")
        lines.append("")

    unused = [
        e.get("id", "<unnamed>")
        for e in allow
        if e.get("layer", "l0") == "l0"
        and not any(eid == e.get("id", "<unnamed>") for _, _, eid in explained_hits)
    ]
    if unused:
        lines += ["## allowlist entries that matched nothing (candidates for removal)",
                  *(f"  {u}" for u in unused), ""]

    (d / "l0-report.txt").write_text("\n".join(lines))
    (d / "l0-report.json").write_text(
        json.dumps(
            {
                "summary": {
                    "probes": n_probes,
                    "clean": n_clean,
                    "parity_rate": parity,
                    "explained": len(explained_hits),
                    "unexplained": len(unexplained),
                    "by_category": by_cat,
                },
                "probes": [asdict(p) for p in probes],
            },
            indent=2,
        )
    )
    print("\n".join(lines))
    print(f"\nwrote {d/'l0-report.txt'} and {d/'l0-report.json'}")
    return 1 if unexplained else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
