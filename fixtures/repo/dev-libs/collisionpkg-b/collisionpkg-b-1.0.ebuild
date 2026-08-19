EAPI=8
DESCRIPTION="fixture package: real FEATURES=collision-protect -- collides with collisionpkg-a on both an ordinary file and a symlink-over-directory"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/collisiontest
	echo "hello from collisionpkg-b" > "${T}/shared.txt" || die
	doins "${T}/shared.txt"
	dosym nowhere /usr/share/collisiontest/adir
}
