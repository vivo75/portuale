#!/bin/bash
# mk.sh <repo-dir> : add the R1 S0 shapes (dev-libs/r592*) to a scratch repo
set -eu
R=$1
eb() { # eb <pn> <pv> <rdepend> [extra]
  mkdir -p "$R/dev-libs/$1"
  { echo 'EAPI=8'; echo "DESCRIPTION=\"fixture package: R1 S0 (#592) $1-$2\""; echo 'SLOT="0"'; echo "KEYWORDS=\"${KW:-amd64}\""
    [ -n "$3" ] && echo "RDEPEND=\"$3\""; [ -n "${4:-}" ] && echo "$4"; true; } > "$R/dev-libs/$1/$1-$2.ebuild"
}
for p in a c d; do
  case $p in
    a) MISS="dev-libs/r592agone" ;;
    c) MISS="dev-libs/r592cmasked" ;;
    d) MISS="dev-libs/r592dflag[r592flag]" ;;
  esac
  eb r592${p}top 1 "dev-libs/r592${p}x dev-libs/r592${p}y"
  eb r592${p}x 1 "dev-libs/r592${p}s"
  eb r592${p}x 2 "=dev-libs/r592${p}s-1"
  eb r592${p}y 1 "dev-libs/r592${p}s"
  eb r592${p}y 2 "=dev-libs/r592${p}s-2 $MISS"
  eb r592${p}s 1 ""
  eb r592${p}s 2 ""
done
KW="~amd64" eb r592cmasked 1 ""
eb r592dflag 1 "" 'IUSE="r592flag"'
eb r592etop 1 "dev-libs/r592em dev-libs/r592ex dev-libs/r592ey"
eb r592em 1 ""
eb r592em 2 "dev-libs/r592egone"
eb r592ex 1 "=dev-libs/r592es-1"
eb r592ey 1 "dev-libs/r592es"
eb r592ey 2 "=dev-libs/r592es-2"
eb r592es 1 ""
eb r592es 2 ""
eb r592ftop 1 "dev-libs/r592fm dev-libs/r592fx dev-libs/r592fy"
eb r592fm 1 ""
eb r592fm 2 "dev-libs/r592fflag[r592flag]"
eb r592fflag 1 "" 'IUSE="r592flag"'
eb r592fx 1 "=dev-libs/r592fs-1"
eb r592fy 1 "dev-libs/r592fs"
eb r592fy 2 "=dev-libs/r592fs-2"
eb r592fs 1 ""
eb r592fs 2 ""
# hand-written md5-cache entries (egencache wants a Manifest)
for f in "$R"/dev-libs/r592*/*.ebuild; do
  pf=$(basename "$f" .ebuild); out="$R/metadata/md5-cache/dev-libs/$pf"
  { echo "DEFINED_PHASES=-"
    sed -n 's/^DESCRIPTION="\(.*\)"$/DESCRIPTION=\1/p' "$f"; echo "EAPI=8"
    sed -n 's/^IUSE="\(.*\)"$/IUSE=\1/p' "$f"; sed -n 's/^KEYWORDS="\(.*\)"$/KEYWORDS=\1/p' "$f"
    sed -n 's/^RDEPEND="\(.*\)"$/RDEPEND=\1/p' "$f"; echo "SLOT=0"
    echo "_md5_=$(md5sum "$f" | cut -d' ' -f1)"; } > "$out"
done
