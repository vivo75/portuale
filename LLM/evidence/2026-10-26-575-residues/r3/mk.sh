#!/bin/bash
# mk.sh <repo-dir> : add the R3 S0 shapes (dev-libs/r594*) to a scratch repo
set -eu
R=$1
eb() { # eb <pn> <pv> <rdepend> [extra-lines]
  mkdir -p "$R/dev-libs/$1"
  { echo 'EAPI=8'; echo "DESCRIPTION=\"fixture package: R3 S0 (#594) $1-$2\""; echo 'SLOT="0"'; echo "KEYWORDS=\"${KW:-amd64}\""
    [ -n "$3" ] && echo "RDEPEND=\"$3\""; [ -n "${4:-}" ] && printf '%s\n' "$4"; true; } > "$R/dev-libs/$1/$1-$2.ebuild"
}
# <x>top-1 -> <x>m ; <x>m-1 has no deps ; <x>m-2 needs the masked shape
two() { eb r594${1}top 1 "dev-libs/r594${1}m"; eb r594${1}m 1 ""; eb r594${1}m 2 "$2"; }
two p "dev-libs/maskeddep"                       # package.mask (user package.mask)
two k "dev-libs/kwmaskeddep"                     # ~amd64 only
two n "dev-libs/r594nokw"                        # KEYWORDS="" (missing keyword)
two l "dev-libs/licensemaskedpkg"                # LICENSE not accepted
two u "dev-libs/r594ukw[r594flag]"               # masked (~amd64) and has the flag
two x "dev-libs/r594xdep[r594flag]"              # visible -1 lacks IUSE, masked -2 has it
two v "dev-libs/r594vdep[r594flag]"              # visible, flag off (USE-only control)
two g "dev-libs/r594gone"                        # absent (plain-miss control)
two c "dev-libs/r594vdep[r594flag] dev-libs/maskeddep"   # autounmask USE change, then masked dep
two d "dev-libs/maskeddep dev-libs/r594vdep[r594flag]"   # masked dep, then autounmask USE change
two e "dev-libs/r594xdep[r594flag] dev-libs/maskeddep"   # USE-only stop (missing IUSE), then masked dep
# cross-parent order: <x>top-1 -> <x>a <x>b ; a-2 USE-only stop, b-2 masked dep
three() { eb r594${1}top 1 "$2"; for q in a b; do eb r594${1}$q 1 ""; done; eb r594${1}a 2 "dev-libs/r594xdep[r594flag]"; eb r594${1}b 2 "dev-libs/maskeddep"; }
three f "dev-libs/r594fa dev-libs/r594fb"
three h "dev-libs/r594hb dev-libs/r594ha"
threev() { eb r594${1}top 1 "$2"; for q in a b; do eb r594${1}$q 1 ""; done; eb r594${1}a 2 "dev-libs/r594vdep[r594flag]"; eb r594${1}b 2 "dev-libs/maskeddep"; }
threev j "dev-libs/r594ja dev-libs/r594jb"
threev q "dev-libs/r594qb dev-libs/r594qa"
KW="" eb r594nokw 1 ""
KW="~amd64" eb r594ukw 1 "" 'IUSE="r594flag"'
eb r594xdep 1 ""
KW="~amd64" eb r594xdep 2 "" 'IUSE="r594flag"'
eb r594vdep 1 "" 'IUSE="r594flag"'
# installed masked dep: parent new, dep (package.mask'd) already installed (vdb in setup)
eb r594itop 1 "dev-libs/r594im"; eb r594im 1 ""; eb r594im 2 "dev-libs/maskeddep"
for f in "$R"/dev-libs/r594*/*.ebuild; do
  pf=$(basename "$f" .ebuild); out="$R/metadata/md5-cache/dev-libs/$pf"
  { echo "DEFINED_PHASES=-"
    sed -n 's/^DESCRIPTION="\(.*\)"$/DESCRIPTION=\1/p' "$f"; echo "EAPI=8"
    sed -n 's/^IUSE="\(.*\)"$/IUSE=\1/p' "$f"; sed -n 's/^KEYWORDS="\(.*\)"$/KEYWORDS=\1/p' "$f"
    sed -n 's/^RDEPEND="\(.*\)"$/RDEPEND=\1/p' "$f"; echo "SLOT=0"
    echo "_md5_=$(md5sum "$f" | cut -d' ' -f1)"; } > "$out"
done
eb r594ip 1 "dev-libs/hardmaskedpkg"   # installed parent, masked dep (vdb in mkcell)
eb r594iq 1 "dev-libs/r594gone"        # installed parent, absent dep (vdb in mkcell)
for f in "$R"/dev-libs/r594i[pq]/*.ebuild; do
  pf=$(basename "$f" .ebuild); out="$R/metadata/md5-cache/dev-libs/$pf"
  { echo "DEFINED_PHASES=-"; sed -n 's/^DESCRIPTION="\(.*\)"$/DESCRIPTION=\1/p' "$f"; echo "EAPI=8"
    sed -n 's/^KEYWORDS="\(.*\)"$/KEYWORDS=\1/p' "$f"; sed -n 's/^RDEPEND="\(.*\)"$/RDEPEND=\1/p' "$f"; echo "SLOT=0"
    echo "_md5_=$(md5sum "$f" | cut -d' ' -f1)"; } > "$out"
done
