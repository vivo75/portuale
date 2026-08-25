EAPI=8
DESCRIPTION="fixture package: real FEATURES=collision-protect -- the first, already-installed half"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/collisiontest
	echo "hello from collisionpkg-a" > "${T}/shared.txt" || die
	doins "${T}/shared.txt"
	keepdir /usr/share/collisiontest/adir
}
