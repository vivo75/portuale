#!/bin/bash
# S0b: the two cgp keeper cells, Portage --debug vs portuale PORTUALE_BT_TRACE=1.
# Scratch copy of pmtest/fixtures per cell, world += consumer (the contract's
# _world_extra_env), Portage gets an absolute repos.conf + make.profile.
# Usage: WORK=/var/tmp/claude-s0b OUT=<captures dir> bash probe-s0b-cgp.sh
set -u
WORK=${WORK:?}; OUT=${OUT:?}
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
PORTUALE_BIN=${PORTUALE_BIN:-/home/vivo/repo/PORTUALE/portuale/target/release/portuale}
mkdir -p "$WORK" "$OUT"
mkcell() { # mkcell <name> <world atom>
  local d=$WORK/$1; rm -rf "$d" "$d.cfg"; cp -a "$SRC" "$d"
  echo "$2" >> "$d/var/lib/portage/world"
  local cfg=$d.cfg; mkdir -p "$cfg/etc"; cp -a "$d/etc/portage" "$cfg/etc/portage"
  rm -f "$cfg/etc/portage/make.profile"; ln -s "$d/repo/profiles/default" "$cfg/etc/portage/make.profile"
  sed -E "s#^location = (.*)#location = $d/\1#" "$d/etc/portage/repos.conf/repos.conf" > "$cfg/etc/portage/repos.conf/repos.conf"
}
run() { # run <pm> <cell> <tag> <args...>
  local pm=$1 cell=$2 tag=$3; shift 3
  local d=$WORK/$cell name=$pm-$tag
  mkdir -p "$WORK/tmp"
  if [ "$pm" = portage ]; then
    printf 'PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/%s.cfg ROOT=$W/%s PORTAGE_RUNNING_ROOT=$W/%s DISTDIR=$W/%s/distfiles PORTAGE_TMPDIR=$W/tmp emerge %s\n' "$cell" "$cell" "$cell" "$cell" "$*" > "$OUT/$name.argv"
    PORTAGE_CONFIGROOT=$d.cfg ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles PORTAGE_TMPDIR=$WORK/tmp PYTHONHASHSEED=0 \
      emerge "$@" > "$WORK/$name.raw" 2>&1
  else
    printf 'PORTUALE_BT_TRACE=1 PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/%s ROOT=$W/%s PORTAGE_RUNNING_ROOT=$W/%s DISTDIR=$W/%s/distfiles PORTAGE_TMPDIR=$W/tmp portuale emerge %s\n' "$cell" "$cell" "$cell" "$cell" "$*" > "$OUT/$name.argv"
    PORTUALE_BT_TRACE=1 PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$d ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles PORTAGE_TMPDIR=$WORK/tmp \
      "$PORTUALE_BIN" emerge "$@" > "$WORK/$name.raw" 2>&1
  fi
  printf '%s\n' "$?" > "$OUT/$name.rc"
}
mkcell cgp0 dev-libs/cgp0q
mkcell cgp1 dev-libs/cgp1a
B=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n --pretend)
for pm in portage portuale; do
  dbg=(); [ $pm = portage ] && dbg=(--debug)
  run $pm cgp0 use "${B[@]}" "${dbg[@]}" dev-libs/cgp0x
  run $pm cgp1 ver-up "${B[@]}" "${dbg[@]}" '>=dev-libs/cgp1x-2'
  run $pm cgp1 ver-down "${B[@]}" "${dbg[@]}" '<dev-libs/cgp1x-1'
  # no-debug Portage for the plain output / count
  [ $pm = portage ] && { run $pm cgp0 use-nodebug "${B[@]}" dev-libs/cgp0x
    run $pm cgp1 ver-up-nodebug "${B[@]}" '>=dev-libs/cgp1x-2'
    run $pm cgp1 ver-down-nodebug "${B[@]}" '<dev-libs/cgp1x-1'; }
done
for f in "$OUT"/*.rc; do printf '%s: rc %s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
