#!/bin/bash
# L0 -- resolver parity, in-container half.
#
# Runs `emerge -pv` for every entry in an atom list with BOTH the real
# portage (`/usr/sbin/emerge`) and portuale (`/usr/local/bin/emerge`),
# saving each raw output + exit code. The host-side
# `TEST/compare/resolve-compare.py` does the diffing.
#
# Usage (from the host orchestrator; see TEST/run/l0-resolver.sh):
#   podman run ... --entrypoint /bin/bash IMAGE \
#     /TEST/layers/l0/in-container.sh /TEST/atomlists/l0-resolve.txt /TEST/logs/l0
#
# Env:
#   L0_SKIP_PORTAGE_UPGRADE=1   skip the `=sys-apps/portage-<PIN>` step
#   L0_PORTAGE_PIN=3.0.82.2     the portage version portuale mirrors
#   L0_EMERGE_OPTS="-pv"        emerge flags for every probe

set -u
ATOMLIST=${1:?atom list path}
OUTDIR=${2:?output dir}
PIN=${L0_PORTAGE_PIN:-3.0.82.2}
EOPTS=${L0_EMERGE_OPTS:--pv}

REAL=/usr/sbin/emerge
PTL=/usr/local/bin/emerge

export PORTAGE_CONFIGROOT=/ ROOT=/
export LC_ALL=C.UTF-8 TZ=UTC
# Real portage's `_serialize_tasks` (and `_display_autounmask`) iterate
# plain `set`s in a few spots, so its merge order / autounmask flag order
# is PYTHONHASHSEED-randomised for a handful of order-independent
# adjacent pairs (e.g. `llvm-core/llvmgold` <-> `llvm-core/llvm-toolchain-
# symlinks`). Pin the seed so the real reference is reproducible and the
# comparison is against one stable target -- portuale's own order is
# deterministic and matches real at `PYTHONHASHSEED=0`.
export PYTHONHASHSEED=0
umask 022

mkdir -p "$OUTDIR"/real "$OUTDIR"/portuale
: > "$OUTDIR/meta.tsv"          # slug \t kind \t real_rc \t ptl_rc
: > "$OUTDIR/run.log"

log() { printf '%s\n' "$*" | tee -a "$OUTDIR/run.log" ; }

# -- environment fingerprint (host compare aborts if this drifts) --------
{
  echo "date_utc	$(date -u +%FT%TZ)"
  echo "portuale_bin	$($PTL --help 2>&1 | head -1 | tr -d '\r' || true)"
  for repo in gentoo buildovl porttest ; do
    d=/var/db/repos/$repo
    [ -d "$d/.git" ] || { echo "repo_${repo}	(absent)"; continue; }
    ref=$(sed -n 's/^ref: //p' "$d/.git/HEAD" 2>/dev/null)
    if [ -n "$ref" ] && [ -f "$d/.git/$ref" ]; then
      echo "repo_${repo}	$(cat "$d/.git/$ref")"
    elif [ -f "$d/.git/HEAD" ]; then
      echo "repo_${repo}	$(cat "$d/.git/HEAD")"
    fi
  done
  echo "profile	$(readlink -f /etc/portage/make.profile | sed 's#.*/profiles/##')"
  echo "python	$(python3 --version 2>&1)"
} > "$OUTDIR/fingerprint.tsv"
log "fingerprint:"; sed 's/^/  /' "$OUTDIR/fingerprint.tsv" | tee -a "$OUTDIR/run.log"

# -- portage version pin ------------------------------------------------
if [ "${L0_SKIP_PORTAGE_UPGRADE:-0}" != 1 ]; then
  cur=$("$REAL" --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')
  if [ "$cur" = "$PIN" ]; then
    log "portage already $PIN"
  else
    log "upgrading portage $cur -> $PIN (ACCEPT_KEYWORDS=~amd64)"
    ACCEPT_KEYWORDS=~amd64 "$REAL" -q "=sys-apps/portage-$PIN" >> "$OUTDIR/run.log" 2>&1 \
      && log "portage now $("$REAL" --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')" \
      || log "!!! portage upgrade FAILED -- comparison is against $cur, not $PIN"
  fi
fi
"$REAL" --version 2>/dev/null | head -1 >> "$OUTDIR/fingerprint.tsv"

# -- seed a realistic @world -------------------------------------------
# The image ships an empty /var/lib/portage/world, so `@world` collapses
# to `@system` and the `@world` probes test nothing `@system` doesn't
# (L0 finding H). Seed a handful of common, dependency-rich leaf
# packages -- none installed, so `emerge -pv @world` proposes each plus
# its full closure, exercising far more of the resolver. Both PMs read
# the same file, so it stays a fair comparison.
WORLD=/var/lib/portage/world
mkdir -p "$(dirname "$WORLD")"
cat > "$WORLD" <<-'EOF'
	app-misc/tmux
	app-portage/eix
	app-portage/gentoolkit
	dev-libs/blake3
	EOF
log "seeded @world:"; sed 's/^/  /' "$WORLD" | tee -a "$OUTDIR/run.log"
cp "$WORLD" "$OUTDIR/world.txt"

slug() { printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_' ; }

probe() {
  local atom=$1 kind=$2 s rrc prc
  s=$(slug "$atom")
  # shellcheck disable=SC2086
  timeout 600 "$REAL" $EOPTS --color=n "$atom" > "$OUTDIR/real/$s.txt" 2>&1 ; rrc=$?
  # shellcheck disable=SC2086
  timeout 600 "$PTL"  $EOPTS --color=n "$atom" > "$OUTDIR/portuale/$s.txt" 2>&1 ; prc=$?
  printf '%s\t%s\t%s\t%s\n' "$s" "$kind" "$rrc" "$prc" >> "$OUTDIR/meta.tsv"
  log "  [$kind] $atom  (real rc=$rrc  ptl rc=$prc)"
}

log "running probes from $ATOMLIST"
n=0
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%%#*}; line=$(printf '%s' "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
  [ -z "$line" ] && continue
  case $line in
    @*) probe "$line" set ;;
    *)  probe "$line" atom ;;
  esac
  n=$((n+1))
done < "$ATOMLIST"

# -- a few whole-graph runs on top of the per-atom probes --------------
if [ "${L0_SKIP_MULTI:-0}" = 1 ]; then
  log "done: $n atoms (L0_SKIP_MULTI=1, no multi-graph runs) -> $OUTDIR"
  exit 0
fi
probe_multi() {  # label + emerge args
  local label=$1; shift
  local s="MULTI_$(slug "$label")" rrc prc
  timeout 1200 "$REAL" "$@" --color=n > "$OUTDIR/real/$s.txt" 2>&1 ; rrc=$?
  timeout 1200 "$PTL"  "$@" --color=n > "$OUTDIR/portuale/$s.txt" 2>&1 ; prc=$?
  printf '%s\t%s\t%s\t%s\n' "$s" "multi" "$rrc" "$prc" >> "$OUTDIR/meta.tsv"
  log "  [multi] $label  (real rc=$rrc  ptl rc=$prc)"
}
probe_multi "emptytree-system"  -pe   @system
probe_multi "deep-update-world" -puvD @world
probe_multi "depclean"          -pc

log "done: $n atoms + 3 multi-graph runs -> $OUTDIR"
