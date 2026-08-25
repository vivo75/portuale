EAPI=8
DESCRIPTION="fixture package: real merge-time blocker collision exclusion -- RDEPEND blocks dev-libs/mergeblockedbypkg, whose own file this package also installs"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="!dev-libs/mergeblockedbypkg"

src_install() {
	insinto /usr/share/mergeblockertest
	echo "hello from mergeblockerpkg" > "${T}/shared.txt" || die
	doins "${T}/shared.txt"
}
