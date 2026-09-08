#!/bin/bash
# usage: probe-host.sh <cells-file> <outdir>
# Host half of the #270 probe: runs probe-in.sh in the L0 container image with the worktree PM.
set -euo pipefail
WT_PMTEST=/home/vivo/repo/PORTUALE/wt-270-bt0/pmtest
. "$WT_PMTEST/differential-test-bed/run/lib.sh"
CELLS=$(realpath -e "$1"); OUT=$(realpath -m "$2"); mkdir -p "$OUT"
ensure_pm_built "$OUT"
ensure_image
{
  echo "date_utc $(date -u +%FT%TZ)"
  echo "portuale_head $(git -C "$PM_REPO" rev-parse --short HEAD) dirty=$(git -C "$PM_REPO" status --short | wc -l)"
  echo "pmtest_head $(git -C "$WT_PMTEST" rev-parse --short HEAD)"
  echo "bin_mtime $(stat -c %y "$PM_BIN_DIR/emerge")"
} | tee "$OUT/provenance.txt"
podman_run_pm "porttest-p270-$$" \
  -v "$REPO_ROOT/fixtures:/fixtures:ro" \
  -v "$(dirname "$CELLS"):/probe-in:ro" \
  -v "$OUT:/probe-out" \
  -v /home/vivo/repo/PORTUALE/oc-270/probe/probe-in.sh:/probe-in.sh:ro \
  -v "$TEST_DIR/layers/l0-fixture-oracle/stage.sh:/stage.sh:ro" \
  --entrypoint /bin/bash "$IMAGE" /probe-in.sh "/probe-in/$(basename "$CELLS")" /probe-out
