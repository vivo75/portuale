#!/usr/bin/env python3
"""Tree-wide primitive differential: real Portage vs Rust
(`docs/second_python_copy_removal.md` §3).

Feeds every dependency atom, every dependency string and every
REQUIRED_USE string of a whole `metadata/md5-cache` through the kept
primitive harnesses -- `python/*_harness.py` wrap the real
`portage.dep`, `rust/*-harness` wrap portuale's crates -- and diffs the
answers line by line:

* `atom`         -- `parse <atom>` for each atom in *DEPEND/PDEPEND/IDEPEND;
* `use_reduce`   -- each dependency string under `matchnone`, `matchall`,
                    no USE, and every IUSE flag enabled;
* `required_use` -- `check` and `reduce` of each REQUIRED_USE under no
                    USE, the IUSE defaults (`+flag`), and every IUSE flag.

A mismatch is appended (deduplicated) to
`tests/primitive_regressions/<harness>.txt`, which
`tests/test_primitive_regressions.py` replays on every test run. Re-run
this after every `3rdparty/portage` re-pin.

Usage:
  scripts/primitive_tree_differential.py [--repo /var/db/repos/gentoo] [--limit N]
Exit status 1 when any mismatch was found.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
RUST = REPO / "rust"
REGRESSIONS = REPO / "tests" / "primitive_regressions"
DEP_KEYS = ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND")
HARNESSES = {
    "atom": ("atom-harness", "atom_harness.py"),
    "use_reduce": ("use-reduce-harness", "use_reduce_harness.py"),
    "required_use": ("required-use-harness", "required_use_harness.py"),
}


def md5_entries(repo: Path, limit: int | None):
    n = 0
    for entry in sorted((repo / "metadata" / "md5-cache").glob("*/*")):
        if entry.name.startswith("Manifest") or not entry.is_file():
            continue
        md = {}
        for line in entry.read_text(errors="replace").splitlines():
            key, sep, value = line.partition("=")
            if sep:
                md[key] = value
        yield md
        n += 1
        if limit and n >= limit:
            return


def iuse_flags(md) -> tuple[list[str], list[str]]:
    flags, defaults = [], []
    for tok in md.get("IUSE", "").split():
        name = tok.lstrip("+-")
        flags.append(name)
        if tok.startswith("+"):
            defaults.append(name)
    return sorted(set(flags)), sorted(set(defaults))


def use_arg(flags: list[str]) -> str:
    return ",".join(flags) if flags else "-"


def build_batches(repo: Path, limit: int | None) -> dict[str, list[str]]:
    atoms: set[str] = set()
    reduce_lines: set[str] = set()
    required_lines: set[str] = set()
    for md in md5_entries(repo, limit):
        flags, defaults = iuse_flags(md)
        for key in DEP_KEYS:
            dep = " ".join(md.get(key, "").split())
            if not dep:
                continue
            for tok in dep.split():
                if tok in ("||", "^^", "??", "(", ")") or tok.endswith("?"):
                    continue
                atoms.add(tok)
            for mode, uses in (("matchnone", "-"), ("matchall", "-"),
                               ("normal", "-"), ("normal", use_arg(flags))):
                reduce_lines.add(f"reduce {mode} {uses} {dep}")
        required = " ".join(md.get("REQUIRED_USE", "").split())
        if required:
            iuse = use_arg(flags)
            for enabled in ("-", use_arg(defaults), iuse):
                for op in ("check", "reduce"):
                    required_lines.add(f"{op} {enabled} {iuse} {required}")
    return {
        "atom": sorted(f"parse {a}" for a in atoms),
        "use_reduce": sorted(reduce_lines),
        "required_use": sorted(required_lines),
    }


def run_batch(cmd: list[str], lines: list[str]) -> list[str]:
    p = subprocess.run([*cmd, "batch"], input="".join(f"{l}\n" for l in lines),
                       capture_output=True, text=True, check=False)
    out = p.stdout.splitlines()
    if p.returncode != 0 or len(out) != len(lines):
        raise RuntimeError(f"{cmd[-1]} batch failed (rc {p.returncode}, "
                           f"{len(out)}/{len(lines)} lines): {p.stderr[-500:]}")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Tree-wide primitive differential: real Portage vs Rust.")
    ap.add_argument("--repo", type=Path, default=Path("/var/db/repos/gentoo"))
    ap.add_argument("--limit", type=int, help="only the first N md5-cache entries")
    opts = ap.parse_args()
    if not (opts.repo / "metadata" / "md5-cache").is_dir():
        sys.exit(f"{opts.repo}: no metadata/md5-cache")

    subprocess.run(["cargo", "build", "--release", "--quiet",
                    *[a for rust, _ in HARNESSES.values() for a in ("-p", rust)]],
                   cwd=RUST, check=True)
    batches = build_batches(opts.repo, opts.limit)
    REGRESSIONS.mkdir(parents=True, exist_ok=True)
    total = 0
    for name, (rust_bin, py_script) in HARNESSES.items():
        lines = batches[name]
        rust = run_batch([str(RUST / "target" / "release" / rust_bin)], lines)
        real = run_batch([sys.executable, str(REPO / "python" / py_script)], lines)
        mismatches = [line for line, r, p in zip(lines, rust, real) if r != p]
        print(f"{name}: {len(lines)} inputs, {len(mismatches)} mismatches")
        for line in mismatches[:20]:
            i = lines.index(line)
            print(f"  {line}\n    rust: {rust[i]}\n    real: {real[i]}")
        if mismatches:
            path = REGRESSIONS / f"{name}.txt"
            known = set(path.read_text().splitlines()) if path.exists() else set()
            path.write_text("".join(f"{l}\n" for l in sorted(known | set(mismatches))))
        total += len(mismatches)
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
