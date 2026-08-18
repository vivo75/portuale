EAPI=8
DESCRIPTION="fixture package: real SRC_URI grammar (arrow-rename + USE-conditional group), verified-in-place so no live network is needed"
SRC_URI="
	https://example.invalid/payload.bin -> verifiedfetchpkg-1.0.tar.gz
	test? ( https://example.invalid/tests.bin -> verifiedfetchpkg-tests-1.0.tar.gz )
"
SLOT="0"
KEYWORDS="amd64"
IUSE="test"

src_install() {
	echo "A=${A}" > "${T}/fetch-vars.txt" || die
	echo "AA=${AA}" >> "${T}/fetch-vars.txt" || die
}
