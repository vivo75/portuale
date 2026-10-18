#!/bin/bash
# S0 portuale side (host). Usage: run-s0-portuale.sh <cells-s0.txt> <outdir> <emerge-binary> <staged-fixtures>
# The stage is read-only for portuale (--pretend never writes it), so one
# stage serves all cells; the DELTA=maskedusedrift cell uses a pre-made
# second stage <staged-fixtures>-maskedusedrift. Debug rows (dbg-*) are
# Portage-only and skipped here (portuale has no --debug).
set -u
CELLS=$1; OUT=$2; BIN=$3; FX=$4
mkdir -p "$OUT"
trim() { printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'; }
grep -v '^#' "$CELLS" | while IFS='|' read -r raw_name raw_args raw_env; do
  name=$(trim "$raw_name"); [ -z "$name" ] && continue
  case "$name" in dbg-*) echo "skip $name (Portage-only --debug)" >&2; continue;; esac
  args=$(trim "$raw_args"); envspec=$(trim "$raw_env")
  stage="$FX"
  extra_env=()
  for tok in $envspec; do
    case "$tok" in
      DELTA=maskedusedrift) stage="$FX-maskedusedrift" ;;
      *=*) extra_env+=("$tok") ;;
    esac
  done
  {
    echo "### emerge $args --color=n"
    # shellcheck disable=SC2086
    env -i PATH=/usr/bin:/bin HOME=/var/tmp/s8 PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0 \
      PORTAGE_CONFIGROOT="$stage" ROOT="$stage" PORTAGE_RUNNING_ROOT="$stage" DISTDIR="$stage/distfiles" \
      "${extra_env[@]}" "$BIN" $args --color=n 2>&1
    echo "rc=$?"
  } > "$OUT/$name.txt"
done
echo "done"
