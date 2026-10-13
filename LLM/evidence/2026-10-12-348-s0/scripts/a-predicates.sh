#!/usr/bin/env bash
# S0.3 (worker a): predicate parity -- every ___eapi_* predicate of eapi.sh
# for EAPI in {7, 8, 99} on host bash vs portuale's brush backend.
#
# Runners (cell provenance):
#   host-bash      host bash sourcing the HOST eapi.sh (the oracle: byte-identical
#                  to bin/eapi.sh per S0.2 `cmp`, same no-arg call form as every
#                  in-tree call site).
#   portuale-bash  `portuale ebuild <probe> setup --shell bash` (disambiguator:
#                  separates PM/env deltas from shell deltas).
#   portuale-brush `portuale ebuild <probe> setup --shell brush` (the shell under
#                  test, exactly as portuale executes phases).
# Probes live in /var/tmp (NOT fixtures: S0.3 needs no repo-348 fixture ebuilds).
# Each run gets a fresh PORTAGE_TMPDIR so phase-marker skips cannot bite.
# Captures: <out>/P/R-<runner>-<eapi>.{argv,rc,stdout,stderr}; the 222-row table
# is built from the PRED lines by scripts/a-predicates-table.sh (run after).
set -u
ROOT=${1:?portuale root}
OUT=${2:?output dir (a/P)}
WORK=${3:-/var/tmp/t348-a}
PORTUALE="$ROOT/target/release/portuale"
HOST_EAPI_SH=/usr/lib/portage/python3.14/eapi.sh
mkdir -p "$OUT" "$WORK/t348-a-pred"

# The 74 predicate names, in eapi.sh definition order.
mapfile -t PREDS < <(grep -o '^___eapi_[A-Za-z0-9_.-]*' "$ROOT/bin/eapi.sh")
echo "predicates: ${#PREDS[@]}"

LOOP='rc=0; "${p}" || rc=$?; echo "PRED ${p} ${rc}"'
PREF_LINE=$(printf '%s ' "${PREDS[@]}")

for E in 7 8 99; do
  # --- probe ebuild (EAPI baked in; predicates called with no args,
  # --- exactly like every in-tree call site) ---
  {
  echo "EAPI=${E}"
  echo 'DESCRIPTION="s0.3 predicate probe"'
  echo 'SLOT="0"'
  echo 'KEYWORDS="amd64"'
  echo 'S="${WORKDIR}"'
  echo 'pkg_setup() {'
  echo "	for p in ${PREF_LINE}; do"
  echo "		${LOOP}"
  echo '	done'
  echo '}'
  } > "$WORK/t348-a-pred/t348-a-pred-${E}.ebuild"

  # --- runner 1: host bash, host eapi.sh ---
  C="R-host-bash-${E}"
  {
  echo "EAPI=${E}"
  echo "source \"${HOST_EAPI_SH}\""
  echo "for p in ${PREF_LINE}; do"
  echo "	${LOOP}"
  echo "done"
  } > "$WORK/pred-loop-${E}.sh"
  printf 'bash %s/pred-loop-%s.sh' "$WORK" "$E" > "$OUT/$C.argv"
  bash "$WORK/pred-loop-${E}.sh" >"$OUT/$C.stdout" 2>"$OUT/$C.stderr"
  echo "$?" > "$OUT/$C.rc"

  # --- runners 2+3: portuale, both shells, hermetic tmpdir ---
  for SH in bash brush; do
    C="R-portuale-${SH}-${E}"
    rm -rf "$WORK/tmp-pb-${SH}-${E}"
    printf 'PORTAGE_TMPDIR=%s portuale ebuild %s setup --shell %s' \
      "$WORK/tmp-pb-${SH}-${E}" "$WORK/t348-a-pred/t348-a-pred-${E}.ebuild" "$SH" \
      > "$OUT/$C.argv"
    PORTAGE_TMPDIR="$WORK/tmp-pb-${SH}-${E}" "$PORTUALE" ebuild \
      "$WORK/t348-a-pred/t348-a-pred-${E}.ebuild" setup --shell "$SH" \
      >"$OUT/$C.stdout" 2>"$OUT/$C.stderr"
    echo "$?" > "$OUT/$C.rc"
  done
done
echo "runs done:"; ls "$OUT" | head -40
