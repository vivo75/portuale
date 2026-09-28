#!/bin/bash
# g195c S0 probe: pfgraphparent bare-miss grounding (B20 option a).
set -u
bash /stage.sh /tmp/fxstage > /tmp/stage.log 2>&1 || { cat /tmp/stage.log; exit 2; }
FX=/tmp/fxstage/fixtures
export PORTAGE_CONFIGROOT=$FX ROOT=$FX DISTDIR=$FX/distfiles PORTAGE_RUNNING_ROOT=$FX
echo "### CELL pfgraph-default: emerge --pretend dev-libs/pfgraphparent"
emerge --pretend dev-libs/pfgraphparent 2>&1; echo "rc=$?"
echo "### CELL pfgraph-backtrack-y: emerge --pretend --autounmask-backtrack=y dev-libs/pfgraphparent"
emerge --pretend --autounmask-backtrack=y dev-libs/pfgraphparent 2>&1; echo "rc=$?"
echo "### CELL pfgraph-use-n: emerge --pretend --autounmask-use=n dev-libs/pfgraphparent"
emerge --pretend --autounmask-use=n dev-libs/pfgraphparent 2>&1; echo "rc=$?"
