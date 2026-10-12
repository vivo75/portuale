#!/bin/bash
# S0 section-E redo (runs INSIDE localhost/test-portuale:latest).
# Sets/world use oracle run5's eight lines (bare).
# Usage: PM=portage|portuale bash /s0/s0-sectionE.sh
set -u
PM=${PM:-portage}
OUT=/s0/out/$PM
S0W=/s0/out/work
mkdir -p "$OUT" "$S0W"
if [ "$PM" = portuale ]; then EMERGE=(/pm/portuale emerge); else EMERGE=(emerge); fi
BASE_OPTS=(--ignore-default-opts --getbinpkg=n --usepkg=n)
T341=/s0/repo-341
GPROF=/var/db/repos/gentoo/profiles/default/linux/amd64/23.0/systemd
R6='*/t341-dep
dev-libs/t341-*
dev-libs/t341-dep::t341
=dev-libs/t341-dep-1-2
=dev-libs/t341-dep-1-02
dev-libs/t341-dep-1
!dev-libs/t341-dep
dev-libs/t341-dep[x]'

E=$S0W/cfg-cfgE2; rm -rf "$E"; mkdir -p "$E/etc/portage/sets" "$E/var/lib/portage" "$E/etc/portage"
printf '[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = /var/db/repos/gentoo\n\n[t341]\nlocation = %s\n' "$T341" > "$E/etc/portage/repos.conf"
echo 'PKGDIR="/var/tmp/s0/pkgdir"' > "$E/etc/portage/make.conf"
ln -sfn "$GPROF" "$E/etc/portage/make.profile"
printf '%s\n' "$R6" > "$E/etc/portage/sets/s341"
printf '%s\n' "$R6" > "$E/var/lib/portage/world"

for cell in E-sets-s341 E-sets-selected; do
  target=@s341; [ "$cell" = E-sets-selected ] && target=@selected
  printf '%s\n' "${EMERGE[*]} -pv ${BASE_OPTS[*]} $target" > "$OUT/$cell.argv"
  PORTAGE_CONFIGROOT=$E ROOT=$E "${EMERGE[@]}" -pv "${BASE_OPTS[@]}" "$target" \
    >"$OUT/$cell.stdout" 2>"$OUT/$cell.stderr"
  printf '%s\n' "$?" > "$OUT/$cell.rc"
done
echo "PM=$PM sectionE redone"
