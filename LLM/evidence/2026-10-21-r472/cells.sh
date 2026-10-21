#!/bin/bash
# usage: cells.sh "<emerge command>"   (e.g. emerge, or "portuale emerge"; needs ../2026-10-21-r463/setup.sh first, S459 on its #459 root)
# Six cells: --usepkgonly of a good and of a postinst-failing binpkg, plain, -q and --jobs=2.
# Seconds and the ROOT trailing slash are normalised; compare Portage's and portuale's output with diff.
EM=$1; S459=${S459:-/var/tmp/r472f}
cd "$S459" || exit 1
norm() { cat -v | sed 's/\^\[/E/g;s/took [0-9.]* s/took/;s|/root/|/root|g;s/line 18[0-9]/line N/'; }
for c in "" "-q" "--jobs=2"; do for pk in f459-good f459-postinstfail; do
  echo "=== $c $pk"
  ( unset PORTAGE_TMPDIR; export PORTAGE_CONFIGROOT=$S459 ROOT=$S459/root DISTDIR=$S459/dist PKGDIR=$S459/pkgdir EMERGE_DEFAULT_OPTS=--package-moves=n
    mv etc/portage/package.env pe.keep 2>/dev/null; rm -rf root tmp/portage; mkdir -p root
    $EM $c --usepkgonly dev-util/$pk </dev/null 2>&1; echo rc=$?
    mv pe.keep etc/portage/package.env 2>/dev/null ) | norm
done; done
