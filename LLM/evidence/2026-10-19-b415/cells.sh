#!/bin/bash
# usage: cells.sh <emerge-cmd...>   prints the #415 cells
S=${S7:-/var/tmp/b415}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
run() { local name=$1; shift
  rm -rf $S/dist; mkdir -p $S/dist
  echo "=== $name"
  out=$( env "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; cat $S/stderr.txt; echo "--- dist:"; ls $S/dist
}
EM=("$@")
run "restrict-pf" "${EM[@]}" -pf dev-util/fetchrestrict
run "restrict-f" "${EM[@]}" -f dev-util/fetchrestrict
run "cond-pf" "${EM[@]}" -pf dev-util/fetchcond
run "cond-pf-mirror" FEATURES="mirror" "${EM[@]}" -pf dev-util/fetchcond
run "cond-pF" "${EM[@]}" -pF dev-util/fetchcond
