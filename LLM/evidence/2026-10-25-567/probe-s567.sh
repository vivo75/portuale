#!/bin/bash
# S0 probe for backlog #567 (Encode/Sub-Name version-selection).
#
# Oracle question: the bed log l0-20261025T084209Z shows gnome-shell /
# libreoffice / gimp with 2 [missing] rows each (perl-core/Encode-3.210.0,
# dev-perl/Sub-Name-0.280.0-r1: real merges them, portuale does not), while
# the old-tree log l0-20261023T164600Z has both managers at Total 453 with
# neither package. Is there a named tree-side cause, or is it a resolver gap?
#
# Method: throwaway container from the bed image (exact vdb: perl-5.42.2 +
# 5.42-era virtuals installed; exact tree: snapshot 350c0882 with
# perl-5.44.0 visible). Minimal atom shapes isolate each selection; the old
# snapshot 11c58b7a is NOT reachable in the gentoo repo history (verified
# 2026-10-25: `git cat-file -t 11c58b7a` fails, host clone is shallow), so
# the old pool is grounded in the old bed log alone (see REPORT.md).
#
# Usage: ./probe-s567.sh  (writes captures/*.out next to this script)
# Prereq: podman, image localhost/test-portuale:latest,
#         host binary ../../.. (target/release/portuale) for the portuale side.

set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CAP="$HERE/captures"
mkdir -p "$CAP"
IMAGE=${S567_IMAGE:-localhost/test-portuale:latest}
PTL_BIN=${S567_PTL_BIN:-/home/vivo/repo/PORTUALE/portuale/target/release/portuale}
NAME="s567-probe-$$"

[ -x "$PTL_BIN" ] || { echo "portuale binary not found: $PTL_BIN" >&2; exit 2; }

cleanup() { podman rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

podman run -d --name "$NAME" --entrypoint /bin/bash \
  --security-opt seccomp=unconfined --cgroups=enabled --cgroupns=private \
  -v "$PTL_BIN:/usr/local/bin/emerge:ro" "$IMAGE" \
  -c 'while true; do sleep 3600; done' >/dev/null
sleep 2

# Bed-faithful env (see differential-test-bed/layers/l0/in-container.sh):
# PORTAGE_CONFIGROOT=/ ROOT=/ PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC.
R_ENV="-e PORTAGE_CONFIGROOT=/ -e ROOT=/ -e PYTHONHASHSEED=0 -e LC_ALL=C.UTF-8 -e TZ=UTC"
P_ENV="-e PORTAGE_CONFIGROOT=/ -e ROOT=/ -e LC_ALL=C.UTF-8 -e TZ=UTC"

run() { # $1 = capture file, $2 = pm (real|ptl), $3+ = argv
  local cap=$1 pm=$2; shift 2
  local bin=/usr/sbin/emerge env=$R_ENV
  [ "$pm" = ptl ] && { bin=/usr/local/bin/emerge; env=$P_ENV; }
  local q; printf -v q '%q ' "$@"
  # shellcheck disable=SC2086
  podman exec $env "$NAME" bash -c "$bin $q 2>&1; echo \"S567_RC=\$?\"" >"$CAP/$cap"
  echo "wrote $cap"
}

run p01-portage-Encode-Locale-alone.out real -pv dev-perl/Encode-Locale
run p02-portage-upgrade-trio-tree.out real -pv --tree =dev-lang/perl-5.44.0 dev-perl/Encode-Locale dev-perl/Try-Tiny
run p03-portage-Try-Tiny-upgrade.out real -pv dev-perl/Try-Tiny =dev-lang/perl-5.44.0
run p04-portage-SLU-virtual-upgrade.out real -pv ">=virtual/perl-Scalar-List-Utils-1.400.0" =dev-lang/perl-5.44.0
run p05-portage-Encode-virtual-upgrade.out real -pv ">=virtual/perl-Encode-2.0.0" =dev-lang/perl-5.44.0
run p06-portage-direct-atoms.out real -pv perl-core/Encode dev-perl/Sub-Name
run p07-portuale-upgrade-trio-tree.out ptl -pv --tree =dev-lang/perl-5.44.0 dev-perl/Encode-Locale dev-perl/Try-Tiny
run p08-portuale-SLU-virtual-upgrade.out ptl -pv ">=virtual/perl-Scalar-List-Utils-1.400.0" =dev-lang/perl-5.44.0
run p09-portuale-Encode-virtual-upgrade.out ptl -pv ">=virtual/perl-Encode-2.0.0" =dev-lang/perl-5.44.0
run p10-portuale-Try-Tiny-upgrade.out ptl -pv dev-perl/Try-Tiny =dev-lang/perl-5.44.0
run p11-portuale-Encode-Locale-alone.out ptl -pv dev-perl/Encode-Locale
run p12-portuale-direct-atoms.out ptl -pv perl-core/Encode dev-perl/Sub-Name

{
  echo "== image =="; podman image inspect "$IMAGE" --format '{{.Id}} {{.RepoTags}}'
  echo "== container tree (new snapshot shape) =="
  podman exec "$NAME" bash -c 'ls /var/db/repos/gentoo/dev-lang/perl/ /var/db/repos/gentoo/perl-core/Encode/ /var/db/repos/gentoo/dev-perl/Sub-Name/ /var/db/repos/gentoo/virtual/perl-Encode/ /var/db/repos/gentoo/virtual/perl-Scalar-List-Utils/ /var/db/repos/gentoo/perl-core/Scalar-List-Utils/ 2>&1'
  echo "== container vdb (relevant installed set) =="
  podman exec "$NAME" bash -c 'ls -d /var/db/pkg/dev-lang/perl-* /var/db/pkg/virtual/perl-Encode-* /var/db/pkg/virtual/perl-Scalar-List-Utils-* 2>&1'
  echo "== portage =="; podman exec $R_ENV "$NAME" /usr/sbin/emerge --version 2>&1 | head -1
  echo "== portuale =="; md5sum "$PTL_BIN"; "$PTL_BIN" --help 2>&1 | head -1
  echo "== old snapshot reachability =="; git -C /var/db/repos/gentoo cat-file -t 11c58b7a 2>&1 || true
} >"$CAP/p13-provenance.out" 2>&1
echo "wrote p13-provenance.out"
