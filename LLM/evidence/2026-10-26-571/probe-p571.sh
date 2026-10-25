#!/bin/bash
# Task 12 (#571) S0 oracle probe: hermetic `repo-p571` + `overlay-p571`
# fixture under both managers.
#
# Shape: main repo `p571main` lists only `p571-listed` in
# `profiles/categories` but carries valid ebuilds in three category dirs:
# `p571-listed` (listed: control), `p571-hidden` (listed nowhere: masked
# everywhere), `p571-shared` (listed only by the overlay's
# `profiles/categories`: valid via the cross-repo union). A fourth
# package, `p571-listed/p571-need`, RDEPENDs on the hidden cp.
#
# Config: both managers share ROOT=vdb=DISTDIR under `fixtures/` (the same
# tree the Rust pin resolves) and the same scratch configroot, which
# copies `fixtures/etc/portage` verbatim except `repos.conf` (absolute
# paths, shared set plus the two p571 sections). Host runs are unfiltered
# (the host `gentoo` joins Portage's repo set, but every p571 name is
# unique, so no host cp contaminates the cells -- verified by the
# `*_shared` control cells); the `--regen` cell is scoped with
# `PORTAGE_REPOSITORIES` so Portage never walks the host tree.
#
# Usage: bash LLM/evidence/2026-10-26-571/probe-p571.sh
# Out:   LLM/evidence/2026-10-26-571/captures/<pm>-<cell>.{argv,rc,stdout,stderr}
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
    echo
    echo "[p571main]"
    echo "location = $FIX/repo-p571"
    echo "priority = 50"
    echo
    echo "[p571overlay]"
    echo "location = $FIX/overlay-p571"
    echo "priority = 60"
    echo "masters = p571main"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
  printf '%s\n' "$cfg"
}

CFG=$(mkcfg)

# Scoped repo set for the --regen cell: the shared set plus the p571 repos
# (absolute locations), so Portage never walks the host tree. Must include
# testrepo+overlay: the scratch make.profile's parent
# (`overlay:crossrepo-parent`) only resolves with those sections present.
# PORTAGE_REPOSITORIES replaces both repos.conf files (no host `gentoo`).
REPOS571="[DEFAULT]
main-repo = testrepo

[testrepo]
location = $FIX/repo

[overlay]
location = $FIX/overlay
priority = 10

[p571main]
location = $FIX/repo-p571
priority = 50

[p571overlay]
location = $FIX/overlay-p571
priority = 60
masters = p571main"

run() { # run <pm> <cell> -- <args...>
  local pm=$1 cell=$2; shift 2
  [ "${1:-}" = -- ] && shift
  local name="$pm-$cell"
  if [ "$pm" = portuale ]; then
    printf '%s\n' "portuale emerge $*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$CFG ROOT=$FIX PORTAGE_RUNNING_ROOT=$FIX DISTDIR=$FIX/distfiles \
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

run_regen() { # --regen, Portage scoped to the p571 repos via PORTAGE_REPOSITORIES
  local name="portage-regen-scoped"
  printf '%s\n' "PORTAGE_REPOSITORIES=<p571 sections> emerge --regen" > "$OUT/$name.argv"
  PORTAGE_CONFIGROOT=$CFG ROOT=$FIX PORTAGE_RUNNING_ROOT=$FIX DISTDIR=$FIX/distfiles \
    PORTAGE_TMPDIR=$WORK/tmp-real PORTAGE_REPOSITORIES="$REPOS571" \
    emerge --regen >"$OUT/$name.stdout" 2>"$OUT/$name.stderr"
  printf '%s\n' "$?" > "$OUT/$name.rc"
}

mkdir -p "$WORK/tmp-ptl" "$WORK/tmp-real"
BASE=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n)
for pm in portage portuale; do
  run "$pm" listed-direct -- -p "${BASE[@]}" p571-listed/p571-listed
  run "$pm" hidden-direct -- -p "${BASE[@]}" p571-hidden/p571-hidden
  run "$pm" shared-direct -- -p "${BASE[@]}" p571-shared/p571-shared
  run "$pm" hidden-short -- -p "${BASE[@]}" p571-hidden
  run "$pm" shared-short -- -p "${BASE[@]}" p571-shared
  run "$pm" need-hidden-dep -- -p "${BASE[@]}" p571-listed/p571-need
  run "$pm" search-hidden -- --search "${BASE[@]}" p571-hidden
  run "$pm" search-shared -- --search "${BASE[@]}" p571-shared
done
run_regen

echo "== rc =="; for f in "$OUT"/*.rc; do printf '%s: %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
