#!/bin/bash
# bhrun.sh portage|portuale PKG -- remote binhost (python http.server on 127.0.0.1:18766, request log) ; empty local PKGDIR
pm=$1 p=$2; B=/var/tmp/ev500
rm -rf $B/binhost-cache-$pm-$p $B/root-bh-$pm-$p $B/pt-bh-$pm-$p $B/pkgdir-bh-$pm-$p $B/cfg-bh-$pm-$p; mkdir -p $B/root-bh-$pm-$p $B/pt-bh-$pm-$p $B/pkgdir-bh-$pm-$p
cp -a $B/cfg $B/cfg-bh-$pm-$p; mkdir -p $B/cfg-bh-$pm-$p/etc/portage/binrepos.conf
cat > $B/cfg-bh-$pm-$p/etc/portage/binrepos.conf/bh.conf <<C
[bh]
sync-uri = http://127.0.0.1:18766/
priority = 10
verify-signature = false
location = $B/binhost-cache-$pm-$p
C
( cd $B/bh && exec python3 -m http.server 18766 --bind 127.0.0.1 ) >$B/http-$pm-$p.log 2>&1 &
sp=$!; sleep 1
export ROOT=$B/root-bh-$pm-$p PORTAGE_CONFIGROOT=$B/cfg-bh-$pm-$p PKGDIR=$B/pkgdir-bh-$pm-$p PORTAGE_TMPDIR=$B/pt-bh-$pm-$p
if [ $pm = portage ]; then EM=/usr/bin/emerge; else EM=$B/bin/emerge; fi
$EM -1 --getbinpkgonly --nodeps --color=n hostile/$p; echo "rc=$?"
kill $sp; wait $sp 2>/dev/null
echo "--- server requests"; grep -ao '"[A-Z]* [^ ]* HTTP[^"]*" [0-9]*' $B/http-$pm-$p.log
echo "--- installed"; ls $ROOT/var/db/pkg/hostile 2>&1; ls $ROOT/opt/h 2>&1 | head -3
echo "--- fetched file"; find $PKGDIR -name '*.gpkg.tar' 2>/dev/null
