# E3 (R4): obj plus a sym whose name sorts AFTER its target (mykeys is
# reverse-sorted, vartree.py:2918-2920). E3b (T355_V=2): the file is merged
# through the live symlink, so it is owned only through the parent inode.
EAPI=8
DESCRIPTION="t355 dir-symlink all-owned probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	dodir /usr/share/t355/a-real
	dosym a-real /usr/share/t355/z-link
	if [[ ${T355_V:-1} == 1 ]]; then
		insinto /usr/share/t355/a-real
		echo f > "${T}"/f; doins "${T}"/f
	else
		insinto /usr/share/t355/z-link
		echo f > "${T}"/f; doins "${T}"/f
	fi
}
