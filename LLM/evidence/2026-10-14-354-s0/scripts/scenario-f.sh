#!/bin/bash
# scenario-f.sh <root-name> [extra FEATURES]: cell f, preserve-libs on a same-version reinstall.
# lib-1 v1 ships libt354a.so.1 + libt354b.so.1; cons-1 NEEDs both; lib-1 v2 drops libt354b.so.1.
set -eu
RN=$1; XF=${2:-}
S=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-14-354-s0/scripts
rm -rf /var/tmp/t354/roots/$RN /var/tmp/t354/scratch/$RN
# A real system gets /usr/lib64 into ld.so.conf from env.d LDPATH; without it
# portage.util.getlibpaths only searches /usr/lib and /lib and the linkmap sees no consumer.
mkdir -p /var/tmp/t354/roots/$RN/etc/env.d
echo 'LDPATH="/usr/lib64"' > /var/tmp/t354/roots/$RN/etc/env.d/50t354
echo /usr/lib64 > /var/tmp/t354/roots/$RN/etc/ld.so.conf
EM="emerge --ignore-default-opts -v -1"
$S/run.sh $RN-1 $RN T354_V=1 "FEATURES=$XF" PKGDIR=/var/tmp/t354/pkgF -- $EM =app-misc/t354-lib-1
$S/run.sh $RN-1c $RN "FEATURES=$XF" PKGDIR=/var/tmp/t354/pkgF -- $EM =app-misc/t354-cons-1
$S/snap.sh $RN $RN-before
$S/run.sh $RN-2 $RN T354_V=2 "FEATURES=$XF" PKGDIR=/var/tmp/t354/pkgF -- $EM =app-misc/t354-lib-1
$S/snap.sh $RN $RN-after
diff $S/../cells/$RN-before.snap $S/../cells/$RN-after.snap > $S/../cells/$RN.snapdiff || true
