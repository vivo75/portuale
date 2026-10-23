#!/usr/bin/env python3
"""Build gpkg-1 archives with hostile members (plan 02.442 S1, #500).

usage: build-hostile.py OUTDIR   -> OUTDIR/<shape>/hostile/<name>/<name>-1-1.gpkg.tar (each shape is its own PKGDIR)
Metadata base: pmtest fixtures/helpers/gpkg/metadata (CATEGORY=hostile,PF=<shape>-1 rewritten).
Uses the zstd CLI; Manifest = BLAKE2B+SHA512 like gpkg-helper.py.
"""
import hashlib, io, os, subprocess, sys, tarfile

META = "/home/vivo/repo/PORTUALE/pmtest/fixtures/helpers/gpkg/metadata"

def ti(name, typ=tarfile.REGTYPE, data=b"", link="", mode=0o644, maj=0, mnr=0):
    t = tarfile.TarInfo(name); t.type = typ; t.mode = mode; t.linkname = link
    t.devmajor, t.devminor = maj, mnr
    t.size = len(data) if typ == tarfile.REGTYPE else 0
    t.mtime = 1700000000
    return t, data

def mktar(members):
    b = io.BytesIO()
    with tarfile.open(mode="w", fileobj=b, format=tarfile.GNU_FORMAT) as tf:
        for t, d in members:
            tf.addfile(t, io.BytesIO(d) if t.type == tarfile.REGTYPE else None)
    return b.getvalue()

def zst(b): return subprocess.run(["zstd", "-q", "-c"], input=b, capture_output=True, check=True).stdout

def sums(b):
    return f"BLAKE2B {hashlib.blake2b(b).hexdigest()} SHA512 {hashlib.sha512(b).hexdigest()}"

def meta_members(name, rep=None, extra=()):
    ms = []
    for f in sorted(os.listdir(META)):
        data = open(os.path.join(META, f), "rb").read()
        if f == "PF": data = (name + "-1\n").encode()
        if f == "CATEGORY": data = b"hostile\n"
        if rep and f == rep[0]:
            ms.append(rep[1]("metadata/" + f)); continue
        ms.append(ti("metadata/" + f, data=data))
    ms += list(extra)
    return ms

D = tarfile.DIRTYPE; S = tarfile.SYMTYPE; L = tarfile.LNKTYPE
def img_ok(): return [ti("image", D, mode=0o755), ti("image/opt", D, mode=0o755), ti("image/opt/h", D, mode=0o755),
                      ti("image/opt/h/hello", data=b"hi\n")]

SHAPES = {}
def shape(n): 
    def d(f): SHAPES[n] = f; return f
    return d
# name -> (image members, metadata members fn rep/extra)
IMG = {
 "control-benign": img_ok() + [ti("image/opt/h/real", D, mode=0o755), ti("image/opt/h/bin", S, link="real"),
                               ti("image/opt/h/lnk", L, link="image/opt/h/hello"),
                               ti("image/opt/h/abs", S, link="/usr/lib64/libfoo.so.1")],
 "control-contained-symlink-parent": img_ok() + [ti("image/opt/h/real", D, mode=0o755), ti("image/opt/h/bin", S, link="real"),
                               ti("image/opt/h/bin/tool", data=b"t\n")],
 "img-dotdot-sub":   img_ok() + [ti("image/opt/..", D, mode=0o755)],
 "img-dotdot-bare":  img_ok() + [ti("..", D, mode=0o755)],
 "img-dotdot-leading": img_ok() + [ti("image/../outside", data=b"x")],
 "img-dotdot-dotslash": img_ok() + [ti("./../evil", data=b"x")],
 "img-abs-path":     img_ok() + [ti("/etc/evil", data=b"x")],
 "img-symlink-parent-abs": img_ok() + [ti("image/link", S, link="/var/tmp/outside-x"), ti("image/link/marker", data=b"x")],
 "img-symlink-parent-rel": img_ok() + [ti("image/link", S, link="../../../outside-x"), ti("image/link/marker", data=b"x")],
 "img-symlink-root":  img_ok() + [ti("image/x", S, link="/"), ti("image/x/etc/evil", data=b"x")],
 "img-hardlink-to-escaping-symlink-abs": img_ok() + [ti("image/link", S, link="/var/tmp/outside-x"), ti("image/hardlink", L, link="image/link")],
 "img-hardlink-relocated-rel": img_ok() + [ti("marker", data=b"in staging"), ti("image/deep/link", S, link="../../marker"), ti("image/hard", L, link="image/deep/link")],
 "img-hardlink-outside-archive": img_ok() + [ti("image/shadow", L, link="/etc/shadow")],
 "img-device":       img_ok() + [ti("image/hda", tarfile.BLKTYPE, maj=8)],
 "img-duplicate":    img_ok() + [ti("image/opt/h/hello", data=b"dup")],
 "img-dir-through-symlink": img_ok() + [ti("image/link", S, link="/var/tmp/outside-x"), ti("image/./link", D, mode=0o755)],
}
# metadata shapes: replace EAPI member / add extra
def mrep(kind):
    def f(n):
        if kind == "symlink": return ti(n, S, link="SLOT")
        if kind == "hardlink": return ti(n, L, link="metadata/SLOT")
        if kind == "chardev": return ti(n, tarfile.CHRTYPE, maj=1, mnr=3)
        if kind == "dir": return ti(n, D, mode=0o755)
        if kind == "symlink-abs": return ti(n, S, link="/etc/hostname")
    return f
META_SHAPES = {f"meta-{k}": k for k in ("symlink", "hardlink", "chardev", "dir", "symlink-abs")}
META_EXTRA = {"meta-extra-symlink": ti("metadata/EXTRA", S, link="SLOT"),
              "meta-extra-dir": ti("metadata/subdir", D, mode=0o755)}

def build(outdir, name):
    img = IMG.get(name, img_ok()); rep = None; extra = ()
    if name in META_SHAPES: rep = ("EAPI", mrep(META_SHAPES[name]))
    if name in META_EXTRA: extra = (META_EXTRA[name],)
    base = f"{name}-1"
    meta = zst(mktar(meta_members(name, rep, extra))); im = zst(mktar(img))
    parts = [("gpkg-1", b""), ("metadata.tar.zst", meta), ("image.tar.zst", im)]
    man = "".join(f"DATA {n} {len(d)} {sums(d)}\n" for n, d in parts).encode()
    d = os.path.join(outdir, name, "hostile", name); os.makedirs(d, exist_ok=True)
    p = os.path.join(d, base + "-1.gpkg.tar")  # multi-instance: cat/pn/pf-<BUILD_ID>.gpkg.tar
    with tarfile.open(p, "w", format=tarfile.USTAR_FORMAT) as tf:
        for n, data in parts + [("Manifest", man)]:
            t = tarfile.TarInfo(f"{base}/{n}"); t.size = len(data); t.mode = 0o644; t.mtime = 1700000000
            tf.addfile(t, io.BytesIO(data))
    return p

if __name__ == "__main__":
    out = sys.argv[1]
    names = sys.argv[2:] or list(IMG) + list(META_SHAPES) + list(META_EXTRA)
    for n in names: print(build(out, n))
