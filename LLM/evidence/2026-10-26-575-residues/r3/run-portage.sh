#!/bin/bash
# run-portage.sh <name> <args...> : Portage on scratch cell $S/fx (copy of pmtest/fixtures + mk.sh shapes)
set -u
S=/var/tmp/claude-r3; OUT=$S/r3/captures
d=$S/${CELL:-fx}; cfg=$S/${CELL:-fx}.cfg
name=$1; shift
B=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
{ echo "### portage argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=\$S/${CELL:-fx}.cfg ROOT=\$S/${CELL:-fx} PORTAGE_RUNNING_ROOT=\$S/${CELL:-fx} DISTDIR=\$S/${CELL:-fx}/distfiles PORTAGE_TMPDIR=\$S/ptmp PYTHONHASHSEED=0 /usr/bin/emerge ${B[*]} $*"
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$cfg ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles \
    PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 /usr/bin/emerge "${B[@]}" "$@" 2>&1
  echo "### rc: $?"; } | sed -e "s#$S#\$S#g" -e 's/took [0-9.]* s/took N s/' | grep -vE "missing masters attribute|Set 'masters = testrepo'|^/\.gentoo/repos/gentoo/profiles/updates" > "$OUT/$name.txt"
echo "$name: $(grep -ho 'backtrack: [0-9/]*' $OUT/$name.txt | head -1) $(tail -1 $OUT/$name.txt) lines=$(wc -l < $OUT/$name.txt)"
