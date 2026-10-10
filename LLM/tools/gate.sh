#!/bin/sh
# Full verification gate (AGENTS.md step 8), run from the portuale repo root.
# Writes logs to $1 (default: a fresh mktemp dir) and prints a one-screen
# summary: fmt/clippy status, cargo test totals + failing names, contract
# suite tail + failing names. Never run it while a bed run is active.
set -u
cd "$(dirname "$0")/../.." || exit 2
OUT=${1:-$(mktemp -d "${TMPDIR:-/var/tmp}/gate.XXXXXX")}
mkdir -p "$OUT"

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

git -C ../pmtest clean -fdq fixtures/
(cd ../pmtest && python3 -m pytest pytests-contract-suite -q -rfE -p no:cacheprovider \
    --basetemp=/var/tmp/pytest-readability) >"$OUT/contract.txt" 2>&1
echo "contract rc=$?"
tail -1 "$OUT/contract.txt" | sed 's/^/  /'
grep -E '^(FAILED|ERROR) ' "$OUT/contract.txt" | sed 's/^/  /'
echo "logs: $OUT"
