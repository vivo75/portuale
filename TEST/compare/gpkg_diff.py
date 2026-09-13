#!/usr/bin/env python3
"""Normalised gpkg-vs-gpkg diff for the L2 test bed (called by
`gpkg-diff.sh`).

    gpkg_diff.py [--mode strict|payload-tolerant] <a.gpkg.tar> <b.gpkg.tar>

Compares two gpkg containers bucket by bucket, never bytewise:

  outer-layout   container member kind set (prefix + `gpkg-1` /
                 `metadata.tar[.comp]` / `image.tar[.comp]` / `Manifest`)
  metadata:<KEY> `metadata/*` after normalisation -- `BUILD_TIME`/
                 `BUILD_ID`/`COUNTER` blanked, `NEEDED*`/`REQUIRES`/
                 `PROVIDES` sorted, `environment.bz2` through
                 `normalize.py`'s own `norm_environment` (one ruleset,
                 not a copy), `repository` trimmed.
  image:paths    path set + type/mode/uid/gid/symlink target (hard in
                 both modes)
  image:payload  regular-file sha256. `strict` = hard (the porttest
                 fixtures, whose contents are deterministic);
                 `payload-tolerant` = reported under `payload` and
                 non-fatal (the real set: compiler nondeterminism).
  manifest       the embedded Manifest's DATA *record names* only --
                 the digest values are volatile by construction
                 (BUILD_TIME lives inside the metadata member).
  outer-name     the `<pf>-<BUILD_ID>` prefix (informational only: two
                 independent builds may allocate different BUILD_IDs).

Exit: 0 no hard findings, 1 hard finding(s), 2 usage/IO.
Stdlib + zstd/tar; src ruleset from `TEST/compare/normalize.py`.
"""
from __future__ import annotations

import bz2
import hashlib
import io
import subprocess
import sys
import tarfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from normalize import norm_environment  # noqa: E402  (the one ruleset)

VOLATILE = {"BUILD_ID", "BUILD_TIME", "COUNTER", "INSTALL_TIME"}
SORTED = {"NEEDED", "NEEDED.ELF.2", "REQUIRES", "PROVIDES"}

HARD = 0
PAYLOAD = 0
findings: list[str] = []


def hard(bucket: str, detail: str) -> None:
    global HARD
    HARD += 1
    findings.append(f"[{bucket}] {detail}")


def soft(bucket: str, detail: str) -> None:
    global PAYLOAD
    PAYLOAD += 1
    findings.append(f"[{bucket}] {detail}")


def read_inner(container_dir: Path, member: str) -> bytes:
    path = container_dir / member
    if member.endswith(".zst"):
        return subprocess.run(["zstd", "-dc", "--", str(path)], check=True,
                              capture_output=True).stdout
    return path.read_bytes()


def open_inner(container_dir: Path, member: str) -> tarfile.TarFile:
    data = read_inner(container_dir, member)
    return tarfile.open(fileobj=io.BytesIO(data))


class Side:
    def __init__(self, archive: Path, mode: str, tmpdir: Path) -> None:
        self.prefix = ""
        self.meta: dict[str, bytes] = {}
        self.image: dict[str, tuple] = {}
        self.manifest_names: set[str] = set()
        with tarfile.open(archive) as tf:
            tf.extractall(tmpdir, filter="data")
        roots = [p for p in tmpdir.iterdir() if p.is_dir()]
        if len(roots) != 1:
            raise ValueError(f"{archive}: expected one top-level dir, got {roots}")
        self.prefix = roots[0].name
        cdir = roots[0]
        memb = {m.name: m for m in cdir.iterdir()}
        meta_name = next((n for n in memb if n.startswith("metadata.tar")), None)
        image_name = next((n for n in memb if n.startswith("image.tar")), None)
        if not meta_name or not image_name:
            raise ValueError(f"{archive}: missing metadata.tar/image.tar")
        self.meta_member, self.image_member = meta_name, image_name

        with open_inner(cdir, meta_name) as mt:
            for m in mt.getmembers():
                if not m.isfile() or not m.name.startswith("metadata/"):
                    continue
                f = mt.extractfile(m)
                if f is not None:
                    self.meta[m.name.split("/", 1)[1]] = f.read()
        with open_inner(cdir, image_name) as it:
            for m in it.getmembers():
                if m.name in ("image", "image/"):
                    continue
                rel = m.name[len("image/"):] if m.name.startswith("image/") else m.name
                if m.isdir():
                    kind = "d"
                elif m.issym():
                    kind = "l"
                elif m.isfile():
                    kind = "f"
                else:
                    kind = "o"
                sha = ""
                if kind == "f":
                    f = it.extractfile(m)
                    if f is not None:
                        sha = hashlib.sha256(f.read()).hexdigest()
                self.image[rel] = (kind, f"{m.mode:04o}", m.uid, m.gid,
                                   m.linkname if kind == "l" else "", sha)
        mf = cdir / "Manifest"
        if mf.is_file():
            for line in mf.read_text(errors="replace").splitlines():
                parts = line.split()
                if len(parts) == 7 and parts[0] == "DATA":
                    self.manifest_names.add(parts[1])


def norm_meta(key: str, data: bytes, mode: str) -> str:
    if key == "environment.bz2" or key == "environment":
        return norm_environment(data)
    text = data.decode("utf-8", "replace")
    if key in VOLATILE:
        return "<normalised>\n"
    if key in SORTED:
        return "\n".join(sorted(text.splitlines())) + "\n"
    if key == "repository":
        return text.strip() + "\n"
    if key == "SIZE" and mode == "payload-tolerant":
        return "<payload>\n"
    return text


def diff(a: Side, b: Side, mode: str) -> None:
    # outer member kinds (names relative to the prefix)
    def kinds_meta(s: Side) -> set[str]:
        return {"gpkg-1", s.meta_member, s.image_member}

    if kinds_meta(a) != kinds_meta(b):
        only_a = kinds_meta(a) - kinds_meta(b)
        only_b = kinds_meta(b) - kinds_meta(a)
        hard("outer-layout", f"member kind set differs: a-only {sorted(only_a)} b-only {sorted(only_b)}")
    if a.prefix != b.prefix:
        soft("outer-name", f"prefix differs: {a.prefix} vs {b.prefix}")

    # metadata
    for key in sorted(set(a.meta) | set(b.meta)):
        if key not in a.meta:
            hard("metadata", f"missing in a: metadata/{key}")
            continue
        if key not in b.meta:
            hard("metadata", f"missing in b: metadata/{key}")
            continue
        va = norm_meta(key, a.meta[key], mode)
        vb = norm_meta(key, b.meta[key], mode)
        if va != vb:
            label = "payload" if key == "SIZE" and mode == "payload-tolerant" else "metadata"
            (soft if label == "payload" else hard)(
                label, f"metadata/{key} differs: a={va[:120]!r} b={vb[:120]!r}")

    # image paths + attributes
    for rel in sorted(set(a.image) | set(b.image)):
        if rel not in a.image:
            hard("image:paths", f"only in b: {rel}")
            continue
        if rel not in b.image:
            hard("image:paths", f"only in a: {rel}")
            continue
        ta, tb = a.image[rel], b.image[rel]
        if ta[:5] != tb[:5]:
            hard("image:paths", f"{rel}: a={ta[:5]} b={tb[:5]}")
        elif ta[5] != tb[5]:
            label = "payload" if mode == "payload-tolerant" else "image:paths"
            detail = f"{rel}: sha256 {ta[5][:12]} vs {tb[5][:12]}"
            (soft if label == "payload" else hard)(label, f"payload differs: {detail}")

    # manifest: record names only (prefix-insensitive)
    def names(s: Side) -> set[str]:
        return {n.split("/")[-1] for n in s.manifest_names}

    if names(a) != names(b):
        hard("manifest", f"DATA record names differ: a-only {sorted(names(a) - names(b))} "
                         f"b-only {sorted(names(b) - names(a))}")


def main(argv: list[str]) -> int:
    mode = "strict"
    args: list[str] = []
    i = 0
    while i < len(argv):
        if argv[i] == "--mode":
            i += 1
            if i >= len(argv) or argv[i] not in ("strict", "payload-tolerant"):
                print(__doc__)
                return 2
            mode = argv[i]
        elif argv[i] in ("-h", "--help"):
            print(__doc__)
            return 0
        else:
            args.append(argv[i])
        i += 1
    if len(args) != 2:
        print(__doc__)
        return 2
    a_path, b_path = Path(args[0]), Path(args[1])
    for p in (a_path, b_path):
        if not p.is_file():
            print(f"gpkg_diff: not a file: {p}", file=sys.stderr)
            return 2
    import tempfile
    try:
        with tempfile.TemporaryDirectory(prefix="gpkgdiff.a.") as ta, \
             tempfile.TemporaryDirectory(prefix="gpkgdiff.b.") as tb:
            a = Side(a_path, mode, Path(ta))
            b = Side(b_path, mode, Path(tb))
            diff(a, b, mode)
    except (tarfile.TarError, OSError, ValueError, subprocess.CalledProcessError) as e:
        print(f"gpkg_diff: cannot read an archive: {e}", file=sys.stderr)
        return 2
    for line in findings:
        print(line)
    print(f"gpkg-diff: mode={mode} hard={HARD} soft={PAYLOAD}")
    return 1 if HARD else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
