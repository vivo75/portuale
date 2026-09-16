#!/usr/bin/env bash
# musl static-build smoke test (see docs/agent-context.md: "Rust CI also gates
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
# The assertions here are **structural** -- exit codes, the exact cpv
# set/order of the `[ebuild ...]` merge list, and key markers/substrings.
# The byte-exact output pins are `tests/test_emerge_pretend_contract.py`'s
# job; this gate's job is "the statically-linked musl binaries resolve and
# refuse correctly in a container with nothing else in it". Keeping the
# two apart is deliberate: this script sat unrun behind the #61 builder
# drift long enough for its pilot-era exact strings to go stale.
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

# Runs a command with stdout+stderr merged into OUT and its status in RC.
# `set -e` would abort the script on the expected-nonzero checks below, so
# every command goes through here instead of a bare `$(...)`.
OUT=""
RC=0
capture() {
    RC=0
    OUT=$("$@" 2>&1) || RC=$?
}

# Real `emerge` / `ebuild` inside the scratch image, against the fixture
# tree copied to /fixtures (see fixtures/ and rust/portage-repo).
run_emerge() {
    "${ENGINE}" run --rm --entrypoint /bin/emerge \
        -e PORTAGE_CONFIGROOT=/fixtures -e ROOT=/fixtures \
        "${IMAGE}" "$@"
}
run_ebuild() {
    "${ENGINE}" run --rm --entrypoint /bin/ebuild "${IMAGE}" "$@"
}
emerge_pretend() {
    run_emerge --pretend "$@"
}

# The cpvs of every `[ebuild ...]` merge-list line, in order (the masked
# marker `#` is part of the bracket text and skipped either way).
merge_list() {
    printf '%s\n' "$1" | sed -n 's/^\[ebuild[^]]*\] \([^ ]*\).*/\1/p'
}
merge_names() {
    merge_list "$1" | tr '\n' ' ' | sed 's/ $//'
}

# The successful `--pretend` shape: rc 0, exactly these cpvs in this
# order, and every needle present somewhere in the output.
assert_pretend_ok() {
    local desc="$1" expected="$2"
    shift 2
    echo "--- ${desc}"
    local ok=1 actual needle
    actual=$(merge_names "${OUT}")
    if [ "${RC}" -ne 0 ]; then
        echo "  rc=${RC}, expected 0" >&2
        ok=0
    fi
    if [ "${actual}" != "${expected}" ]; then
        echo "  merge list: got [${actual}] want [${expected}]" >&2
        ok=0
    fi
    for needle in "$@"; do
        if ! grep -Fq -- "${needle}" <<<"${OUT}"; then
            echo "  output is missing: ${needle}" >&2
            ok=0
        fi
    done
    if [ "${ok}" -eq 0 ]; then
        echo "FAIL: ${desc}" >&2
        fail=1
    fi
    return 0
}

# The refusal shape: exactly this exit status plus every needle.
assert_rc() {
    local desc="$1" expected_rc="$2"
    shift 2
    echo "--- ${desc}"
    local ok=1 needle
    if [ "${RC}" -ne "${expected_rc}" ]; then
        echo "  rc=${RC}, expected ${expected_rc}" >&2
        ok=0
    fi
    for needle in "$@"; do
        if ! grep -Fq -- "${needle}" <<<"${OUT}"; then
            echo "  output is missing: ${needle}" >&2
            ok=0
        fi
    done
    if [ "${ok}" -eq 0 ]; then
        echo "FAIL: ${desc}" >&2
        fail=1
    fi
    return 0
}

echo "Building ${IMAGE} with ${ENGINE} (context: ${REPO_DIR})"
"${ENGINE}" build --no-cache -f "${CONTAINERFILE}" -t "${TAG}" "${REPO_DIR}"

# versions-harness (default ENTRYPOINT): correctness spot check.
actual=$("${ENGINE}" run --rm "${IMAGE}" vercmp 1.0-r1 1.0)
check "versions-harness vercmp via default entrypoint" \
    test "${actual}" = "1"

# emerge --pretend against the fixture tree copied into the image at
# /fixtures: a real resolution, not just dispatch. Merge order is
# dependencies-first -- the cpv list below is the order the scheduler
# would merge in, requested package last.
capture emerge_pretend dev-libs/newpkg
assert_pretend_ok \
    "emerge --pretend resolves a new install inside the scratch container" \
    "dev-libs/newpkg-1.0"

# emerge --pretend recursion (diamond dependency: dedup + merge order).
capture emerge_pretend dev-libs/diamond
assert_pretend_ok \
    "emerge --pretend resolves a dependency graph inside the scratch container" \
    "dev-libs/common-1.0 dev-libs/shared-a-1.0 dev-libs/shared-b-1.0 dev-libs/diamond-1.0"

# emerge --pretend against the real profile chain + make.conf (see
# fixtures/repo/profiles): the multi-parent chain, its
# make.profile symlink, and make.conf's `source /etc/make.local` must all
# survive the COPY into the scratch image and resolve real USE flags,
# which is what gates dev-libs/useflagpkg's dependency on dev-libs/newpkg.
capture emerge_pretend dev-libs/useflagpkg
assert_pretend_ok \
    "emerge --pretend resolves real profile-derived USE flags inside the scratch container" \
    "dev-libs/newpkg-1.0 dev-libs/useflagpkg-1.0" \
    'USE="foo -missingflag"'

# emerge --pretend against package.mask/package.unmask (see
# fixtures/etc/portage/): a masked package stays hidden, and a
# masked-then-unmasked one is visible (with the `#` marker).
capture emerge_pretend dev-libs/hardmaskedpkg
assert_rc \
    "emerge --pretend hides a package.mask-ed package inside the scratch container" \
    1 'All ebuilds that could satisfy "dev-libs/hardmaskedpkg" have been masked' \
    'masked by: package.mask'

capture emerge_pretend dev-libs/maskedandunmaskedpkg
assert_pretend_ok \
    "emerge --pretend respects package.unmask inside the scratch container" \
    "dev-libs/maskedandunmaskedpkg-1.0" \
    '#] dev-libs/maskedandunmaskedpkg-1.0'

# emerge --pretend against package.use (see fixtures/etc/portage/):
# per-package USE overrides, not just the global profile-derived set, must
# survive the COPY into the scratch image.
capture emerge_pretend dev-libs/packageuseenablepkg
assert_pretend_ok \
    "emerge --pretend applies a package.use-enabled flag inside the scratch container" \
    "dev-libs/newpkg-1.0 dev-libs/packageuseenablepkg-1.0" \
    'USE="pkguseflag"'

capture emerge_pretend dev-libs/packageusedisablepkg
assert_pretend_ok \
    "emerge --pretend applies a package.use-disabled flag inside the scratch container" \
    "dev-libs/packageusedisablepkg-1.0" \
    'USE="-foo"'

# emerge --pretend against blockers (see fixtures/etc/portage/ and
# the dev-libs/blockerpkg*/weakblockerpkg/graphblockerparent fixture
# packages): a strong blocker matching an installed package aborts (rc 1,
# real's blocked-packages error), while a weak blocker matching another
# package this same run would also newly merge is reported but does not
# abort.
capture emerge_pretend dev-libs/blockerpkg
assert_rc \
    "emerge --pretend reports a strong blocker against an installed package inside the scratch container" \
    1 '[blocks B' 'hard blocking dev-libs/blockerpkg-1.0' \
    'packages which cannot be' 'installed at the same time'

capture emerge_pretend dev-libs/graphblockerparent
assert_pretend_ok \
    "emerge --pretend reports a weak blocker against an in-graph package inside the scratch container" \
    "dev-libs/blockerpartnerpkg-1.0 dev-libs/weakblockerpkg-1.0 dev-libs/graphblockerparent-1.0" \
    '[blocks B' 'soft blocking'

# emerge --pretend against the overlay repo (see
# fixtures/etc/portage/repos.conf, which registers a second,
# higher-priority repo alongside the main one, and fixtures/overlay):
# an overlay-only package is found, and a same-version tie across both
# repos is broken toward the higher-priority overlay copy.
capture emerge_pretend dev-libs/overlayonlypkg
assert_pretend_ok \
    "emerge --pretend finds an overlay-only package inside the scratch container" \
    "dev-libs/overlayonlypkg-1.0"

capture emerge_pretend dev-libs/overlaytiepkg
assert_pretend_ok \
    "emerge --pretend breaks a same-version repo tie toward the higher-priority overlay inside the scratch container" \
    "dev-libs/newpkg-1.0 dev-libs/overlaytiepkg-1.0"

# emerge --pretend against the slotconflict* fixtures: the two consumers
# need slotconflicttarget at >=2.0 and <2.0, but 1.0 satisfies both atoms,
# so the resolver settles on 1.0 with no conflict (the contract suite
# pins the same resolution) -- and the multislot* pair needs genuinely
# different slots of the same package, which coexist as separate entries,
# not a conflict.
capture emerge_pretend dev-libs/slotconflictparent
assert_pretend_ok \
    "emerge --pretend settles a cross-constraint slot case inside the scratch container" \
    "dev-libs/slotconflicttarget-1.0 dev-libs/slotconflictnewconsumer-1.0 dev-libs/slotconflictoldconsumer-1.0 dev-libs/slotconflictparent-1.0"

capture emerge_pretend dev-libs/multislotparent
assert_pretend_ok \
    "emerge --pretend lets different slots of the same package coexist inside the scratch container" \
    "dev-libs/multislotpkg-1.0 dev-libs/multislotpkg-2.0 dev-libs/multislotparent-1.0"

# emerge --pretend against a virtual (see dev-libs/virtualconsumerpkg and
# virtual/texteditor, shaped exactly like the real virtual/pager): needs
# no dedicated code, just the ordinary category + any-of-group machinery.
capture emerge_pretend dev-libs/virtualconsumerpkg
assert_pretend_ok \
    "emerge --pretend resolves a virtual as a dependency inside the scratch container" \
    "virtual/texteditor-0 dev-libs/virtualconsumerpkg-1.0"

# emerge --pretend against REQUIRED_USE (see dev-libs/requiredusebadpkg):
# a genuinely violated REQUIRED_USE constraint aborts the whole run, not
# just the one package -- real depgraph.py's own severity for this.
capture emerge_pretend dev-libs/requiredusebadpkg
assert_rc \
    "emerge --pretend reports a REQUIRED_USE violation inside the scratch container" \
    1 'has unmet requirements' 'foo? ( bar )' 'USE="foo -bar"'

# CLI surface recognition (see portuale/src/emerge_options.rs): a real
# emerge option this pilot doesn't implement gets a specific message, not
# a generic one, even with nothing else in the image to fall back on.
# (--nobindeps is one such option; if it is ever implemented, pick
# another from emerge_options.rs's tables -- the assertion is the
# message shape, not the flag.)
capture run_emerge --pretend --nobindeps dev-libs/newpkg
assert_rc \
    "emerge reports a real, unimplemented option by name inside the scratch container" \
    2 'is a real emerge option, but is not yet implemented in portuale' \
    '"--nobindeps"'

# ebuild dispatch: the applet's own usage, and a real command name it
# recognizes vs a genuinely invalid one rejected by name, with nothing
# else in the image to fall back on.
capture run_ebuild --help
assert_rc "ebuild prints its own usage" 0 \
    'command-line interface to the Portuale package manager' \
    'ebuild <ebuild file> <command>'

capture run_ebuild foo-1.0.ebuild not-a-real-phase
assert_rc \
    "ebuild rejects an unrecognized command by name inside the scratch container" \
    1 'not one of the valid ebuild commands'

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
