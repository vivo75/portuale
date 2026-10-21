#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first; EMERGE_DEFAULT_OPTS=--package-moves=n)
S=${S7:-/var/tmp/b456}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
EM=$1
run() { local name=$1; shift
  rm -rf $S/root; mkdir -p $S/root $S/dist
  echo "=== $name"
  out=$( "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; grep -v summary.log $S/stderr.txt
}
run "default" $EM dev-libs/schedparent
run "quiet-build-n" $EM --quiet-build=n dev-libs/schedparent
run "quiet-build-y" $EM --quiet-build=y dev-libs/schedparent
run "jobs2" $EM --jobs=2 dev-libs/schedparent
run "quiet-build-n-jobs2" $EM --quiet-build=n --jobs=2 dev-libs/schedparent
run "verbose" $EM --verbose dev-libs/schedparent
run "quiet" $EM --quiet dev-libs/schedparent
