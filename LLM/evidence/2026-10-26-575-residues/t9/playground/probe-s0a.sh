#!/bin/bash
# Task 9 S0a: which `||` view an installed consumer gets, by --deep and complete mode.
# Builds scratch cells (copies of pmtest/fixtures, as t8/playground/probe-t8c.sh and
# probe-t8.sh), runs host Portage through views.py, writes captures/s0a-<run>.txt.
# Usage: WORK=<scratch dir> bash probe-s0a.sh
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=$HERE/../captures
WORK=${WORK:?}
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
rm -rf "$WORK"; mkdir -p "$WORK/tmp" "$OUT"

mkcfg() { # mkcfg <root>
  local d=$1 cfg=$1.cfg; mkdir -p "$cfg/etc"; cp -a "$SRC/etc/portage" "$cfg/etc/portage"
  rm -f "$cfg/etc/portage/make.profile"; ln -s "$2/repo/profiles/default" "$cfg/etc/portage/make.profile"
  { echo "[DEFAULT]"; echo "main-repo = testrepo"; echo
    echo "[testrepo]"; echo "location = $2/repo"; echo
    echo "[overlay]"; echo "location = $2/overlay"; echo "priority = 10"; echo
    echo "[independentoverlay]"; echo "location = $2/independentoverlay"; echo "priority = 20"; echo "masters = overlay"; echo
    echo "[layoutmasteroverlay]"; echo "location = $2/layoutmasteroverlay"; echo "priority = 30"; echo
    echo "[repnamesection]"; echo "location = $2/repnamerepo"; echo "priority = 40"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
}
mkcell() { # same edit as probe-t8c.sh
  local d=$WORK/$1; cp -a "$SRC" "$d" 2>/dev/null
  local eb=$d/repo/dev-libs/p569virt/p569virt-1.ebuild
  sed -i "s#^RDEPEND=.*#RDEPEND=\"$2\"#" "$eb"
  local md5; md5=$(md5sum "$eb" | cut -d' ' -f1)
  local mc=$d/repo/metadata/md5-cache/dev-libs/p569virt-1
  sed -i -e "s#^RDEPEND=.*#RDEPEND=$2#" -e "s#^_md5_=.*#_md5_=$md5#" "$mc"
  printf '%s\n' "$3" > "$d/var/db/pkg/dev-libs/p569virt-1/RDEPEND"
  mkcfg "$d" "$d"
}
vdb() { local root=$1 cat=$2 pf=$3 slot=$4; shift 4; local d=$root/var/db/pkg/$cat/$pf; mkdir -p "$d"
  echo "$cat" > "$d/CATEGORY"; echo "$slot" > "$d/SLOT"; echo testrepo > "$d/repository"
  local kv; for kv in "$@"; do printf '%s\n' "${kv#*=}" > "$d/${kv%%=*}"; done; }

run() { # run <name> <cell> <cp-regex> <args...>
  local name=$1 cell=$2 cp=$3; shift 3
  local d=$WORK/$cell repo=$WORK/$cell; [ -d "$d/repo" ] || repo=$WORK/fix
  { echo "### argv: PORTAGE_CONFIGROOT=\$W/$cell.cfg ROOT=\$W/$cell PYTHONHASHSEED=0 T9_CP='$cp' python3 t9/playground/views.py $*"
    PORTAGE_CONFIGROOT=$d.cfg ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$repo/distfiles PORTAGE_TMPDIR=$WORK/tmp \
      PYTHONHASHSEED=0 T9_CP=$cp python3 "$HERE/views.py" "$@" 2>&1
    echo "### rc: $?"; } | grep -E '^###|^T9 |^\[ebuild|backtrack:|skipped due|triggered by|^!!! [^RS]' | sed -e "s#$WORK#\$W#g" -e "s/depth=<object[^>]*>/depth=UNREACHABLE/" | cut -c1-700 > "$OUT/s0a-$name.txt"
}

SW='|| ( =dev-libs/p569prov-5.40* =dev-libs/p569prov-5.42* =dev-libs/p569prov-5.38* ~dev-libs/p569core-1.0 )'
OR='|| ( =dev-libs/p569prov-5.42* =dev-libs/p569prov-5.40* =dev-libs/p569prov-5.38* ~dev-libs/p569core-1.0 )'
mkcell swap "$SW" "$SW"
mkcell orig "$OR" "$OR"
# R4 ad-hoc root (probe-t8.sh section 6); repo = a fixture copy
cp -a "$SRC" "$WORK/fix"
R4=$WORK/r4; mkdir -p "$R4/var/lib/portage"; mkcfg "$R4" "$WORK/fix"
printf 'dev-libs/mlocaml\ndev-libs/mllablgl\n' > "$R4/var/lib/portage/world"
LDEP='>=dev-libs/mlocaml-3.10.2:0/4.01.0= || ( dev-libs/mllabltk:= <dev-libs/mlocaml-4.02 )'
vdb "$R4" dev-libs mlocaml-4.01.0 0/4.01.0 EAPI=5
vdb "$R4" dev-libs mllablgl-1.05 0 EAPI=5 "DEPEND=$LDEP" "RDEPEND=$LDEP"

B=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
run t1-nodeep      swap p569virt "${B[@]}" dev-libs/p569pull
run t1-deep        swap p569virt "${B[@]}" --deep dev-libs/p569pull
run t1-deep1       swap p569virt "${B[@]}" --deep=1 dev-libs/p569pull
run t1-deep2       swap p569virt "${B[@]}" --deep=2 dev-libs/p569pull
run t1-nocomplete  swap p569virt "${B[@]}" --complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n dev-libs/p569pull
run t1-cg          swap p569virt "${B[@]}" --complete-graph dev-libs/p569pull
run orig-nodeep    orig p569virt "${B[@]}" dev-libs/p569pull
run orig-deep      orig p569virt "${B[@]}" --deep dev-libs/p569pull
run r4-deep        r4   mllablgl "${B[@]}" --update --deep @world
run r4-nodeep      r4   mllablgl "${B[@]}" --update @world
# consumer not forward-reachable from the arguments: only _complete_graph's set args reach it
mkcell swapw "$SW" "$SW"; echo dev-libs/p569virt >> "$WORK/swapw/var/lib/portage/world"
run revdep-deep-inworld  swapw p569virt "${B[@]}" --deep =dev-libs/p569prov-5.44
run revdep-deep-notworld swap  p569virt "${B[@]}" --deep =dev-libs/p569prov-5.44
run r4-deep-atomarg      r4    mllablgl "${B[@]}" --update --deep dev-libs/mlocaml
for f in "$OUT"/s0a-*.txt; do echo "== $(basename "$f")"; grep -E 'T9 OR|backtrack:|### rc' "$f" | cut -c1-260; done
