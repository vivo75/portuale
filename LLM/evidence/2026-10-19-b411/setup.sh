#!/bin/bash
# #411 oracle root: #421's root plus a package whose compile sleeps (marker file shows survival).
S=${S7:-/var/tmp/b411}
HERE=$(cd "$(dirname "$0")" && pwd)
S7=$S bash "$HERE/../2026-10-19-b421/setup.sh"
mkdir -p $S/repo/dev-util/slowprobe
cat > $S/repo/dev-util/slowprobe/slowprobe-1.ebuild <<'EOT'
EAPI=8
DESCRIPTION="slow probe"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}
src_compile() { echo $$ > "${T}/../../../../pid.slow"; sleep 300 & echo $! > "${T}/../../../../sleep.pid"; wait; }
EOT
