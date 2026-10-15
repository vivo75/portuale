# C1 (safety): B also records dir /bin, so the R4b owners search keeps it.
EAPI=8
DESCRIPTION="t355 merged-usr probe B"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /bin
	echo b > "${T}"/oth; doins "${T}"/oth
}
