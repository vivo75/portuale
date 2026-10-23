#!/bin/bash
# runpm.sh portage|portuale SHAPE  -- hermetic: scratch ROOT, CONFIGROOT, PKGDIR, PORTAGE_TMPDIR
pm=$1 s=$2; B=/var/tmp/ev500
rm -rf $B/root-$pm-$s $B/pt-$pm-$s; mkdir -p $B/root-$pm-$s $B/pt-$pm-$s
export ROOT=$B/root-$pm-$s PORTAGE_CONFIGROOT=$B/cfg PKGDIR=$B/arch-$pm/$s PORTAGE_TMPDIR=$B/pt-$pm-$s
rm -rf $PKGDIR; mkdir -p $(dirname $PKGDIR); cp -a $B/arch/$s $PKGDIR
if [ $pm = portage ]; then EM=/usr/bin/emerge; else EM=$B/bin/emerge; fi
$EM -1 --usepkgonly --nodeps --color=n hostile/$s; rc=$?
echo "rc=$rc"
echo "--- root contents"; (cd $ROOT && find . -mindepth 1 -not -path "./var/*" -not -path "./tmp*" -not -path "./etc/*" | sort | head -40; ls ./var/db/pkg/hostile 2>&1)
echo "--- outside leak check"; ls -A /var/tmp/outside-x | wc -l | sed "s/^/files in outside-x: /"; ls /etc/evil 2>&1 | head -1
