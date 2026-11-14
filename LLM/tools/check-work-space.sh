#!/bin/sh
# Preflight before a test run that writes a lot (parallel pytest, an L0/L1 bed,
# a corpus run): /tmp and /var/tmp must each have at least MIN_PERCENT free.
#
# usage: sh LLM/tools/check-work-space.sh [min_percent]   (default 50)
# exit 0 = both mounts have enough room; 1 = one is below the threshold;
# 2 = usage error. Read-only: it never deletes anything.
#
# Why: one full contract-suite run leaves ~11 GB in its PMTEST_BASETEMP under
# /var/tmp (a 48 GB mount), so two or three stale runs fill it and the next
# suite fails with ENOSPC, not with a real test failure.
set -u
MIN=${1:-50}
case $MIN in
    ''|*[!0-9]*) echo "check-work-space: min_percent must be a number" >&2; exit 2 ;;
esac
[ "$MIN" -ge 1 ] && [ "$MIN" -le 99 ] || {
    echo "check-work-space: min_percent must be 1..99" >&2; exit 2
}

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
    pc=$(( free * 100 / total ))
    verdict=ok
    [ "$pc" -lt "$MIN" ] && { verdict=LOW; status=1; }
    printf '%-10s %8s %8s %5s%% %s\n' "$d" \
        "$(awk -v k="$total" 'BEGIN {printf "%.1fG", k/1048576}')" \
        "$(awk -v k="$free" 'BEGIN {printf "%.1fG", k/1048576}')" \
        "$pc" "$verdict"
done

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
