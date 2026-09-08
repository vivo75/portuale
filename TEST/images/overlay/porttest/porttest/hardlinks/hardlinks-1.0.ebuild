# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="porttest: hardlinked regular files (CONTENTS obj dedup + link count)"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_install() {
	dodir /usr/share/porttest
	local d="${ED}"/usr/share/porttest
	echo "shared payload" > "${d}"/pt-hl-a
	ln "${d}"/pt-hl-a "${d}"/pt-hl-b
	ln "${d}"/pt-hl-a "${d}"/pt-hl-c
	# a hardlink pair in a different directory too
	dodir /usr/bin
	echo '#!/bin/sh' > "${ED}"/usr/bin/pt-hl-tool
	ln "${ED}"/usr/bin/pt-hl-tool "${ED}"/usr/bin/pt-hl-tool2
	fperms 0755 /usr/bin/pt-hl-tool /usr/bin/pt-hl-tool2
}
