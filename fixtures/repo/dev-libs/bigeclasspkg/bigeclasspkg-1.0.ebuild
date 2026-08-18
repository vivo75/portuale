EAPI=8
DESCRIPTION="fixture package: real inherit()'s own __save_ebuild_env pipe must not deadlock on a large scope"
SLOT="0"
KEYWORDS="amd64"

inherit bigfixture

src_install() {
	bigfixture_marker > "${T}/bigfixture-marker.txt" || die
}
