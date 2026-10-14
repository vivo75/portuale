#!/bin/bash
# run.sh <tag> <root-name> [VAR=val ...] -- <command...>
# Hermetic env: PORTAGE_CONFIGROOT=/var/tmp/t354/cfg, ROOT=/var/tmp/t354/roots/<root-name>,
# PORTAGE_TMPDIR/LOGDIR/DEPCACHEDIR under that root's scratch. Extra VAR=val are exported.
# Writes $EV/cells/<tag>.{argv,env,rc,stdout,stderr}.
set -u
TAG=$1; RN=$2; shift 2
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-14-354-s0/cells
mkdir -p "$EV"
B=/var/tmp/t354
export PORTAGE_CONFIGROOT=$B/cfg
export ROOT=$B/roots/$RN
export BROOT=$ROOT
export PORTAGE_TMPDIR=$B/scratch/$RN/tmp
export PORTAGE_LOGDIR=$B/scratch/$RN/logs
export PORTAGE_DEPCACHEDIR=$B/scratch/$RN/depcache
mkdir -p "$ROOT" "$PORTAGE_TMPDIR" "$PORTAGE_LOGDIR" "$PORTAGE_DEPCACHEDIR"
: > "$EV/$TAG.env"
while [[ $1 != -- ]]; do export "$1"; echo "$1" >> "$EV/$TAG.env"; shift; done; shift
for v in PORTAGE_CONFIGROOT ROOT BROOT PORTAGE_TMPDIR PORTAGE_LOGDIR PORTAGE_DEPCACHEDIR; do echo "$v=${!v}" >> "$EV/$TAG.env"; done
printf '%s\n' "$*" > "$EV/$TAG.argv"
"$@" >"$EV/$TAG.stdout" 2>"$EV/$TAG.stderr"; RC=$?
echo $RC > "$EV/$TAG.rc"
echo "tag=$TAG rc=$RC"
