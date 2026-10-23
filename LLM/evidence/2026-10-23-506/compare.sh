#!/bin/bash
# Compare portage vs portuale captures in <dir>: rc, stdout after dropping
# Portage's non-tty preamble (header, "Calculating dependencies", "Dependency
# resolution took" and the blank lines before the list), and stderr with the
# flags inside "(Change USE: ...)" sorted (Portage prints them in hash order,
# LLM/portage-quirks.md Q4).
d=$1
norm() { sed -e '/These are the packages that would be merged/d' \
  -e '/^Calculating dependencies/d' -e '/^Dependency resolution took/d' "$1" | sed -e '/./,$!d'; }
normerr() { python3 -c '
import re,sys
t=open(sys.argv[1]).read()
print(re.sub(r"\(Change USE: ([^)]*)\)", lambda m: "(Change USE: "+" ".join(sorted(m.group(1).split()))+")", t), end="")' "$1"; }
for f in "$d"/*.portage.rc; do id=$(basename ${f%.portage.rc})
  a=$(cat $d/$id.portage.rc); b=$(cat $d/$id.portuale.rc 2>/dev/null)
  if diff -q <(norm $d/$id.portage.out) <(norm $d/$id.portuale.out) >/dev/null; then so=same; else so=DIFF; fi
  if cmp -s $d/$id.portage.err $d/$id.portuale.err; then se=same
  elif diff -q <(normerr $d/$id.portage.err) <(normerr $d/$id.portuale.err) >/dev/null; then se=same-sorted
  else se=DIFF; fi
  printf '%-18s rc %s/%s  stdout %-4s stderr %s\n' $id $a $b $so $se
done
