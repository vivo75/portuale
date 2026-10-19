#!/bin/bash
# cells.sh [pm] [cell ...]: #381 S0 oracle cells, each run for Portage and portuale.
# pm: portage (default, host /usr/bin) | portuale (/var/tmp/pt-381-s0/bin first on PATH).
# Roots are hermetic per cell+PM: /var/tmp/pt-381-s0/roots/<cell>-<pg|pl>.
set -u
S=/var/tmp/pt-381-s0
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-19-381/cells
B=/var/tmp/pt-381-s0
EM="emerge --ignore-default-opts -v -1"

snap_cell() { # $1=RN: save tree listing + both CONTENTS texts.
	local RN=$1 R=$B/roots/$1
	(cd $R && find . -path ./var/tmp -prune -o -print | sort) > $EV/$RN.tree 2>/dev/null
	for cpv in $R/var/db/pkg/app-misc/*/; do
		[ -d "$cpv" ] || continue
		n=$(basename $(dirname $cpv))-$(basename $cpv)
		cp $cpv/CONTENTS $EV/$RN.contents-$n 2>/dev/null || true
	done
}

cell_V1() { # replace: old records normal spelling, new records alias spelling.
	local RN=V1-$T R=$B/roots/V1-$T
	PM=$PM $S/run.sh $RN-1 $RN T381_W=normal -- $EM =app-misc/t381-rep-1
	ln -s usr/share/t381 $R/alias
	PM=$PM $S/run.sh $RN-2 $RN T381_W=alias -- $EM =app-misc/t381-rep-2
	snap_cell $RN
}
cell_V2() { # replace mirror: old records alias spelling, new records normal.
	local RN=V2-$T R=$B/roots/V2-$T
	mkdir -p $R/usr/share/t381
	ln -s usr/share/t381 $R/alias
	PM=$PM $S/run.sh $RN-1 $RN T381_W=alias -- $EM =app-misc/t381-rep-1
	PM=$PM $S/run.sh $RN-2 $RN T381_W=normal -- $EM =app-misc/t381-rep-2
	snap_cell $RN
}
cell_V3() { # replace: old is keepdir sub, new records file under alias/sub.
	local RN=V3-$T R=$B/roots/V3-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t381-k-1
	ln -s usr/share/t381 $R/alias
	PM=$PM $S/run.sh $RN-2 $RN -- $EM =app-misc/t381-k-2
	snap_cell $RN
}
ALL="V1 V2 V3"
T=${T:-pg}
PM=${PM:-portage}
if [[ $# -gt 0 ]]; then PM=$1; shift; fi
if [[ $# -gt 0 ]]; then
	for c in "$@"; do cell_$c; done
else
	for c in $ALL; do cell_$c; done
fi
