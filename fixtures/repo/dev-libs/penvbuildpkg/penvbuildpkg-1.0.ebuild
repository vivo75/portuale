EAPI=8
DESCRIPTION="fixture package: package.env's non-USE half overrides the build-phase flags"
SLOT="0"
KEYWORDS="amd64"
IUSE=""

src_install() {
	# Records the build flags the phase env carried -- with a matching
	# /etc/portage/package.env entry these come from its env file, not
	# make.conf / the env layer.
	printf 'CFLAGS=%s\nMAKEOPTS=%s\n' "${CFLAGS}" "${MAKEOPTS}" > "${T}/flags" || die
	insinto /usr/share/${PN}
	doins "${T}/flags"
}
