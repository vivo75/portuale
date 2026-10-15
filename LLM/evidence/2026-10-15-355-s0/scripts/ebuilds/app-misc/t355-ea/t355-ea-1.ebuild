# E1/E2 (R4+R4b): A owns a symlink to a directory.
EAPI=8
DESCRIPTION="t355 dir-symlink probe A"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	dodir /usr/share/t355/real
	dosym real /usr/share/t355/link
}
