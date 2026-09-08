#!/bin/bash
# L0 -- resolver parity at real-tree scale.
#
# Builds portuale, runs `emerge -pv` for every entry in the atom list
# under both real portage and portuale inside a throwaway container, then
# diffs the merge lists / errors / exit codes on the host.
#
#   TEST/run/l0-resolver.sh [atomlist]
#
# Env: PORTTEST_IMAGE, PORTTEST_PODMAN, L0_SKIP_PORTAGE_UPGRADE,
#      L0_EMERGE_OPTS  (see TEST/layers/l0/in-container.sh).
#
# Exit: 0 green, 1 unexplained divergences, 2 setup error.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/lib.sh"

ATOMLIST=$(realpath -e "${1:-$TEST_DIR/atomlists/l0-resolve.txt}" 2>/dev/null) \
  || { echo "atom list not found: ${1:-$TEST_DIR/atomlists/l0-resolve.txt}" >&2; exit 2; }
case $ATOMLIST in
  "$TEST_DIR"/*) REL_ATOMLIST="/TEST/${ATOMLIST#"$TEST_DIR"/}" ;;
  *) echo "atom list must live under $TEST_DIR/ (it is bind-mounted at /TEST)" >&2; exit 2 ;;
esac

RUN="l0-$(timestamp)"
OUT="$LOGS_DIR/$RUN"
mkdir -p "$OUT"
ln -sfn "$RUN" "$LOGS_DIR/l0-latest"

ensure_portuale_built
ensure_image

echo ">>> running L0 probes in $IMAGE  (out: $OUT)"
podman_run_portuale "porttest-l0-$$" \
  -e "L0_SKIP_PORTAGE_UPGRADE=${L0_SKIP_PORTAGE_UPGRADE:-0}" \
  -e "L0_SKIP_MULTI=${L0_SKIP_MULTI:-0}" \
  -e "L0_EMERGE_OPTS=${L0_EMERGE_OPTS:--pv}" \
  -e "L0_PORTAGE_PIN=${L0_PORTAGE_PIN:-3.0.82.2}" \
  --entrypoint /bin/bash "$IMAGE" \
  /TEST/layers/l0/in-container.sh "$REL_ATOMLIST" "/TEST/logs/$RUN"

echo ">>> comparing"
set +e
python3 "$TEST_DIR/compare/resolve-compare.py" "$OUT" "$TEST_DIR/compare/known-divergences.yaml"
rc=$?
set -e

ln -sfn "$RUN/l0-report.txt" "$LOGS_DIR/l0-report.txt"
ln -sfn "$RUN/l0-report.json" "$LOGS_DIR/l0-report.json"
echo ">>> report: $OUT/l0-report.txt   (rc=$rc)"
exit $rc
