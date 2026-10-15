#!/bin/bash
# Host portuale side. usage: run-portuale.sh <cells.txt> <outdir> <emerge-binary> <staged-fixtures>
# Staged with pmtest's layers/l0-fixture-oracle/stage.sh (host copy: `cp -r <pmtest>/fixtures/.`
# instead of /fixtures, no `touch /etc/make.local`); portuale resolves make.conf's
# `source /etc/make.local` under PORTAGE_CONFIGROOT, so the host file is not read.
CELLS=$1; OUT=$2; BIN=$3; FX=$4
mkdir -p "$OUT"
grep -v '^#' "$CELLS" | while IFS='|' read -r name args; do
  name=$(echo $name); [ -z "$name" ] && continue
  { echo "### emerge$args --color=n"; env -i PATH=/usr/bin:/bin HOME=/var/tmp/plan359 PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0 \
      PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles $BIN $args --color=n 2>&1; echo "rc=$?"; } > "$OUT/$name.txt"
done
