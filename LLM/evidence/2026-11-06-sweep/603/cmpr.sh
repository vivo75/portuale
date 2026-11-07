#!/bin/bash
# cmpr.sh <root> <emerge args...>: configroot = fx, ROOT = <root>; same env + argv for both PMs
FX=/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/3c60fa85-0c1a-42e7-b42c-5a4d843557bb/scratchpad/w3/fx; R=$1; shift
for who in portage portuale; do
  if [ $who = portage ]; then BIN=(/usr/bin/emerge); else BIN=(/home/vivo/repo/PORTUALE/portuale/target/release/portuale emerge); fi
  echo "### $who argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=\$FX ROOT=\$R PORTAGE_RUNNING_ROOT=\$R DISTDIR=\$FX/distfiles PORTAGE_TMPDIR=\$S PYTHONHASHSEED=0 ${BIN[*]##*/} --ignore-default-opts --getbinpkg=n --usepkg=n --color=n $*"
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$FX ROOT=$R PORTAGE_RUNNING_ROOT=$R DISTDIR=$FX/distfiles PORTAGE_TMPDIR=/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/3c60fa85-0c1a-42e7-b42c-5a4d843557bb/scratchpad/w3 PYTHONHASHSEED=0 CLEAN_DELAY=0 "${BIN[@]}" --ignore-default-opts --getbinpkg=n --usepkg=n --color=n "$@" 2>&1 | sed -e "s#$R/\?#\$R#g" -e "s#$FX/\?#\$FX#g" | grep -vE "^NOTE: Each|unique name for|^$|would be merged, in order|Calculating dep"
  echo "### rc: ${PIPESTATUS[0]}"
done
