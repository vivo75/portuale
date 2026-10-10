#!/usr/bin/env bash
# usage: ocgrep.sh <slug> [pattern]
# Prints RUNNING if scope oc-<slug> exists and (when given) a process in it
# matches <pattern> (grep, fixed string), else STOPPED. Exit 0 = RUNNING, 1 = STOPPED.
# Read-only. Never matches the checker itself (it is not inside the scope).
slug=${1:?slug}; pattern=${2:-}
case $slug in oc-*) ;; *) slug=oc-$slug ;; esac
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
systemctl --user is-active --quiet "$slug.scope" || { echo STOPPED; exit 1; }
if [ -n "$pattern" ]; then
  systemd-cgls --user-unit "$slug.scope" --no-pager 2>/dev/null | grep -qF -- "$pattern" \
    || { echo STOPPED; exit 1; }
fi
echo RUNNING
