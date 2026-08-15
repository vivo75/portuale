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
PORTING_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
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

echo "Building ${IMAGE} with ${ENGINE} (context: ${PORTING_DIR})"
"${ENGINE}" build --no-cache -f "${CONTAINERFILE}" -t "${TAG}" "${PORTING_DIR}"

# versions-harness (default ENTRYPOINT): correctness spot check.
actual=$("${ENGINE}" run --rm "${IMAGE}" vercmp 1.0-r1 1.0)
check "versions-harness vercmp via default entrypoint" \
    test "${actual}" = "1"

# emerge --pretend against the fixture tree copied into the image at
# /fixtures (see PORTING/fixtures and PORTING/rust/portage-repo): proves
# the real emerge --pretend pilot slice, not just dispatch, works in a
# statically-linked, nothing-but-the-binaries container.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/newpkg)
check "emerge --pretend resolves a new install inside the scratch container" \
    test "${actual}" = "[ebuild  N] dev-libs/newpkg-1.0"

# emerge --pretend recursion (diamond dependency: dedup + discovery order).
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/diamond)
check "emerge --pretend resolves a dependency graph inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/diamond-1.0\n[ebuild  N] dev-libs/shared-a-1.0\n[ebuild  N] dev-libs/shared-b-1.0\n[ebuild  N] dev-libs/common-1.0')"

# emerge --pretend against the real profile chain + make.conf (see
# PORTING/fixtures/repo/profiles): the multi-parent chain, its
# make.profile symlink, and make.conf's `source /etc/make.local` must all
# survive the COPY into the scratch image and resolve real USE flags,
# which is what gates dev-libs/useflagpkg's dependency on dev-libs/newpkg.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/useflagpkg)
check "emerge --pretend resolves real profile-derived USE flags inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/useflagpkg-1.0\n[ebuild  N] dev-libs/newpkg-1.0')"

# emerge --pretend against package.mask/package.unmask (see
# PORTING/fixtures/etc/portage/): a masked package stays hidden, and a
# masked-then-unmasked one is visible, inside the minimal container.
if "${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/hardmaskedpkg >/dev/null 2>&1; then
    masked_exit=0
else
    masked_exit=$?
fi
check "emerge --pretend hides a package.mask-ed package inside the scratch container" \
    test "${masked_exit}" -eq 1

actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/maskedandunmaskedpkg)
check "emerge --pretend respects package.unmask inside the scratch container" \
    test "${actual}" = "[ebuild  N] dev-libs/maskedandunmaskedpkg-1.0"

# emerge --pretend against package.use (see PORTING/fixtures/etc/portage/):
# per-package USE overrides, not just the global profile-derived set, must
# survive the COPY into the scratch image.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/packageuseenablepkg)
check "emerge --pretend applies a package.use-enabled flag inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/packageuseenablepkg-1.0\n[ebuild  N] dev-libs/newpkg-1.0')"

actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/packageusedisablepkg)
check "emerge --pretend applies a package.use-disabled flag inside the scratch container" \
    test "${actual}" = "[ebuild  N] dev-libs/packageusedisablepkg-1.0"

# emerge --pretend against blockers (see PORTING/fixtures/etc/portage/ and
# the dev-libs/blockerpkg*/weakblockerpkg/graphblockerparent fixture
# packages): a strong blocker matching an installed package, and a weak
# blocker matching another package this same run would also newly merge.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/blockerpkg)
check "emerge --pretend reports a strong blocker against an installed package inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/blockerpkg-1.0\n[blocks] dev-libs/blockerpkg-1.0 hard blocks dev-libs/samepkg-1.0 ("!!dev-libs/samepkg")')"

actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/graphblockerparent)
check "emerge --pretend reports a weak blocker against an in-graph package inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/graphblockerparent-1.0\n[ebuild  N] dev-libs/blockerpartnerpkg-1.0\n[ebuild  N] dev-libs/weakblockerpkg-1.0\n[blocks] dev-libs/weakblockerpkg-1.0 soft blocks dev-libs/blockerpartnerpkg-1.0 ("!dev-libs/blockerpartnerpkg")')"

# emerge --pretend against the overlay repo (see
# PORTING/fixtures/etc/portage/repos.conf, which registers a second,
# higher-priority repo alongside the main one, and PORTING/fixtures/overlay):
# an overlay-only package is found, and a same-version tie across both
# repos is broken toward the higher-priority overlay copy.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/overlayonlypkg)
check "emerge --pretend finds an overlay-only package inside the scratch container" \
    test "${actual}" = "[ebuild  N] dev-libs/overlayonlypkg-1.0"

actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/overlaytiepkg)
check "emerge --pretend breaks a same-version repo tie toward the higher-priority overlay inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/overlaytiepkg-1.0\n[ebuild  N] dev-libs/newpkg-1.0')"

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
