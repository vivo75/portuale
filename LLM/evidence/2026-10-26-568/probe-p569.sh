#!/bin/bash
# #568/#569 S0 oracle probe: hermetic #569-shape fixture (`p569prov` 5.42/5.44
# + installed `p569virt-1` with a version-menu `||` + `prov:=` RDEPEND +
# installed `p569core-1.0` + merge-bound `p569pull-1`) under both managers.
#
# Shape mirrors the L0 `dev-perl_XML-Parser` probe: installed virtuals record
# `|| ( =perl-5.44* =perl-5.42* =perl-5.40* =perl-5.38* ~perl-core/... )`
# plus a built `perl:0/5.42=` slot-operator dep, while merge-bound pullers
# want `=perl-5.44*`/`=perl-5.42*`.
#
# Cells per manager (all `emerge -pv`, like the L0 probes):
#   p569pull   `dev-libs/p569pull`  (heal + skip block expected on both)
#   p569prov   `dev-libs/p569prov`  (control: no pinner in-graph)
#
# Config mirrors LLM/evidence/2026-10-25-563/probe-p563.sh (fixture ROOT,
# scratch Portage configroot with absolute repo paths).
#
# Usage: bash LLM/evidence/2026-10-26-568/probe-p569.sh
# Out:   LLM/evidence/2026-10-26-568/captures/<pm>-<cell>.{argv,rc,stdout,stderr}
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
  run "$pm" p569pull -- -pv "${BASE[@]}" dev-libs/p569pull
  run "$pm" p569prov -- -pv "${BASE[@]}" dev-libs/p569prov
done

echo "== rc =="; for f in "$OUT"/*.rc; do printf '%s: %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
