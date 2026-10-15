# M1 (R5): protected /etc file; the admin edits the content and restores
# the mtime, so only the md5 differs. Portage keeps it (--- !md5).
EAPI=8
DESCRIPTION="t355 protected-md5 probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /etc/t355
	echo mod > "${T}"/mod.conf; doins "${T}"/mod.conf
}
