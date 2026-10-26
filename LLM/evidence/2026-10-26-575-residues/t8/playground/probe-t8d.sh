#!/bin/bash
# Task 8 G0 counter-shape (scratch-only fixture, never added to pmtest):
# the healable p569pull residual plus a missing dependency whose parent CAN
# be re-picked (t8x-2 needs package.mask'd maskeddep, t8x-1 needs nothing).
# Question: does a masked-dep disclosure in a pass doom the dropped-pin heal
# (G0's premise), or only defer it to a later pass?
# Usage: WORK=<scratch dir> bash probe-t8d.sh
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=$HERE/../captures
WORK=${WORK:?}
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
PORTUALE_BIN=${PORTUALE_BIN:-/home/vivo/repo/PORTUALE/portuale/target/release/portuale}
rm -rf "$WORK"; mkdir -p "$WORK" "$OUT"
d=$WORK/g0x; cp -a "$SRC" "$d" 2>/dev/null
mk() { # mk <ver> <RDEPEND or empty>
  local eb=$d/repo/dev-libs/t8x/t8x-$1.ebuild; mkdir -p "${eb%/*}" "$d/repo/metadata/md5-cache/dev-libs"
  { echo 'EAPI=8'; echo 'DESCRIPTION="t8 scratch: G0 counter-shape"'; echo 'SLOT="0"'; echo 'KEYWORDS="amd64"'
    [ -n "$2" ] && echo "RDEPEND=\"$2\""; } > "$eb"
  { echo 'DEFINED_PHASES=-'; echo 'DESCRIPTION=t8 scratch: G0 counter-shape'; echo 'EAPI=8'; echo 'KEYWORDS=amd64'
    [ -n "$2" ] && echo "RDEPEND=$2"; echo 'SLOT=0'; echo "_md5_=$(md5sum "$eb" | cut -d' ' -f1)"; } > "$d/repo/metadata/md5-cache/dev-libs/t8x-$1"
}
mk 1 ""; mk 2 "dev-libs/maskeddep"
cfg=$d.cfg; mkdir -p "$cfg/etc"; cp -a "$d/etc/portage" "$cfg/etc/portage"
rm -f "$cfg/etc/portage/make.profile"; ln -s "$d/repo/profiles/default" "$cfg/etc/portage/make.profile"
{ echo "[DEFAULT]"; echo "main-repo = testrepo"; echo
  echo "[testrepo]"; echo "location = $d/repo"; echo
  echo "[overlay]"; echo "location = $d/overlay"; echo "priority = 10"; echo
  echo "[independentoverlay]"; echo "location = $d/independentoverlay"; echo "priority = 20"; echo "masters = overlay"; echo
  echo "[layoutmasteroverlay]"; echo "location = $d/layoutmasteroverlay"; echo "priority = 30"; echo
  echo "[repnamesection]"; echo "location = $d/repnamerepo"; echo "priority = 40"
} > "$cfg/etc/portage/repos.conf/repos.conf"
mkdir -p "$WORK/tmp"
BASE=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
for args in "dev-libs/p569pull dev-libs/t8x" "dev-libs/t8x"; do
  tag=$(echo "$args" | tr ' /' '_-')
  PORTAGE_CONFIGROOT=$cfg ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles PORTAGE_TMPDIR=$WORK/tmp PYTHONHASHSEED=0 \
    emerge "${BASE[@]}" $args > "$WORK/portage-$tag.raw" 2>&1; r1=$?
  PORTAGE_CONFIGROOT=$d ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles PORTAGE_TMPDIR=$WORK/tmp \
    "$PORTUALE_BIN" emerge "${BASE[@]}" $args > "$WORK/portuale-$tag.raw" 2>&1; r2=$?
  for pm in portage portuale; do
    r=$r1; [ $pm = portuale ] && r=$r2
    { echo "### $pm argv: PORTAGE_CONFIGROOT=<cfg> ROOT=\$W/g0x emerge ${BASE[*]} $args"; echo "### rc: $r"
      grep -vE "masters|layout.conf|repo_name|^\s*$|^!!! Set|^#|unique name|^WARNING: One or more repositories|^   ebuild|^\s+name" "$WORK/$pm-$tag.raw" | sed "s#$WORK#\$W#g" | head -40; } > "$OUT/g0x-$pm-$tag.txt"
  done
  echo "$args: portage rc $r1 $(grep -o 'backtrack: [0-9]*/[0-9]*' $WORK/portage-$tag.raw) | portuale rc $r2 $(grep -o 'backtrack: [0-9]*/[0-9]*' $WORK/portuale-$tag.raw)"
done
