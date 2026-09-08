#!/usr/bin/env python3
"""Normalise a snapshot.sh manifest + VDB tar in place for diff.py.

    normalize.py <out-prefix>

Reads  <prefix>.files.tsv  and  <prefix>.vdb.tar
Writes <prefix>.files.norm.tsv  and  <prefix>.vdb/  (unpacked + normalised)

Applies the ruleset in TEST/compare/normalize.md -- the code is the
authority, keep the prose in sync. stdlib only.
"""
from __future__ import annotations

import bz2
import re
import sys
import tarfile
from pathlib import Path

# --- filesystem: regenerated caches -> compare presence, not bytes ------
# a path whose sha256 is blanked to "-" before the diff (still a MISSING
# finding if it is absent on one side, but never a CONTENT finding).
PRESENCE_ONLY = [
    re.compile(r"^/etc/ld\.so\.cache$"),
    re.compile(r"\.py[co]$"),
    re.compile(r"/__pycache__/"),
    re.compile(r"^/usr/share/info/dir$"),
    re.compile(r"^/usr/share/mime/"),
    re.compile(r"/(icon-theme\.cache|gtk-update-icon-cache)"),
    re.compile(r"^/usr/lib\d*/gio/modules/giomodule\.cache$"),
    re.compile(r"/fonts\.(dir|scale)$"),
    re.compile(r"^/etc/ssl/certs/"),          # ca-certificates hash symlinks + bundle
    re.compile(r"^/var/lib/portage/config$"), # CONFIG_PROTECT hash db (key set only)
    re.compile(r"^/var/cache/"),
    re.compile(r"/\.keep(_[^/]*)?$"),
    re.compile(r"^/etc/\.(pwd\.lock|updated)$"),
    re.compile(r"^/usr/share/applications/mimeinfo\.cache$"),
    re.compile(r"^/etc/environment\.d/"),
]

# volatile env-file lines (env_update output) -- these files legitimately
# differ in ordering / a timestamp; blank the whole sha and let the
# presence check carry them.
ENV_FILES = {"/etc/profile.env", "/etc/csh.env", "/etc/environment"}


def norm_files(prefix: Path) -> None:
    src = prefix.with_suffix(".files.tsv")
    if not src.exists():
        (prefix.parent / (prefix.name + ".files.norm.tsv")).write_text("")
        return
    out = []
    for line in src.read_text().splitlines():
        parts = line.split("\t")
        if len(parts) != 9:
            continue
        path, typ, mode, uid, gid, size, sha, link, xattr = parts
        if path in ENV_FILES or any(rx.search(path) for rx in PRESENCE_ONLY):
            sha = "-"
        # a directory's st_size is filesystem-internal (hash-tree/block
        # allocation) -- not meaningful, and it drifts even between two
        # dirs with an identical entry set.
        if typ == "d":
            size = "-"
        out.append("\t".join([path, typ, mode, uid, gid, size, sha, link, xattr]))
    out.sort()
    (prefix.parent / (prefix.name + ".files.norm.tsv")).write_text("\n".join(out) + "\n")


# --- VDB --------------------------------------------------------------
BLANK_FILES = {"BUILD_TIME", "BUILD_ID", "COUNTER", "INSTALL_TIME"}
SORT_FILES = {"NEEDED", "NEEDED.ELF.2", "REQUIRES", "PROVIDES"}
# the consolidated `metadata` file (`#format=1` then KEY=value): blank
# the same volatile keys inline.
META_BLANK = re.compile(r"^(BUILD_TIME|BUILD_ID|COUNTER|INSTALL_TIME)=.*$", re.M)


def norm_metadata(text: str) -> str:
    text = META_BLANK.sub(lambda m: m.group(1) + "=<normalised>", text)
    # the md5-cache-style `#dir_mtime=<nanoseconds>` header is the vdb
    # dir's own mtime at write time -- pure noise.
    text = re.sub(r"^#dir_mtime=.*$", "#dir_mtime=<x>", text, flags=re.M)
    return "\n".join(sorted(text.splitlines())) + "\n"

# saved-env lines to drop outright (`declare -x KEY=…` or bare `KEY=…`):
# volatile bash internals, plus the locale vars (LANG/LC_*): the two
# consumer containers are invoked with different locale env (`consume.sh`
# exports `LC_ALL`; the portage side ends up with a bare `LANG`), and the
# regenerated binpkg env keeps whatever the phase inherited -- a
# test-harness difference, not a portuale bug.
# FEATURES / PORTAGE_FEATURES are NOT dropped -- L1-c's PORTAGE_UPDATE_ENV
# regeneration makes them match real. Nor are EMERGE_DEFAULT_OPTS /
# PORTAGE_RUNNING_ROOT / O -- L1-f's phase-env whitelist keeps them out.
ENV_DROP = re.compile(
    r"^(declare (-[-x]+ )?)?"
    r"(SRANDOM|EPOCHREALTIME|EPOCHSECONDS|SECONDS|BASHPID|PPID|BUILD_TIME|BUILD_ID|"
    r"HOSTNAME|SANDBOX_PID|PORTAGE_PID|PORTAGE_IPC_KEY|"
    r"COLUMNS|LINES|RANDOM|"
    r"LANG|LC_[A-Z]+)="
)
ENV_MASK = re.compile(
    r"^((?:declare (?:-[-x]+ )?)?(T|WORKDIR|PORTAGE_BUILDDIR|HOME|PWD|OLDPWD|"
    r"PORTAGE_LOG_FILE|EMERGE_FROM|MERGE_TYPE))=.*$"
)
BUILDDIR = re.compile(r"/var/tmp/portage/[^ \t\"']+")


def norm_environment(raw: bytes) -> str:
    try:
        text = bz2.decompress(raw).decode("utf-8", "replace")
    except OSError:
        text = raw.decode("utf-8", "replace")
    kept = []
    for ln in text.splitlines():
        if ENV_DROP.match(ln):
            continue
        ln = BUILDDIR.sub("<builddir>", ln)
        ln = ENV_MASK.sub(r"\1=<x>", ln)
        kept.append(ln)
    kept.sort()
    return "\n".join(kept) + "\n"


def norm_contents(text: str) -> str:
    out = []
    for ln in text.splitlines():
        f = ln.split()
        if not f:
            continue
        if f[0] == "obj" and len(f) >= 4:
            # obj <path> <md5> <mtime>  -- blank mtime, keep md5
            out.append(f"obj {' '.join(f[1:-1])} <mtime>")
        elif f[0] == "sym" and len(f) >= 4:
            # sym <path> -> <target> <mtime>
            out.append(f"sym {' '.join(f[1:-1])} <mtime>")
        else:
            out.append(ln.rstrip())
    out.sort()
    return "\n".join(out) + "\n"


def norm_vdb(prefix: Path) -> None:
    tar = prefix.with_suffix(".vdb.tar")
    dest = prefix.parent / (prefix.name + ".vdb")
    if dest.exists():
        import shutil

        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    if not tar.exists():
        return
    with tarfile.open(tar) as tf:
        tf.extractall(dest, filter="data")
    pkgroot = dest / "pkg"
    if not pkgroot.is_dir():
        return
    for entry in sorted(pkgroot.glob("*/*")):
        if not entry.is_dir():
            continue
        for f in sorted(entry.iterdir()):
            if not f.is_file():
                continue
            name = f.name
            if name in BLANK_FILES:
                f.write_text("<normalised>\n")
            elif name in SORT_FILES:
                f.write_text("\n".join(sorted(f.read_text().splitlines())) + "\n")
            elif name == "environment.bz2":
                (entry / "environment").write_text(norm_environment(f.read_bytes()))
                f.unlink()
            elif name == "environment":
                f.write_text(norm_environment(f.read_bytes()))
            elif name == "CONTENTS":
                f.write_text(norm_contents(f.read_text()))
            elif name == "metadata":
                f.write_text(norm_metadata(f.read_text()))
            elif name == "repository":
                f.write_text(f.read_text().strip() + "\n")


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print(__doc__)
        return 2
    prefix = Path(argv[0])
    norm_files(prefix)
    norm_vdb(prefix)
    print(f"normalised {prefix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
