# M2 (R5): unprotected file; same md5-only drift, unmerged with
# FEATURES=-unmerge-orphans. Portage keeps it (!md5).
EAPI=8
DESCRIPTION="t355 unprotected-md5 probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /usr/share/t355
	echo plain > "${T}"/plain; doins "${T}"/plain
}
