#!/bin/bash
# Compare Portage (*.portage.*) and portuale (*.portuale.*) captures in <dir> for #513.
# Portage's installed module path is normalised away on its side (the Python
# warnings line starts with the interpreter's install prefix, which portuale
# cannot know; residue: the location is `portage/package/...:54`).
# Per shape: rc; `virtuals` = the virtuals lines (UserWarning pair and the
# /etc/portage/virtuals block) compared exactly; `other` = the rest of stderr
# with those lines and Portage's fixture-only "Update type not recognized"
# line removed. `other` DIFFs are pre-existing differences, not virtuals ones.
d=$1
pat='UserWarning|  warnings\.warn\(|^!!! /etc/portage/virtuals|^!!! Support for PROVIDE|^!!! portage-2\.3\.25|^!!! instead\. Please remove'
virt() { sed 's#/usr/lib/python3\.14/site-packages/##' "$1" | grep -E "$pat"; }
rest() { sed 's#/usr/lib/python3\.14/site-packages/##' "$1" | grep -v -E "$pat" | grep -v 'Update type not recognized' | grep -v '^$'; }
for f in "$d"/*.portage.rc; do id=$(basename ${f%.portage.rc})
  a=$(cat $d/$id.portage.rc); b=$(cat $d/$id.portuale.rc 2>/dev/null)
  if diff <(virt $d/$id.portage.err) <(virt $d/$id.portuale.err) >/dev/null; then v=same; else v=DIFF; fi
  if diff <(rest $d/$id.portage.err) <(rest $d/$id.portuale.err) >/dev/null; then o=same; else o=other; fi
  printf '%-26s rc %s/%s  virtuals %-4s other-stderr %s\n' $id "$a" "$b" "$v" "$o"
done
