#!/usr/bin/env bash
# Real-world spot check: runs this pilot's own `emerge --pretend` (the Rust
# portuale binary, dispatched via a real `emerge` symlink -- argv[0]-based,
# same mechanism a real installation uses, see portuale/src/main.rs) against
# the REAL system's own `/usr/bin/emerge`, on the REAL live ROOT/
# PORTAGE_CONFIGROOT (whatever this machine actually has installed) rather
# than the synthetic fixtures tree every other test in this repo
# uses.
#
# This is NOT a correctness gate and is NOT run in CI: real-world trees
# exercise plenty of real emerge behavior this pilot deliberately hasn't
# built yet (no --getbinpkg/binhost support, no default non-"-v" USE
# display, no combined U+D display column, no --autounmask-use, no explicit
# repos.conf masters=, etc. -- see README.md's own "What this
# proves" section for the maintained list of what's actually in scope).
# Divergence here is expected and informative, not a bug report -- the
# point is to see how far real-world data agrees with the pilot's own core
# resolution decisions (New / Upgrade / Downgrade / Reinstall /
# AlreadyInstalled), on packages neither implementation was ever hand-tuned
# against, not to chase byte-for-byte parity with real emerge's own display
# formatting.
#
# Usage:
#   scripts/real_world_spotcheck.sh [atom ...]
#
# With no arguments, checks a small, diverse default set of real packages
# (drawn from whatever's actually installed on this machine at the time
# this script was written -- edit DEFAULT_ATOMS below, or just pass your
# own atoms, if none of these exist on your system). Every atom is run
# through both binaries with `--pretend --nodeps` (nodeps keeps each check
# isolated to one package's own top-level resolution, avoiding the
# dependency-recursion cascade this pilot's own lack of binhost support can
# trigger on a system that tracks one -- see the README).
#
# Requires real emerge on PATH (a real Gentoo system) and a built pilot
# binary (built automatically below if missing).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
RUST_DIR="${REPO_DIR}/rust"
PILOT_BIN="${RUST_DIR}/target/release/portuale"

REAL_EMERGE="$(command -v emerge || true)"
if [ -z "${REAL_EMERGE}" ]; then
    echo "real_world_spotcheck: no real 'emerge' found on PATH -- this script needs an actual Gentoo system" >&2
    exit 1
fi

if [ ! -x "${PILOT_BIN}" ]; then
    echo "Building pilot binary (cargo build --release)..."
    (cd "${RUST_DIR}" && cargo build --release) || exit 1
fi

SYMLINK_DIR="$(mktemp -d)"
trap 'rm -rf "${SYMLINK_DIR}"' EXIT
ln -s "${PILOT_BIN}" "${SYMLINK_DIR}/emerge"
PILOT_EMERGE="${SYMLINK_DIR}/emerge"

# A small, deliberately diverse default set: some already-installed leaf
# tools (expect Reinstall/AlreadyInstalled), and a couple of common enough
# packages to plausibly have a visible upgrade/downgrade on most systems.
# Not every atom needs to exist on every machine -- a "no ebuilds to
# satisfy" from either side is reported like any other outcome, not treated
# as a script error.
DEFAULT_ATOMS=(
    sys-apps/coreutils
    sys-apps/busybox
    app-editors/nano
    dev-vcs/git
    app-portage/eix
)

if [ "$#" -gt 0 ]; then
    ATOMS=("$@")
else
    ATOMS=("${DEFAULT_ATOMS[@]}")
fi

# Reduces one `--pretend --nodeps` run's own stdout to just its bracket
# lines (drops real emerge's own banner/progress/timing chatter, which this
# pilot never prints at all) and collapses whitespace runs to single spaces
# (real emerge pads columns; this pilot doesn't) so the two sides can be
# compared on substance, not incidental spacing.
normalize() {
    grep -E '^\[' <<<"$1" | tr -s ' '
}

echo "Real emerge:  ${REAL_EMERGE}"
echo "Pilot emerge: ${PILOT_BIN} (via ${PILOT_EMERGE})"
echo

agree=0
differ=0
for atom in "${ATOMS[@]}"; do
    echo "=== ${atom} ==="
    real_out="$("${REAL_EMERGE}" --pretend --nodeps "${atom}" 2>&1)"
    real_status=$?
    pilot_out="$("${PILOT_EMERGE}" --pretend --nodeps "${atom}" 2>&1)"
    pilot_status=$?

    real_norm="$(normalize "${real_out}")"
    pilot_norm="$(normalize "${pilot_out}")"

    echo "--- real (exit ${real_status}) ---"
    echo "${real_out}" | sed -n '1,15p'
    echo "--- pilot (exit ${pilot_status}) ---"
    echo "${pilot_out}"

    if [ "${real_norm}" = "${pilot_norm}" ]; then
        echo "=> MATCH (bracket lines identical after normalization)"
        agree=$((agree + 1))
    else
        echo "=> TEXT DIFFERS -- compare the two blocks above by eye; a differing bracket word or missing USE string is usually a known scope gap (see README.md), not a bug"
        differ=$((differ + 1))
    fi
    echo
done

echo "Summary: ${agree} matched, ${differ} differed (of ${#ATOMS[@]})"
echo "(Differences are expected for anything outside this pilot's documented scope -- this is informational, not a pass/fail gate.)"
