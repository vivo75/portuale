#!/bin/bash
# usage: cells.sh <emerge>   (needs ../2026-10-21-r463/setup.sh first; EMERGE_DEFAULT_OPTS=--package-moves=n)
# Seconds are cut; the similar-names candidates come from the host tree on Portage's side, so
# only their structure is compared.
EM=$1
S7=${S7:-/var/tmp/r468e}; S459=${S459:-/var/tmp/r468f}
norm() { sed -E 's/took [0-9.]+ s/took/;s/took \.?/took/;s|/root/|/root|g;s/line 18[0-9]/line N/;s/(Maybe you meant).*/\1 .../'; }
r458() { export PORTAGE_CONFIGROOT=$S7 PORTAGE_TMPDIR=$S7/tmp ROOT=$S7/root DISTDIR=$S7/dist PKGDIR=$S7/pkgdir EMERGE_DEFAULT_OPTS=--package-moves=n
         rm -rf $S7/root; mkdir -p $S7/root/var/cache/edb; }
cell() { local n=$1; shift; echo "=== $n"; "$@" </dev/null 2>&1 | norm; echo "rc=${PIPESTATUS[0]}"; }
r458
echo "{\"resume\": {\"mergelist\": [[\"ebuild\", \"$S7/root/\", \"dev-libs/schedleaf-a-1.0\", \"merge\"],[\"ebuild\", \"$S7/root/\", \"dev-libs/schedleaf-b-1.0\", \"merge\"]], \"favorites\": [], \"myopts\": {}}, \"starttime\": 0}" > $S7/root/var/cache/edb/mtimedb
cell "471 -pq --resume" $EM -pq --resume
r458
cell "469 -pv (no distfiles)" $EM -pv dev-libs/schedparent
for a in "-p nosuch/pkg" "-p nosuch" "-pq nosuch" "-p --nodeps nosuch" "nosuch"; do r458; cell "470 $a" $EM $a; done
export PORTAGE_CONFIGROOT=$S459 ROOT=$S459/root DISTDIR=$S459/dist PKGDIR=$S459/pkgdir; unset PORTAGE_TMPDIR; mv $S459/etc/portage/package.env $S459/pe.keep 2>/dev/null
for q in "" "-q"; do rm -rf $S459/root $S459/tmp/portage; mkdir -p $S459/root; cell "468 --usepkgonly $q" $EM $q --usepkgonly dev-util/f459-postinstfail; done
mv $S459/pe.keep $S459/etc/portage/package.env 2>/dev/null
