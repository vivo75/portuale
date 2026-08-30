#!/bin/bash
set +e
export PORTAGE_CONFIGROOT=/ ROOT=/
LOG=/TEST/logs/20-real-compare.log
: > "$LOG"
for pkg in sys-apps/coreutils dev-libs/libevent app-misc/tmux app-shells/bash sys-apps/portage; do
  {
    echo "### $pkg"
    echo "-- portuale:"
    emerge --pretend --color=n "$pkg" 2>&1 | grep -nE "^\[(ebuild|binary)|there are no ebuilds|no visible|REQUIRED_USE|emerge:" | cat -A | head -12
    echo "-- real emerge:"
    /usr/sbin/emerge --pretend --color=n "$pkg" 2>&1 | grep -nE "^\[(ebuild|binary)" | cat -A | head -12
    echo
  } | tee -a "$LOG"
done
