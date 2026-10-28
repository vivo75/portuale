#!/bin/bash
# mkcell.sh <name> <world-line...> : cells <name> (Portage, cfg <name>.cfg) and <name>p (portuale) from fx,
# with vdb entries for the installed shapes and a world file holding only the given lines.
set -eu
S=/var/tmp/claude-r3; c=$1; shift
rm -rf $S/$c $S/${c}p $S/$c.cfg
cp -a $S/fx $S/$c
vdb() { local d=$S/$c/var/db/pkg/dev-libs/$1; mkdir -p $d; echo dev-libs > $d/CATEGORY; echo 0 > $d/SLOT; echo 8 > $d/EAPI; echo testrepo > $d/repository; [ -n "${2:-}" ] && echo "$2" > $d/RDEPEND; true; }
vdb maskeddep-1.0; vdb r594ip-1 dev-libs/hardmaskedpkg; vdb r594iq-1 dev-libs/r594gone
printf '%s\n' "$@" > $S/$c/var/lib/portage/world
cp -a $S/$c $S/${c}p
cp -a $S/fx.cfg $S/$c.cfg
sed -i "s#$S/fx/#$S/$c/#" $S/$c.cfg/etc/portage/repos.conf/repos.conf
rm $S/$c.cfg/etc/portage/make.profile; ln -s $S/$c/repo/profiles/default $S/$c.cfg/etc/portage/make.profile
