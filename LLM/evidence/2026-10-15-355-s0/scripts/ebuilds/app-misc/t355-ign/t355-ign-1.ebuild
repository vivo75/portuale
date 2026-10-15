# U1/U1d (R1): a file under the UNINSTALL_IGNORE probe path plus a plain
# file under /lib/modules (the make.globals default UNINSTALL_IGNORE entry).
EAPI=8
DESCRIPTION="t355 UNINSTALL_IGNORE probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	insinto /usr/share/t355/ign
	echo x > "${T}"/x; doins "${T}"/x
	insinto /lib/modules/t355
	echo mod > "${T}"/m.ko; doins "${T}"/m.ko
}
