# T3 (R6/R7): recorded dir; the admin replaces it with a regular file.
# (a) default FEATURES: orphan arm unlinks it. (b) -unmerge-orphans: !dir.
EAPI=8
DESCRIPTION="t355 dir-replaced-by-file probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	keepdir /usr/share/t355/dd
}
