#!/bin/bash
# Task 9 S0c/S0d (adapted from t8/playground/probe-t8c.sh): keeps raw --debug
# dumps in $WORK and the backtracking try / runtime_pkg_mask / to-mask lines.
# Task 8 discriminator, dynamic-deps-proof: the p569virt `||` menu is
# rewritten consistently in the tree ebuild, its md5-cache entry (real md5)
# and the vdb RDEPEND of a COPY of the fixture tree, then both managers run
# the same argv. Question: which `||` arm does Portage keep as the installed
# consumer's parent atom when the installed-matching arm is not first?
# Usage: WORK=<scratch dir> bash probe-t8c.sh
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=${OUT:-$HERE/../captures}
WORK=${WORK:?}
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
PORTUALE_BIN=${PORTUALE_BIN:-/home/vivo/repo/PORTUALE/portuale/target/release/portuale}
rm -rf "$WORK"; mkdir -p "$WORK" "$OUT"

mkcell() { # mkcell <name> <RDEPEND for ebuild+cache> <RDEPEND for vdb>
  local d=$WORK/$1; cp -a "$SRC" "$d" 2>/dev/null
  local eb=$d/repo/dev-libs/p569virt/p569virt-1.ebuild
  sed -i "s#^RDEPEND=.*#RDEPEND=\"$2\"#" "$eb"
  local md5; md5=$(md5sum "$eb" | cut -d' ' -f1)
  local mc=$d/repo/metadata/md5-cache/dev-libs/p569virt-1
  sed -i -e "s#^RDEPEND=.*#RDEPEND=$2#" -e "s#^_md5_=.*#_md5_=$md5#" "$mc"
  printf '%s\n' "$3" > "$d/var/db/pkg/dev-libs/p569virt-1/RDEPEND"
  local cfg=$d.cfg; mkdir -p "$cfg/etc"; cp -a "$d/etc/portage" "$cfg/etc/portage"
  rm -f "$cfg/etc/portage/make.profile"; ln -s "$d/repo/profiles/default" "$cfg/etc/portage/make.profile"
  { echo "[DEFAULT]"; echo "main-repo = testrepo"; echo
    echo "[testrepo]"; echo "location = $d/repo"; echo
    echo "[overlay]"; echo "location = $d/overlay"; echo "priority = 10"; echo
    echo "[independentoverlay]"; echo "location = $d/independentoverlay"; echo "priority = 20"; echo "masters = overlay"; echo
    echo "[layoutmasteroverlay]"; echo "location = $d/layoutmasteroverlay"; echo "priority = 30"; echo
    echo "[repnamesection]"; echo "location = $d/repnamerepo"; echo "priority = 40"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
}

run() { # run <pm> <cell> <args...>
  local pm=$1 cell=$2; shift 2
  local d=$WORK/$cell name=$pm-c-$cell
  mkdir -p "$WORK/tmp"
  if [ "$pm" = portage ]; then
    printf 'PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/%s.cfg ROOT=$W/%s emerge %s\n' "$cell" "$cell" "$*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$d.cfg ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles PORTAGE_TMPDIR=$WORK/tmp PYTHONHASHSEED=0 \
      emerge "$@" > "$WORK/$name.raw" 2>&1
  else
    printf 'PORTUALE_BT_TRACE=1 PORTAGE_CONFIGROOT=$W/%s ROOT=$W/%s portuale emerge %s\n' "$cell" "$cell" "$*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$d ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles PORTAGE_TMPDIR=$WORK/tmp \
      PORTUALE_BT_TRACE=1 PYTHONHASHSEED=0 "$PORTUALE_BIN" emerge "$@" > "$WORK/$name.raw" 2>&1
  fi
  printf '%s\n' "$?" > "$OUT/$name.rc"
  { echo "### argv: $(cat "$OUT/$name.argv")"; echo "### rc: $(cat "$OUT/$name.rc")"
    grep -nE -A1 "backtracking try|runtime_pkg_mask|package\(s\) to mask|backtracking loop|backtracking aborted|^Parent: .*p569virt|^Candidates: \['=dev-libs/p569prov|backtracking due|^\[ebuild|^Total:|backtrack:|skipped due|required by \(dev-libs/p569virt|triggered|slot conflict:" "$WORK/$name.raw" \
      | grep -vE "^[0-9]+-(Priority|$)|^--$" | sed "s#$WORK#\$W#g" | cut -c1-600; } > "$OUT/$name.trim.txt"
}

SW='|| ( =dev-libs/p569prov-5.40* =dev-libs/p569prov-5.42* =dev-libs/p569prov-5.38* ~dev-libs/p569core-1.0 )'
mkcell swap-slotop "$SW dev-libs/p569prov:=" "$SW dev-libs/p569prov:0/5.42="
OR='|| ( =dev-libs/p569prov-5.42* =dev-libs/p569prov-5.40* =dev-libs/p569prov-5.38* ~dev-libs/p569core-1.0 )'
mkcell orig-noslotop "$OR" "$OR"
BASE=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
for cell in swap-slotop orig-noslotop; do
  run portage "$cell" "${BASE[@]}" --debug dev-libs/p569pull
  run portuale "$cell" "${BASE[@]}" dev-libs/p569pull
done
for f in "$OUT"/*-c-*.rc; do printf '%s: rc %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
