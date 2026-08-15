#!/usr/bin/env bash
# musl static-build smoke test (see PORTING/PROMPT.md: "Rust CI also gates
# on a musl static build smoke-tested inside a minimal (scratch/busybox-
# level) container").
#
# Builds Containerfile (a two-stage build: Alpine/musl compiler stage,
# `FROM scratch` runtime stage) and runs the resulting binaries with
# literally nothing else in the image -- no libc, no shell, no busybox --
# proving both the static-linking requirement (hard goal 3: "must run on
# even the most minimal Linux system") and that the multicall dispatch
# mechanism works when invoked as `emerge`/`ebuild`.
#
# Requires podman or docker. Exits nonzero on any failure, so it's usable
# directly as a CI gate.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_DIR="$(cd "${SCRIPT_DIR}/../rust" && pwd)"
CONTAINERFILE="${SCRIPT_DIR}/Containerfile"
TAG="${MUSL_SMOKE_TAG:-portage-rust-musl-smoke:pilot}"

if command -v podman >/dev/null 2>&1; then
    ENGINE=podman
    # Podman stores locally-built, unpushed images under the implicit
    # `localhost/` namespace; referencing them without that prefix forces
    # podman down the registry-resolution path instead of a local lookup.
    IMAGE="localhost/${TAG}"
elif command -v docker >/dev/null 2>&1; then
    ENGINE=docker
    IMAGE="${TAG}"
else
    echo "musl smoke test: neither podman nor docker found on PATH" >&2
    exit 1
fi

fail=0
check() {
    local desc="$1"
    shift
    echo "--- ${desc}"
    if ! "$@"; then
        echo "FAIL: ${desc}" >&2
        fail=1
    fi
}

echo "Building ${IMAGE} with ${ENGINE} (context: ${RUST_DIR})"
"${ENGINE}" build --no-cache -f "${CONTAINERFILE}" -t "${TAG}" "${RUST_DIR}"

# versions-harness (default ENTRYPOINT): correctness spot check.
actual=$("${ENGINE}" run --rm "${IMAGE}" vercmp 1.0-r1 1.0)
check "versions-harness vercmp via default entrypoint" \
    test "${actual}" = "1"

# emerge/ebuild dispatch via argv[0]: the same binary, invoked under two
# different names copied into the image (see Containerfile comment on why
# these are copies rather than symlinks in the scratch stage).
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge "${IMAGE}" --pretend sys-apps/foo)
check "emerge dispatch prints the emerge stub" \
    grep -q "emerge (pilot stub)" <<<"${actual}"

actual=$("${ENGINE}" run --rm --entrypoint /bin/ebuild "${IMAGE}" foo-1.0.ebuild merge)
check "ebuild dispatch prints the ebuild stub" \
    grep -q "ebuild (pilot stub)" <<<"${actual}"

# batch mode inside the minimal container, to make sure stdin plumbing
# works with no shell/coreutils present to help it along.
actual=$(printf 'vercmp 1.0 1.0\nververify 1.0_pre2\n' \
    | "${ENGINE}" run --rm -i --entrypoint /bin/versions-harness "${IMAGE}" batch)
check "batch mode inside scratch container" \
    test "${actual}" = "$(printf '0\nTrue')"

# atom-harness: correctness spot check for the atom-matching pilot slice.
actual=$("${ENGINE}" run --rm --entrypoint /bin/atom-harness "${IMAGE}" \
    match ">=dev-libs/foo-1.2.3" "dev-libs/foo-1.0" "dev-libs/foo-2.0")
check "atom-harness match via explicit entrypoint" \
    test "${actual}" = "dev-libs/foo-2.0"

# use-reduce-harness: correctness spot check for the USE-conditional
# dependency flattening pilot slice.
actual=$("${ENGINE}" run --rm --entrypoint /bin/use-reduce-harness "${IMAGE}" \
    reduce normal bar dev-libs/foo bar? "(" dev-libs/baz ")")
check "use-reduce-harness reduce via explicit entrypoint" \
    test "${actual}" = "dev-libs/foo,dev-libs/baz"

echo
if [ "${fail}" -eq 0 ]; then
    echo "musl smoke test: PASS (image ${IMAGE})"
else
    echo "musl smoke test: FAIL" >&2
fi
exit "${fail}"
