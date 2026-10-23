#!/bin/sh
# Full verification gate (AGENTS.md step 8), run from the portuale repo root.
# Writes logs to $1 (default: a fresh mktemp dir) and prints a one-screen
# summary: fmt/clippy status, cargo test totals + failing names, contract
# suite tail + failing names. Never run it while a bed run is active.
set -u
cd "$(dirname "$0")/../.." || exit 2
OUT=${1:-$(mktemp -d "${TMPDIR:-/var/tmp}/gate.XXXXXX")}
mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd)

# Doc checks (plan 02.493 Task 3): cheap, no cargo. ALLOW in doc_status_check.py
# is shrink-only, so compare it with main's copy. Until the checker is merged
# to main that file does not exist there: the comparison is then skipped.
BASE_CHECK="$OUT/doc_status_check.main.py"
if git show main:LLM/tools/doc_status_check.py >"$BASE_CHECK" 2>/dev/null; then
    python3 LLM/tools/doc_status_check.py --allow-base "$BASE_CHECK" >"$OUT/doc_status.txt" 2>&1
else
    python3 LLM/tools/doc_status_check.py >"$OUT/doc_status.txt" 2>&1
fi
echo "doc-status rc=$?"; sed 's/^/  /' "$OUT/doc_status.txt" | grep -v ': OK$'
python3 LLM/tools/cut_registry_check.py >"$OUT/cut_registry.txt" 2>&1
echo "cut-registry rc=$?"; sed 's/^/  /' "$OUT/cut_registry.txt" | grep -v ': OK$'
# Pairing / merge-path gate evidence (plan 02.493 Task 4).
sh LLM/tools/pair_check.sh >"$OUT/pair_check.txt" 2>&1
echo "pair-check rc=$?"; sed 's/^/  /' "$OUT/pair_check.txt" | grep -v ': OK$'

cargo fmt --check >"$OUT/fmt.txt" 2>&1; echo "fmt rc=$?"
cargo clippy --release --all-targets >"$OUT/clippy.txt" 2>&1
echo "clippy rc=$? warnings=$(grep -c '^warning' "$OUT/clippy.txt")"
# Unit tests in crates/portuale exec target/release/portuale, which
# `cargo test` does not build: build it first, or they run a stale binary.
cargo build --release >"$OUT/build.txt" 2>&1; echo "build rc=$?"
cargo test --release --no-fail-fast >"$OUT/test.txt" 2>&1; echo "test rc=$?"
grep -E '^test result' "$OUT/test.txt" |
    awk '{p+=$4; f+=$6} END {print "  passed", p, "failed", f, "in", NR, "suites"}'
grep -E '^test .* \.\.\. FAILED$' "$OUT/test.txt" | sed 's/^/  /'

git -C ../pmtest checkout -- fixtures/
git -C ../pmtest clean -fdq fixtures/
(cd ../pmtest && python3 -m pytest pytests-contract-suite -q -rfE -p no:cacheprovider \
    --basetemp="$OUT/pytest") >"$OUT/contract.txt" 2>&1
echo "contract rc=$?"
tail -1 "$OUT/contract.txt" | sed 's/^/  /'
grep -E '^(FAILED|ERROR) ' "$OUT/contract.txt" | sed 's/^/  /'
echo "logs: $OUT"
