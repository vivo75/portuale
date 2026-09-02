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
}
