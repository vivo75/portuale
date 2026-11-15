#!/bin/sh
# Preflight before a test run that writes a lot (parallel pytest, an L0/L1 bed,
# a corpus run): /tmp and /var/tmp must each have at least MIN_PERCENT of their
# space and MIN_INODES free inodes.
#
# usage: sh LLM/tools/check-work-space.sh [min_percent]        (default 50)
#        sh LLM/tools/check-work-space.sh --require-inodes <path> [min]
#        sh LLM/tools/check-work-space.sh --basetemp <path> [min_percent]
#            (both: the space tables plus the run-size inode guard on <path>)
# exit 0 = enough room; 1 = below a threshold; 2 = usage error. A filesystem
# that reports 0 inodes in total (btrfs, zfs) is not sized: WARN, not a refusal.
# Read-only: it never deletes and never creates anything.
#
# Why space: one full contract-suite run leaves ~11 GB in its PMTEST_BASETEMP
# under /var/tmp (a 48 GB mount), so two or three stale runs fill it and the
# next suite fails with ENOSPC, not with a real test failure.
#
# Why inodes (backlog #756): a full run's basetemp needs ~1.8 M inodes
# (measured 2026-11-15: 1 804 465 for 4967 tests). `/tmp` here is a tmpfs with
# 1 048 576 in total, so a basetemp there exhausts the inode table while
# gigabytes are still free and the suite reports ENOSPC as if the product
# were broken. The 200 000 floor below is a sanity floor ("this mount is in
# trouble"), NOT a run size: the run-size guard is `--basetemp`/`--require-inodes`, which
# `gate.sh` and pmtest's `pytest-parallel.sh` call on the basetemp they will
# actually use.
set -u
MIN_INODES=${PORTUALE_MIN_FREE_INODES:-200000}

usage() {
    echo "check-work-space: $1" >&2
    exit 2
}

case ${MIN_INODES} in
    ''|*[!0-9]*) usage "PORTUALE_MIN_FREE_INODES must be a number" ;;
esac

# Resolve the filesystem that will hold <path>: the nearest existing ancestor,
# so a not-yet-created basetemp is judged without creating it.
fs_of() {
    p=$1
    while [ -n "$p" ] && [ "$p" != "/" ] && [ ! -e "$p" ]; do
        p=$(dirname "$p")
    done
    [ -e "$p" ] || return 1
    printf '%s\n' "$p"
}

# The run-size inode guard: does the filesystem holding <path> have <min> free
# inodes? Returns 1 after printing the refusal; usage errors exit 2. A
# filesystem that reports 0 inodes in total (btrfs, zfs allocate them on
# demand) cannot be sized: say so with WARN and let the run through.
run_inodes() { # path min
    case $2 in
        ''|*[!0-9]*) usage "the inode minimum (PMTEST_MIN_BASETEMP_INODES) must be a positive integer (got '$2')" ;;
    esac
    [ "$2" -gt 0 ] || usage "the inode minimum (PMTEST_MIN_BASETEMP_INODES) must be greater than 0 (got $2)"
    anchor=$(fs_of "$1")
    # `usage` inside the command substitution above would only exit that
    # subshell, leaving $anchor empty: say it here, where the exit is real.
    [ -n "$anchor" ] || usage "cannot resolve a filesystem for '$1'"
    # `set --` rather than `read < <(...)`: this script is /bin/sh (dash has
    # no process substitution).
    set -- "$1" "$2" $(df -Pi "$anchor" 2>/dev/null | awk 'NR==2 {print $2, $4}')
    i_total=${3:-}
    i_free=${4:-}
    if [ -z "$i_total" ] || [ -z "$i_free" ]; then
        echo "check-work-space: cannot read inode counts for '$anchor'" >&2
        return 1
    fi
    if [ "$i_total" -eq 0 ]; then
        echo "check-work-space: WARN '$anchor' reports 0 inodes (allocated on demand); inode check skipped" >&2
        return 0
    fi
    if [ "$i_free" -lt "$2" ]; then
        echo "check-work-space: '$1' is on a filesystem with only ${i_free} free inodes (need $2)" >&2
        echo "  a complete suite run needs ~1.8 M inodes (backlog #756); a 1 M-inode tmpfs like /tmp cannot hold one" >&2
        echo "  point the basetemp at a bigger filesystem, or lower the floor deliberately: PMTEST_MIN_BASETEMP_INODES=<n>" >&2
        return 1
    fi
    return 0
}

run_status=0
case ${1:-} in
    --require-inodes)
        [ $# -ge 2 ] || usage "--require-inodes needs a path"
        run_inodes "$2" "${3:-${PMTEST_MIN_BASETEMP_INODES:-2500000}}"
        exit $?
        ;;
    --basetemp)
        # The space tables below plus the run-size guard on <path>, in one
        # call (gate.sh and pytest-parallel.sh).
        [ $# -ge 2 ] || usage "--basetemp needs a path"
        run_inodes "$2" "${PMTEST_MIN_BASETEMP_INODES:-2500000}" || run_status=1
        shift 2
        ;;
esac

MIN=${1:-50}
case $MIN in
    ''|*[!0-9]*) usage "min_percent must be a number" ;;
esac
[ "$MIN" -ge 1 ] && [ "$MIN" -le 99 ] || usage "min_percent must be 1..99"

status=0
printf '%-10s %8s %8s %6s %s\n' mount total free use threshold
for d in /tmp /var/tmp; do
    # -P: one line per filesystem, no wrapping; -k: 1K blocks.
    set -- $(df -Pk "$d" 2>/dev/null | awk 'NR==2 {print $2, $4, $5}')
    if [ $# -lt 2 ]; then
        printf '%-10s %s\n' "$d" "cannot read (not mounted?)"
        status=1
        continue
    fi
    total=$1
    free=$2
    if [ "$total" -eq 0 ]; then
        printf '%-10s %s\n' "$d" "0 blocks reported (cannot be sized)"
        status=1
        continue
    fi
    pc=$(( free * 100 / total ))
    verdict=ok
    [ "$pc" -lt "$MIN" ] && { verdict=LOW; status=1; }
    printf '%-10s %8s %8s %5s%% %s\n' "$d" \
        "$(awk -v k="$total" 'BEGIN {printf "%.1fG", k/1048576}')" \
        "$(awk -v k="$free" 'BEGIN {printf "%.1fG", k/1048576}')" \
        "$pc" "$verdict"
done

printf '\ninodes (sanity floor %s; the run-size guard is --require-inodes):\n' "$MIN_INODES"
printf '%-10s %10s %10s %6s %s\n' mount total free use threshold
for d in /tmp /var/tmp; do
    set -- $(df -Pi "$d" 2>/dev/null | awk 'NR==2 {print $2, $4, $5}')
    if [ $# -lt 2 ]; then
        printf '%-10s %s\n' "$d" "cannot read (not mounted?)"
        status=1
        continue
    fi
    i_total=$1
    i_free=$2
    if [ "$i_total" -eq 0 ]; then
        printf '%-10s %s\n' "$d" "WARN 0 inodes reported (allocated on demand); not checked"
        continue
    fi
    ipc=$(( i_free * 100 / i_total ))
    iverdict=ok
    [ "$i_free" -lt "$MIN_INODES" ] && { iverdict=LOW; status=1; }
    printf '%-10s %10s %10s %5s%% %s\n' "$d" "$i_total" "$i_free" "$ipc" "$iverdict"
done

[ "$run_status" -eq 0 ] || status=1
if [ "$status" -ne 0 ]; then
    echo
    echo "check-work-space: not enough free space; a big test run would fail with ENOSPC."
    echo "Largest entries (age is the last modification, newest last):"
    for d in /tmp /var/tmp; do
        # Only the top-level entries: cheap, and a stale basetemp is always one.
        du -xk -d1 "$d" 2>/dev/null | sort -rn | head -6 | while read -r kb path; do
            [ "$path" = "$d" ] && continue
            printf '  %8s  %s  %s\n' "$(awk -v k="$kb" 'BEGIN {printf "%.1fG", k/1048576}')" \
                "$(date -r "$path" '+%m-%d %H:%M' 2>/dev/null)" "$path"
        done
    done
    echo
    echo "Delete only directories no live run uses. A run started through"
    echo "LLM/tools/scope/scoperun.sh can be checked with ocgrep.sh/ocstatus.sh;"
    echo "never decide with ps/pgrep, and never delete the PMTEST_BASETEMP of a"
    echo "suite that is still running."
fi
exit "$status"
