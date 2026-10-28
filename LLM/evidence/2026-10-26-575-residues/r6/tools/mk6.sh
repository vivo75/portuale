#!/bin/bash
# mk6.sh <repo>: R6 probe shapes (scratch only). r6x has IUSE r6f with r6f? ( r6y ); r6a needs r6x plain, r6b needs r6x[r6f].
# r6p has IUSE r6g (no conditional deps); r6pa needs r6p, r6pb needs r6p[r6g] (USE change that changes no deps).
set -eu
R=$1
eb() { mkdir -p "$R/dev-libs/$1"; { echo 'EAPI=8'; echo "DESCRIPTION=\"R6 probe $1-$2\""; echo 'SLOT="0"'; echo 'KEYWORDS="amd64"'
  [ -n "$3" ] && echo "RDEPEND=\"$3\""; [ -n "${4:-}" ] && echo "IUSE=\"$4\""; true; } > "$R/dev-libs/$1/$1-$2.ebuild"
  f="$R/dev-libs/$1/$1-$2.ebuild"; out="$R/metadata/md5-cache/dev-libs/$1-$2"
  { echo "DEFINED_PHASES=-"; echo "DESCRIPTION=R6 probe $1-$2"; echo "EAPI=8"; [ -n "${4:-}" ] && echo "IUSE=$4"; echo "KEYWORDS=amd64"
    [ -n "$3" ] && echo "RDEPEND=$3"; echo "SLOT=0"; echo "_md5_=$(md5sum "$f" | cut -d' ' -f1)"; } > "$out"; }
eb r6x 1 "r6f? ( dev-libs/r6y )" "r6f"
eb r6y 1 ""
eb r6a 1 "dev-libs/r6x"
eb r6b 1 "dev-libs/r6x[r6f]"
eb r6p 1 "" "r6g"
eb r6pa 1 "dev-libs/r6p"
eb r6pb 1 "dev-libs/r6p[r6g]"
# chain of missing-dependency masks: r6c0 -> r6c1 -> r6c2 -> r6c3 -> r6gone (absent)
eb r6c0 1 "dev-libs/r6c1"
eb r6c1 1 "dev-libs/r6c2"
eb r6c2 1 "dev-libs/r6c3"
eb r6c3 1 "dev-libs/r6gone"
