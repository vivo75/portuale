#!/bin/bash
# run.sh <tag> <ebuild-path> <pm> [EXTRA_FEATURES]
# One hermetic `ebuild <file> merge` in a fresh per-tag base dir:
#   /var/tmp/s385/<tag>/ with root/, tmp/ (PORTAGE_TMPDIR), dist/ (DISTDIR).
# pm: portage (host /usr/bin/ebuild) | portuale (this tree's target/release/portuale).
# EXTRA_FEATURES is appended to make.conf FEATURES for the duration (mc_set/restore).
# `clean` is not implemented by `portuale ebuild`, and Portage's own merge tail
# runs the clean phase unless FEATURES=noclean -- so every cell passes noclean
# (it changes no QA behaviour) and the post-QA image survives for `find $D`.
# Writes $EV/cells/<tag>.{argv,env,rc,stdout,stderr,image.find,image.empty,root.find,contents}.
set -u
TAG=$1; EBF=$2; PM=$3; EXTRA=${4:-}
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-19-385/cells
B=/var/tmp/s385
CFG=$B/cfg
mkdir -p "$EV"
BASE=$B/$TAG
rm -rf "$BASE"
mkdir -p "$BASE/root" "$BASE/tmp" "$BASE/dist"
export PORTAGE_CONFIGROOT=$CFG
export ROOT=$BASE/root
export BROOT=$ROOT
export PORTAGE_TMPDIR=$BASE/tmp
export DISTDIR=$BASE/dist
export EMERGE_DEFAULT_OPTS=
cp "$CFG/etc/portage/make.conf" "$CFG/etc/portage/make.conf.sav385"
printf 'FEATURES="${FEATURES} %s"\n' "$EXTRA" >> "$CFG/etc/portage/make.conf"
: > "$EV/$TAG.env"
echo "EXTRA_FEATURES=$EXTRA" >> "$EV/$TAG.env"
for v in PORTAGE_CONFIGROOT ROOT BROOT PORTAGE_TMPDIR DISTDIR EMERGE_DEFAULT_OPTS PM; do echo "$v=${!v}" >> "$EV/$TAG.env"; done
if [[ $PM == portuale ]]; then
    CMD=(/home/vivo/repo/PORTUALE/portuale/target/release/portuale ebuild "$EBF" merge)
    echo "portuale_HEAD=$(git -C /home/vivo/repo/PORTUALE/portuale rev-parse HEAD)" >> "$EV/$TAG.env"
    echo "portuale_mtime=$(stat -c %y /home/vivo/repo/PORTUALE/portuale/target/release/portuale)" >> "$EV/$TAG.env"
else
    CMD=(ebuild "$EBF" merge)
    echo "portage_version=$(emerge --version 2>/dev/null | head -1)" >> "$EV/$TAG.env"
fi
printf 'cmd:' >> "$EV/$TAG.argv"; printf ' %q' "${CMD[@]}" >> "$EV/$TAG.argv"; printf '\n' >> "$EV/$TAG.argv"
timeout 600 "${CMD[@]}" >"$EV/$TAG.stdout" 2>"$EV/$TAG.stderr"; RC=$?
echo $RC > "$EV/$TAG.rc"
mv "$CFG/etc/portage/make.conf.sav385" "$CFG/etc/portage/make.conf"
# Image tree ($D) of the merged package, if the builddir survived.
# (PORTAGE_BUILDDIR is $PORTAGE_TMPDIR/portage/<cat>/<pf>, no PN level.)
PF=t385-${TAG:0:1}-${TAG:1:1}
CATPF=app-misc/$PF
IMG=$BASE/tmp/portage/$CATPF/image
if [[ -d $IMG ]]; then
    ( cd "$IMG" && find . -mindepth 1 | LC_ALL=C sort ) > "$EV/$TAG.image.find"
    ( cd "$IMG" && find . -mindepth 1 -type d -empty | LC_ALL=C sort ) > "$EV/$TAG.image.empty"
else
    echo "<no image dir: $IMG>" > "$EV/$TAG.image.find"
    echo "<no image dir>" > "$EV/$TAG.image.empty"
fi
# Merged ROOT tree (minus the vdb itself and tmp).
( cd "$BASE/root" && find . \( -path ./var/db/pkg -o -path ./tmp \) -prune -o -print | LC_ALL=C sort ) > "$EV/$TAG.root.find"
# vdb CONTENTS of the merged package.
shopt -s nullglob
CONTS=( "$BASE/root/var/db/pkg/$CATPF"/CONTENTS )
if [[ ${#CONTS[@]} == 1 ]]; then
    cp "${CONTS[0]}" "$EV/$TAG.contents"
else
    echo "<${#CONTS[@]} CONTENTS matches>" > "$EV/$TAG.contents"
fi
echo "tag=$TAG rc=$RC"
