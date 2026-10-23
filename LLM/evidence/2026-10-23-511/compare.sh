#!/bin/bash
# Compare portage vs portuale captures in <dir> for the #511 mixed-binhost
# shapes. Compared: rc; the stdout merge-list rows (`[binary`/`[ebuild]`,
# the `Total:` line, the no-ebuilds abort); stderr with the binhost
# `ignored:` warning and the `Unsupported binary package format` line
# (Portage's own preamble and `... done!` noise are NOT compared).
d=$1
keep_out() { grep -E '^\[|Total:|no ebuilds|Calculating|These are' || true; }
keep_err() { grep -E 'ignored:|Unsupported|no ebuilds|Maybe you meant|searching for similar' || true; }
for f in "$d"/*.portage.rc; do id=$(basename ${f%.portage.rc})
  a=$(cat $d/$id.portage.rc); b=$(cat $d/$id.portuale.rc 2>/dev/null || echo MISSING)
  if diff <(keep_out < $d/$id.portage.out 2>/dev/null) <(keep_out < $d/$id.portuale.out 2>/dev/null) >/dev/null 2>&1; then so=same; else so=DIFF; fi
  if diff <(keep_err < $d/$id.portage.err 2>/dev/null) <(keep_err < $d/$id.portuale.err 2>/dev/null) >/dev/null 2>&1; then se=same; else se=DIFF; fi
  printf '%-24s rc %s/%s  stdout %-4s stderr %s\n' $id "$a" "$b" "$so" "$se"
done
