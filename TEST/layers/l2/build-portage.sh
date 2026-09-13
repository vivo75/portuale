#!/bin/bash
# L2 -- build the atom list from source with PORTAGE into a $PKGDIR,
# archive-only (`--buildpkgonly`). The reference builder for L2.
#
# Usage (from the host orchestrator):
#   podman run ... IMAGE /TEST/layers/l2/build-portage.sh /TEST/atomlists/<set>
#
# Env:
#   PKGDIR                     (required) binpkg output dir, rw mount
#   DISTDIR                    (required) shared distfile cache, rw mount
#   L2_PORTAGE_PIN=3.0.82.2    portage version portuale mirrors
#   L2_JOBS=1                  MAKEOPTS -j / --jobs
#   L2_SKIP_PORTAGE_UPGRADE=0
#   L2_BUILD_MODE=bpkgonly|deep
#     bpkgonly -- `--buildpkgonly` (archive-only), for sets whose deps are
#                 already installed (the porttest fixtures);
#     deep     -- `-b --deep --usepkg=n`, which also builds+merges any
#                 missing dependency into the throwaway builder first.
#                 Needed for the real L1 set: real `-B` refuses when a
#                 dep is not merged ("--buildpkgonly requires all
#                 dependencies to be merged"). G0.1 revisited at S5.
set -u
ATOMLIST=${1:?atom list path}
PIN=${L2_PORTAGE_PIN:-3.0.82.2}
JOBS=${L2_JOBS:-1}
: "${PKGDIR:?PKGDIR must be set (a rw mount)}"
: "${DISTDIR:?DISTDIR must be set (a rw mount)}"

export PORTAGE_CONFIGROOT=/ ROOT=/
export LC_ALL=C.UTF-8 TZ=UTC
export PKGDIR DISTDIR
export MAKEOPTS="-j${JOBS}"
export EMERGE_DEFAULT_OPTS=""
# binpkg-multi-instance is explicit on both builders: real portage gets it
# from make.globals anyway, but portuale's default is deliberately off
# (PackageOptions::binpkg_multi_instance), so pinning it in the process
# env keeps the two archive layouts identical.
export FEATURES="buildpkg binpkg-multi-instance splitdebug xattr filecaps -cgroup -ccache -distcc -sign parallel-fetch"
export BINPKG_FORMAT="gpkg"
umask 022
mkdir -p "$PKGDIR" "$DISTDIR"

log() { printf '[l2-build-portage] %s\n' "$*"; }

# porttest overlay (same live-mount convention as layers/l1/build.sh)
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

if [ "${L2_SKIP_PORTAGE_UPGRADE:-0}" != 1 ]; then
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
log "building ${#atoms[@]} atoms (archive-only, --buildpkgonly) with MAKEOPTS=$MAKEOPTS"

BUILD_MODE=${L2_BUILD_MODE:-bpkgonly}
if [ "$BUILD_MODE" = deep ]; then
  log "mode=deep: building the dep closure too (--buildpkg --deep --usepkg=n)"
  emerge_args=(--buildpkg --oneshot --deep --usepkg=n --color=n --quiet-build=y)
else
  emerge_args=(--buildpkgonly --oneshot --color=n --quiet-build=y)
fi
if ! /usr/sbin/emerge "${emerge_args[@]}" "${atoms[@]}"; then
  log "!!! build failed"
  exit 1
fi

count=$(find "$PKGDIR" -name '*.gpkg.tar' | wc -l)
log "done: $count binpkgs in $PKGDIR"
[ "$count" -gt 0 ]
