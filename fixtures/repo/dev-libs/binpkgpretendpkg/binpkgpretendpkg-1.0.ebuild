EAPI=8
DESCRIPTION="fixture package: pkg_pretend runs on a binary-package merge, before pkg_setup"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "binpkgpretendpkg payload" > "${T}/payload.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/payload.txt"
}

pkg_pretend() {
	# Real Scheduler._run_pkg_pretend runs pkg_pretend for a *binary*
	# package too. It runs before the merge, so the payload must not be
	# visible under ${ROOT} yet.
	if [[ -e ${EROOT}/usr/share/${PN}/payload.txt ]] ; then
		die "pkg_pretend ran after the image was merged"
	fi
	mkdir -p "${EROOT}/var/lib" || die
	echo "pretend" >> "${EROOT}/var/lib/${PN}.log" || die
}

pkg_setup() {
	echo "setup" >> "${EROOT}/var/lib/${PN}.log" || die
}

pkg_preinst() {
	echo "preinst" >> "${EROOT}/var/lib/${PN}.log" || die
}

pkg_postinst() {
	echo "postinst" >> "${EROOT}/var/lib/${PN}.log" || die
}
