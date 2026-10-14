#!/bin/bash
# scenario-kd.sh <eapi>: S0.9, `ebuild t354-kd-<eapi>.ebuild clean merge` into ROOT K<eapi>;
# records the image tree (left in PORTAGE_TMPDIR after merge), ROOT tree and CONTENTS.
set -eu
E=$1; RN=K$E
S=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-14-354-s0/scripts
rm -rf /var/tmp/t354/roots/$RN /var/tmp/t354/scratch/$RN
$S/run.sh $RN-install $RN PORTAGE_VERBOSE=1 -- ebuild /var/tmp/t354/repo/app-misc/t354-kd/t354-kd-$E.ebuild clean install
IMG=/var/tmp/t354/scratch/$RN/tmp/portage/app-misc/t354-kd-$E/image
IMGLIST=$( cd $IMG && find . | LC_ALL=C sort )
$S/run.sh $RN-qmerge $RN PORTAGE_VERBOSE=1 -- ebuild /var/tmp/t354/repo/app-misc/t354-kd/t354-kd-$E.ebuild qmerge
{ echo "## image $IMG (after install, before qmerge)"; echo "$IMGLIST"
  echo "## ROOT /var/tmp/t354/roots/$RN (t354kd paths)"; (cd /var/tmp/t354/roots/$RN && find . -path '*t354kd*' | LC_ALL=C sort)
  echo "## CONTENTS"; cat /var/tmp/t354/roots/$RN/var/db/pkg/app-misc/t354-kd-$E/CONTENTS
} > $S/../cells/$RN.tree
