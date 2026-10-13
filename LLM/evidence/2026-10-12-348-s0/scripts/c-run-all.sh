#!/bin/bash
# t348 S0.6 (worker c): run cell batches. Usage: c-run-all.sh [pkg...]
# Each package t348-c-<fam>-<phase> runs its ONE phase (both EAPI versions,
# both PMs). No args = everything.
set -u
S=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-12-348-s0/scripts/c-run-cell.sh
phase_of() { # pkg -> phase (last dash-component)
	echo "${1##*-}"
}
run_pkg() {
	local p=$1 ph
	case $p in
		t348-c-failglob) ph=compile;;
		*) ph=$(phase_of "$p");;
	esac
	# map package suffix word to ebuild phase name
	case $ph in
		prepare|compile|install|unpack) :;;
		configure) ph=configure;;
	esac
	"$S" "$p-7" "$ph"
	"$S" "$p-8" "$ph"
}
ALL="t348-c-eapply-prepare t348-c-eapply-compile t348-c-edo-compile t348-c-edo-install t348-c-ver-compile t348-c-ver-install t348-c-useflag-compile t348-c-useflag-install t348-c-libdir-compile t348-c-libdir-install t348-c-dosym-compile t348-c-dosym-install t348-c-strip-compile t348-c-strip-install t348-c-dodoc-compile t348-c-dodoc-install t348-c-doin-compile t348-c-doin-install t348-c-newins-compile t348-c-newins-install t348-c-einstalldocs-compile t348-c-einstalldocs-install t348-c-econf-configure t348-c-econf-compile t348-c-unpack-unpack t348-c-unpack-prepare t348-c-verq-compile t348-c-verq-install t348-c-die-compile t348-c-die-install t348-c-envunset-compile t348-c-envunset-install t348-c-keepdir-compile t348-c-keepdir-install t348-c-failglob t348-c-banned-compile t348-c-banned-install t348-c-hasq-compile t348-c-hasq-install t348-c-einstall-compile t348-c-einstall-install t348-c-insopts-compile t348-c-insopts-install"
if [[ $# == 0 ]]; then
	for p in $ALL; do run_pkg "$p"; done
else
	for p in "$@"; do run_pkg "$p"; done
fi
echo BATCH-DONE
