EAPI=8
DESCRIPTION="fixture package: real SRC_URI grammar (arrow-rename + USE-conditional group), verified-in-place so no live network is needed"
SRC_URI="
	https://example.invalid/payload.bin -> verifiedfetchpkg-1.0.tar.gz
	test? ( https://example.invalid/tests.bin -> verifiedfetchpkg-tests-1.0.tar.gz )
"
SLOT="0"
KEYWORDS="amd64"
IUSE="test"

# The distfiles are digest-verified stand-ins, not real archives -- this
# fixture exercises the SRC_URI grammar and the verified-skip-fetch path,
# not unpacking. Skip the EAPI-8 default src_unpack (which would `tar` the
# stand-in and fail).
src_unpack() { :; }

src_install() {
	echo "A=${A}" > "${T}/fetch-vars.txt" || die
	echo "AA=${AA}" >> "${T}/fetch-vars.txt" || die
}
