#!/bin/bash
# S0b, in-container: reproduce one L0 cell's environment
# (pmtest differential-test-bed/layers/l0/in-container.sh: PORTAGE_CONFIGROOT=/ ROOT=/,
# LC_ALL=C.UTF-8 TZ=UTC PYTHONHASHSEED=0 umask 022, seeded @world, `emerge -pv --color=n <atom>`)
# and run Portage with --debug through which-trees-or.py, plus plain Portage and portuale.
# Usage (inside the bed container): bash /S0B/bed-cell.sh <atom> <outdir>
set -u
atom=$1 out=$2; mkdir -p "$out"
export PORTAGE_CONFIGROOT=/ ROOT=/ LC_ALL=C.UTF-8 TZ=UTC PYTHONHASHSEED=0
umask 022
printf '%s\n' app-misc/tmux app-portage/eix app-portage/gentoolkit dev-libs/blake3 > /var/lib/portage/world
s=$(printf '%s' "$atom" | tr -c 'A-Za-z0-9._-' '_')
emerge -pv --color=n "$atom" > "$out/$s.portage.txt" 2>&1; echo "rc $?" >> "$out/$s.portage.txt"
python3 -I /S0B/which-trees-or.py -pv --color=n --debug "$atom" > "$out/$s.portage-debug.raw" 2>&1; echo "rc $?" >> "$out/$s.portage-debug.raw"
PORTUALE_BT_TRACE=1 portuale emerge -pv --color=n "$atom" > "$out/$s.portuale.txt" 2>&1; echo "rc $?" >> "$out/$s.portuale.txt"
