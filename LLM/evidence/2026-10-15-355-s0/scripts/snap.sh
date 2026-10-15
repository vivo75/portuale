#!/bin/bash
# snap.sh <root-name> <tag>: filesystem listing (type, mode, mtime, md5/target) of
# ROOT minus var/db/pkg and var/cache, plus vdb CONTENTS/COUNTER per installed pkg,
# the config memory file and the preserved-libs registry.
set -u
RN=$1; TAG=$2
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-15-355-s0/cells
R=/var/tmp/t355/roots/$RN
O=$EV/$TAG.snap
{
echo "## fs listing of ROOT=$R (excluding var/db/pkg, var/cache, var/log, tmp)"
cd "$R" && find . \( -path ./var/db/pkg -o -path ./var/cache -o -path ./var/log -o -path ./tmp \) -prune -o -print | LC_ALL=C sort | while read -r p; do
  [[ $p == . ]] && continue
  if [[ -L $p ]]; then echo "sym $p -> $(readlink "$p") mtime=$(stat -c %Y "$p")"
  elif [[ -d $p ]]; then echo "dir $p"
  elif [[ -p $p ]]; then echo "fif $p mtime=$(stat -c %Y "$p")"
  elif [[ -S $p ]]; then echo "sock $p mtime=$(stat -c %Y "$p")"
  elif [[ -b $p || -c $p ]]; then echo "dev $p mtime=$(stat -c %Y "$p")"
  else echo "obj $p md5=$(md5sum < "$p" | cut -c1-32) mtime=$(stat -c %Y "$p") content=$(head -c 40 "$p" | tr '\n\0' '|.' | LC_ALL=C tr -c '[:print:]' '?')"; fi
done
for d in "$R"/var/db/pkg/*/*/; do
  [[ -d $d ]] || continue
  echo "## vdb $d"; ls "$d" | tr '\n' ' '; echo
  echo "COUNTER=$(cat "$d/COUNTER" 2>/dev/null || echo '<no COUNTER file>')"
  if [[ -f $d/CONTENTS ]]; then echo "--- CONTENTS"; cat "$d/CONTENTS"; else echo "--- no CONTENTS file"; fi
done
echo "## var/lib/portage/config (confmem)"; cat "$R/var/lib/portage/config" 2>/dev/null || echo '<absent>'
echo "## preserved_libs_registry"; cat "$R/var/lib/portage/preserved_libs_registry" 2>/dev/null || echo '<absent>'
echo "## var/cache/edb/counter"; cat "$R/var/cache/edb/counter" 2>/dev/null || echo '<absent>'
} > "$O"
echo "snap $O"
