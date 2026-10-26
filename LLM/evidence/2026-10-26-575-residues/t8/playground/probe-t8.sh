#!/bin/bash
# Task 8 oracle traces: host Portage 3.0.83 `emerge --debug` on the pin
# fixtures. Read-only on pmtest: the whole fixture tree is COPIED to
# $WORK/fix first and every run uses the copy as ROOT and repo source, so
# Portage's global updates and vdb reads never touch pmtest.
# Usage: WORK=<scratch dir> bash probe-t8.sh
# Out:   ../captures/<cell>.{argv,rc,stdout,stderr}
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=$HERE/../captures
WORK=${WORK:?set WORK to a scratch dir}
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
rm -rf "$WORK"; mkdir -p "$WORK" "$OUT"
cp -a "$SRC" "$WORK/fix"
FIX=$WORK/fix

mkcfg() {
  local cfg=$WORK/cfg
  rm -rf "$cfg"; mkdir -p "$cfg/etc"
  cp -a "$FIX/etc/portage" "$cfg/etc/portage"
  rm -f "$cfg/etc/portage/make.profile"
  ln -s "$FIX/repo/profiles/default" "$cfg/etc/portage/make.profile"
  {
    echo "[DEFAULT]"; echo "main-repo = testrepo"; echo
    for spec in "testrepo repo" "overlay overlay" "independentoverlay independentoverlay" "layoutmasteroverlay layoutmasteroverlay"; do
      set -- $spec
      echo "[$1]"; echo "location = $FIX/$2"
      case $1 in
        overlay) echo "priority = 10" ;;
        independentoverlay) echo "priority = 20"; echo "masters = overlay" ;;
        layoutmasteroverlay) echo "priority = 30" ;;
      esac
      echo
    done
    echo "[repnamesection]"; echo "location = $FIX/repnamerepo"; echo "priority = 40"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
  printf '%s\n' "$cfg"
}
CFG=$(mkcfg)

vdb() { # vdb <root> <cat> <pf> <slot> [KEY=VALUE...]
  local root=$1 cat=$2 pf=$3 slot=$4; shift 4
  local d=$root/var/db/pkg/$cat/$pf
  mkdir -p "$d"
  echo "$cat" > "$d/CATEGORY"; echo "$slot" > "$d/SLOT"; echo testrepo > "$d/repository"
  local kv; for kv in "$@"; do printf '%s\n' "${kv#*=}" > "$d/${kv%%=*}"; done
}

run() { # run <cell> <root> -- <args...>
  local cell=$1 root=$2; shift 2; [ "${1:-}" = -- ] && shift
  printf 'PORTAGE_CONFIGROOT=%s ROOT=%s emerge %s\n' "$CFG" "$root" "$*" > "$OUT/$cell.argv"
  mkdir -p "$WORK/tmp"
  PORTAGE_CONFIGROOT=$CFG ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$FIX/distfiles \
    PORTAGE_TMPDIR=$WORK/tmp PYTHONHASHSEED=0 \
    emerge "$@" >"$OUT/$cell.stdout" 2>"$OUT/$cell.stderr"
  printf '%s\n' "$?" > "$OUT/$cell.rc"
}

BASE=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv --debug)

# 1. #563 heal (dropped_pin_mask_restart, all gates open)
run p563-heal "$FIX" -- "${BASE[@]}" dev-libs/p563pull
# 2. #569 heal with the p569virt || menu (or_branch_vdb_deselected_set, scan site)
run p569-heal "$FIX" -- "${BASE[@]}" dev-libs/p569pull
# 3. #570 G0 combo
run g0-combo "$FIX" -- "${BASE[@]}" dev-libs/p569pull dev-libs/abort-masked-mid

# 4. #54 K3 keeper (G1): keeper world-listed in a ROOT copy
K3=$WORK/k3; cp -a "$FIX" "$K3"; echo dev-libs/keeper >> "$K3/var/lib/portage/world"
run k3-paired-arg "$K3" -- "${BASE[@]}" =dev-libs/paired-2.0
run k3-needer "$K3" -- "${BASE[@]}" dev-libs/needer

# 5. #223 igw contrast (G1)
IGW=$WORK/igw; cp -a "$FIX" "$IGW"; echo dev-libs/igw0a >> "$IGW/var/lib/portage/world"
run igw-plain "$IGW" -- "${BASE[@]}" '>=dev-libs/igw0x-2'

# 6. #35 R4 or-choice, bug 531656 (G2 keeper): ad-hoc root as the contract pin
R4=$WORK/r4; mkdir -p "$R4/var/lib/portage"
printf 'dev-libs/mlocaml\ndev-libs/mllablgl\n' > "$R4/var/lib/portage/world"
LDEP='>=dev-libs/mlocaml-3.10.2:0/4.01.0= || ( dev-libs/mllabltk:= <dev-libs/mlocaml-4.02 )'
vdb "$R4" dev-libs mlocaml-4.01.0 0/4.01.0 EAPI=5
vdb "$R4" dev-libs mllablgl-1.05 0 EAPI=5 "DEPEND=$LDEP" "RDEPEND=$LDEP"
run r4-orchoice "$R4" -- "${BASE[@]}" --update --deep @world
run r4-orchoice-bt0 "$R4" -- "${BASE[@]}" --update --deep --backtrack=0 @world

# 7. #575 Task 16 / pmtest 6bdb765: installed parent, slot-op atom in a || branch
P491=$WORK/p491or; mkdir -p "$P491/var/lib/portage"
printf 'dev-libs/p491orcon\ndev-libs/p491keep\n' > "$P491/var/lib/portage/world"
vdb "$P491" dev-libs p491lib-1 6/7 EAPI=8
vdb "$P491" dev-libs p491orcon-1 6 EAPI=8 'RDEPEND=|| ( =dev-libs/p491lib-2:6/7= dev-libs/p491oralt )'
vdb "$P491" dev-libs p491oralt-1 0 EAPI=8
vdb "$P491" dev-libs p491keep-1 6 EAPI=8 'RDEPEND=dev-libs/p491lib:6/7='
run p491or-world "$P491" -- "${BASE[@]}" --update --deep --backtrack=4 @world

for f in "$OUT"/*.rc; do printf '%s: rc %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
