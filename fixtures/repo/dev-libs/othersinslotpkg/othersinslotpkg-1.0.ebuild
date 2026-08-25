EAPI=8
DESCRIPTION="fixture package: real unmerge others_in_slot check -- shares a file with othersinslotpkg-2.0, same SLOT"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo "shared, from ${PVR}" > "${T}/shared.txt" || die
	doins "${T}/shared.txt"
	echo "only in 1.0" > "${T}/only-in-v1.txt" || die
	doins "${T}/only-in-v1.txt"
}
