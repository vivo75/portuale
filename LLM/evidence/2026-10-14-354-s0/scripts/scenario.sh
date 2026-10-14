#!/bin/bash
# scenario.sh <root-name> <mode> [extra FEATURES]
#  mode=src : T354_V=1 emerge =t354-re-1 ; mods ; T354_V=2 emerge =t354-re-1  (same-version source reinstall)
#  mode=up  : T354_V=1 emerge =t354-re-1 ; mods ; T354_V=2 emerge =t354-re-2  (version upgrade control)
#  mode=bin : emerge -K =t354-re-1 from pkg1 ; mods ; emerge -K =t354-re-1 from pkg2 (needs pkg1/pkg2 from a src run)
set -eu
RN=$1; MODE=$2; XF=${3:-}
S=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-14-354-s0/scripts
rm -rf /var/tmp/t354/roots/$RN /var/tmp/t354/scratch/$RN
EM="emerge --ignore-default-opts -v -1"
case $MODE in
src) $S/run.sh $RN-1 $RN T354_V=1 "FEATURES=buildpkg $XF" PKGDIR=/var/tmp/t354/pkg1 -- $EM =app-misc/t354-re-1 ;;
up)  $S/run.sh $RN-1 $RN T354_V=1 "FEATURES=$XF" PKGDIR=/var/tmp/t354/pkgX -- $EM =app-misc/t354-re-1 ;;
bin) $S/run.sh $RN-1 $RN "FEATURES=$XF" PKGDIR=/var/tmp/t354/pkg1 -- $EM --usepkgonly =app-misc/t354-re-1 ;;
esac
$S/mods.sh $RN
$S/snap.sh $RN $RN-before
case $MODE in
src) $S/run.sh $RN-2 $RN T354_V=2 "FEATURES=buildpkg $XF" PKGDIR=/var/tmp/t354/pkg2 -- $EM =app-misc/t354-re-1 ;;
up)  $S/run.sh $RN-2 $RN T354_V=2 "FEATURES=$XF" PKGDIR=/var/tmp/t354/pkgX -- $EM =app-misc/t354-re-2 ;;
bin) $S/run.sh $RN-2 $RN "FEATURES=$XF" PKGDIR=/var/tmp/t354/pkg2 -- $EM --usepkgonly =app-misc/t354-re-1 ;;
esac
$S/snap.sh $RN $RN-after
diff $S/../cells/$RN-before.snap $S/../cells/$RN-after.snap > $S/../cells/$RN.snapdiff || true
