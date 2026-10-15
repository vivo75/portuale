# E1: B installs a file through A's live symlink, so B's CONTENTS records
# dir /usr/share/t355/link and the owners search finds B.
EAPI=8
DESCRIPTION="t355 dir-symlink probe B"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /usr/share/t355/link
	echo b > "${T}"/b.txt; doins "${T}"/b.txt
}
