#!/bin/bash
# usage: cells.sh <emerge>  (setup.sh first)
S=${S7:-/var/tmp/b428}; PORT=${PORT:-18428}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
EM=$1
python3 -m http.server $PORT --bind 127.0.0.1 --directory $S/binhost >$S/http.log 2>&1 &
SRV=$!; trap 'kill $SRV' EXIT; sleep 1
run() { local name=$1; shift
  echo "=== $name"; out=$( "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; cat $S/stderr.txt
  echo "--- files:"; (cd $S && find pkgdir binrepo var root/var/cache/binhost root/var/db/pkg -type f 2>/dev/null | sort | grep -v CONTENTS | head -20); [ -f $S/fc.log ] && { echo "--- fc.log"; cat $S/fc.log; }; }
rm -rf $S/pkgdir $S/binrepo $S/root/var/cache/binhost; mkdir -p $S/pkgdir
run "f-getbinpkg" $EM -f --getbinpkg dev-libs/gpkgreadpkg
rm -rf $S/pkgdir $S/binrepo $S/root/var/cache/binhost; mkdir -p $S/pkgdir
run "merge-getbinpkgonly" $EM --getbinpkgonly dev-libs/gpkgreadpkg
rm -rf $S/pkgdir $S/binrepo $S/root/var/cache/binhost $S/root/var/db; mkdir -p $S/pkgdir
export FETCHCOMMAND="$S/fc.sh \${DISTDIR} \${FILE} \${URI}"
run "f-fetchcommand" $EM -f --getbinpkg dev-libs/gpkgreadpkg
