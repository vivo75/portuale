EAPI=8
DESCRIPTION="fixture package: Scheduler / parallel-build test (schedleaf-a)"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo "leaf a" > "${T}/f" || die
	doins "${T}/f"
}
