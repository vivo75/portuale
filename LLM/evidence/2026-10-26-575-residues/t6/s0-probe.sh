#!/bin/bash
# S0 probe for plan 02.589 Task 6 (#579): remote-binary `pkg_pretend` order.
# Builds a `file://` binhost serving the `binpkgpretendpkg-1.0.tbz2`
# fixture (DEFINED_PHASES includes `pretend`) and runs
# `emerge -k --getbinpkg --oneshot --nodeps dev-libs/binpkgpretendpkg`
# under Portage (`/usr/sbin/emerge`) and under portuale with the same
# effective env, saving both captures under t6/.
#
# Layout follows pmtest's `_getbinpkg_500_env` /
# `_clientetc_with_500_binhost` (test_portuale.py): a copied clientetc
# whose only binhost is ours, a fresh ROOT with the fixture vdb, an empty
# PKGDIR.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
FX=/home/vivo/repo/PORTUALE/pmtest/fixtures
WHICH="${1:?usage: s0-probe.sh <portage|portuale> [fail-pretend|bad-digest]}"
MODE="${2:-ok}"
WORK=/var/tmp/s0-t6-$WHICH-$MODE
rm -rf "$WORK"
mkdir -p "$WORK/binhost/dev-libs" "$WORK/root" "$WORK/pkgdir" "$WORK/portage-tmpdir"
cp -p "$FX/pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2" "$WORK/binhost/dev-libs/"
BYTES=$(stat -c%s "$WORK/binhost/dev-libs/binpkgpretendpkg-1.0.tbz2")
MD5=$(md5sum "$WORK/binhost/dev-libs/binpkgpretendpkg-1.0.tbz2" | cut -d' ' -f1)
SHA1=$(sha1sum "$WORK/binhost/dev-libs/binpkgpretendpkg-1.0.tbz2" | cut -d' ' -f1)
if [ "$MODE" = bad-digest ]; then SIZE=12345; else SIZE=$BYTES; fi
cat > "$WORK/binhost/Packages" <<EOF
TIMESTAMP: 0
VERSION: 0
PACKAGES: 1

CPV: dev-libs/binpkgpretendpkg-1.0
DEFINED_PHASES: install postinst preinst pretend setup
EAPI: 8
KEYWORDS: amd64
PATH: dev-libs/binpkgpretendpkg-1.0.tbz2
REPO: testrepo
SIZE: $SIZE
MD5: $MD5
SHA1: $SHA1
SLOT: 0
USE:

EOF
mkdir -p "$WORK/clientetc/etc"
cp -r "$FX/etc/portage" "$WORK/clientetc/etc/portage"
rm -f "$WORK/clientetc/etc/portage/binrepos.conf"
printf '[t6binhost]\npriority = 1\nsync-uri = file://%s\nverify-signature = false\nlocation = %s/cache\n' "$WORK/binhost" "$WORK" > "$WORK/clientetc/etc/portage/binrepos.conf"
ln -sfn "$FX/repo/profiles/default" "$WORK/clientetc/etc/portage/make.profile"
# Repoint relative repo locations at the fixture tree, like pmtest's
# `_clientetc_with_500_binhost`.
python3 - "$WORK/clientetc/etc/portage/repos.conf" "$FX" <<'EOF'
import re, sys
d = sys.argv[1]
fx = sys.argv[2]
for conf in __import__('pathlib').Path(d).iterdir():
    if not conf.is_file():
        continue
    out = []
    for line in conf.read_text().splitlines():
        s = line.strip()
        if s.startswith('location') and '=' in line:
            k, _, v = line.partition('=')
            v = v.strip()
            if v and not v.startswith('/'):
                line = f"{k}= {fx}/{v}"
        out.append(line)
    conf.write_text('\n'.join(out) + '\n')
EOF
cp -r "$FX/var" "$WORK/root/var"
printf '\nPKGDIR="%s"\nPORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nRPMDIR="%s"\n' "$WORK/pkgdir" "$WORK/portage-tmpdir" "$FX/distfiles" "$WORK/rpmdir" >> "$WORK/clientetc/etc/portage/make.conf"
mkdir -p "$WORK/rpmdir"
# With ROOT set, host Portage reads the target root's own config dir.
mkdir -p "$WORK/root/etc"
cp -r "$WORK/clientetc/etc/portage" "$WORK/root/etc/portage"
if [ "$MODE" = fail-pretend ]; then
  mkdir -p "$WORK/root/usr/share/binpkgpretendpkg"
  echo "already here" > "$WORK/root/usr/share/binpkgpretendpkg/payload.txt"
fi
export PORTAGE_CONFIGROOT="$WORK/clientetc"
export ROOT="$WORK/root"
if [ "$WHICH" = portage ]; then
  # Host Portage stacks EPREFIX under ROOT; portuale reads the override as
  # the root itself (pmtest convention). Only portuale gets the override.
  unset PORTAGE_OVERRIDE_EPREFIX
else
  export PORTAGE_OVERRIDE_EPREFIX="$WORK/root"
fi
export PORTAGE_RUNNING_ROOT="$FX"
export DISTDIR="$FX/distfiles"
export PORTAGE_TMPDIR="$WORK/portage-tmpdir"
export PKGDIR="$WORK/pkgdir"
export NOCOLOR=true
unset FEATURES
for proxy in http_proxy https_proxy HTTP_PROXY HTTPS_PROXY; do unset "$proxy"; done
if [ "$WHICH" = portage ]; then set -- /usr/sbin/emerge; else set -- /home/vivo/repo/PORTUALE/portuale/target/release/portuale emerge; fi
"$@" -k --getbinpkg --oneshot --nodeps dev-libs/binpkgpretendpkg >"$HERE/$WHICH-$MODE.stdout" 2>"$HERE/$WHICH-$MODE.stderr"
echo "rc=$?" | tee "$HERE/$WHICH-$MODE.rc"
echo "work left at $WORK"
