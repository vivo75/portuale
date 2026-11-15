#!/bin/sh
# Pairing and merge-path gate check (plan 02.493 Task 4), run from the portuale
# root (it cds there itself). Exit 1 on a violation, 0 otherwise; warnings
# print "WARN" and do not change the exit code.
#   PMTEST_DIR        sibling checkout (default ../pmtest)
#   BASE              portuale base (default below; on main itself HEAD^1)
#   PMTEST_BASE       pmtest base, same default
#   PAIR_INTEGRATION  the branch this project merges into (default: opencode
#                     if the repo has it, else main)
#   PAIR_STANDALONE   non-empty reason: waives the same-branch-name check
#
# The default base is the point where the branch left the integration branch --
# merge-base --fork-point on refs/heads/<integration> when it answers and is an
# ancestor of HEAD, else plain merge-base against that ref (local, then
# origin/<integration>). Basing on `main` made every branch cut from `opencode`
# inherit `opencode`'s merge-path obligation, which is how #756 -- a slice
# touching only LLM/tools/ -- was asked for glibc+bash merge evidence (#758).
set -u
cd "$(dirname "$0")/../.." || exit 2
PMTEST_DIR=${PMTEST_DIR:-../pmtest}

# The merge path as path prefixes (one place). Keep in step with
# LLM/agent-context.md "Merge-path safety gate".
MERGE_PATH="crates/portuale/src/ebuild_merge/
crates/portuale/src/emerge_build/
crates/portuale/src/emerge_getbinpkg.rs
crates/portuale/src/ebuild_unmerge.rs"

fail=0
bad() { echo "pair-check: $1"; fail=1; }
warn() { echo "pair-check: WARN $1"; }

# The branch this project integrates into: PAIR_INTEGRATION when set, else
# `opencode` if the repo has it, else `main`. Prints a full ref name.
integration_ref() { # repo [branch]
    name=${PAIR_INTEGRATION:-opencode}
    # On the integration branch itself its own tip is an empty range: anchor
    # at `main` instead (#762).
    [ "${2:-}" = "$name" ] && name=main
    if git -C "$1" rev-parse -q --verify "refs/heads/$name" >/dev/null 2>&1; then
        printf 'refs/heads/%s\n' "$name"
    elif git -C "$1" rev-parse -q --verify "refs/remotes/origin/$name" >/dev/null 2>&1; then
        printf 'refs/remotes/origin/%s\n' "$name"
    else
        printf 'refs/heads/main\n'
    fi
}

# Base of a repo: HEAD^1 on `main`, else the point where this branch left the
# integration branch. `--fork-point` answers only for a LOCAL ref (a
# remote-tracking ref has no branching reflog, and a reflog-less checkout makes
# it fail empty: `fatal: No such ref`), so it is tried on refs/heads/ only and
# accepted only when it is an ancestor of HEAD. Everything else uses plain
# merge-base, which can over-report (a stale origin/<branch> widens the range)
# but can never hide a slice's own commit.
base_of() { # repo branch
    if [ "$2" = main ]; then
        git -C "$1" rev-parse -q --verify HEAD^1 2>/dev/null
        return
    fi
    ref=$(integration_ref "$1" "$2")
    case $ref in
        refs/heads/*)
            fp=$(git -C "$1" merge-base --fork-point "$ref" HEAD 2>/dev/null)
            if [ -n "$fp" ] && git -C "$1" merge-base --is-ancestor "$fp" HEAD 2>/dev/null; then
                printf '%s\n' "$fp"
                return
            fi
            ;;
    esac
    git -C "$1" merge-base HEAD "$ref" 2>/dev/null
}

# The short branch name, with the refs/heads/ (or remotes/<remote>/) prefix
# stripped: `rev-parse --abbrev-ref` yields `heads/<name>` whenever the same
# branch also exists as a remote-tracking ref, which would make the evidence
# file below `merge-gate-heads-<name>.md` and fail the documented convention.
# A detached HEAD has no symbolic ref: name it from the local branch that
# contains it (`name-rev`, with `~N`/`^N` stripped).
short_branch() {
    b=$(git -C "$1" symbolic-ref --quiet --short HEAD 2>/dev/null)
    [ -n "$b" ] || b=$(git -C "$1" name-rev --name-only --no-undefined --refs='refs/heads/*' HEAD 2>/dev/null |
        sed 's#[~^].*$##')
    printf '%s\n' "$b" | sed 's#^heads/##; s#^remotes/[^/]*/##'
}
pbranch=$(short_branch .)
tbranch=$(short_branch "$PMTEST_DIR") || {
    echo "pair-check: cannot read pmtest at $PMTEST_DIR"; exit 2; }

# (a) same branch name
if [ "$pbranch" != "$tbranch" ]; then
    if [ -n "${PAIR_STANDALONE:-}" ]; then
        echo "pair-check: standalone ($PAIR_STANDALONE): portuale=$pbranch pmtest=$tbranch"
    elif [ "$pbranch" = main ]; then
        echo "pair-check: portuale on main, pmtest on $tbranch: accepted"
    else
        bad "branch mismatch: portuale=$pbranch pmtest=$tbranch (set PAIR_STANDALONE=<reason> for a single-repo change)"
    fi
fi

BASE=${BASE:-$(base_of . "$pbranch")}
PMTEST_BASE=${PMTEST_BASE:-$(base_of "$PMTEST_DIR" "$tbranch")}
# The range a reader cannot see is how this checker over-reported for two
# slices: name each base and its anchor.
echo "pair-check: base ${BASE} (portuale, anchor $(integration_ref . "$pbranch"))"
echo "pair-check: base ${PMTEST_BASE} (pmtest, anchor $(integration_ref "$PMTEST_DIR" "$tbranch"))"

# (b) merge-path diff needs LLM/evidence/merge-gate-<branch with / -> ->.md
if [ -n "$BASE" ]; then
    touched=
    for prefix in $MERGE_PATH; do
        if [ -n "$(git diff --name-only "$BASE" HEAD -- "$prefix")" ]; then
            touched="$touched $prefix"
        fi
    done
    if [ -n "$touched" ]; then
        ev="LLM/evidence/merge-gate-$(printf '%s' "$pbranch" | tr / -).md"
        if [ -f "$ev" ] && grep -q 'sys-libs/glibc' "$ev" && grep -q 'app-shells/bash' "$ev"; then
            echo "pair-check: merge path touched ($touched ), gate evidence $ev"
        else
            bad "merge path touched ($touched ) but $ev is missing or does not name both sys-libs/glibc and app-shells/bash merge results"
        fi
    fi
else
    warn "no portuale base: merge-path check skipped"
fi

# (c) pmtest corpus commits must say "corpus bless"
if [ -n "$PMTEST_BASE" ]; then
    for c in $(git -C "$PMTEST_DIR" rev-list "$PMTEST_BASE..HEAD" -- pytests-contract-suite/corpus/); do
        if ! git -C "$PMTEST_DIR" log -1 --format=%B "$c" | grep -qi 'corpus bless'; then
            bad "pmtest commit $(git -C "$PMTEST_DIR" rev-parse --short=7 "$c") touches corpus/ without 'corpus bless' in its message"
        fi
    done
    # (d) warn: portuale commits that quote no pmtest short sha
    shas=$(git -C "$PMTEST_DIR" rev-list --abbrev-commit --abbrev=7 "$PMTEST_BASE..HEAD")
    if [ -n "$shas" ] && [ -n "$BASE" ]; then
        for c in $(git rev-list "$BASE..HEAD"); do
            msg=$(git log -1 --format=%B "$c")
            found=
            for s in $shas; do
                case $msg in *"$s"*) found=1; break ;; esac
            done
            [ -n "$found" ] || warn "portuale commit $(git rev-parse --short=7 "$c") quotes no pmtest short sha (pmtest has commits in $PMTEST_BASE..HEAD)"
        done
    fi
else
    warn "no pmtest base: corpus and sha checks skipped"
fi

[ "$fail" -eq 0 ] && echo "pair-check: OK"
exit "$fail"
