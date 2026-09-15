#!/usr/bin/env python3
"""Backlog #56/#58 S0 -- crafted gpkg containers, one per member-type cell.

#56 cells (`a`-`h`, outer container members) and their probe modes are
unchanged -- see `docs/07.56-gpkg_outer_container.opus.md` section 5 and
`TEST/findings/l2.md` "## #56 S0".

#58 adds the **inner** cell family (`i0`...`i19`) from
`docs/07.058-check_metadata_member_types.md` section S0: each cell starts
from the committed valid `.gpkg.tar`, decompresses its inner
`metadata.tar.zst` / `image.tar.zst`, rebuilds that inner tar with Python
`tarfile` (one crafted member), recompresses it with `zstd`, and rebuilds
the outer container with a **recomputed** `Manifest` (DATA size + digests
for the new member bytes) -- so every #56 outer check passes and the
crafted inner member is the only deviation. Rebuilt inner tars drop
directory members to match what real's writer emits (`_add_metadata` adds
only `metadata/<KEY>` files); the untouched committed fixture (cell `i0`)
keeps its directory member, which is itself an S0 evidence row.

  --mode build        write `<out>/<cell>-<slug>.gpkg.tar` for every cell
                      (plus `<out>/inner/<cell>.tar` for inner cells)
  --mode real         run real Portage's reader over the built archives
  --mode extract      run GNU `tar` the way portuale currently does, into a
                      throwaway directory, and report the exit status +
                      extracted entry types
  --mode all          build, then real, then extract

Use a scratch `--out` (default `/tmp/portuale-58-crafted`); a probe never
writes anywhere else -- `extract` only ever unpacks into
`<out>/extracted/`, the i9 write-through victim is `<out>/victim-dir`
(a directory the probe owns). Cell `e`/`i6`/`i13` (character devices)
need root for GNU tar to `mknod`, so run the `extract` probe for them
with `--sudo` inside that throwaway directory. **Never run the real or
GNU-tar probes for `i18` as root**: its symlink-then-write-through is
aimed at `/etc` and only the non-root permission failure makes it safe.
"""

import argparse
import copy
import hashlib
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
DEFAULT_OUT = Path("/tmp/portuale-58-crafted")

# `/dev/zero`'s own numbers (real `ls -l /dev/zero` -> `1, 5`); a crafted
# header needs the numbers, not the host node.
DEV_ZERO = (1, 5)

# cell -> (one-line description, member the cell acts on)
CELL_DESCRIPTIONS = {
    # #56 outer-container cells (unchanged).
    "a": "`Manifest` becomes a symlink to `/etc/hostname`",
    "b": "`metadata.tar.zst` becomes a symlink to `/etc/hostname`",
    "c": "the prefix name becomes a symlink to `/etc` (members under a sibling prefix)",
    "c_under": "prefix symlink to `/etc` with the members *under* it (GNU tar evidence row)",
    "d": "`metadata.tar.zst` becomes a FIFO",
    "e": "`image.tar.zst` becomes a char device 1:5 (`/dev/zero`)",
    "f": "`metadata.tar.zst` becomes a hardlink to `<prefix>/gpkg-1`",
    "g": "`metadata.tar.zst` becomes a hardlink to `../../etc/hostname`",
    "h": "`gpkg-1` becomes a symlink to `Manifest`",
    # #58 inner cells -- the inner `metadata.tar` (#56 S0 residue class).
    "i0": "the committed fixture, unchanged (its inner `metadata/` directory member is S0 evidence)",
    "i1": "inner `metadata/DESCRIPTION` -> symlink `/etc/hostname`",
    "i2": "inner `metadata/DESCRIPTION` -> symlink `SLOT` (in-archive, relative)",
    "i3": "inner `metadata/DESCRIPTION` -> hardlink `metadata/SLOT` (earlier member, reordered)",
    "i3b": "inner `metadata/AAA` (first member) -> hardlink `metadata/SLOT` (later member)",
    "i4": "inner `metadata/DESCRIPTION` -> hardlink `etc/hostname` (not a member)",
    "i5": "inner `metadata/DESCRIPTION` -> FIFO",
    "i6": "inner `metadata/DESCRIPTION` -> char device 1:5 (`/dev/zero`)",
    "i7": "inner `metadata/sub/` directory + `metadata/sub/KEY` regular file",
    "i7b": "inner `metadata/sub/KEY` regular file, no directory member (K4)",
    "i8": "inner `other/KEY` regular file outside `metadata/`",
    "i9": "inner `metadata/x` -> symlink the probe's victim dir, then `metadata/x/pwned`",
    "i10": "inner `metadata/A` -> `B`, `metadata/B` -> `A` (symlink cycle)",
    "i11": "inner `metadata/SLOT` twice, different bytes",
    "i12": "inner `metadata/../KEY` regular file (traversal name)",
    "i12b": "inner `/metadata/KEY` regular file (absolute name)",
    # #58 image cells (K2).
    "i13": "inner `image/dev/null0` char device 1:5",
    "i14": "inner `image/usr/bin/x` -> hardlink `/etc/hostname`",
    "i15": "inner `image/usr/bin/x` -> hardlink a *later* member",
    "i16": "inner `image/usr/lib/libfoo.so` twice",
    "i17": "inner `image/../etc/x` regular file (traversal name)",
    "i17b": "inner `/image/etc/x` regular file (absolute name)",
    "i18": "inner `image/usr/lib/x` -> symlink `/etc`, then `image/usr/lib/x/passwd`",
    "i19": "valid archive: legit image symlink + hardlink + sparse file + xattr (regression cell)",
}

OUTER_CELLS = ["a", "b", "c", "d", "e", "f", "g", "h"]
INNER_CELLS = [
    "i0",
    "i1", "i2", "i3", "i3b", "i4", "i5", "i6", "i7", "i7b", "i8",
    "i9", "i10", "i11", "i12", "i12b",
    "i13", "i14", "i15", "i16", "i17", "i17b", "i18", "i19",
]
DEFAULT_CELLS = OUTER_CELLS + INNER_CELLS
METADATA_CELLS = {
    "i0", "i1", "i2", "i3", "i3b", "i4", "i5", "i6", "i7", "i7b",
    "i8", "i9", "i10", "i11", "i12", "i12b", "i19",
}
IMAGE_CELLS = {"i13", "i14", "i15", "i16", "i17", "i17b", "i18", "i19"}


# ---------------------------------------------------------------- fixture IO


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


def zstd_decompress(data: bytes) -> bytes:
    return subprocess.run(
        ["zstd", "-dc"], input=data, capture_output=True, check=True
    ).stdout


def zstd_compress(data: bytes) -> bytes:
    return subprocess.run(
        ["zstd", "-q", "-c"], input=data, capture_output=True, check=True
    ).stdout


def read_tar_entries(data: bytes) -> list:
    """`[(TarInfo, bytes|None)]` in archive order; data only for regulars."""
    entries = []
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as tar:
        for info in tar.getmembers():
            handle = tar.extractfile(info) if info.isreg() else None
            entries.append((copy.copy(info), handle.read() if handle else None))
    return entries


def write_tar_entries(entries: list) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(mode="w", fileobj=buf, format=tarfile.USTAR_FORMAT) as tar:
        for info, data in entries:
            tar.addfile(info, io.BytesIO(data) if data is not None else None)
    return buf.getvalue()


def ti(name, kind=tarfile.REGTYPE, data=None, linkname="", mode=0o644):
    """A hand-set inner-tar header (real's own writer emits regular files
    only; the crafted cells need the raw header type)."""
    info = tarfile.TarInfo(name)
    info.type = kind
    info.mode = mode
    info.linkname = linkname
    info.uname = info.gname = "root"
    info.mtime = 1700000000
    if kind == tarfile.CHRTYPE:
        info.devmajor, info.devminor = DEV_ZERO
    if data is not None:
        info.size = len(data)
    return info


def manifest_line(name: str, data: bytes) -> str:
    return (
        f"DATA {name} {len(data)} "
        f"BLAKE2B {hashlib.blake2b(data, digest_size=64).hexdigest()} "
        f"SHA512 {hashlib.sha512(data).hexdigest()}\n"
    )


def build_outer(prefix: str, meta: bytes, image: bytes, dest: Path) -> None:
    """A real-shaped outer container with a Manifest recomputed around the
    (possibly crafted) inner members, so `_verify_binpkg` passes."""
    gpkg1 = b""
    manifest = (
        manifest_line("gpkg-1", gpkg1)
        + manifest_line("metadata.tar.zst", meta)
        + manifest_line("image.tar.zst", image)
    ).encode()
    entries = [
        (ti(f"{prefix}/gpkg-1", data=gpkg1), gpkg1),
        (ti(f"{prefix}/metadata.tar.zst", data=meta), meta),
        (ti(f"{prefix}/image.tar.zst", data=image), image),
        (ti(f"{prefix}/Manifest", data=manifest), manifest),
    ]
    dest.write_bytes(write_tar_entries(entries))


# ------------------------------------------------------------ #56 outer cells


def add_entry(tar, name, data=None, kind=tarfile.REGTYPE, linkname="", mode=0o644):
    tar.addfile(
        ti(name, kind=kind, data=data, linkname=linkname, mode=mode),
        io.BytesIO(data) if data is not None else None,
    )


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
        "i0": "fixture-unmodified",
        "i1": "inner-description-symlink-host", "i2": "inner-description-symlink-slot",
        "i3": "inner-description-hardlink-earlier", "i3b": "inner-aaa-hardlink-later",
        "i4": "inner-description-hardlink-missing", "i5": "inner-description-fifo",
        "i6": "inner-description-chardev", "i7": "inner-subdir-and-nested",
        "i7b": "inner-nested-no-dir", "i8": "inner-outside-metadata",
        "i9": "inner-symlink-write-through", "i10": "inner-symlink-cycle",
        "i11": "inner-duplicate-slot", "i12": "inner-traversal-name",
        "i12b": "inner-absolute-name", "i13": "image-chardev",
        "i14": "image-hardlink-escape", "i15": "image-hardlink-later",
        "i16": "image-duplicate", "i17": "image-traversal-name",
        "i17b": "image-absolute-name", "i18": "image-symlink-write-through",
        "i19": "valid-symlink-sparse-xattr",
    }[cell]


# ------------------------------------------------------------ #58 inner cells


def craft_metadata(cell: str, base: list, victim: Path) -> list:
    """Rebuild the inner metadata tar with the cell's one crafted member.

    Directory members are dropped to mirror real's writer (`_add_metadata`
    emits `metadata/<KEY>` regular files only); the untouched fixture keeps
    its `metadata/` directory member (cell i0), which is evidence in its
    own right.
    """
    entries = [(copy.copy(info), data) for info, data in base if not info.isdir()]
    names = [info.name for info, _ in entries]

    def pos(name):
        return names.index(name)

    def replace(name, info):
        idx = pos(name)
        entries[idx] = (info, None)
        names[idx] = info.name

    def insert(idx, info, data=None):
        entries.insert(idx, (info, data))
        names.insert(idx, info.name)

    def append(info, data=None):
        entries.append((info, data))
        names.append(info.name)

    if cell == "i0":
        # The committed fixture, inner members untouched (directory kept).
        return [(copy.copy(info), data) for info, data in base]
    if cell == "i19":
        # Valid archive: only the directory members are dropped so the
        # inner shape matches real's writer (the image side carries the
        # legitimate symlink/hardlink/sparse/xattr members).
        return entries
    if cell == "i1":
        replace("metadata/DESCRIPTION", ti("metadata/DESCRIPTION",
                                          kind=tarfile.SYMTYPE, linkname="/etc/hostname",
                                          mode=0o777))
    elif cell == "i2":
        replace("metadata/DESCRIPTION", ti("metadata/DESCRIPTION",
                                          kind=tarfile.SYMTYPE, linkname="SLOT",
                                          mode=0o777))
    elif cell == "i3":
        # A hardlink can only target an *earlier* member, so move SLOT
        # ahead of DESCRIPTION and turn DESCRIPTION into the link.
        slot_info, slot_data = next(
            (copy.copy(i), d) for i, d in base if i.name == "metadata/SLOT"
        )
        entries[:] = [(i, d) for i, d in entries if i.name != "metadata/SLOT"]
        names.remove("metadata/SLOT")
        insert(names.index("metadata/DESCRIPTION"), slot_info, slot_data)
        replace("metadata/DESCRIPTION", ti("metadata/DESCRIPTION",
                                          kind=tarfile.LNKTYPE,
                                          linkname="metadata/SLOT"))
    elif cell == "i3b":
        insert(0, ti("metadata/AAA", kind=tarfile.LNKTYPE,
                     linkname="metadata/SLOT"))
    elif cell == "i4":
        replace("metadata/DESCRIPTION", ti("metadata/DESCRIPTION",
                                          kind=tarfile.LNKTYPE,
                                          linkname="etc/hostname"))
    elif cell == "i5":
        replace("metadata/DESCRIPTION", ti("metadata/DESCRIPTION",
                                          kind=tarfile.FIFOTYPE))
    elif cell == "i6":
        replace("metadata/DESCRIPTION", ti("metadata/DESCRIPTION",
                                          kind=tarfile.CHRTYPE, mode=0o666))
    elif cell == "i7":
        append(ti("metadata/sub/", kind=tarfile.DIRTYPE, mode=0o755))
        append(ti("metadata/sub/KEY", data=b"nested\n"), b"nested\n")
    elif cell == "i7b":
        append(ti("metadata/sub/KEY", data=b"nested\n"), b"nested\n")
    elif cell == "i8":
        append(ti("other/KEY", data=b"outside\n"), b"outside\n")
    elif cell == "i9":
        append(ti("metadata/x", kind=tarfile.SYMTYPE, linkname=str(victim),
                  mode=0o777))
        append(ti("metadata/x/pwned", data=b"pwned\n"), b"pwned\n")
    elif cell == "i10":
        append(ti("metadata/A", kind=tarfile.SYMTYPE, linkname="B", mode=0o777))
        append(ti("metadata/B", kind=tarfile.SYMTYPE, linkname="A", mode=0o777))
    elif cell == "i11":
        append(ti("metadata/SLOT", data=b"1\n"), b"1\n")
    elif cell == "i12":
        append(ti("metadata/../KEY", data=b"traversal\n"), b"traversal\n")
    elif cell == "i12b":
        append(ti("/metadata/KEY", data=b"absolute\n"), b"absolute\n")
    else:
        raise SystemExit(f"unknown inner cell {cell!r}")
    return entries


def craft_image(cell: str, base: list) -> list:
    entries = [(copy.copy(info), data) for info, data in base]
    if cell == "i13":
        entries.append((ti("image/dev/null0", kind=tarfile.CHRTYPE, mode=0o666), None))
    elif cell == "i14":
        entries.append((ti("image/usr/bin/x", kind=tarfile.LNKTYPE,
                           linkname="/etc/hostname"), None))
    elif cell == "i15":
        # x first, target y later -- `_find_link_target` only searches
        # *before* a hardlink.
        entries.append((ti("image/usr/bin/x", kind=tarfile.LNKTYPE,
                           linkname="image/usr/bin/y"), None))
        entries.append((ti("image/usr/bin/y", data=b"later\n"), b"later\n"))
    elif cell == "i16":
        entries.append((ti("image/usr/lib/libfoo.so", data=b"first\n"), b"first\n"))
        entries.append((ti("image/usr/lib/libfoo.so", data=b"second\n"), b"second\n"))
    elif cell == "i17":
        entries.append((ti("image/../etc/x", data=b"traversal\n"), b"traversal\n"))
    elif cell == "i17b":
        entries.append((ti("/image/etc/x", data=b"absolute\n"), b"absolute\n"))
    elif cell == "i18":
        entries.append((ti("image/usr/lib/x", kind=tarfile.SYMTYPE,
                           linkname="/etc", mode=0o777), None))
        entries.append((ti("image/usr/lib/x/passwd", data=b"pwned\n"), b"pwned\n"))
    else:
        raise SystemExit(f"unknown image cell {cell!r}")
    return entries


def build_i19_image(out: Path) -> bytes:
    """A legitimately-shaped image tar: a real directory tree with a
    relative symlink, a hardlink to an earlier member, a sparse file and
    a user xattr, packed by GNU tar (`--sparse --xattrs`) exactly the way
    real's writer's `tar.add(..., recursive=True)` would shape it."""
    stage = out / "i19-stage"
    if stage.exists():
        shutil.rmtree(stage)
    img = stage / "image"
    (img / "usr/lib").mkdir(parents=True)
    (img / "usr/bin").mkdir(parents=True)
    (img / "usr/share").mkdir(parents=True)
    (img / "usr/lib/libreal.so").write_bytes(b"real library\n")
    os.symlink("libreal.so", img / "usr/lib/liblink.so")
    (img / "usr/bin/realbin").write_bytes(b"#!/bin/sh\nexit 0\n")
    os.chmod(img / "usr/bin/realbin", 0o755)
    os.link(img / "usr/bin/realbin", img / "usr/bin/hardlink")
    sparse = img / "usr/share/sparse.bin"
    with open(sparse, "wb") as f:
        f.write(b"head")
        f.seek(1024 * 1024)
        f.write(b"tail")
    xattr_file = img / "usr/share/xattr.txt"
    xattr_file.write_bytes(b"xattr carrier\n")
    xattr_ok = True
    try:
        os.setxattr(xattr_file, b"user.portuale", b"s0")
    except OSError as exc:
        xattr_ok = False
        print(json.dumps({"cell": "i19", "warning": f"setxattr failed: {exc}"}))
    dest = out / "inner/i19.image.tar"
    subprocess.run(
        ["tar", "-cf", str(dest), "--sparse", "--xattrs", "-C", str(stage), "image"],
        check=True,
    )
    print(json.dumps({"cell": "i19", "xattr_recorded": xattr_ok,
                      "image_tar": str(dest)}))
    return dest.read_bytes()


def build_inner_cell(cell: str, prefix: str, files: dict, out: Path,
                     victim: Path) -> Path:
    meta = files["metadata.tar.zst"]
    image = files["image.tar.zst"]
    meta_base: list = []
    image_base: list = []
    if cell != "i0":
        meta_base = read_tar_entries(zstd_decompress(meta))
        image_base = read_tar_entries(zstd_decompress(image))
    if cell in METADATA_CELLS and cell != "i0":
        entries = craft_metadata(cell, meta_base, victim)
        raw = write_tar_entries(entries)
        (out / "inner" / f"{cell}.metadata.tar").write_bytes(raw)
        meta = zstd_compress(raw)
    if cell in IMAGE_CELLS:
        if cell == "i19":
            image = zstd_compress(build_i19_image(out))
        else:
            raw = write_tar_entries(craft_image(cell, image_base))
            (out / "inner" / f"{cell}.image.tar").write_bytes(raw)
            image = zstd_compress(raw)
    dest = out / f"{cell}-{slug(cell)}.gpkg.tar"
    if cell == "i0":
        shutil.copy(DEFAULT_SRC, dest)
    else:
        build_outer(prefix, meta, image, dest)
    return dest


def build(src: Path, out: Path, cells: list) -> dict:
    prefix, files = read_valid_gpkg(src)
    out.mkdir(parents=True, exist_ok=True)
    (out / "inner").mkdir(exist_ok=True)
    victim = out / "victim-dir"
    victim.mkdir(exist_ok=True)
    built = {}
    for cell in cells:
        if cell in OUTER_CELLS:
            path = out / f"{cell}-{slug(cell)}.gpkg.tar"
            build_cell(cell, prefix, files, path)
        else:
            path = build_inner_cell(cell, prefix, files, out, victim)
        built[cell] = path
        print(f"built {path}  ({CELL_DESCRIPTIONS[cell]})")
    return built


# --------------------------------------------------------------- real reader


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


def tree_entries(root: Path) -> list:
    """What landed under `root`, without following symlinks; regular files
    carry size/blocks/xattrs so S4's sparse+xattr regression row has an
    exact expectation."""
    rows = []
    if not root.exists():
        return rows
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        for name in sorted(dirnames + filenames):
            p = Path(dirpath) / name
            st = p.lstat()
            row: dict = {
                "entry": str(p.relative_to(root)),
                "kind": file_kind(st),
                "mode": oct(stat.S_IMODE(st.st_mode)),
            }
            if stat.S_ISLNK(st.st_mode):
                row["target"] = os.readlink(p)
            if stat.S_ISREG(st.st_mode):
                row["size"] = st.st_size
                row["blocks"] = st.st_blocks
                try:
                    row["xattrs"] = sorted(os.listxattr(p))
                except OSError:
                    row["xattrs"] = []
            if stat.S_ISCHR(st.st_mode) or stat.S_ISBLK(st.st_mode):
                row["rdev"] = [os.major(st.st_rdev), os.minor(st.st_rdev)]
            rows.append(row)
    return rows


def probe_real(built: dict, out: Path) -> None:
    import portage
    from portage.gpkg import gpkg

    settings = getattr(portage, "config")(clone=getattr(portage, "settings", None))
    scratch = out / "real"
    scratch.mkdir(exist_ok=True)
    for cell, path in built.items():
        row: dict = {"cell": cell, "archive": str(path)}
        # `gpkg`'s second argument is the package basename (the prefix).
        with tarfile.open(path) as outer:
            first = outer.getnames()[0]
        basename = first.split("/", 1)[0]
        inst = None
        try:
            with timeout(20):
                inst = gpkg(settings, basename, str(path))
                inst._verify_binpkg()
            row["_verify_binpkg"] = "OK"
        except BaseException as exc:  # noqa: BLE001 -- the class *is* the result
            row["_verify_binpkg"] = f"{type(exc).__name__}: {exc}".replace("\n", " ")
            print(json.dumps(row))
            continue
        if cell in METADATA_CELLS:
            try:
                with timeout(20):
                    result = inst.get_metadata()
                if not isinstance(result, dict):
                    raise TypeError(f"get_metadata returned {type(result).__name__}")
                meta: dict = result
                row["get_metadata"] = f"OK ({len(meta)} keys)"
                row["keys"] = sorted(meta)
                for key in ("SLOT", "DESCRIPTION", "sub/KEY", "../KEY"):
                    if key in meta:
                        val: object = meta[key]
                        if isinstance(val, bytes):
                            val = val.decode("utf-8", "replace")
                        row[f"value[{key}]"] = str(val).strip()
            except BaseException as exc:  # noqa: BLE001
                row["get_metadata"] = f"{type(exc).__name__}: {exc}".replace("\n", " ")
            dest = scratch / f"{cell}-unpack"
            if dest.exists():
                shutil.rmtree(dest)
            dest.mkdir(parents=True)
            try:
                with timeout(20):
                    inst.unpack_metadata(dest_dir=str(dest))
                row["unpack_metadata"] = "OK"
            except BaseException as exc:  # noqa: BLE001
                row["unpack_metadata"] = f"{type(exc).__name__}: {exc}".replace("\n", " ")
            row["unpacked"] = tree_entries(dest)
        if cell in IMAGE_CELLS:
            dest = scratch / f"{cell}-decompress"
            if dest.exists():
                shutil.rmtree(dest)
            dest.mkdir(parents=True)
            try:
                with timeout(30):
                    inst.decompress(str(dest))
                row["decompress"] = "OK"
            except BaseException as exc:  # noqa: BLE001
                row["decompress"] = f"{type(exc).__name__}: {exc}".replace("\n", " ")
            row["decompressed"] = tree_entries(dest)
        print(json.dumps(row))


def probe_fixture_headers(built: dict, out: Path) -> None:
    """The committed fixture's inner header modes (feeds K2's mode rule)."""
    path = built.get("i0")
    if path is None:
        return
    _, files = read_valid_gpkg(path)
    for kind in ("metadata", "image"):
        inner = zstd_decompress(files[f"{kind}.tar.zst"])
        with tarfile.open(fileobj=io.BytesIO(inner), mode="r:") as tar:
            rows = [
                {"name": m.name, "type": m.type.decode(), "mode": oct(m.mode)}
                for m in tar.getmembers()
            ]
        print(json.dumps({"fixture_inner": kind, "members": rows}))


# ------------------------------------------------------ GNU tar extract probe


def timeout_run(argv, timeout_s=10):
    try:
        return subprocess.run(
            ["timeout", "-s", "KILL", str(timeout_s)] + argv,
            capture_output=True, text=True, check=False,
        )
    except subprocess.TimeoutExpired:  # pragma: no cover -- `timeout` handles it
        return None


def run_extract(argv, dest: Path, use_sudo: bool, out: Path) -> dict:
    if dest.exists():
        if use_sudo:
            subprocess.run(["sudo", "-n", "rm", "-rf", str(dest)], check=False)
        else:
            shutil.rmtree(dest)
    dest.mkdir(parents=True)
    proc = timeout_run((["sudo", "-n"] if use_sudo else []) + argv)
    if proc is None:
        return {"rc": "timeout", "extracted": []}
    return {
        "rc": proc.returncode,
        "stderr": proc.stderr.strip().replace("\n", " | "),
        "extracted": tree_entries(dest),
    }


def probe_extract(built: dict, out: Path, use_sudo: bool) -> None:
    root = out / "extracted"
    victim = out / "victim-dir"
    # Start from a clean victim: the real probe's i9 `unpack_metadata`
    # writes through the symlink into it, and this probe must attribute
    # only what *its* GNU-tar runs leave there.
    victim.mkdir(exist_ok=True)
    for entry in os.listdir(victim):
        p = victim / entry
        if p.is_dir():
            shutil.rmtree(p)
        else:
            p.unlink()
    for cell, path in built.items():
        if cell in OUTER_CELLS:
            row = {"cell": cell, "which": "outer"}
            row.update(run_extract(
                ["tar", "-xf", str(path), "-C", str(root / cell)],
                root / cell, use_sudo, out,
            ))
            print(json.dumps(row))
            continue
        # Inner cells: extract the rebuilt inner tar the way portuale
        # currently does. metadata read path: `tar -xf`; metadata/image
        # merge path: `tar -xpf --strip-components=1`.
        for kind in ("metadata", "image"):
            if cell not in (METADATA_CELLS if kind == "metadata" else IMAGE_CELLS):
                continue
            inner = out / "inner" / f"{cell}.{kind}.tar"
            if not inner.is_file():
                continue
            # portuale's current argv: read path `tar -xf <inner> -C dest`;
            # both merge paths `tar -xpf <inner> -C dest --strip-components=1`.
            for label, argv in (
                ("read", ["tar", "-xf", str(inner), "-C", str(root / f"{cell}-{kind}-read")]),
                ("merge", ["tar", "-xpf", str(inner), "-C", str(root / f"{cell}-{kind}-merge"),
                           "--strip-components=1"]),
            ):
                if label == "read" and kind == "image":
                    continue
                row = {"cell": cell, "which": f"inner-{kind}-{label}"}
                row.update(run_extract(
                    argv, root / f"{cell}-{kind}-{label}", use_sudo, out,
                ))
                print(json.dumps(row))
    # i9 write-through victim: report what landed there.
    victim = out / "victim-dir"
    print(json.dumps({"cell": "i9", "victim_dir": str(victim),
                      "victim_contents": tree_entries(victim)}))


# ------------------------------------------------------------------ main


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Backlog #56/#58 S0: build/probe crafted gpkg containers."
    )
    ap.add_argument("--src", type=Path, default=DEFAULT_SRC,
                    help="valid gpkg to derive the cells from")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--cells", default=",".join(DEFAULT_CELLS),
                    help=f"comma-separated cells (default {' '.join(DEFAULT_CELLS)})")
    ap.add_argument("--mode", choices=("build", "real", "extract", "all"),
                    default="build")
    ap.add_argument("--sudo", action="store_true",
                    help="run the extract probe's `tar` via `sudo -n` "
                         "(needed for the char-device cells' mknod)")
    args = ap.parse_args()

    cells = [c for c in args.cells.split(",") if c]
    unknown = [c for c in cells if c not in CELL_DESCRIPTIONS]
    if unknown:
        ap.error(f"unknown cells {unknown}")
    if args.sudo and "i18" in cells:
        ap.error("refusing --sudo with i18: its write-through targets /etc "
                 "(only the non-root permission failure makes the probe safe)")

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
        probe_fixture_headers(built, args.out)
        probe_real(built, args.out)
    if args.mode in ("extract", "all"):
        probe_extract(built, args.out, args.sudo)
    return 0


if __name__ == "__main__":
    sys.exit(main())
