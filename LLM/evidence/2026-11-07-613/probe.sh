#!/bin/bash
# probe.sh portage|portuale <args...> : abk0 world-B root (= pmtest _abk0_worldb_root), fixture configroot
SP=${SP:?scratch dir holding cells/ (pmtest fixtures copy + .cfg, see clusterC cell.sh) and abkroot/ (_abk0_worldb_root)}
C=$SP/cells/${CELL:-p613}; R=$SP/abkroot; who=$1; shift
if [ $who = portage ]; then CFG=$SP/cells/${CELL:-p613}.cfg; BIN=(/usr/bin/emerge); else CFG=$C; BIN=(${PBIN:-/home/vivo/repo/PORTUALE/portuale/target/release/portuale} emerge); fi
echo "### argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=\$CFG ROOT=\$R PORTAGE_RUNNING_ROOT=\$R DISTDIR=\$C/distfiles PYTHONHASHSEED=0 ${BIN[*]##*/} --ignore-default-opts --getbinpkg=n --usepkg=n --color=n $*"
env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$CFG ROOT=$R PORTAGE_RUNNING_ROOT=$R DISTDIR=$C/distfiles PORTAGE_TMPDIR=$SP/ptmp PYTHONHASHSEED=0 "${BIN[@]}" --ignore-default-opts --getbinpkg=n --usepkg=n --color=n "$@" 2>&1 | sed "s#$R#\$R#g; s#$C#\$C#g" | grep -vE "missing masters attribute|Set 'masters = testrepo'|repo_name|^$"
echo "### rc: ${PIPESTATUS[0]}"
