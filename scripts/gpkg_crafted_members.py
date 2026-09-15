#!/usr/bin/env python3
"""Backlog #56 S0 -- crafted gpkg outer containers, one per member-type cell.

Builds, from a valid committed `.gpkg.tar`, one crafted archive per cell of
the #56 S0 matrix (`docs/07.56-gpkg_outer_container.opus.md` section 5). Each cell
replaces exactly one outer member with a special entry -- symlink, FIFO,
character device, hardlink -- while the regular members and the `Manifest`
stay otherwise valid (the record for the replaced member keeps the original
size/hash, so a size check alone rejects it).

Python's `tarfile` writes the raw headers, so a cell needs neither root nor
a real device node to *build*; extraction by GNU `tar` (what portuale
shells out to) does, which is exactly what the probes measure:

  --mode build    write `<out>/<cell>-<slug>.gpkg.tar` for every cell
  --mode real     run real Portage's reader (`portage.gpkg.gpkg`) over the
                  built archives and print one JSON line per cell
  --mode extract  run `tar -xf` into a throwaway directory and report the
                  exit status + extracted entry types (GNU tar's own
                  answer to each crafted header)
  --mode all      build, then real, then extract

Use a scratch `--out` (default `/tmp/portuale-56-crafted`); a probe never
writes anywhere else -- `extract` only ever unpacks into `<out>/extracted/`.
Cell `e` (character device) needs root for GNU tar to `mknod`, so run the
`extract` probe for it with `--sudo` inside that throwaway directory.
"""

import argparse
import io
import json
import os
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DEFAULT_SRC = REPO / "fixtures/pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"
DEFAULT_OUT = Path("/tmp/portuale-56-crafted")

# `/dev/zero`'s own numbers (real `ls -l /dev/zero` -> `1, 5`); a crafted
# header needs the numbers, not the host node.
DEV_ZERO = (1, 5)

# cell -> (one-line description, member the cell acts on)
CELL_DESCRIPTIONS = {
    "a": "`Manifest` becomes a symlink to `/etc/hostname`",
    "b": "`metadata.tar.zst` becomes a symlink to `/etc/hostname`",
    "c": "the prefix name becomes a symlink to `/etc` (members under a sibling prefix)",
    "c_under": "prefix symlink to `/etc` with the members *under* it (GNU tar evidence row)",
    "d": "`metadata.tar.zst` becomes a FIFO",
    "e": "`image.tar.zst` becomes a char device 1:5 (`/dev/zero`)",
    "f": "`metadata.tar.zst` becomes a hardlink to `<prefix>/gpkg-1`",
    "g": "`metadata.tar.zst` becomes a hardlink to `../../etc/hostname`",
    "h": "`gpkg-1` becomes a symlink to `Manifest`",
}
DEFAULT_CELLS = ["a", "b", "c", "d", "e", "f", "g", "h"]


def read_valid_gpkg(src: Path) -> tuple[str, dict]:
    """(prefix, {basename: bytes}) of a valid gpkg's outer container."""
    with tarfile.open(src) as outer:
        members = {}
        for m in outer.getmembers():
            handle = outer.extractfile(m)
            if handle is not None:
                members[m.name] = handle.read()
    names_of = {Path(n).name: n for n in members}
    for required in ("gpkg-1", "metadata.tar.zst", "image.tar.zst", "Manifest"):
        if required not in names_of:
            raise SystemExit(f"{src}: no {required} member (not a gpkg fixture?)")
    prefix = names_of["gpkg-1"].split("/", 1)[0]
    return prefix, {Path(n).name: b for n, b in members.items()}


def add_entry(tar, name, data=None, kind=tarfile.REGTYPE, linkname="", mode=0o644):
    info = tarfile.TarInfo(name)
    info.type = kind
    info.mode = mode
    info.linkname = linkname
    info.uname = info.gname = "root"
    if kind == tarfile.CHRTYPE:
        info.devmajor, info.devminor = DEV_ZERO
    if data is not None:
        info.size = len(data)
    tar.addfile(info, io.BytesIO(data) if data is not None else None)


def member_order(prefix: str, files: dict) -> list:
    """Real gpkg write order: `gpkg-1`, metadata, image, Manifest."""
    return [
        (f"{prefix}/{base}", files[base])
        for base in ("gpkg-1", "metadata.tar.zst", "image.tar.zst", "Manifest")
    ]


def build_cell(cell: str, prefix: str, files: dict, dest: Path) -> None:
    with tarfile.open(dest, "w", format=tarfile.USTAR_FORMAT) as tar:
        if cell == "a":
            add_entry(tar, f"{prefix}/Manifest", kind=tarfile.SYMTYPE,
                      linkname="/etc/hostname")
            for name, data in member_order(prefix, files):
                if not name.endswith("/Manifest"):
                    add_entry(tar, name, data=data)
        elif cell == "b":
            for name, data in member_order(prefix, files):
                if name.endswith("/metadata.tar.zst"):
                    add_entry(tar, name, kind=tarfile.SYMTYPE,
                              linkname="/etc/hostname")
                else:
                    add_entry(tar, name, data=data)
        elif cell == "c":
            # The prefix name -- a member real never writes -- is a symlink
            # to /etc; the valid members sit under a sibling prefix, so GNU
            # tar can unpack the archive and a naive walk lists /etc *and*
            # still finds a usable gpkg.
            add_entry(tar, prefix, kind=tarfile.SYMTYPE, linkname="/etc")
            for name, data in member_order(f"{prefix}-members", files):
                add_entry(tar, name, data=data)
        elif cell == "c_under":
            # Evidence row: the same symlink with the members *under* the
            # symlinked name. GNU tar refuses (Cannot open: Not a
            # directory), i.e. it protects this exact shape.
            add_entry(tar, prefix, kind=tarfile.SYMTYPE, linkname="/etc")
            for name, data in member_order(prefix, files):
                add_entry(tar, name, data=data)
        elif cell == "d":
            for name, data in member_order(prefix, files):
                if name.endswith("/metadata.tar.zst"):
                    add_entry(tar, name, kind=tarfile.FIFOTYPE)
                else:
                    add_entry(tar, name, data=data)
        elif cell == "e":
            for name, data in member_order(prefix, files):
                if name.endswith("/image.tar.zst"):
                    add_entry(tar, name, kind=tarfile.CHRTYPE, mode=0o666)
                else:
                    add_entry(tar, name, data=data)
        elif cell in ("f", "g"):
            linkname = (f"{prefix}/gpkg-1" if cell == "f"
                        else "../../etc/hostname")
            for name, data in member_order(prefix, files):
                if name.endswith("/metadata.tar.zst"):
                    add_entry(tar, name, kind=tarfile.LNKTYPE, linkname=linkname)
                else:
                    add_entry(tar, name, data=data)
        elif cell == "h":
            for name, data in member_order(prefix, files):
                if name.endswith("/gpkg-1"):
                    add_entry(tar, name, kind=tarfile.SYMTYPE, linkname="Manifest")
                else:
                    add_entry(tar, name, data=data)
        else:
            raise SystemExit(f"unknown cell {cell!r}")


def slug(cell: str) -> str:
    return {
        "a": "manifest-symlink", "b": "metadata-symlink",
        "c": "prefix-symlink", "c_under": "prefix-symlink-under",
        "d": "metadata-fifo", "e": "image-chardev",
        "f": "metadata-hardlink-inside", "g": "metadata-hardlink-escape",
        "h": "gpkg1-symlink-to-manifest",
    }[cell]


def build(src: Path, out: Path, cells: list) -> dict:
    prefix, files = read_valid_gpkg(src)
    out.mkdir(parents=True, exist_ok=True)
    built = {}
    for cell in cells:
        path = out / f"{cell}-{slug(cell)}.gpkg.tar"
        build_cell(cell, prefix, files, path)
        built[cell] = path
        print(f"built {path}  ({CELL_DESCRIPTIONS[cell]})")
    return built


class timeout:
    """Raise `TimeoutError` from a `signal.alarm`, for probe calls that must
    not hang the matrix (real's reader is in-process)."""

    def __init__(self, seconds):
        self.seconds = seconds

    def __enter__(self):
        signal.signal(signal.SIGALRM, self._fire)
        signal.alarm(self.seconds)

    def _fire(self, *_):
        raise TimeoutError(f"no result within {self.seconds}s")

    def __exit__(self, *_):
        signal.alarm(0)
        return False


def probe_real(built: dict, prefix: str) -> None:
    import portage
    from portage.gpkg import gpkg

    settings = getattr(portage, "config")(clone=getattr(portage, "settings", None))
    for cell, path in built.items():
        row = {"cell": cell, "archive": str(path)}
        for label in ("_verify_binpkg", "get_metadata"):
            try:
                with timeout(20):
                    inst = gpkg(settings, prefix, str(path))
                    result = getattr(inst, label)()
                if label == "get_metadata" and isinstance(result, dict):
                    row[label] = f"OK ({len(result)} keys)"
                else:
                    row[label] = "OK"
            except BaseException as exc:  # noqa: BLE001 -- the class *is* the result
                row[label] = f"{type(exc).__name__}: {exc}".replace("\n", " ")
        print(json.dumps(row))


def file_kind(st: os.stat_result) -> str:
    mode = st.st_mode
    for kind, test in (
        ("directory", stat.S_ISDIR), ("symlink", stat.S_ISLNK),
        ("fifo", stat.S_ISFIFO), ("char-device", stat.S_ISCHR),
        ("block-device", stat.S_ISBLK), ("regular", stat.S_ISREG),
        ("socket", stat.S_ISSOCK),
    ):
        if test(mode):
            return kind
    return "other"


def probe_extract(built: dict, out: Path, use_sudo: bool) -> None:
    root = out / "extracted"
    if root.exists():
        if use_sudo:
            subprocess.run(["sudo", "-n", "rm", "-rf", str(root)], check=False)
        else:
            shutil.rmtree(root)
    for cell, path in built.items():
        dest = root / cell
        dest.mkdir(parents=True)
        argv = (["sudo", "-n"] if use_sudo else []) + [
            "tar", "-xf", str(path), "-C", str(dest)
        ]
        proc = subprocess.run(argv, capture_output=True, text=True, check=False)
        entries = []
        for dirpath, dirnames, filenames in os.walk(dest, followlinks=False):
            for name in sorted(dirnames + filenames):
                p = Path(dirpath) / name
                st = p.lstat()
                entries.append(f"{file_kind(st)} {p.relative_to(dest)}")
        row = {
            "cell": cell, "rc": proc.returncode,
            "stderr": proc.stderr.strip().replace("\n", " | "),
            "extracted": entries,
        }
        print(json.dumps(row))


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Backlog #56 S0: build/probe crafted gpkg outer containers."
    )
    ap.add_argument("--src", type=Path, default=DEFAULT_SRC,
                    help="valid gpkg to derive the cells from")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--cells", default=",".join(DEFAULT_CELLS),
                    help="comma-separated cells (default a-h)")
    ap.add_argument("--mode", choices=("build", "real", "extract", "all"),
                    default="build")
    ap.add_argument("--sudo", action="store_true",
                    help="run the extract probe's `tar` via `sudo -n` "
                         "(needed for cell e's mknod)")
    args = ap.parse_args()

    cells = [c for c in args.cells.split(",") if c]
    unknown = [c for c in cells if c not in CELL_DESCRIPTIONS]
    if unknown:
        ap.error(f"unknown cells {unknown}")

    prefix, _ = read_valid_gpkg(args.src)
    built = {}
    if args.mode in ("build", "all"):
        built = build(args.src, args.out, cells)
    else:
        for cell in cells:
            path = args.out / f"{cell}-{slug(cell)}.gpkg.tar"
            if not path.is_file():
                ap.error(f"{path} missing -- run --mode build first")
            built[cell] = path

    if args.mode in ("real", "all"):
        probe_real(built, prefix)
    if args.mode in ("extract", "all"):
        probe_extract(built, args.out, args.sudo)
    return 0


if __name__ == "__main__":
    sys.exit(main())
