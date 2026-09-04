EAPI=8
DESCRIPTION="fixture package: pkg_preinst/pkg_postinst print observable markers, to prove their own output reaches a captured build.log under -jN/--quiet-build (docs/scope-backlog.md's B.1)"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "hello from hookoutputpkg" > "${T}/hello.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/hello.txt"
}

pkg_preinst() {
	echo "HOOKOUTPUTPKG-PREINST-MARKER"
}

pkg_postinst() {
	echo "HOOKOUTPUTPKG-POSTINST-MARKER"
}
