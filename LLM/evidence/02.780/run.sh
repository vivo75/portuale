#!/bin/bash
# usage: run.sh ROOT [extra emerge args]; both solvers, rows + NOTE/causes lines
F=/var/tmp/780/fx; r=$1; shift
export PORTAGE_CONFIGROOT=$F PORTAGE_REPOSITORIES="$(cat $(find $F/etc/portage/repos.conf -type f | sort) | awk 1)"
for s in "" "--solver=pubgrub"; do echo "== ${s:-default} $r $*"
 ROOT=/var/tmp/780/roots/$r PORTAGE_RUNNING_ROOT=/var/tmp/780/roots/$r DISTDIR=$F/distfiles ${BIN:-/var/tmp/780/bin}/emerge $s --pretend --verbose --getbinpkg=n "$@" @world 2>&1 | grep -E '^\[ebuild|causes rebuilds|^    \(|^NOTE|its dependency|WARN' | sed 's/::testrepo//g;s# to /var/tmp/780/roots/[a-z0-9]* 0 KiB##'
done
