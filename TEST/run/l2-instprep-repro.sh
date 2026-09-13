#!/bin/bash
# #38 S4 -- merge-time `instprep` repro: both PMs merge porttest/{docs,setuid}
# with FEATURES="-binpkg-dostrip -binpkg-docompress" (source merge, then a
# binpkg merge of the unstripped archive), in two fresh containers; the
# snapshots are diffed. Real strips/compresses at merge
# (`vartree.py:4440-4450` -> `__dyn_instprep`); a diff is the #38b gap.
#
#   TEST/run/l2-instprep-repro.sh
#
# Exit: 0 snapshots identical, 1 they differ, 2 setup error.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/lib.sh"

RUN="l2-instprep-$(timestamp)"
OUT="$LOGS_DIR/$RUN"
mkdir -p "$OUT"
ensure_portuale_built
ensure_image

for pm in portage portuale; do
  echo ">>> $pm"
  podman_run_portuale "porttest-instprep-$pm-$$" \
    -v "$TEST_DIR/images/overlay/porttest:/porttest-overlay:ro" \
    -e "L2_SKIP_PORTAGE_UPGRADE=${L2_SKIP_PORTAGE_UPGRADE:-0}" \
    --entrypoint /bin/bash "$IMAGE" \
    /TEST/layers/l2/instprep-repro.sh "$pm" "/TEST/logs/$RUN/$pm" 2>&1 | tee "$OUT/$pm.log"
done

rc=0
for cell in src bin; do
  echo ">>> diff $cell (portage vs portuale)"
  diff -u "$OUT/portage/$cell.txt" "$OUT/portuale/$cell.txt" || rc=1
done
echo ">>> $OUT (rc=$rc)"
exit "$rc"
