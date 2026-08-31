EAPI=8
DESCRIPTION="fixture package: real pkg_preinst/pkg_postinst run for a binary-package merge"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "binpkgphasepkg payload" > "${T}/payload.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/payload.txt"
}

pkg_preinst() {
	# Ordering proof: the payload must NOT be visible under ${ROOT} yet
	# when pkg_preinst runs (real dblink.treewalk() runs it before a
	# single file is copied).
	if [[ -e ${EROOT}/usr/share/${PN}/payload.txt ]] ; then
		die "pkg_preinst ran after the image was merged"
	fi
	mkdir -p "${EROOT}/var/lib" || die
	echo "preinst" > "${EROOT}/var/lib/${PN}.phases" || die
}

pkg_postinst() {
	# Ordering proof: by pkg_postinst the payload MUST be merged.
	[[ -e ${EROOT}/usr/share/${PN}/payload.txt ]] || die "pkg_postinst ran before the image was merged"
	echo "postinst" >> "${EROOT}/var/lib/${PN}.phases" || die
}
