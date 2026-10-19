#!/bin/bash
# #421 oracle root: #415's root plus an interactive package that depends on fetchprobe.
S=${S7:-/var/tmp/b421}
HERE=$(cd "$(dirname "$0")" && pwd)
S7=$S bash "$HERE/../2026-10-19-b415/setup.sh"
mkdir -p $S/repo/dev-util/intprobe
cat > $S/repo/dev-util/intprobe/intprobe-1.ebuild <<'EOT'
EAPI=8
DESCRIPTION="interactive probe"
SLOT="0"
KEYWORDS="amd64"
PROPERTIES="interactive"
RDEPEND="dev-util/fetchprobe"
EOT
