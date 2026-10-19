#!/bin/bash
# run.sh <tag> <root-name> [VAR=val ...] -- <command...>
# Hermetic env: PORTAGE_CONFIGROOT=/var/tmp/pt-381-s0/cfg, ROOT=/var/tmp/pt-381-s0/roots/<root-name>.
# PM selection: export PM=portage (default, host /usr/bin) or PM=portuale (/var/tmp/pt-381-s0/bin first on PATH).
# Writes cells/<tag>.{argv,env,rc,stdout,stderr}.
set -u
TAG=$1; RN=$2; shift 2
PM=${PM:-portage}
if [[ $PM == portuale ]]; then export PATH=/var/tmp/pt-381-s0/bin:$PATH; fi
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-19-381/cells
mkdir -p "$EV"
B=/var/tmp/pt-381-s0
export PORTAGE_CONFIGROOT=$B/cfg
export ROOT=$B/roots/$RN
export BROOT=$ROOT
export PORTAGE_TMPDIR=$B/scratch/$RN/tmp
export PORTAGE_LOGDIR=$B/scratch/$RN/logs
export PORTAGE_DEPCACHEDIR=$B/scratch/$RN/depcache
mkdir -p "$ROOT" "$PORTAGE_TMPDIR" "$PORTAGE_LOGDIR" "$PORTAGE_DEPCACHEDIR"
: > "$EV/$TAG.env"
while [[ $1 != -- ]]; do export "$1"; echo "$1" >> "$EV/$TAG.env"; shift; done; shift
for v in PORTAGE_CONFIGROOT ROOT BROOT PORTAGE_TMPDIR PORTAGE_LOGDIR PORTAGE_DEPCACHEDIR PM PATH; do echo "$v=${!v}" >> "$EV/$TAG.env"; done
if [[ $PM == portuale ]]; then
  echo "portuale_HEAD=$(git -C /home/vivo/repo/PORTUALE/portuale rev-parse HEAD)" >> "$EV/$TAG.env"
  echo "portuale_mtime=$(stat -c %y /home/vivo/repo/PORTUALE/portuale/target/release/portuale)" >> "$EV/$TAG.env"
  echo "portuale_which=$(command -v emerge)" >> "$EV/$TAG.env"
else
  echo "portage_version=$(emerge --version 2>/dev/null | head -1)" >> "$EV/$TAG.env"
fi
printf '%s\n' "$*" > "$EV/$TAG.argv"
"$@" >"$EV/$TAG.stdout" 2>"$EV/$TAG.stderr"; RC=$?
echo $RC > "$EV/$TAG.rc"
echo "tag=$TAG rc=$RC"
