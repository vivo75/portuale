EAPI=8
DESCRIPTION="fixture package: the resolved USE reaches the build phase (bin/ebuild.sh use())"
SLOT="0"
KEYWORDS="amd64"
IUSE="buildflag"

src_install() {
	insinto /usr/share/${PN}
	if use buildflag; then
		echo on > "${T}/state" || die
	else
		echo off > "${T}/state" || die
	fi
	doins "${T}/state"
	# Records the build flags the phase env carried -- proves make.conf /
	# env-layer CFLAGS/MAKEOPTS reach the build (empty before that slice).
	printf 'CFLAGS=%s\nMAKEOPTS=%s\n' "${CFLAGS}" "${MAKEOPTS}" > "${T}/flags" || die
	doins "${T}/flags"
}
