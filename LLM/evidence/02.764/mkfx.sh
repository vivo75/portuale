#!/bin/bash
# usage: mkfx.sh DEST ; copies ../pmtest/fixtures to DEST, adds the 12 w764 ebuilds (plan 02.764 sec. 1) and generates their
# md5-cache with egencache ON THE COPY (temporary thin-manifest layout.conf on the copy only). Never writes a cache entry by hand.
set -e
D=${1:?dest}; HERE=$(cd "$(dirname "$0")" && pwd); FX=$HERE/../../../../pmtest/fixtures
rm -rf "$D"; cp -a "$FX" "$D"
mk() { # pn dep-key dep-value
  local dir=$D/repo/dev-libs/$1; mkdir -p "$dir"
  { echo 'EAPI=8'; echo "DESCRIPTION=\"fixture package: one-level build-time closure (backlog #764), shape $4\""
    echo 'SLOT="0"'; echo 'KEYWORDS="amd64"'; echo "$2=\"dev-libs/$3\""; } > "$dir/$1-1.ebuild"
}
mk w764ab BDEPEND w764at V2; mk w764at RDEPEND w764am V2; mk w764am RDEPEND w764au V2
mk w764au RDEPEND w764ax V2; mk w764ax RDEPEND w764am V2
mk w764cb BDEPEND w764ct V1; mk w764ct RDEPEND w764cm V1; mk w764cm RDEPEND w764cu V1; mk w764cu BDEPEND w764cb V1
mk w764db BDEPEND w764dt V3; mk w764dt RDEPEND w764du V3; mk w764du BDEPEND w764db V3
L=$D/repo/metadata/layout.conf; cp "$L" "$L.orig"
printf 'thin-manifests = true\nallow-missing-manifests = true\n' >> "$L"
cps=$(cd "$D/repo"; ls -d dev-libs/w764*)
PORTAGE_CONFIGROOT=$D egencache --update --repo=testrepo --repositories-configuration="[DEFAULT]
main-repo = testrepo
[testrepo]
location = $D/repo" $cps
mv "$L.orig" "$L"
for p in $cps; do n=$(basename $p); echo "$n $(grep '^_md5_' $D/repo/metadata/md5-cache/dev-libs/$n-1 | cut -d= -f2) $(md5sum $D/repo/dev-libs/$n/$n-1.ebuild | cut -d' ' -f1)"; done
