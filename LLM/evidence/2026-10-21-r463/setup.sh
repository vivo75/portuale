#!/bin/bash
# #463-#467 oracle roots: the #458 root (schedparent + two leaves) for the
# stream, resume and --pretend cells, and the #459 root (binpkgs built by real
# `emerge --buildpkgonly`) for the per-entry PORTAGE_TMPDIR cell. The #459 root
# keeps PORTAGE_TMPDIR in make.conf: a calling-env value would beat package.env.
HERE=$(cd "$(dirname "$0")" && pwd)
export S7=${S7:-/var/tmp/r463e} S459=${S459:-/var/tmp/r467}
bash "$HERE/../2026-10-21-f458/setup.sh" || exit 1
bash "$HERE/../2026-10-21-f459/setup.sh" || exit 1
S=$S459
echo "PORTAGE_TMPDIR=\"$S/tmp\"" >> "$S/etc/portage/make.conf"
mkdir -p "$S/etc/portage/env" "$S/tmp2"
echo "PORTAGE_TMPDIR=$S/tmp2" > "$S/etc/portage/env/tmp2.conf"
( unset PORTAGE_TMPDIR; export PORTAGE_CONFIGROOT=$S ROOT=$S/root DISTDIR=$S/dist PKGDIR=$S/pkgdir EMERGE_DEFAULT_OPTS=--package-moves=n
  emerge -q --buildpkgonly dev-util/f459-postinstfail dev-util/f459-good </dev/null >/dev/null 2>&1 )
echo "dev-util/f459-postinstfail tmp2.conf" > "$S/etc/portage/package.env"
