#!/bin/bash
# Nail down real portage's slot-op-rebuild TRIGGER condition: is it
# "consumer is a set member" or "consumer is reachable from a set"?
set +e
LOG=/TEST/logs/31-slotop-trigger.log
: > "$LOG"
exec > >(tee -a "$LOG") 2>&1

OVL=/var/db/repos/buildovl
VDB=/var/db/pkg
WORLD=/var/lib/portage/world

mkeb() { local cp=$1 ver=$2 slot=$3 rdep=$4
    mkdir -p "$OVL/$cp"
    cat > "$OVL/$cp/${cp#*/}-$ver.ebuild" <<EOF
EAPI=8
DESCRIPTION="probe"
SLOT="$slot"
KEYWORDS="amd64 ~amd64"
RDEPEND="$rdep"
EOF
}
mkvdb() { local cp=$1 ver=$2 slot=$3 rdep=$4
    local d="$VDB/$cp-$ver"; mkdir -p "$d"
    echo "${cp%/*}" > "$d/CATEGORY"; echo "${cp#*/}-$ver" > "$d/PF"
    echo "$slot" > "$d/SLOT"; echo 8 > "$d/EAPI"; echo buildovl > "$d/repository"
    echo "$rdep" > "$d/RDEPEND"; : > "$d/DEPEND"; : > "$d/BDEPEND"; : > "$d/PDEPEND"
    echo amd64 > "$d/KEYWORDS"; : > "$d/USE"; : > "$d/IUSE"; : > "$d/CONTENTS"
    : > "$d/DEFINED_PHASES"; echo "$(date +%s)" > "$d/BUILD_TIME"; echo 1 > "$d/COUNTER"
}
reset_state() {
    rm -rf "$VDB"/dev-libs/scl* "$OVL"/dev-libs/scl*
    grep -v '^dev-libs/scl' "$WORLD" > "$WORLD.tmp" 2>/dev/null; mv "$WORLD.tmp" "$WORLD"
    mkeb dev-libs/scltarget 1.0 "0/1" ""
    mkeb dev-libs/scltarget 2.0 "0/2" ""
    mkeb dev-libs/sclmid    1.0 "0/2" "dev-libs/scltarget:="
    mkeb dev-libs/scltail   1.0 "0"   "dev-libs/sclmid:="
    mkeb dev-libs/sclfwd    1.0 "0"   "dev-libs/scltarget:= dev-libs/sclmid:="
    egencache --repo=buildovl --update >/dev/null 2>&1
    mkvdb dev-libs/scltarget 1.0 "0/1" ""
    mkvdb dev-libs/sclmid    1.0 "0/1" "dev-libs/scltarget:0/1="
    mkvdb dev-libs/scltail   1.0 "0"   "dev-libs/sclmid:0/1="
    mkvdb dev-libs/sclfwd    1.0 "0"   "dev-libs/scltarget:0/1= dev-libs/sclmid:0/1="
}
g() { grep -E "^\[(ebuild|binary).*dev-libs/scl|causing rebuilds|^  dev-libs/scl" ; }

echo "### CASE 1: nothing in world, emerge -p scltarget"
reset_state
/usr/bin/emerge -p --color=n dev-libs/scltarget 2>&1 | g
echo

echo "### CASE 2: only scltail in world (reverse-dep chain: scltail->sclmid->scltarget)"
reset_state
echo dev-libs/scltail >> "$WORLD"
/usr/bin/emerge -p --color=n dev-libs/scltarget 2>&1 | g
echo

echo "### CASE 3: only sclfwd in world; sclfwd forward-deps BOTH scltarget and sclmid"
reset_state
echo dev-libs/sclfwd >> "$WORLD"
/usr/bin/emerge -p --color=n dev-libs/scltarget 2>&1 | g
echo

echo "### CASE 4: sclmid + scltail in world, emerge -p scltarget"
reset_state
printf 'dev-libs/sclmid\ndev-libs/scltail\n' >> "$WORLD"
/usr/bin/emerge -p --color=n dev-libs/scltarget 2>&1 | g
echo

echo "### CASE 5: same as 4 but emerge -p sclmid (mid of chain)"
/usr/bin/emerge -p --color=n dev-libs/sclmid 2>&1 | g
