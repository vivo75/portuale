EAPI=8
DESCRIPTION="fixture package: real merge/filesystem mutation (task #55) -- a real file plus a real symlink"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "hello from mergepkg" > "${T}/hello.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/hello.txt"
	dosym hello.txt /usr/share/${PN}/hello-link.txt
}
