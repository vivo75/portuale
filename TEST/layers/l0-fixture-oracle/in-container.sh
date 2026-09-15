#!/bin/bash
# Real `emerge` as a fixture oracle (backlog #49) -- in-container half.
#
# Stages the fixture tree (`stage.sh`), then runs every case in the atom
# list with BOTH real `emerge` and portuale under the same staged
# environment, saving outputs + exit codes in the layout
# `TEST/compare/resolve-compare.py` already consumes:
#
#   <outdir>/real/<slug>.txt       real `emerge -p <args>`
#   <outdir>/portuale/<slug>.txt   portuale `emerge -p <args>`
#   <outdir>/meta.tsv              slug \t kind \t real_rc \t ptl_rc
#
# Usage (from TEST/run/l0-fixture-oracle.sh):
#   in-container.sh <atomlist> <outdir>
#
# Atom list format: one case per line, shell words = emerge args
# (`--update dev-libs/paired`); `#` comments and blank lines skipped.

set -u

ATOMLIST=${1:?atom list path}
OUTDIR=${2:?output dir}
REAL=/usr/sbin/emerge
PTL=/usr/local/bin/emerge
STAGE=/tmp/l0-fixture-oracle

export LC_ALL=C.UTF-8 TZ=UTC
umask 022

mkdir -p "$OUTDIR"/real "$OUTDIR"/portuale
: > "$OUTDIR/meta.tsv"
: > "$OUTDIR/run.log"

log() { printf '%s\n' "$*" | tee -a "$OUTDIR/run.log" ; }

# Portage pin: portuale mirrors PIN (TEST/run/l0-resolver.sh's own step),
# and real's --pretend output differs between patch releases, so a stale
# image must be upgraded before the cases run (the image's own config is
# still in place here; PORTAGE_REPOSITORIES comes after).
PIN=${L0_PORTAGE_PIN:-3.0.82.2}
cur=$("$REAL" --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')
if [ "$cur" != "$PIN" ]; then
  log "upgrading portage $cur -> $PIN"
  ACCEPT_KEYWORDS=~amd64 "$REAL" -q "=sys-apps/portage-$PIN" >> "$OUTDIR/run.log" 2>&1 \
    && log "portage now $("$REAL" --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')" \
    || log "!!! portage upgrade FAILED -- comparison is against $cur, not $PIN"
fi

log "staging the fixture tree"
"$(dirname "${BASH_SOURCE[0]}")/stage.sh" "$STAGE" >> "$OUTDIR/run.log" 2>&1 \
  || { log "!!! staging failed"; exit 2; }
FX="$STAGE/fixtures"

# The running root's installed set is what real's "The following installed
# packages are masked" check reads; replace the image's gentoo set with the
# fixture vdb so both roots agree (portuale's `fixture_env` pins
# PORTAGE_RUNNING_ROOT to fixtures for the same reason). The portage
# upgrade above already installed 3.0.82.2's code on disk; only its vdb
# entry disappears, which --pretend does not need.
rm -rf /var/db/pkg
cp -r "$FX/var/db/pkg" /var/db/pkg
if [ -f "$FX/var/lib/portage/world" ]; then
  mkdir -p /var/lib/portage
  cp "$FX/var/lib/portage/world" /var/lib/portage/world
fi

export PORTAGE_CONFIGROOT="$FX" ROOT="$FX" DISTDIR="$FX/distfiles"
# Hide the image's own repository configuration: with PORTAGE_REPOSITORIES
# set, real parses exactly this INI text and ignores repos.conf on disk
# (portage/repository/config.py::load_repository_config).
export PORTAGE_REPOSITORIES="$(cat "$FX/etc/portage/repos.conf/repos.conf")"
# Real's `_serialize_tasks`/`_display_autounmask` iterate plain `set`s, so
# its order is PYTHONHASHSEED-randomised for order-independent pairs; the
# L0 bed pins it and so does this one.
export PYTHONHASHSEED=0

slug() { printf '%s' "$*" | tr -c 'A-Za-z0-9._-' '_' ; }

{
  echo "date_utc	$(date -u +%FT%TZ)"
  echo "portuale_bin	$($PTL --help 2>&1 | head -1 | tr -d '\r' || true)"
  echo "real_portage	$($REAL --version 2>/dev/null | head -1)"
  echo "portage_pin	$PIN"
  echo "staged_at	$FX"
} > "$OUTDIR/fingerprint.tsv"

log "running fixture-oracle cases"
n=0
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%%#*}; line=$(printf '%s' "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
  [ -z "$line" ] && continue
  read -r -a args <<< "$line"
  s=$(slug "$line")
  timeout 600 "$REAL" -p --color=n "${args[@]}" > "$OUTDIR/real/$s.txt" 2>&1 ; rrc=$?
  timeout 600 "$PTL"  -p --color=n "${args[@]}" > "$OUTDIR/portuale/$s.txt" 2>&1 ; prc=$?
  printf '%s\t%s\t%s\t%s\n' "$s" "case" "$rrc" "$prc" >> "$OUTDIR/meta.tsv"
  log "  $line  (real rc=$rrc  ptl rc=$prc)"
  n=$((n+1))
done < "$ATOMLIST"

log "done: $n case(s) -> $OUTDIR"
