#!/bin/bash
# #491 S0 oracle probe: hermetic libplasma-shape fixture (`p491lib`/`p491con`/
# `p491keep`) under both managers.
#
# Shape: installed `p491lib-1` (`6/7`) + installed `p491con-1` (`:6`,
# `>=p491lib-1:6/7=`) + installed `p491keep-1` (no tree ebuild, pins
# `p491lib:6/7=`); the tree also carries `p491lib-2` (`6/8`) and
# `p491con-2` (`:6`, `>=p491lib-1:6`) -- mirrors installed
# libplasma-6.7.5/plasma-pa-6.7.5 vs the 6.8.1 ebuilds, with keeper
# merkuro-26.08.1-r1 ("cannot be rebuilt, ebuild unavailable").
#
# Cells per manager (all `-pv`, like the L0 probes):
#   upd      `-uDN =dev-libs/p491con-2` (live-pair mirror)
#   bare     `=dev-libs/p491con-2`      (explicit new parent, no update flags)
#   con-upd  `-uDN dev-libs/p491con`    (unpinned update)
#   top-upd  `-uDN =dev-libs/p491top-2` (update root pulling parent + keeper;
#                                        plays the live @world hermetically)
#   top-bare `-uDN dev-libs/p491top`    (unpinned update root)
#   v1       `-uDN =dev-libs/p491con-1 =dev-libs/p491lib-2 dev-libs/p491keep`
#                                       (pinned-installed consumer + pinned
#                                        new child + keeper: the live
#                                        libplasma:6 conflict family —
#                                        Portage rc 1, lib:6 kept by both
#                                        pins; vehicle: the `=con-1` arg
#                                        plays selective-keep)
#
# Config: both managers share ROOT=vdb=DISTDIR under `fixtures/` (the same
# tree the Rust pin resolves). Portage needs absolute repo locations, so it
# gets a scratch configroot that copies `fixtures/etc/portage` verbatim
# except `repos.conf` (absolute paths); portuale uses `fixtures` directly,
# like the pmtest contract harness.
#
# Usage: bash LLM/evidence/2026-10-25-491/probe-p491.sh
# Out:   LLM/evidence/2026-10-25-491/captures/<pm>-<cell>.{argv,rc,stdout,stderr}
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=$HERE/captures
WORK=$OUT/work
mkdir -p "$OUT" "$WORK"

PORTUALE_BIN=${PORTUALE_BIN:-$(cd "$HERE/../../.." && pwd)/target/release/portuale}
FIX=$(cd "$HERE/../../../fixtures" && pwd)

mkcfg() { # scratch Portage configroot: fixture config, absolute repo paths
  local cfg=$WORK/cfg-portage
  rm -rf "$cfg"; mkdir -p "$cfg/etc"
  cp -a "$FIX/etc/portage" "$cfg/etc/portage"
  # The fixture `make.profile` is relative (`../../repo/...`); re-point it at
  # the fixture profile absolutely so it resolves under the scratch root.
  rm -f "$cfg/etc/portage/make.profile"
  ln -s "$FIX/repo/profiles/default" "$cfg/etc/portage/make.profile"
  {
    echo "[DEFAULT]"
    echo "main-repo = testrepo"
    echo
    for spec in "testrepo repo" "overlay overlay" "independentoverlay independentoverlay" "layoutmasteroverlay layoutmasteroverlay"; do
      set -- $spec
      echo "[$1]"
      echo "location = $FIX/$2"
      case $1 in
        overlay) echo "priority = 10" ;;
        independentoverlay) echo "priority = 20"; echo "masters = overlay" ;;
        layoutmasteroverlay) echo "priority = 30" ;;
      esac
      echo
    done
    echo "[repnamesection]"
    echo "location = $FIX/repnamerepo"
    echo "priority = 40"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
  printf '%s\n' "$cfg"
}

CFG=$(mkcfg)

run() { # run <pm> <cell> -- <args...>
  local pm=$1 cell=$2; shift 2
  [ "${1:-}" = -- ] && shift
  local name="$pm-$cell"
  if [ "$pm" = portuale ]; then
    printf '%s\n' "portuale emerge $*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$FIX ROOT=$FIX PORTAGE_RUNNING_ROOT=$FIX DISTDIR=$FIX/distfiles \
      PORTAGE_TMPDIR=$WORK/tmp-ptl \
      "$PORTUALE_BIN" emerge "$@" >"$OUT/$name.stdout" 2>"$OUT/$name.stderr"
  else
    printf '%s\n' "emerge $*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$CFG ROOT=$FIX PORTAGE_RUNNING_ROOT=$FIX DISTDIR=$FIX/distfiles \
      PORTAGE_TMPDIR=$WORK/tmp-real \
      emerge "$@" >"$OUT/$name.stdout" 2>"$OUT/$name.stderr"
  fi
  printf '%s\n' "$?" > "$OUT/$name.rc"
}

mkdir -p "$WORK/tmp-ptl" "$WORK/tmp-real"
BASE=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n)
for pm in portage portuale; do
  run "$pm" upd -- -pv "${BASE[@]}" -uDN =dev-libs/p491con-2
  run "$pm" bare -- -pv "${BASE[@]}" =dev-libs/p491con-2
  run "$pm" con-upd -- -pv "${BASE[@]}" -uDN dev-libs/p491con
  run "$pm" top-upd -- -pv "${BASE[@]}" -uDN =dev-libs/p491top-2
  run "$pm" top-bare -- -pv "${BASE[@]}" -uDN dev-libs/p491top
  run "$pm" v1 -- -pv "${BASE[@]}" -uDN =dev-libs/p491con-1 =dev-libs/p491lib-2 dev-libs/p491keep
done

echo "== rc =="; for f in "$OUT"/*-*.rc; do printf '%s: %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
