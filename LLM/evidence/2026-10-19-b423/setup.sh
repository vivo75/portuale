#!/bin/bash
# #423 oracle root: the #415 root plus bad (src_compile dies), good (independent), top (needs bad).
S=${S7:-/var/tmp/b423}
HERE=$(cd "$(dirname "$0")" && pwd)
S7=$S bash "$HERE/../2026-10-19-b415/setup.sh"
mk() { mkdir -p $S/repo/dev-util/$1; cat > $S/repo/dev-util/$1/$1-1.ebuild; }
mk bad <<'E'
EAPI=8
DESCRIPTION="fails to compile"
SLOT="0"
KEYWORDS="amd64"
src_unpack() { mkdir -p "${S}"; }
src_compile() { die "compile exploded"; }
E
mk good <<'E'
EAPI=8
DESCRIPTION="independent"
SLOT="0"
KEYWORDS="amd64"
src_unpack() { mkdir -p "${S}"; }
src_install() { keepdir /opt/good; }
E
mk top <<'E'
EAPI=8
DESCRIPTION="needs bad"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-util/bad"
src_unpack() { mkdir -p "${S}"; }
src_install() { keepdir /opt/top; }
E
