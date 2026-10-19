#!/bin/bash
# #424 oracle root: the #415 root plus a served binhost (fixture gpkgreadpkg-1.0.gpkg.tar) and an empty PKGDIR.
S=${S7:-/var/tmp/b424}
PORT=${PORT:-18424}
HERE=$(cd "$(dirname "$0")" && pwd)
FIX=$(cd "$HERE/../../../../pmtest/fixtures" && pwd)
S7=$S bash "$HERE/../2026-10-19-b415/setup.sh"
printf "dev-util\ndev-libs\n" > $S/repo/profiles/categories
mkdir -p $S/binhost/dev-libs $S/pkgdir
cp $FIX/pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar $S/binhost/dev-libs/
size=$(stat -c %s $S/binhost/dev-libs/gpkgreadpkg-1.0.gpkg.tar)
printf 'TIMESTAMP: 1\nVERSION: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/gpkgreadpkg-1.0\nDEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nMTIME: 1000\nBUILD_TIME: 1000\nPATH: dev-libs/gpkgreadpkg-1.0.gpkg.tar\nREPO: fetchrepo\nSIZE: %s\nSLOT: 0\nUSE:\n' $size > $S/binhost/Packages
printf '[tmpbinhost]\nsync-uri = http://127.0.0.1:%s\npriority = 1\nlocation = %s/binrepo\n' $PORT $S > $S/etc/portage/binrepos.conf
printf 'PKGDIR="%s/pkgdir"\nPORTAGE_TRUST_HELPER="/bin/true"\nFEATURES="${FEATURES} binpkg-ignore-signature -binpkg-request-signature -binpkg-signing"\n' $S >> $S/etc/portage/make.conf
