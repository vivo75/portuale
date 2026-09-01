EAPI=8
DESCRIPTION="fixture package: Scheduler / parallel-build test (schedleaf-b)"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo "leaf b" > "${T}/f" || die
	doins "${T}/f"
}
