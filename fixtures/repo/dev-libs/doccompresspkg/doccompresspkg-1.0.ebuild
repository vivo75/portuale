EAPI=8
DESCRIPTION="fixture package: docompress (binpkg-docompress + PORTAGE_COMPRESS) compresses \${D} docs"
SLOT="0"
KEYWORDS="amd64"
IUSE=""

src_install() {
	mkdir -p "${S}/docs" || die
	printf 'doccompresspkg payload\n%s\n' "$(seq 1 40)" > "${S}/docs/BIG.txt" || die
	echo short > "${S}/docs/small.txt" || die
	dodoc -r docs/. || die
	# A dangling-after-compression link real `bin/ecompress`'s
	# `fix_symlinks` step must repair (and suffix) once BIG.txt.bz2
	# replaces BIG.txt.
	dosym BIG.txt "/usr/share/doc/${PF}/link-to-big.txt" || die
}
