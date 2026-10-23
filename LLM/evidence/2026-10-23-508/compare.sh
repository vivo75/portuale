#!/bin/bash
# Compare portage vs portuale captures in <dir> for `emerge --depclean` cycle notices.
# Compared: rc; the stdout lines that carry the cycle notice, the verbose
# "pulled in by" block, the empty-cleanlist message and the summary block
# (Portage's own preamble, the Global Updates listing and the removal-list
# rendering are NOT compared: see backlog residues); stderr with Portage's
# fixture "Update type not recognized" line removed.
d=$1
keep() { grep -E 'is only required by|itself\. Pass|emerge --depclean =|pulled in by|requires dev-libs|No packages selected|To see reverse|^Packages installed|^Packages in|^Required packages|^Number (to remove|removed)'; }
for f in "$d"/*.portage.rc; do id=$(basename ${f%.portage.rc})
  a=$(cat $d/$id.portage.rc); b=$(cat $d/$id.portuale.rc 2>/dev/null)
  if diff <(keep < $d/$id.portage.out) <(keep < $d/$id.portuale.out) >/dev/null; then so=same; else so=DIFF; fi
  if diff <(grep -v 'Update type not recognized' $d/$id.portage.err) $d/$id.portuale.err >/dev/null; then se=same; else se=DIFF; fi
  printf '%-22s rc %s/%s  notice+summary %-4s stderr %s\n' $id "$a" "$b" "$so" "$se"
done
