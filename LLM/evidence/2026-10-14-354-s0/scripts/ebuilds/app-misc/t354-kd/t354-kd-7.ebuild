# #354 S0.9: keepdir + empty dodir under EAPI 7.
EAPI=7
DESCRIPTION="t354 keepdir probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	keepdir /var/lib/t354kd
	keepdir /usr/share/t354kd/kept
	dodir /usr/share/t354kd/emptydodir
	dodir /var/t354kd-emptyvar
	insinto /usr/share/t354kd
	echo f > "${T}"/f; doins "${T}"/f
}
