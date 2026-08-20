EAPI=8
DESCRIPTION="fixture package: preserve-libs collision exclusion -- an unrelated package that takes over a registered preserved lib path"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/lib/preservedtest
	echo "new library content" > "${T}/libfoo.so.1" || die
	doins "${T}/libfoo.so.1"
}
