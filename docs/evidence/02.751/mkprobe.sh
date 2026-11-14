#!/bin/bash
# Build an S0 probe tree under $1 (variant $2: virt | mid; base $3: kept | nobase).
# Mirrors crates/portuale/src/check_world.rs add_ebuild/add_installed exactly.
set -euo pipefail
DEST="$1"; VARIANT="$2"; BASE="$3"
SRC=/home/vivo/repo/PORTUALE/portuale/fixtures
rm -rf "$DEST"; mkdir -p "$DEST"
cp -a "$SRC/." "$DEST/"
ROOT="$DEST"

add_ebuild_fixed() { # cpv slot "VAR=val" ... (dir-safe version)
  local cpv="$1" slot="$2"; shift 2
  local cat="${cpv%%/*}"
  local rest="${cpv#*/}"
  local pkg="${rest%-*}"
  local pf="$rest" dir="$ROOT/repo/$cat/$pkg" cdir="$ROOT/repo/metadata/md5-cache/$cat"
  mkdir -p "$dir" "$cdir"
  { echo 'EAPI="8"'; echo "SLOT=\"$slot\""; echo 'KEYWORDS="amd64"';
    for f in "$@"; do k="${f%%=*}"; v="${f#*=}"; echo "$k=\"$v\""; done; } > "$dir/$pf.ebuild"
  local sum; sum=$(md5sum "$dir/$pf.ebuild" | cut -d' ' -f1)
  { echo 'EAPI=8'; echo 'KEYWORDS=amd64';
    for f in "$@"; do echo "$f"; done; echo "SLOT=$slot"; } | LC_ALL=C sort > "$cdir/$pf"
  echo "_md5_=$sum" >> "$cdir/$pf"
}

add_installed() { # cpf slot "VAR=val" ...
  local cpf="$1" slot="$2"; shift 2
  local cat="${cpf%%/*}" dir="$ROOT/var/db/pkg/$cpf"; mkdir -p "$dir"
  echo "$cat" > "$dir/CATEGORY"; echo "$slot" > "$dir/SLOT"; echo "8" > "$dir/EAPI"
  for f in "$@"; do k="${f%%=*}"; v="${f#*=}"; echo "$v" > "$dir/$k"; done
}

if [ "$VARIANT" = virt ]; then MID=virtual/w751virt; else MID=dev-libs/w751mid; fi
if [ "${AMEND:-0}" = 1 ]; then
  # §3 step 3a: base BDEPENDs on prov, so prov is build-time-used and
  # (prov,user) becomes uncuttable; base upgrades 0/1 -> 0/2 (still a row).
  add_ebuild_fixed "$MID-1" "0" "RDEPEND=dev-libs/w751prov"
  add_ebuild_fixed "dev-libs/w751prov-2" "0/2" "RDEPEND=dev-libs/w751base dev-libs/w751user"
  add_ebuild_fixed "dev-libs/w751base-2" "0/2" "BDEPEND=dev-libs/w751prov"
  add_ebuild_fixed "dev-libs/w751user-1" "0" "BDEPEND=$MID"
  add_installed "$MID-1" "0" "RDEPEND=dev-libs/w751prov"
  add_installed "dev-libs/w751prov-1" "0/1" "RDEPEND=dev-libs/w751user dev-libs/w751base:0/1="
  add_installed "dev-libs/w751base-1" "0/1"
else
add_ebuild_fixed "$MID-1" "0" "RDEPEND=dev-libs/w751prov"
add_ebuild_fixed "dev-libs/w751prov-2" "0/2" "RDEPEND=dev-libs/w751base dev-libs/w751user"
add_ebuild_fixed "dev-libs/w751base-2" "0/2"
add_ebuild_fixed "dev-libs/w751user-1" "0" "BDEPEND=$MID"
add_installed "$MID-1" "0" "RDEPEND=dev-libs/w751prov"
add_installed "dev-libs/w751prov-1" "0/1" "RDEPEND=dev-libs/w751user dev-libs/w751base:0/1="
if [ "$BASE" = kept ]; then add_installed "dev-libs/w751base-2" "0/2"; fi
fi
printf 'dev-libs/w751user\ndev-libs/w751prov\n' >> "$ROOT/var/lib/portage/world"
echo "tree ready: $DEST (variant=$VARIANT base=$BASE amend=${AMEND:-0})"
