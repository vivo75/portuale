#!/bin/bash
# Shared helpers for the TEST/run/l*.sh orchestrators.
# Source, don't execute.

set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
TEST_DIR="$REPO_ROOT/TEST"
LOGS_DIR="$TEST_DIR/logs"

IMAGE=${PORTTEST_IMAGE:-localhost/test-portuale:latest}
MRG_CLIENT_IMAGE=${PORTTEST_MRG_CLIENT_IMAGE:-localhost/test-mrg-client:latest}
NET=${PORTTEST_NET:-porttest-net}

# podman that can build (root containers are fine -- doc real-world-testing.md §1.10)
PODMAN=${PORTTEST_PODMAN:-podman}

# Common `podman run` args for a portuale-capable throwaway container.
# The portuale binaries are mounted from the host build dir; the whole
# repo is mounted at its build-time path so `ebuild_phases::repo_root()`
# (a compile-time CARGO_MANIFEST_DIR/../.. path) resolves for L1+ phase
# execution. L0 never runs phases but the mount is harmless.
podman_run_portuale() {
  local name=$1; shift
  "$PODMAN" run --rm --name "$name" \
    --security-opt seccomp=unconfined \
    --cgroups=enabled --cgroupns=private \
    -v "$REPO_ROOT/rust/target/release:/usr/local/bin:ro" \
    -v "$REPO_ROOT:$REPO_ROOT:ro" \
    -v "$TEST_DIR:/TEST:ro" \
    -v "$LOGS_DIR:/TEST/logs" \
    "$@"
}

ensure_portuale_built() {
  echo ">>> building portuale (release)"
  ( cd "$REPO_ROOT/rust" && cargo build --release -p portuale )
  local rel="$REPO_ROOT/rust/target/release"
  for l in emerge ebuild mrg; do
    [ -e "$rel/$l" ] || ln -s portuale "$rel/$l"
  done
}

ensure_image() {
  if ! "$PODMAN" image exists "$IMAGE"; then
    echo "!!! image $IMAGE missing -- build it with:  sudo TEST/create-container.bash" >&2
    exit 2
  fi
}

timestamp() { date -u +%Y%m%dT%H%M%SZ; }
