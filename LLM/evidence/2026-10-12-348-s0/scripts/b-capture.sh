#!/bin/bash
# b-capture.sh <portage|portuale> -- S0.4 (env) + S0.5 (phase sequence)
# capture harness, prefix b (plan LLM/02.348-eapi-phase-audit.opus.md).
# One cell = <cell>.argv/.rc/.stdout/.stderr + <cell>.stage-* (copies of
# ${T348_LOGDIR} content produced by the cell). Verdicts are NOT assigned
# here; comparison happens afterwards against the Portage cells.
set -u
PM=${1:?usage: b-capture.sh portage|portuale}
EVDIR=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-12-348-s0/b
FX=/home/vivo/repo/PORTUALE/pmtest/fixtures/repo-348
BASE=/var/tmp/t348-b
CFG=$BASE/cfg-$PM
ROOT=$BASE/root-$PM
STAGE=$BASE/stage
PORT=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
export PORTAGE_CONFIGROOT=$CFG ROOT=$ROOT
# S0 method note: PORTAGE_TMPDIR is exported (same value as make.conf) so
# that both PMs build under tmp-$PM. Portage honours it from either source;
# portuale's direct `ebuild` path honours it ONLY from the environment and
# ignores the make.conf value (dedicated notmpenv cells below isolate this).
export PORTAGE_TMPDIR=$BASE/tmp-$PM

cell() {
	# cell <name> <command...> : clear stage, run, save all artefacts.
	local name=$1
	shift
	rm -rf "$STAGE"
	mkdir -p "$STAGE"
	{
		local a
		for a in "$@"; do printf '%s ' "$a"; done
		printf '\n'
	} >"$EVDIR/$name.argv"
	"$@" >"$EVDIR/$name.stdout" 2>"$EVDIR/$name.stderr"
	echo $? >"$EVDIR/$name.rc"
	local f
	for f in "$STAGE"/*; do
		[[ -e $f ]] || continue
		cp "$f" "$EVDIR/$name.stage-$(basename "$f")"
	done
}

cell_noclear() {
	# cell_noclear <name> <command...> : like cell but keeps stage content
	# (for two-step cells such as install+qmerge sharing one image).
	local name=$1
	shift
	{
		local a
		for a in "$@"; do printf '%s ' "$a"; done
		printf '\n'
	} >"$EVDIR/$name.argv"
	"$@" >"$EVDIR/$name.stdout" 2>"$EVDIR/$name.stderr"
	echo $? >"$EVDIR/$name.rc"
	local f
	for f in "$STAGE"/*; do
		[[ -e $f ]] || continue
		cp "$f" "$EVDIR/$name.stage-$(basename "$f")"
	done
}

if [[ $PM == portage ]]; then
	EBUILD=(ebuild)
	EMERGE=(emerge)
else
	EBUILD=("$PORT" ebuild)
	EMERGE=("$PORT" emerge)
fi

reset_tmp() { rm -rf "$BASE/tmp-$PM"; mkdir -p "$BASE/tmp-$PM"; reset_stray; }
# portuale's direct `ebuild` path ignores PORTAGE_TMPDIR from make.conf and
# falls back to /var/tmp/portage (see notmpenv cells); with PORTAGE_TMPDIR
# exported above it stays hermetic, but a stale fallback builddir would
# poison stamp checks, so it is cleared together with tmp-$PM.
reset_stray() { rm -rf /var/tmp/portage/app-misc/t348-b-*; }
reset_root() { rm -rf "$ROOT"; mkdir -p "$ROOT"; }

ENVF=$FX/app-misc/t348-b-env/t348-b-env
PHF=$FX/app-misc/t348-b-phases/t348-b-phases
LIMF=$FX/app-misc/t348-b-phaseslim/t348-b-phaseslim
RTF=$FX/app-misc/t348-b-phasert/t348-b-phasert

########## S0.4: make.conf-PORTAGE_TMPDIR isolation cells (EAPI 7 only) ##########
# Same argv as the setup cell but WITHOUT PORTAGE_TMPDIR in the environment:
# Portage honours the make.conf value (builddir stays under tmp-$PM);
# portuale falls back to /var/tmp/portage. Stray fallback dirs removed
# before (clean slate) and after (leave no trace in the shared junkyard).
reset_tmp
reset_stray
cell "b-env-7-$PM-ebuild-setup-notmpenv" env -u PORTAGE_TMPDIR "${EBUILD[@]}" --skip-manifest "$ENVF-7.ebuild" setup
reset_stray

########## S0.4: environment per phase, direct ebuild calls ##########
for E in 7 8; do
	reset_tmp
	reset_stray
	for PH in pretend setup unpack prepare configure compile test install preinst postinst prerm postrm; do
		cell "b-env-$E-$PM-ebuild-$PH" "${EBUILD[@]}" --skip-manifest "$ENVF-$E.ebuild" "$PH"
	done
done

########## S0.4: emerge install / pretend / upgrade / unmerge cycle ##########
reset_root
reset_tmp
cell "b-env-$PM-emerge-install7" "${EMERGE[@]}" --oneshot --ignore-default-opts =app-misc/t348-b-env-7
cell_noclear "b-env-$PM-emerge-pretend8" "${EMERGE[@]}" --pretend --ignore-default-opts =app-misc/t348-b-env-8
cell_noclear "b-env-$PM-emerge-upgrade78" "${EMERGE[@]}" --oneshot --ignore-default-opts =app-misc/t348-b-env-8
cell_noclear "b-env-$PM-emerge-unmerge8" "${EMERGE[@]}" --unmerge --ignore-default-opts =app-misc/t348-b-env-8
# binary MERGE_TYPE attempt: build a binpkg, then install from it alone
cell "b-env-$PM-emerge-buildpkg8" "${EMERGE[@]}" --oneshot --ignore-default-opts --buildpkgonly =app-misc/t348-b-env-8
reset_root
cell "b-env-$PM-emerge-binpkg8" "${EMERGE[@]}" --oneshot --ignore-default-opts --usepkgonly =app-misc/t348-b-env-8

########## S0.5: direct pretend + full sequence per package/EAPI ##########
for spec in "$PHF:t348-b-phases" "$LIMF:t348-b-phaseslim" "$RTF:t348-b-phasert"; do
	F=${spec%%:*}
	PN=${spec##*:}
	for E in 7 8; do
		reset_tmp
		cell "b-phases-$PN-$E-$PM-ebuild-pretend" "${EBUILD[@]}" --skip-manifest "$F-$E.ebuild" pretend
		reset_tmp
		cell "b-phases-$PN-$E-$PM-ebuild-seq" "${EBUILD[@]}" --skip-manifest "$F-$E.ebuild" setup unpack prepare configure compile test install
	done
done
# skip/default semantics for undefined src_prepare/src_configure (lim only)
for E in 7 8; do
	reset_tmp
	cell "b-phases-t348-b-phaseslim-$E-$PM-ebuild-prepare" "${EBUILD[@]}" --skip-manifest "$LIMF-$E.ebuild" prepare
	cell_noclear "b-phases-t348-b-phaseslim-$E-$PM-ebuild-configure" "${EBUILD[@]}" --skip-manifest "$LIMF-$E.ebuild" configure
done

########## S0.5: emerge -p, --oneshot, FEATURES/RESTRICT, --buildpkgonly, qmerge ##########
reset_root
reset_tmp
for spec in t348-b-phases-7 t348-b-phases-8 t348-b-phaseslim-7 t348-b-phaseslim-8 t348-b-phasert-7 t348-b-phasert-8; do
	cell_noclear "b-phases-$spec-$PM-emerge-pretend" "${EMERGE[@]}" --pretend --ignore-default-opts "=app-misc/$spec"
done
reset_root
reset_tmp
for spec in t348-b-phases-7 t348-b-phases-8 t348-b-phaseslim-7 t348-b-phaseslim-8 t348-b-phasert-7 t348-b-phasert-8; do
	cell_noclear "b-phases-$spec-$PM-emerge-oneshot" "${EMERGE[@]}" --oneshot --ignore-default-opts "=app-misc/$spec"
done
# FEATURES=test forced on rt (RESTRICT=test must still skip src_test)
cell_noclear "b-phases-t348-b-phasert-8-$PM-emerge-featurestest" env FEATURES=test "${EMERGE[@]}" --oneshot --ignore-default-opts =app-misc/t348-b-phasert-8
# FEATURES=-test forced off full (src_test must be skipped)
cell_noclear "b-phases-t348-b-phases-8-$PM-emerge-featuresnotest" env FEATURES=-test "${EMERGE[@]}" --oneshot --ignore-default-opts =app-misc/t348-b-phases-8
cell_noclear "b-phases-t348-b-phases-7-$PM-emerge-buildpkgonly" "${EMERGE[@]}" --oneshot --ignore-default-opts --buildpkgonly =app-misc/t348-b-phases-7
# qmerge after a fresh direct install (preinst/postinst run at merge time)
reset_tmp
cell "b-phases-t348-b-phases-7-$PM-ebuild-install" "${EBUILD[@]}" --skip-manifest "$PHF-7.ebuild" setup unpack prepare configure compile test install
cell_noclear "b-phases-t348-b-phases-7-$PM-ebuild-qmerge" "${EBUILD[@]}" --skip-manifest "$PHF-7.ebuild" qmerge

echo "done $PM"
