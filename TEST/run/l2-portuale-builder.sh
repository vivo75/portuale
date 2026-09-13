#!/bin/bash
# L2 -- portuale as builder: build the atom list from source with BOTH
# package managers (`--buildpkgonly`), structurally validate every
# portuale archive, diff it against the portage-built sibling, then
# cross-install: real Portage merges the PORTUALE-built set (candidate)
# and the PORTAGE-built set (reference) into two identical fresh
# containers, and the resulting $ROOT + VDB snapshots are diffed.
#
#   TEST/run/l2-portuale-builder.sh [atomlist]      (default l1-merge.txt)
#   TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt
#
# Env: PORTTEST_IMAGE, PORTTEST_PODMAN,
#      L2_MODE=strict|payload-tolerant  (default strict; the real set
#          wants payload-tolerant -- compiled bytes legitimately differ)
#      L2_REBUILD=1 (wipe the two pkgcaches), L2_SKIP_BUILD=1 (reuse),
#      L2_JOBS, L2_SKIP_PORTAGE_UPGRADE, L2_KEEP_UNKNOWN=1 (report-only)
#
# Exit: 0 green (every finding known/adjudicated), 1 unexplained
#       finding(s), 2 setup error.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/lib.sh"

ATOMLIST=$(realpath -e "${1:-$TEST_DIR/atomlists/l1-merge.txt}" 2>/dev/null) \
  || { echo "atom list not found" >&2; exit 2; }
case $ATOMLIST in
  "$TEST_DIR"/*) REL_ATOMLIST="/TEST/${ATOMLIST#"$TEST_DIR"/}" ;;
  *) echo "atom list must live under $TEST_DIR/" >&2; exit 2 ;;
esac

MODE=${L2_MODE:-strict}
case $MODE in strict|payload-tolerant) ;; *) echo "L2_MODE must be strict|payload-tolerant" >&2; exit 2 ;; esac

PKG_PORTAGE="$LOGS_DIR/_l2-pkgcache-portage"
PKG_PORTUALE="$LOGS_DIR/_l2-pkgcache-portuale"
DISTFILES="$LOGS_DIR/_l2-distfiles"
RUN="l2-$(timestamp)"
OUT="$LOGS_DIR/$RUN"
mkdir -p "$OUT" "$PKG_PORTAGE" "$PKG_PORTUALE" "$DISTFILES"
ln -sfn "$RUN" "$LOGS_DIR/l2-latest"

ensure_portuale_built
ensure_image

[ "${L2_REBUILD:-0}" = 1 ] && { echo ">>> L2_REBUILD: wiping the pkgcaches"; rm -rf "${PKG_PORTAGE:?}"/* "${PKG_PORTUALE:?}"/*; }

PORTTEST_OVL="$TEST_DIR/images/overlay/porttest"
ovl_mount=()
[ -d "$PORTTEST_OVL/porttest" ] && ovl_mount=(-v "$PORTTEST_OVL:/porttest-overlay:ro")

UNEXPLAINED=0
KNOWN_HITS=0

# finding lines that are a filed portuale producer gap, not a regression
# (ids match TEST/findings/l2.md; delete an entry when its gap is fixed).
KNOWN_FINDINGS=(
  "l2-gpkg-metadata-members|missing metadata/(SIZE|IUSE|IUSE_EFFECTIVE|repository|REPO_REVISIONS)$"
  "l2-gpkg-metadata-members|missing in b: metadata/(SIZE|IUSE|IUSE_EFFECTIVE|repository|REPO_REVISIONS|RDEPEND|REQUIRES|PROVIDES|NEEDED|NEEDED\.ELF\.2)$"
  "l2-gpkg-metadata-members|metadata/NEEDED(\.ELF\.2)? differs:"
  "l2-gpkg-metadata-members|Packages stanza has no REPO field$"
  "l2-bpkgonly-env|metadata/(FEATURES|USE) differs:"
  "l2-bpkgonly-env|metadata/environment\.bz2 differs:"
  "l2-bpkgonly-env|only in [ab]: var/lib/porttest/kept/.*\.keep_porttest_emptydirs-"
  "l2-gpkg-dostrip-splitdebug|only in [ab]: usr/lib/debug/"
  "l2-gpkg-dostrip-splitdebug|only in [ab]: usr/lib(64)?/libptsd"
  "l2-gpkg-dostrip-splitdebug|only in [ab]: usr/lib$"
  "l2-gpkg-dostrip-splitdebug|only in [ab]: usr/lib/debug$"
  "l2-gpkg-dostrip-splitdebug|payload differs: usr/bin/pt-"
  "l2-gpkg-dostrip-splitdebug|payload differs: usr/lib(64)?/libptsd"
  "l2-gpkg-docompress|BIG\.txt"
)

classify_file() {  # <label> <findings-file>
  local label=$1 f=$2 line id kid pat
  [ -s "$f" ] || return 0
  while IFS= read -r line; do
    case $line in
      "[UNEXPLAINED]"*|"gpkg-diff:"*|"gpkg-structure"*) continue ;;
      "[outer-name]"*|"[payload]"*) continue ;;   # informational (soft)
    esac
    case $line in
      "["*) : ;; *) continue ;;
    esac
    id=""
    for k in "${KNOWN_FINDINGS[@]}"; do
      kid=${k%%|*}; pat=${k#*|}
      if printf '%s\n' "$line" | grep -qE "$pat"; then id=$kid; break; fi
    done
    if [ -n "$id" ]; then
      echo "[known:$id] $label: $line" >> "$OUT/classification.txt"
      KNOWN_HITS=$((KNOWN_HITS + 1))
    else
      echo "[UNEXPLAINED] $label: $line" >> "$OUT/classification.txt"
      UNEXPLAINED=$((UNEXPLAINED + 1))
    fi
  done < <(grep -E '^\[' "$f" || true)
}
: > "$OUT/classification.txt"

# --- build ---------------------------------------------------------------
if [ "${L2_SKIP_BUILD:-0}" != 1 ]; then
  echo ">>> building the set with Portage (archive-only) -> $PKG_PORTAGE"
  "$PODMAN" run --rm --name "porttest-l2-build-portage-$$" \
    --security-opt seccomp=unconfined --cgroups=enabled --cgroupns=private \
    -v "$TEST_DIR:/TEST:ro" -v "$PKG_PORTAGE:/pkgs" -v "$DISTFILES:/distfiles" \
    "${ovl_mount[@]}" \
    -e PKGDIR=/pkgs -e DISTDIR=/distfiles \
    -e "L2_JOBS=${L2_JOBS:-1}" \
    -e "L2_SKIP_PORTAGE_UPGRADE=${L2_SKIP_PORTAGE_UPGRADE:-0}" \
    -e "L2_PORTAGE_PIN=${L2_PORTAGE_PIN:-3.0.82.2}" \
    --entrypoint /bin/bash "$IMAGE" \
    /TEST/layers/l2/build-portage.sh "$REL_ATOMLIST" 2>&1 | tee "$OUT/build-portage.log"

  echo ">>> building the set with portuale (archive-only) -> $PKG_PORTUALE"
  podman_run_portuale "porttest-l2-build-portuale-$$" \
    -v "$PKG_PORTAGE:/ref-pkgs:ro" -v "$PKG_PORTUALE:/pkgs" -v "$DISTFILES:/distfiles" \
    "${ovl_mount[@]}" \
    -e PKGDIR=/pkgs -e DISTDIR=/distfiles \
    -e "L2_JOBS=${L2_JOBS:-1}" \
    -e "L2_SKIP_PORTAGE_UPGRADE=${L2_SKIP_PORTAGE_UPGRADE:-0}" \
    -e "L2_PORTAGE_PIN=${L2_PORTAGE_PIN:-3.0.82.2}" \
    --entrypoint /bin/bash "$IMAGE" \
    /TEST/layers/l2/build-portuale.sh "$REL_ATOMLIST" 2>&1 | tee "$OUT/build-portuale.log"
else
  echo ">>> L2_SKIP_BUILD: reusing $(find "$PKG_PORTAGE" -name '*.gpkg.tar' | wc -l) portage + $(find "$PKG_PORTUALE" -name '*.gpkg.tar' | wc -l) portuale archives"
fi

# --- structural validation ----------------------------------------------
echo ">>> gpkg-structure over both pkgdirs"
set +e
"$TEST_DIR/compare/gpkg-structure.sh" --dir "$PKG_PORTAGE" --packages > "$OUT/structure-portage.txt" 2>&1
rc_p=$?
"$TEST_DIR/compare/gpkg-structure.sh" --dir "$PKG_PORTUALE" --packages > "$OUT/structure-portuale.txt" 2>&1
rc_u=$?
set -e
classify_file "structure-portage" "$OUT/structure-portage.txt"
classify_file "structure-portuale" "$OUT/structure-portuale.txt"

# --- archive-vs-archive pairs -------------------------------------------
echo ">>> gpkg-diff portage-built vs portuale-built per atom"
: > "$OUT/archive-diffs.txt"
while IFS= read -r line || [ -n "$line" ]; do
  atom=${line%%#*}; atom=$(printf '%s' "$atom" | tr -d '[:space:]')
  [ -n "$atom" ] || continue
  cat=${atom%%/*}; pn=${atom#*/}
  a=$(find "$PKG_PORTAGE/$cat" -name "$pn-*.gpkg.tar" 2>/dev/null | LC_ALL=C sort | head -1)
  b=$(find "$PKG_PORTUALE/$cat" -name "$pn-*.gpkg.tar" 2>/dev/null | LC_ALL=C sort | head -1)
  pair_out="$OUT/archive-$cat-$pn.txt"
  {
    echo "### $atom"
    echo "portage : ${a:-MISSING}"
    echo "portuale: ${b:-MISSING}"
  } > "$pair_out"
  if [ -n "$a" ] && [ -n "$b" ]; then
    set +e
    "$TEST_DIR/compare/gpkg-diff.sh" --mode "$MODE" "$a" "$b" >> "$pair_out" 2>&1
    set -e
    cat "$pair_out" >> "$OUT/archive-diffs.txt"
    classify_file "archive-diff:$atom" "$pair_out"
  else
    echo "[UNEXPLAINED] archive-diff:$atom: archive missing (portage='$a' portuale='$b')" >> "$OUT/classification.txt"
    UNEXPLAINED=$((UNEXPLAINED + 1))
  fi
done < "$ATOMLIST"

# --- cross-install: real Portage consumes the portuale-built set --------
consume() {  # <label> <pkgdir> <portage|portuale>
  local label=$1 pkgdir=$2 pm=$3
  echo ">>> cross-install $label: $pm merges $(basename "$pkgdir")"
  podman_run_portuale "porttest-l2-$label-$$" \
    -v "$pkgdir:/pkgs:ro" \
    -e PKGDIR=/pkgs \
    -e "L1_SKIP_PORTAGE_UPGRADE=${L2_SKIP_PORTAGE_UPGRADE:-0}" \
    --entrypoint /bin/bash "$IMAGE" \
    /TEST/layers/l1/consume.sh "$pm" "$REL_ATOMLIST" "/TEST/logs/$RUN/$label"
}
consume ref  "$PKG_PORTAGE"  portage
consume cand "$PKG_PORTUALE" portage
consume ctrl "$PKG_PORTAGE"  portuale

echo ">>> normalising"
for d in ref cand ctrl; do python3 "$TEST_DIR/compare/normalize.py" "$OUT/$d" >/dev/null; done

TOL=()
[ "$MODE" = payload-tolerant ] && TOL=(--tolerate-payload)
echo ">>> diffing ref vs cand"
set +e
python3 "$TEST_DIR/compare/diff.py" --layer l2 "${TOL[@]}" \
  "$OUT/ref" "$OUT/cand" "$TEST_DIR/compare/known-divergences.yaml" \
  | tee "$OUT/cross-install.txt"
rc_diff=${PIPESTATUS[0]}
set -e
echo ">>> control diff (portuale consumes the portage-built set)"
set +e
python3 "$TEST_DIR/compare/diff.py" --layer l2 "${TOL[@]}" \
  "$OUT/ref" "$OUT/ctrl" "$TEST_DIR/compare/known-divergences.yaml" \
  > "$OUT/control.txt" 2>&1
rc_ctrl=$?
set -e

# --- report --------------------------------------------------------------
{
  echo "# L2 report -- portuale as builder"
  echo
  echo "atoms   : $REL_ATOMLIST"
  echo "mode    : $MODE"
  echo "dates   : $(date -u +%FT%TZ)"
  echo "portage : $PKG_PORTAGE ($(find "$PKG_PORTAGE" -name '*.gpkg.tar' | wc -l) archives)"
  echo "portuale: $PKG_PORTUALE ($(find "$PKG_PORTUALE" -name '*.gpkg.tar' | wc -l) archives)"
  echo
  echo "## structure"
  echo "  portage findings : $(grep -cE '^\[' "$OUT/structure-portage.txt" || true)"
  echo "  portuale findings: $(grep -cE '^\[' "$OUT/structure-portuale.txt" || true)"
  echo
  echo "## archive diffs (portage-built vs portuale-built)"
  grep -E '^gpkg-diff:' "$OUT/archive-diffs.txt" || true
  echo
  echo "## cross-install (real Portage merges portuale-built)"
  sed -n '/^## summary/,/^$/p' "$OUT/cross-install.txt"
  echo "## control (portuale merges portage-built)"
  sed -n '/^## summary/,/^$/p' "$OUT/control.txt"
  echo
  echo "## classification"
  echo "  known (filed portuale producer gaps): $KNOWN_HITS"
  echo "  UNEXPLAINED                         : $UNEXPLAINED"
  echo "  cross-install diff rc               : $rc_diff"
  echo "  control diff rc                     : $rc_ctrl"
  echo
  if [ "$UNEXPLAINED" -gt 0 ]; then
    echo "## unexplained findings"
    grep '^\[UNEXPLAINED\]' "$OUT/classification.txt" || true
    echo
  fi
  if [ "$KNOWN_HITS" -gt 0 ]; then
    echo "## known findings (adjudicated, see TEST/findings/l2.md)"
    grep '^\[known:' "$OUT/classification.txt" | sort | uniq -c | sort -rn || true
    echo
  fi
} > "$OUT/l2-report.txt"
ln -sfn "$RUN/l2-report.txt" "$LOGS_DIR/l2-report.txt"

cat "$OUT/l2-report.txt"
rc=0
[ "$UNEXPLAINED" = 0 ] || rc=1
[ "$rc_diff" = 0 ] || rc=1
[ "$rc_ctrl" = 0 ] || rc=1
echo ">>> report: $OUT/l2-report.txt   (rc=$rc)"
exit "$rc"
