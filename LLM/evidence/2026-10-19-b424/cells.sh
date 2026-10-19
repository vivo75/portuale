#!/bin/bash
# usage: cells.sh <emerge>   (needs setup.sh first; serves $S/binhost on $PORT while it runs)
S=${S7:-/var/tmp/b424}
PORT=${PORT:-18424}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
EM=$1
export FETCHCOMMAND='wget -q -O "${DISTDIR}/${FILE}" "${URI}"' RESUMECOMMAND='wget -q -c -O "${DISTDIR}/${FILE}" "${URI}"'
python3 -m http.server $PORT --bind 127.0.0.1 --directory $S/binhost >$S/http.log 2>&1 &
SRV=$!
trap 'kill $SRV' EXIT
sleep 1
run() { local name=$1; shift
  echo "=== $name"
  out=$( "$@" </dev/null 2>$S/stderr.txt ); rc=$?
  echo "$out"; echo "rc=$rc"; echo "--- stderr"; cat $S/stderr.txt
  echo "--- pkgdir:"; (cd $S && find pkgdir binrepo -type f | sort)
}
rm -rf $S/pkgdir $S/binrepo; mkdir -p $S/pkgdir
run "pf-getbinpkg" $EM -pf --getbinpkg dev-libs/gpkgreadpkg
run "pfv-getbinpkgonly" $EM -pfv --getbinpkgonly dev-libs/gpkgreadpkg
run "f-getbinpkg" $EM -f --getbinpkg dev-libs/gpkgreadpkg
run "f-getbinpkg-again" $EM -f --getbinpkg dev-libs/gpkgreadpkg
run "f-getbinpkgonly" $EM -f --getbinpkgonly dev-libs/gpkgreadpkg
run "pf-getbinpkg-after" $EM -pf --getbinpkg dev-libs/gpkgreadpkg
echo "--- http.log"; sed 's/[0-9:.]* - - \[[^]]*\]//' $S/http.log
