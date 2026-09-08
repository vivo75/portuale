# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="porttest: dodoc tree -> docompress, newdoc, doman, doinfo"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_install() {
	# a doc tree -- docompress will .bz2 (or .gz/.zst) most of these
	mkdir -p "${S}"/docs/sub
	printf 'porttest docs payload\n%s\n' "$(seq 1 200)" > "${S}"/docs/BIG.txt
	echo short > "${S}"/docs/small.txt
	echo nested > "${S}"/docs/sub/NESTED.md
	dodoc -r docs/.
	newdoc docs/small.txt RENAMED.txt

	# a man page (compressed) and an info file (NOT compressed by default)
	printf '.TH PT 1\n.SH NAME\npt \\- porttest\n' > "${S}"/pt.1
	doman "${S}"/pt.1
	printf 'This is pt.info, node Top.\n' > "${S}"/pt.info
	doinfo "${S}"/pt.info

	# an explicitly-not-compressed doc
	docinto html
	echo '<html></html>' > "${S}"/index.html
	dodoc "${S}"/index.html
}
