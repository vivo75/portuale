#!/usr/bin/env bash
# Lives in portuale/LLM/tools/scope/ (tracked; tmpfs /tmp is wiped on container restart); see README.md.
# usage: oc.sh <slug> <agent> <workdir> <brief.md> <log>
# Runs `opencode run` inside its own transient systemd user scope `oc-<slug>`;
# stop exactly that scope (and all descendants) with ocstop.sh <slug>. No pkill.
set -u
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
slug=$1; agent=$2; dir=$3; brief=$4; log=$5
cd "$dir" || exit 1   # opencode no longer has --dir (checked 2026-10-02)
exec systemd-run --user --scope --quiet --collect --unit="oc-$slug" -p Delegate=yes \
  opencode run --agent "$agent" "$(cat "$brief")" > "$log" 2>&1
