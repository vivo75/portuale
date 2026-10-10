#!/usr/bin/env bash
# Lives in portuale/LLM/tools/scope/ (tracked; tmpfs /tmp is wiped on container restart); see README.md.
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
systemctl --user stop "oc-$1.scope"
