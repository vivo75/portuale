#!/bin/bash
# #538 overlap probe: two different packages whose binpkg files share a
# basename in different PKGDIR subdirs, with equal SIZE and mtime. Portage
# keys its vouch map by (dirname, basename) (`bintree._populate_local`),
# so each file vouches only against its own dir's stanza; a basename-only
# key lets one stanza vouch for the other's file.
#
# Layout: app-misc/dup-1.0.tbz2 and dev-libs/dup-1.0.tbz2 (same SIZE via
# padding, same mtime), one index stanza for the app-misc file only.
# Usage: probe-overlap538.sh <portage|portuale> <outdir>
set -u
pm=$1; out=$(readlink -f "$2"); here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev538-pkgdir
rm -rf $T; mkdir -p $T/cfg/etc/portage/repos.conf $T/profile $T/pkgdir/app-misc $T/pkgdir/dev-libs $T/root/var/lib/portage
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $T/profile/parent
ln -sfn $T/profile $T/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n' "$FX" > $T/cfg/etc/portage/repos.conf/r.conf
printf 'PKGDIR="%s/pkgdir"\nFEATURES="-news -pkgdir-index-trusted"\nACCEPT_LICENSE="*"\n' "$T" > $T/cfg/etc/portage/make.conf
cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $T/cfg/etc/portage/ 2>/dev/null
python3 - "$T/pkgdir" <<'EOF'
import bz2, io, os, struct, sys, tarfile
pkgdir = sys.argv[1]
be32 = lambda n: struct.pack(">I", n)

def make_xpak(cat, pf):
    # Members the scanners read: CATEGORY/PF/SLOT (+ a fixed-size payload
    # so both archives share SIZE).
    members = {"CATEGORY": cat + "\n", "PF": pf + "\n", "SLOT": "0\n", "KEYWORDS": "amd64\n"}
    index, data = b"", b""
    for k, v in members.items():
        kb, vb = k.encode(), v.encode()
        index += be32(len(kb)) + kb + be32(len(data)) + be32(len(vb))
        data += vb
    seg = b"XPAKPACK" + be32(len(index)) + be32(len(data)) + index + data + b"XPAKSTOP"
    img = io.BytesIO()
    with tarfile.open(fileobj=img, mode="w") as tf:
        ti = tarfile.TarInfo("payload")
        blob = b"x" * 512
        ti.size = len(blob)
        tf.addfile(ti, io.BytesIO(blob))
    # Layout both readers accept: bz2(tar) + segment (XPAKPACK..XPAKSTOP
    # inclusive) + be32(len(segment)) + STOP.
    return bz2.compress(img.getvalue()) + seg + be32(len(seg)) + b"STOP"

for cat in ("app-misc", "dev-libs"):
    raw = make_xpak(cat, "dup-1.0")
    with open(os.path.join(pkgdir, cat, "dup-1.0.tbz2"), "wb") as f:
        f.write(raw)
# Equal SIZE is what makes the basename-only key collide; equal mtime too.
a = os.path.join(pkgdir, "app-misc/dup-1.0.tbz2")
b = os.path.join(pkgdir, "dev-libs/dup-1.0.tbz2")
sa, sb = os.path.getsize(a), os.path.getsize(b)
assert sa == sb, (sa, sb)
os.utime(a, (1700000000, 1700000000))
os.utime(b, (1700000000, 1700000000))
EOF
size=$(stat -c %s $T/pkgdir/app-misc/dup-1.0.tbz2)
# Full minimum-keys stanza (Portage writes every key as `KEY: value`, empty
# ones as `KEY: ` with the space): only then does the fast path consider
# it, which is what the basename-only key gets wrong.
python3 - "$T/pkgdir" "$size" <<PYEOF
import os, sys
pkgdir, size = sys.argv[1], sys.argv[2]
keys = ["BDEPEND", "BUILD_ID", "BUILD_TIME", "CHOST", "CPV", "DEFINED_PHASES",
        "DEPEND", "EAPI", "IDEPEND", "IUSE", "KEYWORDS", "LICENSE", "PDEPEND",
        "PROPERTIES", "PROVIDES", "RDEPEND", "repository", "REQUIRES", "RESTRICT", "SIZE", "SLOT",
        "USE", "PATH", "_mtime_"]
vals = {"BUILD_ID": "1", "BUILD_TIME": "1000", "CPV": "app-misc/dup-1.0",
        "DEFINED_PHASES": "-", "EAPI": "8", "KEYWORDS": "amd64", "SLOT": "0",
        "SIZE": size, "PATH": "app-misc/dup-1.0.tbz2", "_mtime_": "1700000000"}
with open(os.path.join(pkgdir, "Packages"), "w") as f:
    f.write("VERSION: 0\nTIMESTAMP: 0\nPACKAGES: 1\n\n")
    for k in keys:
        f.write("%s: %s\n" % (k, vals.get(k, "")))
    f.write("\nDESCRIPTION: backlog #538 overlap probe\n")
PYEOF
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
mkdir -p $T/root
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev511-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev511-bin/emerge; bin=/var/tmp/ev511-bin/emerge; fi
mkdir -p "$out"
for id in dup-app dup-dev; do
  case $id in dup-app) atom=app-misc/dup;; dup-dev) atom=dev-libs/dup;; esac
  (cd $T && env $bin -p --color=n --usepkgonly $atom >"$out/ov538-$id.$pm.out" 2>"$out/ov538-$id.$pm.err"; echo $? >"$out/ov538-$id.$pm.rc")
  (cd $T && asciinema rec --headless --overwrite -q -c "env $bin -p --color=n --usepkgonly $atom" "$out/ov538-$id.$pm.cast" >/dev/null 2>&1)
done
