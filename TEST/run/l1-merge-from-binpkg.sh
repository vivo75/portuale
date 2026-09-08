#!/bin/bash
# L1 -- merge parity from an identical prebuilt binpkg set.
#
# Portage builds the TEST/atomlists/l1-merge.txt set from source once
# (cached in a persistent host dir); then Portage and portuale each
# merge that same $PKGDIR into their own fresh container; then the two
# resulting $ROOT + VDB snapshots are normalised and diffed. Every
# unexplained hard diff is a portuale merge-path bug.
#
#   TEST/run/l1-merge-from-binpkg.sh [atomlist]
#
# Env: PORTTEST_IMAGE, PORTTEST_PODMAN, L1_REBUILD=1 (wipe the pkgcache
#      first), L1_SKIP_BUILD=1 (reuse whatever is in the pkgcache),
#      L1_JOBS, L1_SKIP_PORTAGE_UPGRADE.
#
# Exit: 0 green, 1 unexplained divergence, 2 setup error.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/lib.sh"

ATOMLIST=$(realpath -e "${1:-$TEST_DIR/atomlists/l1-merge.txt}" 2>/dev/null) \
  || { echo "atom list not found" >&2; exit 2; }
case $ATOMLIST in
  "$TEST_DIR"/*) REL_ATOMLIST="/TEST/${ATOMLIST#"$TEST_DIR"/}" ;;
  *) echo "atom list must live under $TEST_DIR/" >&2; exit 2 ;;
esac

PKGCACHE="$LOGS_DIR/_l1-pkgcache"
RUN="l1-$(timestamp)"
OUT="$LOGS_DIR/$RUN"
mkdir -p "$OUT" "$PKGCACHE"
ln -sfn "$RUN" "$LOGS_DIR/l1-latest"

ensure_portuale_built
ensure_image

[ "${L1_REBUILD:-0}" = 1 ] && { echo ">>> L1_REBUILD: wiping $PKGCACHE"; rm -rf "${PKGCACHE:?}"/*; }

# --- build (Portage only) ------------------------------------------------
if [ "${L1_SKIP_BUILD:-0}" != 1 ]; then
  echo ">>> building the set from source with Portage (pkgcache: $PKGCACHE)"
  "$PODMAN" run --rm --name "porttest-l1-build-$$" \
    --security-opt seccomp=unconfined --cgroups=enabled --cgroupns=private \
    -v "$TEST_DIR:/TEST:ro" -v "$PKGCACHE:/pkgs" \
    -e PKGDIR=/pkgs \
    -e "L1_JOBS=${L1_JOBS:-1}" \
    -e "L1_SKIP_PORTAGE_UPGRADE=${L1_SKIP_PORTAGE_UPGRADE:-0}" \
    --entrypoint /bin/bash "$IMAGE" \
    /TEST/layers/l1/build.sh "$REL_ATOMLIST"
else
  echo ">>> L1_SKIP_BUILD: using $(find "$PKGCACHE" -name '*.gpkg.tar' | wc -l) cached binpkgs"
fi

# --- consume (both PMs, identical fresh containers) --------------------
consume() {  # pm
  local pm=$1
  echo ">>> merging with $pm"
  podman_run_portuale "porttest-l1-$pm-$$" \
    -v "$PKGCACHE:/pkgs:ro" \
    -e PKGDIR=/pkgs \
    -e "L1_SKIP_PORTAGE_UPGRADE=${L1_SKIP_PORTAGE_UPGRADE:-0}" \
    --entrypoint /bin/bash "$IMAGE" \
    /TEST/layers/l1/consume.sh "$pm" "$REL_ATOMLIST" "/TEST/logs/$RUN/$pm"
}
consume portage
consume portuale

# --- normalise + diff -------------------------------------------------
echo ">>> normalising"
python3 "$TEST_DIR/compare/normalize.py" "$OUT/portage"
python3 "$TEST_DIR/compare/normalize.py" "$OUT/portuale"

echo ">>> diffing"
set +e
python3 "$TEST_DIR/compare/diff.py" "$OUT/portage" "$OUT/portuale" \
  "$TEST_DIR/compare/known-divergences.yaml" | tee "$OUT/l1-report.txt"
rc=${PIPESTATUS[0]}
set -e

ln -sfn "$RUN/l1-report.txt" "$LOGS_DIR/l1-report.txt"
echo ">>> report: $OUT/l1-report.txt   (rc=$rc)"
exit "$rc"
