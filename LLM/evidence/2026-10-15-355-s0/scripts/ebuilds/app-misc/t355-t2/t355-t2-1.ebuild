# T2 (R6): recorded sym; the admin replaces it with a regular file and
# restores the recorded mtime. Unmerged with FEATURES=-unmerge-orphans.
EAPI=8
DESCRIPTION="t355 sym-replaced-by-file probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /usr/share/t355
	echo keep > "${T}"/keep; doins "${T}"/keep
	dosym keep /usr/share/t355/s
}
