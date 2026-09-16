#!/bin/bash
# L0 fixture oracle -- real `emerge` on the checked-in fixture tree
# (backlog #49 / docs/second_python_copy_removal.md §6).
#
# Stages a copy of `fixtures/` inside the test container, runs real
# `emerge -p` and portuale `-p` on the same cases, and lets the L0
# comparator produce typed findings. Real-execution-only (AGENTS.md
# step 4's carve-out: no contract CASES entry, no Python mirror).
#
#   TEST/run/l0-fixture-oracle.sh [atomlist]
#
# Env: PORTTEST_IMAGE, PORTTEST_PODMAN. Exit: 0 green, 1 unexplained
# findings, 2 setup error.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/lib.sh"

ATOMLIST=$(realpath -e "${1:-$TEST_DIR/atomlists/l0-fixture-oracle.txt}" 2>/dev/null) \
  || { echo "atom list not found: ${1:-$TEST_DIR/atomlists/l0-fixture-oracle.txt}" >&2; exit 2; }
case $ATOMLIST in
  "$TEST_DIR"/*) REL_ATOMLIST="/TEST/${ATOMLIST#"$TEST_DIR"/}" ;;
  *) echo "atom list must live under $TEST_DIR/ (it is bind-mounted at /TEST)" >&2; exit 2 ;;
esac

RUN="l0-fx-$(timestamp)"
OUT="$LOGS_DIR/$RUN"
mkdir -p "$OUT"
ln -sfn "$RUN" "$LOGS_DIR/l0-fixture-oracle-latest"

ensure_portuale_built
ensure_image

echo ">>> running fixture-oracle cases in $IMAGE  (out: $OUT)"
podman_run_portuale "porttest-l0fx-$$" \
  -v "$REPO_ROOT/fixtures:/fixtures:ro" \
  -e "FX_SLOTOP_BDEP=${FX_SLOTOP_BDEP:-}" \
  --entrypoint /bin/bash "$IMAGE" \
  /TEST/layers/l0-fixture-oracle/in-container.sh "$REL_ATOMLIST" "/TEST/logs/$RUN"

echo ">>> comparing"
set +e
python3 "$TEST_DIR/compare/resolve-compare.py" "$OUT" \
  "$TEST_DIR/compare/known-divergences-fixture-oracle.yaml"
rc=$?
set -e

echo ">>> report: $OUT/l0-report.txt   (rc=$rc)"
exit $rc
