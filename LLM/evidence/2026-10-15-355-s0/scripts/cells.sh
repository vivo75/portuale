#!/bin/bash
# cells.sh [pm] [cell ...]: S0 oracle cells, each run for Portage and portuale.
# pm: portage (default, host /usr/bin) | portuale (/var/tmp/t355/bin first on PATH).
# Roots are hermetic per cell+PM: /var/tmp/t355/roots/<cell>-<pg|pl>.
# Usage: ./cells.sh              # everything, both PMs
#        ./cells.sh portage P1 U1a  # subset for one PM
set -u
S=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-15-355-s0/scripts
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-15-355-s0/cells
B=/var/tmp/t355
CFG=$B/cfg/etc/portage
EM="emerge --ignore-default-opts -v -1"
UC="emerge --ignore-default-opts -v -C"

seed_ld() { # $1=root-name: LDPATH/ld.so.conf seed like #354 scenario-f.sh
	mkdir -p $B/roots/$1/etc/env.d
	echo 'LDPATH="/usr/lib64"' > $B/roots/$1/etc/env.d/50t355
	echo /usr/lib64 > $B/roots/$1/etc/ld.so.conf
}
snapdiff() { # $1=root-name
	$S/snap.sh $1 $1-after
	diff $EV/$1-before.snap $EV/$1-after.snap > $EV/$1.snapdiff || true
}
contents_of() { # $1=root $2=cpv $3=tag
	cp $B/roots/$1/var/db/pkg/$2/CONTENTS $EV/$3.contents
}
mc_set() { cp $CFG/make.conf $CFG/make.conf.sav; printf '%s\n' "$1" >> $CFG/make.conf; }
mc_restore() { mv $CFG/make.conf.sav $CFG/make.conf; }
editkeep() { local m; m=$(stat -c %Y "$1"); echo "admin edit" >> "$1"; touch -d "@$m" "$1"; }

cell_P1() { # $1=pm-tag(pg|pl): #355 main. base-1,mid-1(v1),app-1(v1); base-2; reinstall mid-1 v2.
	local RN=P1-$T
	seed_ld $RN
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-base-1
	PM=$PM $S/run.sh $RN-2 $RN T355_V=1 -- $EM =app-misc/t355-mid-1
	PM=$PM $S/run.sh $RN-3 $RN T355_V=1 -- $EM =app-misc/t355-app-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-4 $RN -- $EM =app-misc/t355-base-2
	PM=$PM $S/run.sh $RN-5 $RN T355_V=2 -- $EM =app-misc/t355-mid-1
	snapdiff $RN
}
cell_P1c() { # control: app NEEDs base.so.1 too.
	local RN=P1c-$T
	seed_ld $RN
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-base-1
	PM=$PM $S/run.sh $RN-2 $RN T355_V=1 -- $EM =app-misc/t355-mid-1
	PM=$PM $S/run.sh $RN-3 $RN T355_V=2 -- $EM =app-misc/t355-app-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-4 $RN -- $EM =app-misc/t355-base-2
	PM=$PM $S/run.sh $RN-5 $RN T355_V=2 -- $EM =app-misc/t355-mid-1
	snapdiff $RN
}
cell_P2() { # control: different-version mid upgrade.
	local RN=P2-$T
	seed_ld $RN
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-base-1
	PM=$PM $S/run.sh $RN-2 $RN T355_V=1 -- $EM =app-misc/t355-mid-1
	PM=$PM $S/run.sh $RN-3 $RN T355_V=1 -- $EM =app-misc/t355-app-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-4 $RN -- $EM =app-misc/t355-base-2
	PM=$PM $S/run.sh $RN-5 $RN -- $EM =app-misc/t355-mid-2
	snapdiff $RN
}
cell_U1a() { # R1 literal UNINSTALL_IGNORE (drops the default).
	local RN=U1a-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-ign-1
	$S/snap.sh $RN $RN-before
	mc_set 'UNINSTALL_IGNORE="/usr/share/t355/ign/*"'
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-ign-1
	mc_restore
	snapdiff $RN
}
cell_U1b() { # R1 append-form UNINSTALL_IGNORE (keeps /lib/modules/*).
	local RN=U1b-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-ign-1
	$S/snap.sh $RN $RN-before
	mc_set 'UNINSTALL_IGNORE="${UNINSTALL_IGNORE} /usr/share/t355/ign/*"'
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-ign-1
	mc_restore
	snapdiff $RN
}
cell_U1d() { # R1 default: no UNINSTALL_IGNORE in make.conf.
	local RN=U1d-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-ign-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-ign-1
	snapdiff $RN
}
cell_L1() { # R2: /lib and /usr/lib symlinks.
	local RN=L1-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-liblink-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-liblink-1
	snapdiff $RN
}
cell_E1() { # R4+R4b: B owns a file through A's dir symlink.
	local RN=E1-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-ea-1
	PM=$PM $S/run.sh $RN-2 $RN -- $EM =app-misc/t355-eb-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-3 $RN -- $UC =app-misc/t355-ea-1
	snapdiff $RN
}
cell_E2() { # R4b: untracked file, no B.
	local RN=E2-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-ea-1
	R=$B/roots/$RN; mkdir -p $R/usr/share/t355/real; echo admin > $R/usr/share/t355/real/u.txt
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-ea-1
	snapdiff $RN
}
cell_E3() { # R4: every child owned, regular.
	local RN=E3-$T
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-e3-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-e3-1
	snapdiff $RN
}
cell_E3b() { # R4 narrow inode form: file owned through the parent inode.
	local RN=E3b-$T
	PM=$PM $S/run.sh $RN-1 $RN T355_V=2 -- $EM =app-misc/t355-e3-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-e3-1
	snapdiff $RN
}
cell_D1() { # R3: recorded dir replaced by a symlink to an empty dir.
	local RN=D1-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-d1-1
	R=$B/roots/$RN; rm -rf $R/opt/t355/d; mkdir -p $R/opt/t355/other; ln -s other $R/opt/t355/d
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-d1-1
	snapdiff $RN
}
cell_D2() { # R3+R4: symlink target holds an untracked file.
	local RN=D2-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-d1-1
	R=$B/roots/$RN; rm -rf $R/opt/t355/d; mkdir -p $R/opt/t355/other
	echo u > $R/opt/t355/other/u.txt; ln -s other $R/opt/t355/d
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-d1-1
	snapdiff $RN
}
seed_musr() { # $1=root: merged-usr seed with untracked files.
	R=$B/roots/$1
	mkdir -p $R/usr/bin $R/usr/lib64
	ln -s usr/bin $R/bin; ln -s usr/lib64 $R/lib64
	echo u1 > $R/usr/bin/u1; echo u2 > $R/usr/lib64/u2
}
cell_C1a() { # safety: another package still records dir /bin.
	local RN=C1a-$T
	seed_musr $RN
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-c1a-1
	PM=$PM $S/run.sh $RN-2 $RN -- $EM =app-misc/t355-c1b-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-3 $RN -- $UC =app-misc/t355-c1a-1
	snapdiff $RN
}
cell_C1b() { # safety: no other recorder of /bin.
	local RN=C1b-$T
	seed_musr $RN
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-c1a-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-c1a-1
	snapdiff $RN
}
cell_M1() { # R5 protected: md5-only drift.
	local RN=M1-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-m1-1
	editkeep $B/roots/$RN/etc/t355/mod.conf
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-m1-1
	snapdiff $RN
}
cell_M2() { # R5 unprotected with -unmerge-orphans: md5-only drift.
	local RN=M2-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-m2-1
	editkeep $B/roots/$RN/usr/share/t355/plain
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN FEATURES=-unmerge-orphans -- $UC =app-misc/t355-m2-1
	snapdiff $RN
}
cell_T1a() { # R6 obj->dir, new mtime.
	local RN=T1a-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-t1-1
	R=$B/roots/$RN; rm $R/usr/share/t355/o; mkdir $R/usr/share/t355/o; echo f > $R/usr/share/t355/o/f
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-t1-1
	snapdiff $RN
}
cell_T1b() { # R6 obj->dir, recorded mtime restored.
	local RN=T1b-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-t1-1
	R=$B/roots/$RN; MT=$(grep '^obj /usr/share/t355/o ' $R/var/db/pkg/app-misc/t355-t1-1/CONTENTS | awk '{print $NF}')
	rm $R/usr/share/t355/o; mkdir $R/usr/share/t355/o; echo f > $R/usr/share/t355/o/f
	touch -d "@$MT" $R/usr/share/t355/o
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-t1-1
	snapdiff $RN
}
cell_T2() { # R6 sym->file, recorded mtime restored, -unmerge-orphans.
	local RN=T2-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-t2-1
	R=$B/roots/$RN; MT=$(grep '^sym /usr/share/t355/s ' $R/var/db/pkg/app-misc/t355-t2-1/CONTENTS | awk '{print $NF}')
	rm $R/usr/share/t355/s; echo x > $R/usr/share/t355/s; touch -d "@$MT" $R/usr/share/t355/s
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN FEATURES=-unmerge-orphans -- $UC =app-misc/t355-t2-1
	snapdiff $RN
}
cell_T3a() { # R6/R7 dir->file, default FEATURES.
	local RN=T3a-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-t3-1
	R=$B/roots/$RN; rm -rf $R/usr/share/t355/dd; echo file > $R/usr/share/t355/dd
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-t3-1
	snapdiff $RN
}
cell_T3b() { # R6/R7 dir->file, -unmerge-orphans.
	local RN=T3b-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-t3-1
	R=$B/roots/$RN; rm -rf $R/usr/share/t355/dd; echo file > $R/usr/share/t355/dd
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN FEATURES=-unmerge-orphans -- $UC =app-misc/t355-t3-1
	snapdiff $RN
}
cell_F1a() { # R7 fifo, default FEATURES.
	local RN=F1a-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-f1-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-f1-1
	snapdiff $RN
}
cell_F1b() { # R7 fifo, -unmerge-orphans.
	local RN=F1b-$T
	PM=$PM $S/run.sh $RN-1 $RN -- $EM =app-misc/t355-f1-1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN FEATURES=-unmerge-orphans -- $UC =app-misc/t355-f1-1
	snapdiff $RN
}
cell_B1a() { # R8 reinstall dropping files, -unmerge-orphans.
	local RN=B1a-$T
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-drop-1
	R=$B/roots/$RN; editkeep $R/usr/share/t355/dropmod; touch -d '2001-01-01 00:00:00' $R/usr/share/t355/droptouch
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN T355_V=2 FEATURES=-unmerge-orphans -- $EM =app-misc/t355-drop-1
	snapdiff $RN
}
cell_B1b() { # R8 same files through emerge -C, -unmerge-orphans.
	local RN=B1b-$T
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-drop-1
	R=$B/roots/$RN; editkeep $R/usr/share/t355/dropmod; touch -d '2001-01-01 00:00:00' $R/usr/share/t355/droptouch
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN FEATURES=-unmerge-orphans -- $UC =app-misc/t355-drop-1
	snapdiff $RN
}
cell_S1() { # #357 unmerge with spaces + decoy.
	local RN=S1-$T
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-space-1
	echo decoy > $B/roots/$RN/usr/share/t355/a
	contents_of $RN app-misc/t355-space-1 $RN-merge
	PM=$PM $S/run.sh $RN-owners $RN -- portageq owners $B/roots/$RN "$B/roots/$RN/usr/share/t355/a b.txt"
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN -- $UC =app-misc/t355-space-1
	snapdiff $RN
}
cell_S2() { # #357 same-version reinstall dropping "a b.txt".
	local RN=S2-$T
	PM=$PM $S/run.sh $RN-1 $RN T355_V=1 -- $EM =app-misc/t355-space-1
	echo decoy > $B/roots/$RN/usr/share/t355/a
	contents_of $RN app-misc/t355-space-1 $RN-v1
	$S/snap.sh $RN $RN-before
	PM=$PM $S/run.sh $RN-2 $RN T355_V=2 -- $EM =app-misc/t355-space-1
	contents_of $RN app-misc/t355-space-1 $RN-v2
	snapdiff $RN
}
cell_S3() { # #357 target containing " -> ": pin the CONTENTS line.
	local RN=S3-$T
	PM=$PM $S/run.sh $RN-1 $RN T355_V=3 -- $EM =app-misc/t355-space-1
	contents_of $RN app-misc/t355-space-1 $RN-merge
	$S/snap.sh $RN $RN-before
	$S/snap.sh $RN $RN-after
}

ALL="P1 P1c P2 U1a U1b U1d L1 E1 E2 E3 E3b D1 D2 C1a C1b M1 M2 T1a T1b T2 T3a T3b F1a F1b B1a B1b S1 S2 S3"
PMS="portage portuale"
if [[ $# -ge 1 ]]; then PMS="$1"; shift; fi
if [[ $# -ge 1 ]]; then ALL="$*"; fi
docell() { # $1=cell $2=pm-name: sets globals PM/T, wipes the root, runs.
	PM=$2
	if [[ $PM == portuale ]]; then T=pl; else T=pg; fi
	echo "=== cell $1 PM=$2 ==="
	rm -rf $B/roots/$1-$T $B/scratch/$1-$T
	cell_$1
}
for pm in $PMS; do
	for c in $ALL; do
		docell $c $pm
	done
done
