EAPI=8
DESCRIPTION="fixture package: Scheduler / parallel-build test (schedok)"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo ok > "${T}/f" || die
	doins "${T}/f"
}
