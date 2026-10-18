#!/bin/bash
# usage: cells.sh <emerge-cmd...>   prints cell outputs
S=${S7:-/var/tmp/s7-oracle}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
okcmd="cp $S/src/probe-1.tar.gz \"\${DISTDIR}/\${FILE}\""
run() { # name, env-assignments, args
  local name=$1; shift
  rm -rf $S/dist; mkdir -p $S/dist
  echo "=== $name"
  out=$( env FETCHCOMMAND="$okcmd" RESUMECOMMAND="$okcmd" "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; cat $S/stderr.txt
  echo "--- dist:"; ls $S/dist | grep -v '^\.'
}
run "pf" "$@" -pf dev-util/fetchprobe
run "pF" "$@" -pF dev-util/fetchprobe
run "pfv" "$@" -pfv dev-util/fetchprobe
run "f" "$@" -f dev-util/fetchprobe
run "F" "$@" -F dev-util/fetchprobe
rm -rf $S/dist; mkdir -p $S/dist; echo "=== f-fail"; out=$(env FETCHCOMMAND="/bin/false \${FILE}" RESUMECOMMAND="/bin/false \${FILE}" "$@" -f dev-util/fetchprobe </dev/null 2>$S/stderr.txt); rc=$?; echo "$out"; echo "rc=$rc"; echo "--- stderr"; cat $S/stderr.txt
