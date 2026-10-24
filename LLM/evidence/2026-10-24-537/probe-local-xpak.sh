#!/bin/bash
# #537 local probe: a `$PKGDIR` holding only an xpak binpkg (no `Packages`
# index, so both managers walk and parse it live), resolved with
# `--usepkgonly` with and without `FEATURES=binpkg-request-signature`.
# Portage 3.0.83 ignores the xpak under the flag with
# `Local XPAK packages are ignored due to 'binpkg-request-signature'.`
# and the atom has no candidate (rc 1); without the flag it resolves.
# Usage: probe-local-xpak.sh <portage|portuale> <outdir>
# Each shape writes <id>.<pm>.{out,err,rc} (streams apart) and <id>.<pm>.cast.
set -u
pm=$1; out=$(readlink -f "$2"); here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev537-pkgdir
rm -rf $T; mkdir -p $T/cfg/etc/portage/repos.conf $T/profile $T/pkgdir/dev-libs $T/root/var/lib/portage
cp "$FX/pkgdir/dev-libs/binaryonlypkg-1.0.tbz2" $T/pkgdir/dev-libs/
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $T/profile/parent
ln -sfn $T/profile $T/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n' "$FX" > $T/cfg/etc/portage/repos.conf/r.conf
cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $T/cfg/etc/portage/ 2>/dev/null
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
mkdir -p $T/root
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev511-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev511-bin/emerge; bin=/var/tmp/ev511-bin/emerge; fi
mkdir -p "$out"
for env in plain reqsig; do
  if [ "$env" = plain ]; then features="-news -pkgdir-index-trusted"
  else features="-news -pkgdir-index-trusted binpkg-request-signature"; fi
  printf 'PKGDIR="%s/pkgdir"\nFEATURES="%s"\nACCEPT_LICENSE="*"\n' "$T" "$features" > $T/cfg/etc/portage/make.conf
  id="xpak-$env"
  (cd $T && env $bin -p --color=n --usepkgonly dev-libs/binaryonlypkg >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
  (cd $T && asciinema rec --headless --overwrite -q -c "env $bin -p --color=n --usepkgonly dev-libs/binaryonlypkg" "$out/$id.$pm.cast" >/dev/null 2>&1)
done
