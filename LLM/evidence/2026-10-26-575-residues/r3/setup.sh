#!/bin/bash
# setup.sh: fresh scratch cells fx (Portage, own cfg root fx.cfg) and fxp (portuale) from pmtest/fixtures + mk.sh
set -eu
S=/var/tmp/claude-r3; SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
rm -rf $S/fx $S/fxp $S/fx.cfg
cp -a $SRC $S/fx
[ -x $S/mk.sh ] && $S/mk.sh $S/fx/repo
cp -a $S/fx $S/fxp
mkdir -p $S/fx.cfg/etc; cp -a $S/fx/etc/portage $S/fx.cfg/etc/portage
rm -f $S/fx.cfg/etc/portage/make.profile; ln -s $S/fx/repo/profiles/default $S/fx.cfg/etc/portage/make.profile
sed -E "s#^location = (.*)#location = $S/fx/\1#" $S/fx/etc/portage/repos.conf/repos.conf > $S/fx.cfg/etc/portage/repos.conf/repos.conf
mkdir -p $S/ptmp
