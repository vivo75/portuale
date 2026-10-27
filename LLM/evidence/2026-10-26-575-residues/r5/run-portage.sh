#!/bin/bash
# Copied from the R5 S0 scratch (/var/tmp/claude-r5/run2.sh). CELL=<name> picks the scratch cell (default fx); PRE=<cmd> replaces /usr/bin/emerge.
# run.sh <name> <args...> : Portage 3.0.83 on scratch cell /var/tmp/claude-r5/fx (copy of pmtest/fixtures)
set -u
S=/var/tmp/claude-r5; SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
OUT=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-26-575-residues/r5/captures
d=$S/${CELL:-fx}; cfg=$S/${CELL:-fx}.cfg
if [ ! -d $d ]; then
  mkdir -p "$S/ptmp" "$cfg/etc"; cp -a "$SRC" "$d"
  cp -a "$SRC/etc/portage" "$cfg/etc/portage"; rm -f "$cfg/etc/portage/make.profile"
  ln -s "$d/repo/profiles/default" "$cfg/etc/portage/make.profile"
  { echo "[DEFAULT]"; echo "main-repo = testrepo"; echo; echo "[testrepo]"; echo "location = $d/repo"; echo
    echo "[overlay]"; echo "location = $d/overlay"; echo "priority = 10"; echo
    echo "[independentoverlay]"; echo "location = $d/independentoverlay"; echo "priority = 20"; echo "masters = overlay"; echo
    echo "[layoutmasteroverlay]"; echo "location = $d/layoutmasteroverlay"; echo "priority = 30"; echo
    echo "[repnamesection]"; echo "location = $d/repnamerepo"; echo "priority = 40"
  } > "$cfg/etc/portage/repos.conf/repos.conf"
fi
name=$1; shift
B=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
{ echo "### portage argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=\$S/${CELL:-fx}.cfg ROOT=\$S/${CELL:-fx} PYTHONHASHSEED=0 ${PRE:-/usr/bin/emerge} ${B[*]} $*"
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$cfg ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles \
    PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 ${PRE:-/usr/bin/emerge} "${B[@]}" "$@" 2>&1
  echo "### rc: $?"; } | sed -e "s#$S#\$S#g" -e 's/took [0-9.]* s/took N s/' | grep -vE "missing masters attribute|Set 'masters = testrepo'|^/\.gentoo/repos/gentoo/profiles/updates" > "$OUT/$name.txt"
echo "$name: $(grep -ho 'backtrack: [0-9/]*' $OUT/$name.txt | head -1) $(tail -1 $OUT/$name.txt)"
