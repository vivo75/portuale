#!/usr/bin/env bash
# musl static-build smoke test (see PROMPT.md: "Rust CI also gates
# on a musl static build smoke-tested inside a minimal (scratch/busybox-
# level) container").
#
# Builds Containerfile (a two-stage build: Alpine/musl compiler stage,
# `FROM scratch` runtime stage) and runs the resulting binaries with
# literally nothing else in the image -- no libc, no shell, no busybox --
# proving both the static-linking requirement (hard goal 3: "must run on
# even the most minimal Linux system") and that the portuale dispatch
# mechanism works when invoked as `emerge`/`ebuild`.
#
# Requires podman or docker. Exits nonzero on any failure, so it's usable
# directly as a CI gate.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
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

echo "Building ${IMAGE} with ${ENGINE} (context: ${REPO_DIR})"
"${ENGINE}" build --no-cache -f "${CONTAINERFILE}" -t "${TAG}" "${REPO_DIR}"

# versions-harness (default ENTRYPOINT): correctness spot check.
actual=$("${ENGINE}" run --rm "${IMAGE}" vercmp 1.0-r1 1.0)
check "versions-harness vercmp via default entrypoint" \
    test "${actual}" = "1"

# emerge --pretend against the fixture tree copied into the image at
# /fixtures (see fixtures and rust/portage-repo): proves
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
# fixtures/repo/profiles): the multi-parent chain, its
# make.profile symlink, and make.conf's `source /etc/make.local` must all
# survive the COPY into the scratch image and resolve real USE flags,
# which is what gates dev-libs/useflagpkg's dependency on dev-libs/newpkg.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/useflagpkg)
check "emerge --pretend resolves real profile-derived USE flags inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/useflagpkg-1.0\n[ebuild  N] dev-libs/newpkg-1.0')"

# emerge --pretend against package.mask/package.unmask (see
# fixtures/etc/portage/): a masked package stays hidden, and a
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

# emerge --pretend against package.use (see fixtures/etc/portage/):
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

# emerge --pretend against blockers (see fixtures/etc/portage/ and
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
# fixtures/etc/portage/repos.conf, which registers a second,
# higher-priority repo alongside the main one, and fixtures/overlay):
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

# emerge --pretend against slot conflicts (see the dev-libs/slotconflict*/
# multislot* fixture packages): a genuine conflict (two atoms needing the
# same slot at incompatible versions) is reported, while two atoms
# needing genuinely different slots of the same package correctly
# coexist as separate entries, not a conflict.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/slotconflictparent)
check "emerge --pretend reports a slot conflict inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/slotconflictparent-1.0\n[ebuild  N] dev-libs/slotconflictnewconsumer-1.0\n[ebuild  N] dev-libs/slotconflictoldconsumer-1.0\n[ebuild  N] dev-libs/slotconflicttarget-2.0\n[slot conflict] dev-libs/slotconflicttarget:0 resolved to dev-libs/slotconflicttarget-2.0, which does not satisfy "<dev-libs/slotconflicttarget-2.0"')"

actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/multislotparent)
check "emerge --pretend lets different slots of the same package coexist inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/multislotparent-1.0\n[ebuild  N] dev-libs/multislotpkg-1.0\n[ebuild  N] dev-libs/multislotpkg-2.0')"

# emerge --pretend against a virtual (see dev-libs/virtualconsumerpkg and
# virtual/texteditor, shaped exactly like the real virtual/pager): needs
# no dedicated code, just the ordinary category + any-of-group machinery.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/virtualconsumerpkg)
check "emerge --pretend resolves a virtual as a dependency inside the scratch container" \
    test "${actual}" = "$(printf '[ebuild  N] dev-libs/virtualconsumerpkg-1.0\n[ebuild  N] virtual/texteditor-0\n[ebuild  N] dev-libs/newpkg-1.0')"

# emerge --pretend against REQUIRED_USE (see dev-libs/requiredusebadpkg):
# a genuinely violated REQUIRED_USE constraint aborts the whole run, not
# just the one package -- real depgraph.py's own severity for this.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge \
    -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
    "${IMAGE}" --pretend dev-libs/requiredusebadpkg 2>&1 || true)
check "emerge --pretend reports a REQUIRED_USE violation inside the scratch container" \
    grep -q 'REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0' <<<"${actual}"

# CLI surface recognition (see portuale/src/emerge_options.rs): a real
# emerge option this pilot doesn't implement gets a specific message,
# not a generic one, even with nothing else in the image to fall back on.
actual=$("${ENGINE}" run --rm --entrypoint /bin/emerge "${IMAGE}" --jobs dev-libs/newpkg 2>&1 || true)
check "emerge reports a real, unimplemented option by name inside the scratch container" \
    grep -q 'option "--jobs" is a real emerge option' <<<"${actual}"

actual=$("${ENGINE}" run --rm --entrypoint /bin/ebuild "${IMAGE}" foo-1.0.ebuild merge)
check "ebuild dispatch prints the ebuild stub" \
    grep -q "ebuild (pilot stub)" <<<"${actual}"

# ebuild CLI surface recognition (see portuale/src/ebuild_options.rs): a
# real ebuild command this pilot doesn't implement is still accepted as
# a no-op (real phase execution is deferred, not this), but a genuinely
# invalid command name is rejected clearly, even with nothing else in
# the image to fall back on.
actual=$("${ENGINE}" run --rm --entrypoint /bin/ebuild "${IMAGE}" foo-1.0.ebuild not-a-real-phase 2>&1 || true)
check "ebuild rejects an unrecognized command by name inside the scratch container" \
    grep -q 'not one of the valid ebuild commands' <<<"${actual}"

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

# required-use-harness: correctness spot check for the REQUIRED_USE
# pilot slice.
actual=$("${ENGINE}" run --rm --entrypoint /bin/required-use-harness "${IMAGE}" \
    check foo foo,bar "foo?" "(" bar ")")
check "required-use-harness check via explicit entrypoint" \
    test "${actual}" = "false"

echo
if [ "${fail}" -eq 0 ]; then
    echo "musl smoke test: PASS (image ${IMAGE})"
else
    echo "musl smoke test: FAIL" >&2
fi
exit "${fail}"
