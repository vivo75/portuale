#!/bin/bash
set -u
bash /stage.sh /tmp/fxstage > /tmp/stage.log 2>&1 || { cat /tmp/stage.log; exit 2; }
FX=/tmp/fxstage/fixtures
export PORTAGE_CONFIGROOT=$FX ROOT=$FX DISTDIR=$FX/distfiles PORTAGE_RUNNING_ROOT=$FX
echo "### CELL aup0b: emerge --pretend --autounmask =dev-libs/aup0b-1"
emerge --pretend --autounmask =dev-libs/aup0b-1 2>&1; echo "rc=$?"
for order in "dev-libs/aub0c dev-libs/aub0b dev-libs/aub0a" "dev-libs/aub0c dev-libs/aub0a dev-libs/aub0b" "dev-libs/aub0b dev-libs/aub0c dev-libs/aub0a" "dev-libs/aub0b dev-libs/aub0a dev-libs/aub0c" "dev-libs/aub0a dev-libs/aub0c dev-libs/aub0b" "dev-libs/aub0a dev-libs/aub0b dev-libs/aub0c"; do
echo "### CELL aub0 :: $order"
emerge --pretend --autounmask-backtrack=y $order 2>&1; echo "rc=$?"
done
