EAPI=8
DESCRIPTION="fixture package: Scheduler / kill-in-flight-on-hard-failure test (schedslow) -- src_compile sleeps well past any other package in the same run, and only writes its own completion marker afterward, so a test can prove the sleep was actually killed rather than left to finish"
SLOT="0"
KEYWORDS="amd64"

src_compile() {
	sleep 20
	touch "${T}/schedslow-slept-to-completion" || die
}

src_install() {
	insinto /usr/share/${PN}
	echo ok > "${T}/f" || die
	doins "${T}/f"
}
