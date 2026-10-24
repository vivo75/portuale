#!/bin/bash
# #539 oracle: a `binrepos.conf` `[DEFAULT]` section's keys are inherited by
# every other section (Portage configparser), so `[DEFAULT]
# verify-signature = false` opts a keyless xpak-serving repo out of
# `gpkg_only`. Without the default the stock-global `true` applies and the
# xpak entry is ignored.
# Usage: probe.sh <portage|portuale> <outdir>
set -u
pm=$1; out=$(readlink -f "$2"); here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev539-pkgdir
rm -rf $T; mkdir -p $T/cfg/etc/portage/repos.conf $T/profile $T/root $T/binhost
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $T/profile/parent
ln -sfn $T/profile $T/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n' "$FX" > $T/cfg/etc/portage/repos.conf/r.conf
printf 'FEATURES="-news"\nACCEPT_LICENSE="*"\n' > $T/cfg/etc/portage/make.conf
cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $T/cfg/etc/portage/ 2>/dev/null
python3 - "$T/binhost" <<'EOF'
import sys
d = sys.argv[1]
idx = """CPV: app-misc/s539-1
DEFINED_PHASES: -
DESCRIPTION: backlog #539 default-inheritance probe
EAPI: 8
KEYWORDS: amd64
SLOT: 0
SIZE: 100
BUILD_ID: 1
BUILD_TIME: 1000
PATH: app-misc/s539-1.tbz2
"""
open(d + "/Packages", "w").write("VERSION: 0\nTIMESTAMP: 0\nPACKAGES: 1\n\n" + idx + "\n")
EOF
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev511-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev511-bin/emerge; bin=/var/tmp/ev511-bin/emerge; fi
mkdir -p "$out"
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
for shape in default-false no-default; do
  if [ "$shape" = default-false ]; then
    printf '[DEFAULT]\nverify-signature = false\n\n[plain]\nsync-uri = file://%s/binhost\npriority = 1\n' "$T" > $T/cfg/etc/portage/binrepos.conf
  else
    printf '[plain]\nsync-uri = file://%s/binhost\npriority = 1\n' "$T" > $T/cfg/etc/portage/binrepos.conf
  fi
  (cd $T && env $bin -p --color=n --getbinpkg app-misc/s539 >"$out/$shape.$pm.out" 2>"$out/$shape.$pm.err"; echo $? >"$out/$shape.$pm.rc")
  (cd $T && asciinema rec --headless --overwrite -q -c "env $bin -p --color=n --getbinpkg app-misc/s539" "$out/$shape.$pm.cast" >/dev/null 2>&1)
done
