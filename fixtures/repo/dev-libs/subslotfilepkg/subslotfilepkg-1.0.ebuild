# synthetic merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="fixture package: real SLOT=0/1 sub-slot WITH a file (self-collision regression)"
SLOT="0/1"
KEYWORDS="amd64"

src_install() {
	echo "hello from subslotfilepkg" > "${T}/hello.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/hello.txt"
}
