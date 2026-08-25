EAPI=8
DESCRIPTION="fixture package: real merge-time blocker collision exclusion -- the pre-existing 'owner' a blocker atom will exclude"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/mergeblockertest
	echo "hello from mergeblockedbypkg" > "${T}/shared.txt" || die
	doins "${T}/shared.txt"
}
