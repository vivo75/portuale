#!/bin/bash
# Multi-level slot-operator-rebuild cascade: portuale -p vs real emerge -p
# for a three-package chain casctail -> cascmid -> casctarget where
# rebuilding cascmid is itself a sub-slot shift (its tree ebuild moved
# 0/1 -> 0/2 since it was installed), forcing a second rebuild of
# casctail. The diff of the merge list + "causing rebuilds:" block MUST
# be empty -- this is the gate for the slot-op-rebuild cascade slice.
set +e
LOG=/TEST/logs/40-slotop-cascade.log
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

rm -rf "$VDB"/dev-libs/casc* "$OVL"/dev-libs/casc*
grep -v '^dev-libs/casc' "$WORLD" > "$WORLD.tmp" 2>/dev/null; mv "$WORLD.tmp" "$WORLD"

mkeb dev-libs/casctarget 1.0 "0/1" ""
mkeb dev-libs/casctarget 2.0 "0/2" ""
mkeb dev-libs/cascmid    1.0 "0/2" "dev-libs/casctarget:="
mkeb dev-libs/casctail   1.0 "0/1" "dev-libs/cascmid:="
egencache --repo=buildovl --update >/dev/null 2>&1

mkvdb dev-libs/casctarget 1.0 "0/1" ""
mkvdb dev-libs/cascmid    1.0 "0/1" "dev-libs/casctarget:0/1="
mkvdb dev-libs/casctail   1.0 "0/1" "dev-libs/cascmid:0/1="
echo dev-libs/casctail >> "$WORLD"

filt() { grep -E '^\[(ebuild|binary)|causing rebuilds|causes rebuilds for|scheduled for merge'; }

/usr/bin/emerge        -p --color=n dev-libs/casctarget > /tmp/real.raw 2>&1
/usr/local/bin/emerge  -p --color=n dev-libs/casctarget > /tmp/portuale.raw 2>&1
echo "### real RAW:"; cat /tmp/real.raw; echo
filt < /tmp/real.raw > /tmp/real.out
filt < /tmp/portuale.raw > /tmp/portuale.out

echo "### real emerge -p:"
cat /tmp/real.out
echo
echo "### portuale -p:"
cat /tmp/portuale.out
echo
echo "### diff (must be empty):"
if diff -u /tmp/real.out /tmp/portuale.out; then
    echo "PASS: portuale matches real portage byte-for-byte"
else
    echo "FAIL: divergence above"
fi
