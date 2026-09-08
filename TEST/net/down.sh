#!/bin/bash
# Tear down the shared podman infrastructure.
#
#   TEST/net/down.sh [--volumes]
#
# By default only the network is removed; pass --volumes to also drop the
# persistent volumes (loses the binpkg / distfile / snapshot cache).

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/../run/lib.sh"

"$PODMAN" network exists "$NET" && {
  echo ">>> removing network $NET"
  "$PODMAN" network rm -f "$NET"
}

if [ "${1:-}" = --volumes ]; then
  for v in porttest-pkgdir porttest-distfiles porttest-bincache porttest-snapshots; do
    "$PODMAN" volume exists "$v" && {
      echo ">>> removing volume $v"
      "$PODMAN" volume rm -f "$v"
    }
  done
fi

echo ">>> down"
