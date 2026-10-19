#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first)
S=${S7:-/var/tmp/b423}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
EM=$1
run() { local name=$1; shift
  rm -rf $S/root; mkdir -p $S/root $S/dist
  echo "=== $name"
  out=$( "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; cat $S/stderr.txt
}
run "kg-bad-good-top" $EM --keep-going dev-util/bad dev-util/good dev-util/top
run "nokg-bad-good-top" $EM dev-util/bad dev-util/good dev-util/top
run "kg-bad-only" $EM --keep-going dev-util/bad
run "kg-bad-good-jobs" $EM --keep-going --jobs=2 dev-util/bad dev-util/good dev-util/top
