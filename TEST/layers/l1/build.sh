#!/bin/bash
# L1 -- build the merge-parity package set from source with PORTAGE ONLY
# (portage is the reference builder; see docs/real-world-testing.md
# §1.2), into a shared $PKGDIR the two consumers then merge from.
#
# Usage (from the host orchestrator):
#   podman run ... IMAGE /TEST/layers/l1/build.sh /TEST/atomlists/l1-merge.txt
#
# Env:
#   PKGDIR                    (required) the binpkg output dir, a mount
#   L1_PORTAGE_PIN=3.0.82.2   the portage version portuale mirrors
#   L1_JOBS=1                  MAKEOPTS -j / --jobs  (1 = deterministic)
#   L1_SKIP_PORTAGE_UPGRADE=0
#
# Idempotent: a package already in $PKGDIR's Packages index is not
# rebuilt (plain `emerge -k` behaviour), so a re-run is cheap.

set -u
ATOMLIST=${1:?atom list path}
PIN=${L1_PORTAGE_PIN:-3.0.82.2}
JOBS=${L1_JOBS:-1}
: "${PKGDIR:?PKGDIR must be set (a rw mount)}"

export PORTAGE_CONFIGROOT=/ ROOT=/
export LC_ALL=C.UTF-8 TZ=UTC
# the L1 set is deliberately all-stable (see l1-merge.txt) -- no
# ACCEPT_KEYWORDS override, so `--deep` can't cascade into a ~amd64
# system upgrade.
export PKGDIR
export MAKEOPTS="-j${JOBS}"
export EMERGE_DEFAULT_OPTS=""
# -cgroup: the cgroup fs is ro under rootless podman (harmless warning
# spam otherwise). -sign: no signing key. buildpkg: the whole point.
export FEATURES="buildpkg -cgroup -ccache -distcc -sign parallel-fetch"
export BINPKG_FORMAT="gpkg"
umask 022
mkdir -p "$PKGDIR"

log() { printf '[l1-build] %s\n' "$*"; }

if [ "${L1_SKIP_PORTAGE_UPGRADE:-0}" != 1 ]; then
  cur=$(/usr/sbin/emerge --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')
  if [ "$cur" != "$PIN" ]; then
    log "upgrading portage $cur -> $PIN (~amd64 for that one atom only)"
    ACCEPT_KEYWORDS="~amd64" /usr/sbin/emerge -q --usepkg=n "=sys-apps/portage-$PIN" \
      || { log "!!! portage upgrade failed"; exit 1; }
  fi
fi
/usr/sbin/emerge --version | head -1

atoms=()
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%%#*}; line=$(printf '%s' "$line" | tr -d '[:space:]')
  [ -n "$line" ] && atoms+=("$line")
done < "$ATOMLIST"
log "building ${#atoms[@]} atoms (+ deps) with FEATURES=buildpkg, MAKEOPTS=$MAKEOPTS"

# --buildpkg is already in FEATURES; --usepkg=n forces a real from-source
# build (don't pull a stale binpkg), --oneshot keeps @world untouched,
# --deep so every dependency of the set is also built into $PKGDIR.
if ! /usr/sbin/emerge --oneshot --deep --usepkg=n --color=n --quiet-build=y "${atoms[@]}"; then
  log "!!! build failed"
  exit 1
fi

# Portage writes the index as it goes; refresh it so a partial prior run
# is reconciled.
/usr/sbin/emaint --fix binhost 2>/dev/null || /usr/sbin/emerge --regen --quiet 2>/dev/null || true

count=$(find "$PKGDIR" -name '*.gpkg.tar' -o -name '*.tbz2' -o -name '*.xpak' 2>/dev/null | wc -l)
log "done: $count binpkgs in $PKGDIR"
[ "$count" -gt 0 ]
