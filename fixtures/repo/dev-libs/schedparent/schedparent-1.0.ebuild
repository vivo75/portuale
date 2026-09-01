EAPI=8
DESCRIPTION="fixture package: Scheduler / parallel-build test (schedparent)"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/schedleaf-a dev-libs/schedleaf-b"

src_install() {
	insinto /usr/share/${PN}
	echo "parent" > "${T}/f" || die
	doins "${T}/f"
}
