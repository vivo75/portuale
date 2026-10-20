#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first; EMERGE_DEFAULT_OPTS=--package-moves=n)
S=${S7:-/var/tmp/b437}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
EM=$1
run() { local name=$1; shift
  rm -rf $S/root; mkdir -p $S/root $S/dist
  echo "=== $name"
  out=$( "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; grep -v summary.log $S/stderr.txt
}
run "ok-world-order" $EM dev-util/good
run "q-lone-failure" $EM -q dev-util/bad dev-util/good
run "q-quiet-fail" $EM -q --quiet-fail dev-util/bad dev-util/good
run "q-keep-going" $EM -q --keep-going dev-util/bad dev-util/good dev-util/top
run "jobs-lone-failure" $EM --jobs=2 dev-util/bad dev-util/good
run "jobs-keep-going" $EM --keep-going --jobs=2 dev-util/bad dev-util/good dev-util/top
