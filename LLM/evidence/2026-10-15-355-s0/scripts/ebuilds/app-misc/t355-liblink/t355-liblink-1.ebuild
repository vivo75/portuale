# L1 (R2): live symlinks /lib and /usr/lib. Portage never removes them.
EAPI=8
DESCRIPTION="t355 lib-symlink probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	dosym lib64 /lib
	dosym lib64 /usr/lib
}
