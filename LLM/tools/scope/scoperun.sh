#!/usr/bin/env bash
# Lives in portuale/LLM/tools/scope/; see README.md.
# usage: scoperun.sh <slug> <log> <cmd> [args...]
# Runs any command (cargo test, pytest, an L0/L1 bed run, ...) inside its own
# transient systemd user scope `oc-<slug>`, output to <log>, and exits with the
# command's exit code. Same naming as oc.sh, so the read-only checkers work:
#   ocgrep.sh <slug> [pattern]   RUNNING / STOPPED
#   scopewait.sh <slug> [secs]   block until the scope is gone
#   ocstop.sh <slug>             TERM then KILL the whole scope, podman
#                                containers included (Delegate=yes)
# Never use pgrep/pkill to find or stop these jobs.
set -u
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
slug=${1:?slug}; log=${2:?log}; shift 2
[ $# -gt 0 ] || { echo "scoperun.sh: no command" >&2; exit 2; }
if systemctl --user is-active --quiet "oc-$slug.scope"; then
  echo "scoperun.sh: scope oc-$slug.scope is already running" >&2; exit 2
fi
exec systemd-run --user --scope --quiet --collect --unit="oc-$slug" -p Delegate=yes \
  "$@" > "$log" 2>&1
