# D1/D2 (R3): keepdir records dir /opt/t355/d; the admin then replaces it
# with a symlink to an empty dir (D1) or a dir holding an untracked file (D2).
EAPI=8
DESCRIPTION="t355 dir-replaced-by-symlink probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	keepdir /opt/t355/d
}
