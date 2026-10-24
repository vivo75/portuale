#!/bin/bash
# #536 overlap probe: one CPV in both binrepo indexes under different
# `gpkg_only` policies. Portage processes binrepos in descending priority
# and skips an entry whose instance a higher-priority repo already injected
# (`cpv_exists`) *before* its `gpkg_only` check, so the shadowed entry's
# warnings never fire.
#
# Layout (priorities: [mixsig]=1 verify-signature=true -> gpkg_only,
# [mixplain]=2 verify-signature=false):
#   s511ov-1  .gpkg.tar in mixplain (higher, injected), .weird in mixsig
#             -> resolves, NO `Unsupported ... s511ov` line (skipped silently).
#   s511ow-1  .tbz2 in mixplain (higher, injected, no warning there),
#             .weird in mixsig -> resolves, NO `Unsupported ... s511ow` line.
#   s511ox-1  .tbz2 in mixplain (higher, injected), .weird in mixsig
#             -> resolves, NO `Unsupported ... s511ox` line.
#   (The once-per-repo xpak warning cannot distinguish: s511-b always
#   triggers the [mixsig] line. Resolution rows do not name the repo.)
# Usage: probe-overlap.sh <portage|portuale> <outdir>
# Each shape writes <id>.<pm>.{out,err,rc} (streams apart) and <id>.<pm>.cast.
set -u
pm=$1; out=$(readlink -f "$2"); here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev511-overlap
rm -rf $T; mkdir -p $T/cfg/etc/portage/repos.conf $T/profile $T/s511/profiles $T/s511/metadata
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $T/profile/parent
ln -sfn $T/profile $T/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n\n[s511]\nlocation = %s/s511\npriority = 70\n' "$FX" "$T" > $T/cfg/etc/portage/repos.conf/r.conf
printf 'PKGDIR="%s/pkgdir"\nFEATURES="-news"\nACCEPT_LICENSE="*"\n' "$T/cfg" > $T/cfg/etc/portage/make.conf
cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $T/cfg/etc/portage/ 2>/dev/null
printf 's511\n' > $T/s511/profiles/repo_name
printf 'app-misc\n' > $T/s511/profiles/categories
printf 'masters = testrepo\n' > $T/s511/metadata/layout.conf
# The shared overlap index heads: VERSION/TIMESTAMP/PACKAGES headers are
# written by idx_head, entries by idx (same stanza shape as probe.sh).
idx_head() { printf 'VERSION: 0\nTIMESTAMP: 0\nPACKAGES: %s\n\n' "$1"; }
idx() {
  local cpv=$1 rest=$2
  printf 'CPV: %s\nDEFINED_PHASES: -\nDESCRIPTION: backlog #536 overlap entry %s\nEAPI: 8\nIUSE:\nKEYWORDS: amd64\nSLOT: 0\nUSE:\nREPO: s511\nSIZE: 100\n%s\n' "$cpv" "$cpv" "$rest"
}
{
  idx_head 4
  idx app-misc/s511-a-1 'PATH: app-misc/s511-a-1.gpkg.tar
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511ov-1 'PATH: app-misc/s511ov-1.weird
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511ow-1 'PATH: app-misc/s511ow-1.weird
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511ox-1 'PATH: app-misc/s511ox-1.weird
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
} > $T/binhost-Packages
mkdir -p $T/binhost-sig $T/binhost-plain
cp $T/binhost-Packages $T/binhost-sig/Packages
{
  idx_head 4
  idx app-misc/s511-g-1 'PATH: app-misc/s511-g-1.xpak
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511ov-1 'PATH: app-misc/s511ov-1.gpkg.tar
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511ow-1 'PATH: app-misc/s511ow-1.tbz2
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511ox-1 'PATH: app-misc/s511ox-1.tbz2
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
} > $T/binhost-plain/Packages
printf '[mixsig]\nsync-uri = file://%s/binhost-sig\npriority = 1\nverify-signature = true\n\n[mixplain]\nsync-uri = file://%s/binhost-plain\npriority = 2\nverify-signature = false\n' "$T" "$T" > $T/cfg/etc/portage/binrepos.conf
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
mkdir -p $T/root
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev511-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev511-bin/emerge; bin=/var/tmp/ev511-bin/emerge; fi
mkdir -p "$out"
for id in s511ov s511ow s511ox; do
  (cd $T && env $bin -p --color=n --getbinpkg app-misc/$id >"$out/ov-$id.$pm.out" 2>"$out/ov-$id.$pm.err"; echo $? >"$out/ov-$id.$pm.rc")
  (cd $T && asciinema rec --headless --overwrite -q -c "env $bin -p --color=n --getbinpkg app-misc/$id" "$out/ov-$id.$pm.cast" >/dev/null 2>&1)
done
# Both repos `gpkg_only` (request-signature): the plain xpak warns and drops
# without injecting, so the sig xpak is NOT shadowed and warns too.
(cd $T && env FEATURES="-news binpkg-request-signature" $bin -p --color=n --getbinpkg app-misc/s511ox >"$out/ov-s511ox-reqsig.$pm.out" 2>"$out/ov-s511ox-reqsig.$pm.err"; echo $? >"$out/ov-s511ox-reqsig.$pm.rc")
(cd $T && asciinema rec --headless --overwrite -q -c "env FEATURES=\"-news binpkg-request-signature\" $bin -p --color=n --getbinpkg app-misc/s511ox" "$out/ov-s511ox-reqsig.$pm.cast" >/dev/null 2>&1)
