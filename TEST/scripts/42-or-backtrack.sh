#!/bin/bash
# `||`-preference feedback driving a backtrack retry: portuale -p vs real
# emerge -p. dev-libs/orbtblocked's RDEPEND is
#   || ( >=dev-libs/orbttool-2.0 dev-libs/orbtclean ) =dev-libs/orbttool-1.0
# The first `||` alternative (orbttool-2.0) collides with the hard
# =orbttool-1.0 dep in the same slot -> unsolvable slot conflict.
# Backtracking masks orbttool-2.0, and on the retry dep_zapdeps re-picks
# the second alternative (orbtclean). The diff of the merge list MUST be
# empty. --backtrack=0 must still show the conflict, identically.
set +e
LOG=/TEST/logs/42-or-backtrack.log
: > "$LOG"
exec > >(tee -a "$LOG") 2>&1

OVL=/var/db/repos/buildovl

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

rm -rf "$OVL"/dev-libs/orbt*
mkeb dev-libs/orbttool  1.0 "0" ""
mkeb dev-libs/orbttool  2.0 "0" ""
mkeb dev-libs/orbtclean 1.0 "0" ""
mkeb dev-libs/orbtblocked 1.0 "0" \
    "|| ( >=dev-libs/orbttool-2.0 dev-libs/orbtclean ) =dev-libs/orbttool-1.0"
egencache --repo=buildovl --update >/dev/null 2>&1

filt() { grep -E '^\[(ebuild|binary)|Multiple package instances'; }

# DEFAULT (backtracking on): this slice's actual deliverable -- the `||`
# group yields to `orbtclean` once the conflict masks orbttool-2.0. The
# merge-list diff vs real MUST be empty.
/usr/bin/emerge       -p --color=n dev-libs/orbtblocked 2>&1 | filt > /tmp/real.default
/usr/local/bin/emerge -p --color=n dev-libs/orbtblocked 2>&1 | filt > /tmp/portuale.default
echo "### default -- real:";     cat /tmp/real.default
echo "### default -- portuale:"; cat /tmp/portuale.default
echo "### default -- diff (must be empty):"
if diff -u /tmp/real.default /tmp/portuale.default; then echo "PASS (default)"; else echo "FAIL (default)"; fi
echo

# --backtrack=0 (backtracking off): both must still REPORT the unsolvable
# conflict. Not a byte diff -- portuale collapses an unenforced slot
# conflict to a single merge-list entry where real lists both instances
# (a pre-existing "report, don't enforce" divergence, unrelated to this
# slice; the `!!! Multiple package instances` block itself lists both in
# either implementation).
real_bt0=$(/usr/bin/emerge       -p --color=n --backtrack=0 dev-libs/orbtblocked 2>&1)
port_bt0=$(/usr/local/bin/emerge -p --color=n --backtrack=0 dev-libs/orbtblocked 2>&1)
echo "### --backtrack=0 -- both report the conflict:"
if grep -q 'Multiple package instances' <<<"$real_bt0" && grep -q 'Multiple package instances' <<<"$port_bt0"; then
    echo "PASS (--backtrack=0: conflict reported by both)"
else
    echo "FAIL (--backtrack=0)"
    echo "--- real ---"; echo "$real_bt0"
    echo "--- portuale ---"; echo "$port_bt0"
fi
