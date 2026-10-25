#!/bin/bash
# #570 S0 oracle probe: healable dropped-pin residual (p569pull) + benign
# masked disclosure (abort-masked-mid, an unrelated cp whose dep
# dev-libs/maskeddep is package.mask'd) on one probe, under both managers.
#
# Cells per manager (all `emerge -pv`, like the L0 probes):
#   g0combo  `dev-libs/p569pull dev-libs/abort-masked-mid` (the G0 cell:
#            healable residual + masked disclosure — which abort first?)
#   g0heal   `dev-libs/p569pull` (control: heals, rc 0, skip block)
#   g0mask   `dev-libs/abort-masked-mid` (control: masked abort, rc 1)
#
# Config mirrors LLM/evidence/2026-10-26-568/probe-p569.sh (fixture ROOT,
# scratch Portage configroot with absolute repo paths). Pretend-only: no
# vdb writes, so the shared fixtures tree needs no restore.
#
# Usage: bash LLM/evidence/2026-10-25-570/probe-g0.sh
# Out:   LLM/evidence/2026-10-25-570/captures/<pm>-<cell>.{argv,rc,stdout,stderr}
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
  run "$pm" g0combo -- -pv "${BASE[@]}" dev-libs/p569pull dev-libs/abort-masked-mid
  run "$pm" g0heal -- -pv "${BASE[@]}" dev-libs/p569pull
  run "$pm" g0mask -- -pv "${BASE[@]}" dev-libs/abort-masked-mid
done

echo "== rc =="; for f in "$OUT"/*.rc; do printf '%s: %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
