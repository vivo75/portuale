#!/bin/bash
# S0 section-C redo (runs INSIDE localhost/test-portuale:latest).
# Faithful to oracle run6.sh: per-file line formats differ.
# Usage: PM=portage|portuale bash /s0/s0-sectionC.sh
set -u
PM=${PM:-portage}
OUT=/s0/out/$PM
S0W=/s0/out/work
mkdir -p "$OUT" "$S0W"
if [ "$PM" = portuale ]; then EMERGE=(/pm/portuale emerge); else EMERGE=(emerge); fi
BASE_OPTS=(--ignore-default-opts --getbinpkg=n --usepkg=n)
T341=/s0/repo-341
GPROF=/var/db/repos/gentoo/profiles/default/linux/amd64/23.0/systemd
U8='*/t341-dep
dev-libs/t341-*
dev-libs/t341-dep::t341
=dev-libs/t341-dep-1-2
=dev-libs/t341-dep-1-02
dev-libs/t341-dep-1
!dev-libs/t341-dep
dev-libs/t341-dep[x]'

C=$S0W/cfg-cfgC2; rm -rf "$C"; mkdir -p "$C/etc/portage/profile" "$C/etc/portage/env" "$C/etc/portage/sets" "$C/var/lib/portage"
printf '[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = /var/db/repos/gentoo\n\n[t341]\nlocation = %s\n' "$T341" > "$C/etc/portage/repos.conf"
echo 'PKGDIR="/var/tmp/s0/pkgdir"' > "$C/etc/portage/make.conf"
ln -sfn "$GPROF" "$C/etc/portage/make.profile"
echo 'X=1' > "$C/etc/portage/env/t341.conf"
printf '%s\n' "$U8" > "$C/etc/portage/package.mask"
printf '%s\n' "$U8" > "$C/etc/portage/package.unmask"
printf '%s\n' "$U8" | sed 's/$/ x/' > "$C/etc/portage/package.use"
printf '%s\n' "$U8" | sed 's/$/ x/' > "$C/etc/portage/package.accept_keywords"
printf '%s\n' "$U8" | sed 's/$/ MIT/' > "$C/etc/portage/package.license"
printf '%s\n' "$U8" | grep -v '\[' | sed 's/$/ t341.conf/' > "$C/etc/portage/package.env"
printf '%s\n' "$U8" > "$C/etc/portage/package.properties"
printf '%s\n' "$U8" > "$C/etc/portage/profile/package.mask"
printf '%s\n' "$U8" | sed 's/$/ x/' > "$C/etc/portage/profile/package.use"
printf '%s\n' "$U8" | sed 's/$/ x/' > "$C/etc/portage/profile/package.use.force"
printf '%s\n' "$U8" > "$C/etc/portage/profile/packages"

cell=C-userconfig
printf '%s\n' "${EMERGE[*]} -pv ${BASE_OPTS[*]} dev-libs/t341-dep" > "$OUT/$cell.argv"
PORTAGE_CONFIGROOT=$C ROOT=$C "${EMERGE[@]}" -pv "${BASE_OPTS[@]}" dev-libs/t341-dep \
  >"$OUT/$cell.stdout" 2>"$OUT/$cell.stderr"
printf '%s\n' "$?" > "$OUT/$cell.rc"

# crash cell, faithful: package.env with ONLY the use-dep line + env file
Cx=$S0W/cfg-cfgCx2; rm -rf "$Cx"; mkdir -p "$Cx/etc/portage/env"
printf '[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = /var/db/repos/gentoo\n\n[t341]\nlocation = %s\n' "$T341" > "$Cx/etc/portage/repos.conf"
echo 'PKGDIR="/var/tmp/s0/pkgdir"' > "$Cx/etc/portage/make.conf"
ln -sfn "$GPROF" "$Cx/etc/portage/make.profile"
echo 'X=1' > "$Cx/etc/portage/env/t341.conf"
printf 'dev-libs/t341-dep[x] t341.conf\n' > "$Cx/etc/portage/package.env"
cell=C-userenv-crash
printf '%s\n' "${EMERGE[*]} -pv ${BASE_OPTS[*]} dev-libs/t341-dep" > "$OUT/$cell.argv"
PORTAGE_CONFIGROOT=$Cx ROOT=$Cx "${EMERGE[@]}" -pv "${BASE_OPTS[@]}" dev-libs/t341-dep \
  >"$OUT/$cell.stdout" 2>"$OUT/$cell.stderr"
printf '%s\n' "$?" > "$OUT/$cell.rc"
echo "PM=$PM sectionC redone"
