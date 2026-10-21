#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first; EMERGE_DEFAULT_OPTS=--package-moves=n)
S=${S7:-/var/tmp/f458}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist PKGDIR=$S/pkgdir
EM=$1
run() { local name=$1; shift
  rm -rf $S/root $S/pkgdir; mkdir -p $S/root $S/dist $S/pkgdir
  echo "=== $name"
  out=$( "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; grep -v summary.log $S/stderr.txt
}
run "buildpkgonly" $EM --buildpkgonly dev-libs/schedparent
run "buildpkgonly-quiet-build-n" $EM --buildpkgonly --quiet-build=n dev-libs/schedparent
