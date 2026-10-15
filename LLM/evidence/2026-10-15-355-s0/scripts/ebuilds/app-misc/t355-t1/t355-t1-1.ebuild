# T1 (R6): recorded obj; the admin replaces it with a directory holding a
# file. T1a keeps the new mtime, T1b restores the recorded one.
EAPI=8
DESCRIPTION="t355 obj-replaced-by-dir probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /usr/share/t355
	echo o > "${T}"/o; doins "${T}"/o
}
