#!/bin/bash
# #563 S0 oracle probe: hermetic perl-shape fixture (`p563prov`/`p563con`)
# under both managers, update-style AND explicit args.
#
# Shape: installed `p563prov-5.42` (`0/5.42`) + installed `p563con-1` pinning
# the old subslot; the tree also carries `p563prov-5.44` (`0/5.44`) --
# mirrors `dev-lang/perl-5.44.0` vs the installed 5.42-era reverse deps
# (L0 `logs/l0-20261025T084209Z`: `dev-lang_perl`, `dev-perl_XML-Parser`).
#
# Cells per manager (all `emerge -pv`, like the L0 probes):
#   pull-bare    `dev-libs/p563pull`      (the `dev-perl_XML-Parser` mirror:
#                                          Portage heals with the skip-block,
#                                          rc 0; portuale aborts rc 1)
#   prov-bare    `dev-libs/p563prov`      (control: no pinner in-graph, both
#                                          managers upgrade clean, rc 0)
#   pull-oldver  `=dev-libs/p563pull-1`   (pinned-clean control: old puller +
#                                          old pins, both merge clean, rc 0)
#   pull-newver  `=dev-libs/p563pull-2`   (Task 2: pinned-newver explicit arg.
#                                          The new puller is unmaskable (it is
#                                          the argument), so the conflict with
#                                          the installed pinner cannot heal;
#                                          grounds the explicit-args report.)
#
# Config: both managers share ROOT=vdb=DISTDIR under `fixtures/` (the same
# tree the Rust pin resolves). Portage needs absolute repo locations, so it
# gets a scratch configroot that copies `fixtures/etc/portage` verbatim
# except `repos.conf` (absolute paths); portuale uses `fixtures` directly,
# like the pmtest contract harness.
#
# Usage: bash LLM/evidence/2026-10-25-563/probe-p563.sh
# Out:   LLM/evidence/2026-10-25-563/captures/<pm>-<cell>.{argv,rc,stdout,stderr}
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
  run "$pm" pull-bare -- -pv "${BASE[@]}" dev-libs/p563pull
  run "$pm" prov-bare -- -pv "${BASE[@]}" dev-libs/p563prov
  run "$pm" pull-oldver -- -pv "${BASE[@]}" =dev-libs/p563pull-1
  run "$pm" pull-newver -- -pv "${BASE[@]}" =dev-libs/p563pull-2
done

echo "== rc =="; for f in "$OUT"/*.rc; do printf '%s: %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
