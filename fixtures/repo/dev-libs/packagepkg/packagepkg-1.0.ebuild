EAPI=8
DESCRIPTION="fixture package: real binary-package building (ebuild <file> package)"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/samepkg"

src_install() {
	echo "hello from packagepkg" > "${T}/hello.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/hello.txt"
}
