#!/bin/bash
# Compare portage vs portuale captures in <dir> for the user-patches
# oracle (#509). Compared: rc; the merge-list rows; the ignored-binaries
# block, the verbose patch tail and the NOTE; the masked-abort block and
# mask reason. NOT compared: the Global Updates listing, the fixture
# `updates` error, the `to <root>` vs `for <root>` pretend convention
# (#425: Portage `for <root>/`, portuale `to <root>`), the `R` vs `N`
# outcome letter when the shape's ROOT already has the package installed,
# the resolution timing, and the stray-file shape's stderr beyond rc.
d=$1
keep_out() { grep -E '^\[(binary|ebuild)|^Total|^Number|Calculating skipped|Dependency skipped' "$1"; }
keep_err() { grep -vE 'Update type not recognized|^Performing|Could take|^  [.=sp#%@]|^/|^[0-9]Q-|^$|^Calculating|^Dependency|^These are' "$1"; }
for f in "$d"/*.portage.rc; do id=$(basename ${f%.portage.rc})
  [ -f "$d/$id.portuale.rc" ] || continue
  a=$(cat $d/$id.portage.rc); b=$(cat $d/$id.portuale.rc)
  if diff <(keep_out $d/$id.portage.out | sed 's/ R    \]/ N    ]/;s/ to .*root.*/ to ROOT/') \
          <(keep_out $d/$id.portuale.out | sed 's/ R    \]/ N    ]/;s/ to .*root.*/ to ROOT/') >/dev/null; then so=same; else so=DIFF; fi
  if diff <(keep_err $d/$id.portage.err | sed 's/ for .*root.\// for ROOT/') \
          <(keep_err $d/$id.portuale.err | sed 's/ for .*root.\// for ROOT/') >/dev/null; then se=same; else se=DIFF; fi
  printf '%-18s rc %s/%s  rows %-4s stderr %s\n' $id "$a" "$b" "$so" "$se"
done
