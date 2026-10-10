#!/usr/bin/env bash
# usage: ocstatus.sh <slug> <log> [hung_after_seconds=240]
# One word: STOPPED (no scope oc-<slug>), HUNG (scope alive, log still empty after
# hung_after_seconds -- opencode's silent hang), RUNNING otherwise.
# Exit 0 = RUNNING, 1 = STOPPED, 2 = HUNG. Read-only; see README.md.
slug=${1:?slug}; log=${2:?log}; limit=${3:-240}
case $slug in oc-*) ;; *) slug=oc-$slug ;; esac
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
systemctl --user is-active --quiet "$slug.scope" || { echo STOPPED; exit 1; }
if [ ! -s "$log" ]; then
  started=$(systemctl --user show "$slug.scope" -p ActiveEnterTimestampMonotonic --value)
  now=$(awk '{printf "%d", $1*1000000}' /proc/uptime)
  if [ -n "$started" ] && [ $(( (now - started) / 1000000 )) -ge "$limit" ]; then echo HUNG; exit 2; fi
fi
echo RUNNING
