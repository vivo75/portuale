#!/bin/bash
# t348 S0.6 (worker c): run one cell (one ebuild + one phase) on BOTH PMs.
# Usage: c-run-cell.sh <pf> <phase>
#   pf like t348-c-eapply-7 ; phase like compile|install|configure|prepare|unpack
# Env (hermetic, never ROOT=/):
#   PORTAGE_CONFIGROOT=/var/tmp/t348-c/cfg ROOT=/var/tmp/t348-c/root
#   PORTAGE_TMPDIR=/var/tmp/t348-c/tmp  (exported so BOTH PMs share the layout)
# Captures: LLM/evidence/2026-10-12-348-s0/c/<cell>-<pm>.{argv,rc,stdout,stderr}
#   plus <cell>-<pm>.plog (copy of ${T}/t348.log when the phase wrote one).
set -u
PF=$1; PHASE=$2
PN=${PF%-*}   # t348-c-<fam> (strip only the -<ver> tail)
PKGDIR=/home/vivo/repo/PORTUALE/pmtest/fixtures/repo-348/app-misc/${PN}
EB=${PKGDIR}/${PF}.ebuild
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-12-348-s0/c
PORTUALE=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
TMPD=/var/tmp/t348-c/tmp

export PORTAGE_CONFIGROOT=/var/tmp/t348-c/cfg
export ROOT=/var/tmp/t348-c/root
export PORTAGE_TMPDIR=${TMPD}

run_pm() { # <pm> <cmd...>
	local pm=$1; shift
	local cell="c-${PF}-${PHASE}-${pm}"
	rm -rf "${TMPD}/portage"
	# shellcheck disable=SC2086
	"$@" > "${EV}/${cell}.stdout" 2> "${EV}/${cell}.stderr"
	echo "$?" > "${EV}/${cell}.rc"
	printf '%s\n' "$*" > "${EV}/${cell}.argv"
	local plog="${TMPD}/portage/app-misc/${PF}/temp/t348.log"
	if [[ -f ${plog} ]]; then
		cp "${plog}" "${EV}/${cell}.plog"
	else
		: > "${EV}/${cell}.plog"
	fi
	echo "${cell}: rc=$(cat "${EV}/${cell}.rc")"
}

run_pm portage ebuild "${EB}" "${PHASE}"
run_pm portuale "${PORTUALE}" ebuild "${EB}" "${PHASE}"
