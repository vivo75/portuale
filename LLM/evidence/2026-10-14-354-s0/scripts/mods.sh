#!/bin/bash
# mods.sh <root-name>: on-disk admin edits between the two merges of t354-re.
#  *mod files: content changed, mtime restored to the recorded one (md5-only drift)
#  *touch files: content unchanged, mtime moved to 2001 (mtime-only drift)
set -eu
R=/var/tmp/t354/roots/$1
S=$R/usr/share/t354; C=$R/etc/t354
editkeep() { local m; m=$(stat -c %Y "$1"); echo "admin edit" >> "$1"; touch -d "@$m" "$1"; }
editkeep $S/dropmod
touch -d '2001-01-01 00:00:00' $S/droptouch
editkeep $S/chgmod
editkeep $C/mod.conf
touch -d '2001-01-01 00:00:00' $C/touch.conf
editkeep $C/bothmod.conf
editkeep $C/same.conf
echo "mods applied to $R"
