#!/bin/bash
# Compare the #531 captures in <dir>: rc, the `masked by:` line carrying the
# dependency error, and the message's exact text. The `!!!` header lines are
# compared too (they are the same on both sides).
#
# Usage: compare.sh <dir> [<dir> ...]
set -u
for d in "$@"; do
  for id in 01-rdepend-bad-dep; do
    prc=$(cat "$d/$id.portage.rc" 2>/dev/null)
    urc=$(cat "$d/$id.portuale.rc" 2>/dev/null)
    perr=$(grep -a 'masked by: invalid:' "$d/$id.portage.err" 2>/dev/null)
    uerr=$(grep -a 'masked by: invalid:' "$d/$id.portuale.err" 2>/dev/null)
    rc="DIFF"; [ "$prc" = "$urc" ] && rc="same"
    msg="DIFF"; [ "$perr" = "$uerr" ] && msg="same"
    printf '%-22s rc %s/%s  message %s\n' "$id" "$prc" "$urc" "$msg"
    if [ "$msg" = DIFF ]; then
      printf '  portage : %s\n  portuale: %s\n' "$perr" "$uerr"
    fi
  done
done
