#!/bin/bash
# cmp.sh [ENV=..] : rerun portuale on every cell and diff against the Portage capture (root/caret suffixes normalised)
cd "$(dirname "$0")"
norm(){ sed 's#\$R/#$R#g; s/took [0-9.]* s /took /' "$1" | grep -vE "argv|unique name|Global Updates|Could take|update pass|SLOT move|package\.\*|updates/2Q" | sed "s# to '\$R'##g; s#^\(dev-libs/abk0a:0\) for \$R#\1#; s/ *$//"; }
for cell in "def:" "bt0:--backtrack=0" "bt1:--backtrack=1" "bt2:--backtrack=2" "aby:--autounmask-backtrack=y" "aby-bt1:--autounmask-backtrack=y --backtrack=1" "aby-bt2:--autounmask-backtrack=y --backtrack=2" "aby-bt3:--autounmask-backtrack=y --backtrack=3"; do
 n=${cell%%:*}; a=${cell#*:}
 ./probe.sh portuale --pretend $a dev-libs/abk0d > $n.x.txt
 if diff <(norm $n.portage.txt) <(norm $n.x.txt) >/dev/null; then echo "$n SAME"; else echo "$n DIFF"; diff <(norm $n.portage.txt) <(norm $n.x.txt) | head -${LINES_:-8}; fi
done
