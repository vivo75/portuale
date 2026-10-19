#!/bin/bash
# #415 oracle root: #401's root plus a RESTRICT=fetch package and a USE-conditional SRC_URI one.
S=${S7:-/var/tmp/b415}
HERE=$(cd "$(dirname "$0")" && pwd)
S7=$S bash "$HERE/../2026-10-18-401-s0/setup.sh"
mkdir -p $S/repo/dev-util/fetchrestrict $S/repo/dev-util/fetchcond
cat > $S/repo/dev-util/fetchrestrict/fetchrestrict-1.ebuild <<'EOF'
EAPI=8
DESCRIPTION="restrict fetch"
SRC_URI="https://example.invalid/probe-1.tar.gz"
RESTRICT="fetch"
SLOT="0"
KEYWORDS="amd64"
EOF
cat > $S/repo/dev-util/fetchcond/fetchcond-1.ebuild <<'EOF'
EAPI=8
DESCRIPTION="cond src"
SRC_URI="https://example.invalid/probe-1.tar.gz flag? ( https://example.invalid/extra-1.tar.gz )"
IUSE="flag"
SLOT="0"
KEYWORDS="amd64"
EOF
cp "$HERE/Manifest" $S/repo/dev-util/fetchrestrict/
printf 'DIST probe-1.tar.gz 14 BLAKE2B ba6f1169d8188dc6b3c462e9533399b0d794e72dc71aa82e93f3991c5849d25426e2635df655e0a52b0c1935544383af2bd13e7a12fddef012bd5bbdf9695c86 SHA512 695f4a1cdbacda06507284adf4ba71649fbd0d1b85ae767ff5d5f9fad6d53cc17ea0a2fd27f7bba0c1111d6910bec1576c9bfecafa5c5a371ecbab3a4c4b5f7b\nDIST extra-1.tar.gz 3 BLAKE2B ba6f1169d8188dc6b3c462e9533399b0d794e72dc71aa82e93f3991c5849d25426e2635df655e0a52b0c1935544383af2bd13e7a12fddef012bd5bbdf9695c86 SHA512 695f4a1cdbacda06507284adf4ba71649fbd0d1b85ae767ff5d5f9fad6d53cc17ea0a2fd27f7bba0c1111d6910bec1576c9bfecafa5c5a371ecbab3a4c4b5f7b\n' > $S/repo/dev-util/fetchcond/Manifest
