#!/bin/bash
# g216 S0 probe: stage the fixture tree like l0-fixture-oracle/stage.sh,
# then run real emerge on the g216 shapes (default + --backtrack=0).
set -u
export LC_ALL=C.UTF-8 TZ=UTC
umask 022
STAGE=/tmp/g216-probe
/TEST/layers/l0-fixture-oracle/stage.sh "$STAGE" > /tmp/g216-probe-stage.log 2>&1 \
  || { echo "STAGING FAILED"; tail -20 /tmp/g216-probe-stage.log; exit 2; }
FX="$STAGE/fixtures"
rm -rf /var/db/pkg
cp -r "$FX/var/db/pkg" /var/db/pkg
if [ -f "$FX/var/lib/portage/world" ]; then
  mkdir -p /var/lib/portage
  cp "$FX/var/lib/portage/world" /var/lib/portage/world
fi
export PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX" DISTDIR="$FX/distfiles"
export PORTAGE_REPOSITORIES="$(cat "$FX/etc/portage/repos.conf/repos.conf")"
export PYTHONHASHSEED=0
echo "real_portage: $(/usr/sbin/emerge --version 2>/dev/null | head -1)"
run() {
  echo "### emerge -p --color=n $*"
  timeout 600 /usr/sbin/emerge -p --color=n "$@" 2>&1
  echo "rc=$?"
}
run app-misc/g216top
run dev-lang/g216comp
run --backtrack=0 app-misc/g216top
run --backtrack=0 dev-lang/g216comp
