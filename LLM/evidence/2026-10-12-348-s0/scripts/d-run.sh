#!/bin/bash
# d-run.sh: S0.7 capture runner (worker d).
# Usage: d-run.sh <cell> <portage|portuale> [--root DIR] -- <command...>
# Writes LLM/evidence/2026-10-12-348-s0/d/<cell>.<pm>.{argv,rc,stdout,stderr}
# Env: hermetic configroot /var/tmp/t348-d/cfg, ROOT=cfg unless --root given.
set -u
CELL=$1; PM=$2; shift 2
ROOT_OVERRIDE=""
if [[ ${1:-} == "--root" ]]; then ROOT_OVERRIDE=$2; shift 3; fi
if [[ ${1:-} == "--" ]]; then shift; fi

EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-12-348-s0/d
CFG=/var/tmp/t348-d/cfg
ROOTDIR=${ROOT_OVERRIDE:-$CFG}

export PORTAGE_CONFIGROOT=$CFG
export ROOT=$ROOTDIR
# BROOT (build root, where IDEPEND/DEPEND build-time deps merge) defaults
# to the host /. Redirect it hermetically: without this, merging an
# IDEPEND package tries to write the host /var/db/pkg (observed
# 2026-10-12: "Read-only file system: /var/db/pkg", no host write).
export BROOT=$ROOTDIR
export PORTAGE_TMPDIR=$CFG/tmp
# Hermetic logs + dep-cache (pmtest convention: conftest scrubs these,
# test_portuale.py:2961-2994 sets them per-test under tmp_path). Without
# them Portage appends to the host /.gentoo/log/emerge.log and the host
# edb dep-cache even with ROOT set (observed 2026-10-12).
export PORTAGE_LOGDIR=$CFG/logs
export PORTAGE_DEPCACHEDIR=$CFG/depcache

printf '%s\n' "$*" > "$EV/$CELL.$PM.argv"
if [[ $PM == portuale ]]; then
  # Route through the portuale multicall symlinks beside the release binary.
  REL=/home/vivo/repo/PORTUALE/portuale/target/release
  case $1 in
    emerge) shift; set -- "$REL/emerge" "$@" ;;
    ebuild) shift; set -- "$REL/ebuild" "$@" ;;
  esac
fi
OUT=$("$@" 2>"$EV/$CELL.$PM.stderr"); RC=$?
printf '%s' "$OUT" > "$EV/$CELL.$PM.stdout"
printf '%d\n' "$RC" > "$EV/$CELL.$PM.rc"
echo "cell=$CELL pm=$PM rc=$RC"
