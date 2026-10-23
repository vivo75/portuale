#!/bin/sh
# Pairing and merge-path gate check (plan 02.493 Task 4), run from the portuale
# root (it cds there itself). Exit 1 on a violation, 0 otherwise; warnings
# print "WARN" and do not change the exit code.
#   PMTEST_DIR       sibling checkout (default ../pmtest)
#   BASE             portuale base (default: merge-base HEAD main; on main HEAD^1)
#   PMTEST_BASE      pmtest base, same defaults
#   PAIR_STANDALONE  non-empty reason: waives the same-branch-name check
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

# Base of a repo: merge-base with main, or HEAD^1 when on main itself.
base_of() { # repo branch
    if [ "$2" = main ]; then
        git -C "$1" rev-parse -q --verify HEAD^1 2>/dev/null
    else
        git -C "$1" merge-base HEAD main 2>/dev/null
    fi
}

pbranch=$(git rev-parse --abbrev-ref HEAD)
tbranch=$(git -C "$PMTEST_DIR" rev-parse --abbrev-ref HEAD 2>/dev/null) || {
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
