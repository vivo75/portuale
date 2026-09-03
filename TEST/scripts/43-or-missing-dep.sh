#!/bin/bash
# `||`-preference feedback via a MISSING transitive dependency:
# portuale -p vs real emerge -p. dev-libs/ormisstop RDEPENDs
#   || ( dev-libs/ormissbad dev-libs/ormissgood )
# ormissbad is visible but RDEPENDs the nonexistent
# dev-libs/ormiss-nonexistent. Real backtracking masks ormissbad and
# dep_zapdeps re-picks ormissgood. The merge-list diff MUST be empty.
# --backtrack=0 must still report the missing dep, in both.
set +e
LOG=/TEST/logs/43-or-missing-dep.log
: > "$LOG"
exec > >(tee -a "$LOG") 2>&1

OVL=/var/db/repos/buildovl

mkeb() { local cp=$1 ver=$2 rdep=$3
    mkdir -p "$OVL/$cp"
    cat > "$OVL/$cp/${cp#*/}-$ver.ebuild" <<EOF
EAPI=8
DESCRIPTION="probe"
SLOT="0"
KEYWORDS="amd64 ~amd64"
RDEPEND="$rdep"
EOF
}

rm -rf "$OVL"/dev-libs/ormiss*
mkeb dev-libs/ormisstop  1.0 "|| ( dev-libs/ormissbad dev-libs/ormissgood )"
mkeb dev-libs/ormissbad  1.0 "dev-libs/ormiss-nonexistent"
mkeb dev-libs/ormissgood 1.0 ""
egencache --repo=buildovl --update >/dev/null 2>&1

filt() { grep -E '^\[(ebuild|binary)'; }

/usr/bin/emerge       -p --color=n dev-libs/ormisstop 2>&1 | filt > /tmp/real.default
/usr/local/bin/emerge -p --color=n dev-libs/ormisstop 2>&1 | filt > /tmp/portuale.default
echo "### default -- real:";     cat /tmp/real.default
echo "### default -- portuale:"; cat /tmp/portuale.default
echo "### default -- diff (must be empty):"
if diff -u /tmp/real.default /tmp/portuale.default; then echo "PASS (default)"; else echo "FAIL (default)"; fi
echo

real_bt0=$(/usr/bin/emerge       -p --color=n --backtrack=0 dev-libs/ormisstop 2>&1)
port_bt0=$(/usr/local/bin/emerge -p --color=n --backtrack=0 dev-libs/ormisstop 2>&1)
echo "### --backtrack=0 -- both report the missing dep:"
if grep -q 'ormiss-nonexistent' <<<"$real_bt0" && grep -q 'ormiss-nonexistent' <<<"$port_bt0"; then
    echo "PASS (--backtrack=0: missing dep reported by both)"
else
    echo "FAIL (--backtrack=0)"
    echo "--- real ---";    echo "$real_bt0"
    echo "--- portuale ---"; echo "$port_bt0"
fi
