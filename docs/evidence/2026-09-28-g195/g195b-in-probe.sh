#!/bin/bash
# g195b S0 probe: parentflipeqpkg (pin correction grounding) + aup0b
# (re-verify default shape, capture --autounmask-backtrack=y shape).
set -u
bash /stage.sh /tmp/fxstage > /tmp/stage.log 2>&1 || { cat /tmp/stage.log; exit 2; }
FX=/tmp/fxstage/fixtures
export PORTAGE_CONFIGROOT=$FX ROOT=$FX DISTDIR=$FX/distfiles PORTAGE_RUNNING_ROOT=$FX
echo "### CELL parentflip-default: emerge --pretend dev-libs/parentflipeqpkg"
emerge --pretend dev-libs/parentflipeqpkg 2>&1; echo "rc=$?"
echo "### CELL parentflip-autounmask: emerge --pretend --autounmask dev-libs/parentflipeqpkg"
emerge --pretend --autounmask dev-libs/parentflipeqpkg 2>&1; echo "rc=$?"
echo "### CELL parentflip-use-n: emerge --pretend --autounmask-use=n dev-libs/parentflipeqpkg"
emerge --pretend --autounmask-use=n dev-libs/parentflipeqpkg 2>&1; echo "rc=$?"
echo "### CELL aup0b-default: emerge --pretend --autounmask =dev-libs/aup0b-1"
emerge --pretend --autounmask =dev-libs/aup0b-1 2>&1; echo "rc=$?"
echo "### CELL aup0b-backtrack-y: emerge --pretend --autounmask-backtrack=y =dev-libs/aup0b-1"
emerge --pretend --autounmask-backtrack=y =dev-libs/aup0b-1 2>&1; echo "rc=$?"
