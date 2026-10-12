#!/bin/bash
# S0 section-X: packages-file doubling isolation + ambient comparison.
# Runs INSIDE localhost/test-portuale:latest. Portage only.
set -u
OUT=/s0/out/portage
S0W=/s0/out/work
mkdir -p "$OUT" "$S0W"
BASE_OPTS=(--ignore-default-opts --getbinpkg=n --usepkg=n)
T341=/s0/repo-341
GPROF=/var/db/repos/gentoo/profiles/default/linux/amd64/23.0/systemd

# X-a: repo-341 without any `packages` file (scratch, container-local)
rm -rf $S0W/repo-nopkg; cp -a $T341 $S0W/repo-nopkg
rm -f $S0W/repo-nopkg/profiles/t341/packages $S0W/repo-nopkg/profiles/t341/noeapi/packages
XB=$S0W/cfg-cfgXB; rm -rf "$XB"; mkdir -p "$XB/etc/portage"
printf '[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = /var/db/repos/gentoo\n\n[t341]\nlocation = %s\n' "$S0W/repo-nopkg" > "$XB/etc/portage/repos.conf"
echo 'PKGDIR="/var/tmp/s0/pkgdir"' > "$XB/etc/portage/make.conf"
ln -sfn "$S0W/repo-nopkg/profiles/t341/noeapi" "$XB/etc/portage/make.profile"
printf 'emerge -pv %s dev-libs/t341-dep (PORTAGE_CONFIGROOT=%s; repo without packages files)\n' "${BASE_OPTS[*]}" "$XB" > "$OUT/B-prof-nopackages.argv"
PORTAGE_CONFIGROOT=$XB ROOT=$XB emerge -pv "${BASE_OPTS[@]}" dev-libs/t341-dep \
  >"$OUT/B-prof-nopackages.stdout" 2>"$OUT/B-prof-nopackages.stderr"
printf '%s\n' "$?" > "$OUT/B-prof-nopackages.rc"

# X-b: ambient comparison — same user files as cfg-cfgC2, but in /etc/portage, no configroot
C=$S0W/cfg-cfgC2
cp -a "$C"/etc/portage/package.mask "$C"/etc/portage/package.unmask "$C"/etc/portage/package.use \
  "$C"/etc/portage/package.accept_keywords "$C"/etc/portage/package.license "$C"/etc/portage/package.env \
  "$C"/etc/portage/package.properties /etc/portage/
rm -rf /etc/portage/profile; cp -a "$C/etc/portage/profile" /etc/portage/profile
mkdir -p /etc/portage/env; cp -a "$C"/etc/portage/env/t341.conf /etc/portage/env/
printf '[t341]\nlocation = /s0/repo-341\n' > /etc/portage/repos.conf/t341.conf
printf 'emerge -pv %s dev-libs/t341-dep (ambient /etc/portage, no PORTAGE_CONFIGROOT)\n' "${BASE_OPTS[*]}" > "$OUT/C-ambient.argv"
emerge -pv "${BASE_OPTS[@]}" dev-libs/t341-dep \
  >"$OUT/C-ambient.stdout" 2>"$OUT/C-ambient.stderr"
printf '%s\n' "$?" > "$OUT/C-ambient.rc"
# cleanup ambient (leave container clean for later runs in this boot)
rm -f /etc/portage/repos.conf/t341.conf
rm -rf /etc/portage/profile /etc/portage/package.mask /etc/portage/package.unmask \
  /etc/portage/package.use /etc/portage/package.accept_keywords /etc/portage/package.license \
  /etc/portage/package.env /etc/portage/package.properties
echo "sectionX done"
