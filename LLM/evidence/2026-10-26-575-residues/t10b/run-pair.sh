#!/bin/bash
# run-pair.sh <cell> <name> <args...> : Portage 3.0.83 (/usr/bin/emerge) and portuale on scratch cell
# /var/tmp/claude-10b/<cell> (a copy of pmtest/fixtures; <cell>.cfg is the Portage config root with
# absolute repos.conf). WORLD="a b" appends world atoms when the cell is made (mkcell <cell> [world...]).
# Output: captures/<name>-portage.txt and captures/<name>-portuale.txt (first line = argv, last = rc).
set -u
S=/var/tmp/claude-10b
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
BIN=${BIN:-/home/vivo/repo/PORTUALE/portuale/target/release/portuale}
mkcell() { # mkcell <cell> [world atoms...]
  local d=$S/$1 cfg=$S/$1.cfg; shift
  rm -rf "$d" "$cfg"; mkdir -p "$S/ptmp" "$cfg/etc"; cp -a "$SRC" "$d"
  for a in "$@"; do echo "$a" >> "$d/var/lib/portage/world"; done
  cp -a "$SRC/etc/portage" "$cfg/etc/portage"; rm -f "$cfg/etc/portage/make.profile"
  ln -s "$d/repo/profiles/default" "$cfg/etc/portage/make.profile"
  { echo "[DEFAULT]"; echo "main-repo = testrepo"; echo; echo "[testrepo]"; echo "location = $d/repo"; echo
    echo "[overlay]"; echo "location = $d/overlay"; echo "priority = 10"; echo
    echo "[independentoverlay]"; echo "location = $d/independentoverlay"; echo "priority = 20"; echo "masters = overlay"; echo
    echo "[layoutmasteroverlay]"; echo "location = $d/layoutmasteroverlay"; echo "priority = 30"; echo
    echo "[repnamesection]"; echo "location = $d/repnamerepo"; echo "priority = 40"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
}
runpair() { # runpair <cell> <name> <args...>
  local cell=$1 name=$2; shift 2
  local d=$S/$cell cfg=$S/$cell.cfg
  local B=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
  if [ -z "${ONLY_PORTUALE:-}" ]; then
  { echo "### portage argv: ${B[*]} $*"
    env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$cfg ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles \
      PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 /usr/bin/emerge "${B[@]}" "$@" 2>&1
    echo "### rc: $?"; } | sed -e "s#$S#\$S#g" -e 's/took [0-9.]* s/took N s/' > "$HERE/captures/$name-portage.txt"
  fi
  { echo "### portuale argv: emerge ${B[*]} $*"
    env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$d ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles \
      PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 PORTUALE_BT_TRACE=1 "$BIN" emerge "${B[@]}" "$@" 2>&1
    echo "### rc: $?"; } | sed -e "s#$S#\$S#g" -e 's/took [0-9.]* s/took N s/' > "$HERE/captures/$name-portuale${SUF:-}.txt"
  echo "$name: portage $(grep -ho 'backtrack: [0-9/]*' "$HERE/captures/$name-portage.txt" | head -1) $(tail -1 "$HERE/captures/$name-portage.txt") | portuale $(grep -ho 'backtrack: [0-9/]*' "$HERE/captures/$name-portuale${SUF:-}.txt" | head -1) $(tail -1 "$HERE/captures/$name-portuale${SUF:-}.txt")"
}
