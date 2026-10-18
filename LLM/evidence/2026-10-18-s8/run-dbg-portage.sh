#!/bin/bash
bash /TEST/layers/l0-fixture-oracle/stage.sh /tmp/st >/dev/null 2>&1
FX=/tmp/st/fixtures
PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0 PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles \
 PORTAGE_REPOSITORIES="$(cat $FX/etc/portage/repos.conf/repos.conf)" /usr/sbin/emerge --pretend --update --package-moves=n --debug dev-libs/slotmovepkg --color=n 2>&1 | grep -v "^Undefined" | head -60
