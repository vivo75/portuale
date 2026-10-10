#!/usr/bin/env bash
# Lives in portuale/LLM/tools/scope/; see README.md.
# usage: scopewait.sh <slug> [timeout_seconds=7000]
# Blocks until scope oc-<slug> is no longer active. Exit 0 = gone,
# 1 = still running at the timeout. Read-only: it asks systemd about the
# scope by name and never matches process names (so it cannot match itself).
slug=${1:?slug}; limit=${2:-7000}
case $slug in oc-*) ;; *) slug=oc-$slug ;; esac
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
end=$(( $(date +%s) + limit ))
while systemctl --user is-active --quiet "$slug.scope"; do
  [ "$(date +%s)" -ge "$end" ] && { echo "still running: $slug.scope"; exit 1; }
  sleep 10
done
echo "gone: $slug.scope"
