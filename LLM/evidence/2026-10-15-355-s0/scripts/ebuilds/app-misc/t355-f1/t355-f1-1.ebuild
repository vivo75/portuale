# F1 (R7): a fifo. Default FEATURES unlinks it; -unmerge-orphans keeps it.
EAPI=8
DESCRIPTION="t355 fifo probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	dodir /usr/share/t355
	mkfifo "${ED}"/usr/share/t355/p || die
}
