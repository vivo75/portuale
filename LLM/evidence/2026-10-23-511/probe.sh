#!/bin/bash
# #511 oracle: mixed xpak/gpkg binhosts, Portage 3.0.83 (host emerge)
# versus portuale, on scratch repos (no ebuilds: remote-only binaries,
# like the backlog #347 S9 cells). Two file:// binrepos share one index:
# `[mixsig]` (verify-signature = true -> gpkg_only) and `[mixplain]`
# (verify-signature = false). Entries cover gpkg/xpak PATHs, with and
# without BUILD_ID, an unknown extension, and a missing PATH.
# Usage: probe.sh <portage|portuale> <outdir> [shapes.tsv]
# Each shape writes <id>.<pm>.{out,err,rc} (streams apart) and <id>.<pm>.cast.
set -u
pm=$1; out=$(readlink -f "$2"); here=$(cd "$(dirname "$0")" && pwd); shapes=${3:-$here/shapes.tsv}
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev511-tree
rm -rf $T; mkdir -p $T/cfg/etc/portage/repos.conf $T/profile $T/s511/profiles $T/s511/metadata
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $T/profile/parent
ln -sfn $T/profile $T/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n\n[s511]\nlocation = %s/s511\npriority = 70\n' "$FX" "$T" > $T/cfg/etc/portage/repos.conf/r.conf
printf 'PKGDIR="%s/pkgdir"\nFEATURES="-news"\nACCEPT_LICENSE="*"\n' "$T/cfg" > $T/cfg/etc/portage/make.conf
cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $T/cfg/etc/portage/ 2>/dev/null
printf 's511\n' > $T/s511/profiles/repo_name
printf 'app-misc\n' > $T/s511/profiles/categories
printf 'masters = testrepo\n' > $T/s511/metadata/layout.conf
# The shared mixed index: gpkg + xpak PATHs, with/without BUILD_ID,
# an unknown extension (s511-e), no PATH at all (s511-f).
idx() {
  local cpv=$1 rest=$2
  printf 'CPV: %s\nDEFINED_PHASES: -\nDESCRIPTION: backlog #511 mixed-binhost entry %s\nEAPI: 8\nIUSE:\nKEYWORDS: amd64\nSLOT: 0\nUSE:\nREPO: s511\nSIZE: 100\n%s\n' "$cpv" "$cpv" "$rest"
}
{
  printf 'VERSION: 0\nTIMESTAMP: 0\nPACKAGES: 6\n\n'
  idx app-misc/s511-a-1 'PATH: app-misc/s511-a-1.gpkg.tar
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-b-1 'PATH: app-misc/s511-b-1.tbz2
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-c-1 'PATH: app-misc/s511-c-1.gpkg.tar
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-d-1 'PATH: app-misc/s511-d-1.tbz2
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-e-1 'PATH: app-misc/s511-e-1.weird
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-f-1 'BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
} > $T/binhost-Packages
mkdir -p $T/binhost-sig $T/binhost-plain
cp $T/binhost-Packages $T/binhost-sig/Packages
# The plain repo serves only the two plain shapes (xpak usable there).
{
  printf 'VERSION: 0\nTIMESTAMP: 0\nPACKAGES: 4\n\n'
  idx app-misc/s511-g-1 'PATH: app-misc/s511-g-1.xpak
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-h-1 'PATH: app-misc/s511-h-1.gpkg.tar
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-i-1 'PATH: app-misc/s511-i-1.weird
BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
  idx app-misc/s511-j-1 'BUILD_ID: 1
BUILD_TIME: 1000'; printf '\n'
} > $T/binhost-plain/Packages
printf '[mixsig]\nsync-uri = file://%s/binhost-sig\npriority = 1\nverify-signature = true\n\n[mixplain]\nsync-uri = file://%s/binhost-plain\npriority = 2\nverify-signature = false\n' "$T" "$T" > $T/cfg/etc/portage/binrepos.conf
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
mkdir -p $T/root
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev511-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev511-bin/emerge; bin=/var/tmp/ev511-bin/emerge; fi
mkdir -p "$out"
while IFS=$'\t' read -r id envs args; do
  [ -z "$id" ] || [ "${id:0:1}" = "#" ] && continue
  [ "$envs" = - ] && envs=
  cmd="env $envs $bin $args"
  # eval: an envs cell may carry a quoted multi-word value
  # (FEATURES="-news binpkg-request-signature").
  (cd $T && eval "$cmd" >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
  (cd $T && asciinema rec --headless --overwrite -q -c "$cmd" "$out/$id.$pm.cast" >/dev/null 2>&1)
done < "$shapes"
