#!/bin/bash
# run.sh pe|pu <root> args...
S=$(dirname $(readlink -f $0)); C=$S/cells
pm=$1 root=$2; shift 2
if [ $pm = pe ]; then
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$C/w604.cfg ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$C/w604/distfiles PORTAGE_TMPDIR=$C/ptmp PYTHONHASHSEED=0 /usr/bin/emerge --ignore-default-opts --getbinpkg=n --usepkg=n --color=n "$@" 2>&1
else
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$C/w604 ROOT=$root PORTAGE_RUNNING_ROOT=$root DISTDIR=$C/w604/distfiles PORTAGE_TMPDIR=$C/ptmp PYTHONHASHSEED=0 /home/vivo/repo/PORTUALE/portuale/target/release/portuale emerge --ignore-default-opts --getbinpkg=n --usepkg=n --color=n "$@" 2>&1
fi
echo "### rc: $?"
