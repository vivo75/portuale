EAPI=8
DESCRIPTION="fixture package: preserve-libs collision exclusion -- the pre-existing 'owner' of a real preserved lib path"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/lib/preservedtest
	echo "old library content" > "${T}/libfoo.so.1" || die
	doins "${T}/libfoo.so.1"
}
