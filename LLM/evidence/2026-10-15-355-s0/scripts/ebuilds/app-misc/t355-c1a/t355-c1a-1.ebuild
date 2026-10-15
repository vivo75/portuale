# C1 (safety): A records dir /bin (implicit parent of the installed file)
# and obj /bin/t355 on a merged-usr ROOT where /bin -> usr/bin.
EAPI=8
DESCRIPTION="t355 merged-usr probe A"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /bin
	echo a > "${T}"/t355; doins "${T}"/t355
}
