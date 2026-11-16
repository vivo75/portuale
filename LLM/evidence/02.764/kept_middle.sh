#!/bin/bash
# usage: kept_middle.sh DEST ; oracle control of plan 02.764 S0.5: recreates `kept_middle_tree` (crates/portuale/src/plan_world.rs, backlog
# #746) in a scratch copy of ../pmtest/fixtures (ebuilds + egencache md5-cache + installed entries), runs the plan and both checkers.
# Both must report no row on w746*.
set -e
D=${1:?dest}; HERE=$(cd "$(dirname "$0")" && pwd); B=${BIN:-/var/tmp/764/bin}
rm -rf "$D"; cp -a "$HERE/../../../../pmtest/fixtures" "$D"
eb() { # pn slot key value
  local dir=$D/repo/dev-libs/$1; mkdir -p "$dir"
  { echo 'EAPI="8"'; echo "SLOT=\"$2\""; echo 'KEYWORDS="amd64"'; if [ -n "$3" ]; then echo "$3=\"$4\""; fi; } > "$dir/$1-$5.ebuild"; }
eb w746auto 0 BDEPEND dev-libs/w746help 1; eb w746help 0 RDEPEND dev-libs/w746loc 1
eb w746loc 0 RDEPEND "dev-libs/w746get dev-libs/w746prov" 1; eb w746get 0 RDEPEND dev-libs/w746icu 1
eb w746icu 0 BDEPEND dev-libs/w746auto 1; eb w746prov 0/2 "" "" 2
L=$D/repo/metadata/layout.conf; cp "$L" "$L.orig"; printf 'thin-manifests = true\nallow-missing-manifests = true\n' >> "$L"
PORTAGE_CONFIGROOT=$D egencache --update --repo=testrepo --repositories-configuration="[DEFAULT]
main-repo = testrepo
[testrepo]
location = $D/repo" $(cd $D/repo; ls -d dev-libs/w746*) 2>/dev/null
mv "$L.orig" "$L"
vdb() { local e=$D/var/db/pkg/$1; mkdir -p "$e"; echo dev-libs > "$e/CATEGORY"; echo 8 > "$e/EAPI"; echo "$2" > "$e/SLOT"; if [ -n "$3" ]; then echo "$3" > "$e/RDEPEND"; fi; }
vdb dev-libs/w746help-1 0 dev-libs/w746loc; vdb dev-libs/w746loc-1 0 "dev-libs/w746get dev-libs/w746prov:0/1="
vdb dev-libs/w746get-1 0 dev-libs/w746icu; vdb dev-libs/w746prov-2 0/2 ""
echo dev-libs/w746auto >> "$D/var/lib/portage/world"
export PORTAGE_CONFIGROOT=$D PORTAGE_REPOSITORIES="$(cat $(find $D/etc/portage/repos.conf -type f | sort) | awk 1)" ROOT=$D PORTAGE_RUNNING_ROOT=$D DISTDIR=$D/distfiles
$B/mrg --plan-world --json --sort depend > "$D.plan.out" 2> "$D.plan.err"
$B/mrg --plan-world --sort depend 2>/dev/null | grep w746
python3 -I "$HERE/classify.py" --vdb "$D/var/db/pkg" --filter w746 "$D.plan.out"
