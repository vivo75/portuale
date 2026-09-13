#!/bin/bash
# L2 -- build the atom list from source with PORTUALE into a $PKGDIR,
# archive-only (`--buildpkgonly`). The candidate builder for L2.
#
# Usage (from the host orchestrator):
#   podman run ... IMAGE /TEST/layers/l2/build-portuale.sh /TEST/atomlists/<set>
#
# Env:
#   PKGDIR                     (required) binpkg output dir, rw mount
#   DISTDIR                    (required) shared distfile cache, rw mount
#   L2_JOBS=1                  MAKEOPTS -j
#   L2_SKIP_PORTAGE_UPGRADE=0  (base / must match the portage builder's)
#   L2_BUILD_MODE=bpkgonly|deep  (must match build-portage.sh; see there)

set -u
ATOMLIST=${1:?atom list path}
JOBS=${L2_JOBS:-1}
: "${PKGDIR:?PKGDIR must be set (a rw mount)}"
: "${DISTDIR:?DISTDIR must be set (a rw mount)}"

export PORTAGE_CONFIGROOT=/ ROOT=/
export LC_ALL=C.UTF-8 TZ=UTC
export PKGDIR DISTDIR
export MAKEOPTS="-j${JOBS}"
export EMERGE_DEFAULT_OPTS=""
# See build-portage.sh: multi-instance pinned explicitly so both builders
# write the `<cat>/<pn>/<pf>-<BUILD_ID>.gpkg.tar` layout.
export FEATURES="buildpkg binpkg-multi-instance splitdebug xattr filecaps -cgroup -ccache -distcc -sign parallel-fetch"
export BINPKG_FORMAT="gpkg"
umask 022
mkdir -p "$PKGDIR" "$DISTDIR"

log() { printf '[l2-build-portuale] %s\n' "$*"; }

if [ -d /porttest-overlay ] && grep -q '^porttest/' "$ATOMLIST"; then
  rm -rf /var/db/repos/porttest
  cp -a /porttest-overlay /var/db/repos/porttest
  cat > /etc/portage/repos.conf/porttest.conf <<-EOF
	[porttest]
	location = /var/db/repos/porttest
	masters = gentoo
	auto-sync = no
	EOF
  log "porttest overlay staged at /var/db/repos/porttest"
fi

# Same base state as the portage builder (portuale doesn't use the
# installed portage, but the package set / profile resolution must match).
if [ "${L2_SKIP_PORTAGE_UPGRADE:-0}" != 1 ]; then
  PIN=${L2_PORTAGE_PIN:-3.0.82.2}
  cur=$(/usr/sbin/emerge --version 2>/dev/null | sed -n 's/^Portage \([0-9.]*\).*/\1/p')
  if [ "$cur" != "$PIN" ]; then
    log "upgrading portage $cur -> $PIN (~amd64 for that one atom only)"
    ACCEPT_KEYWORDS="~amd64" /usr/sbin/emerge -q --usepkg=n "=sys-apps/portage-$PIN" \
      || { log "!!! portage upgrade failed"; exit 1; }
  fi
fi

atoms=()
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%%#*}; line=$(printf '%s' "$line" | tr -d '[:space:]')
  [ -n "$line" ] && atoms+=("$line")
done < "$ATOMLIST"
log "building ${#atoms[@]} atoms (archive-only, --buildpkgonly)"

BUILD_MODE=${L2_BUILD_MODE:-bpkgonly}
if [ "$BUILD_MODE" = deep ]; then
  log "mode=deep: building the dep closure too (-b --deep)"
  emerge_args=(-b --oneshot --deep --color=n)
else
  emerge_args=(--buildpkgonly --oneshot --color=n)
fi
if ! /usr/local/bin/emerge "${emerge_args[@]}" "${atoms[@]}"; then
  log "!!! build failed"
  exit 1
fi

count=$(find "$PKGDIR" -name '*.gpkg.tar' | wc -l)
log "done: $count binpkgs in $PKGDIR"
[ "$count" -gt 0 ]
