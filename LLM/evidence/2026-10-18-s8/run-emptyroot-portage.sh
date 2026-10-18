#!/bin/bash
# #371: ROOT is an empty directory, config from the staged fixtures (as the
# two portuale pins do). Runs INSIDE localhost/test-portuale:latest.
bash /TEST/layers/l0-fixture-oracle/stage.sh /tmp/st >/dev/null 2>&1
FX=/tmp/st/fixtures
for args in "--pretend @preserved-rebuild" "--pretend @selected" "--pretend @world"; do
  rm -rf /tmp/er; mkdir /tmp/er
  echo "### emerge $args (ROOT empty)"
  PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0 PORTAGE_CONFIGROOT=$FX ROOT=/tmp/er PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles \
    PORTAGE_REPOSITORIES="$(cat $FX/etc/portage/repos.conf/repos.conf)" /usr/sbin/emerge $args --color=n 2>&1 | awk '/^Performing Global/{s=1} s&&/^(These|Calculating|\[|emerge|rc=|Total)/{s=0} !s'
  echo "rc=${PIPESTATUS[0]}"
done
