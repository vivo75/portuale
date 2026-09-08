# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="porttest: files an INSTALL_MASK / a doc-strip should drop at merge"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_install() {
	dodir /usr/share/porttest/im
	echo keep > "${ED}"/usr/share/porttest/im/keep.txt
	echo drop > "${ED}"/usr/share/porttest/im/drop.txt
	echo drop > "${ED}"/usr/share/porttest/im/also-drop.log

	# a .la file (fixlafiles / INSTALL_MASK="*.la" territory)
	dodir /usr/lib64
	cat > "${ED}"/usr/lib64/libporttest.la <<-EOF
		# fake libtool archive
		dlname='libporttest.so.0'
		library_names='libporttest.so.0 libporttest.so'
	EOF

	# a locale file (NO INSTALL_MASK -- control) and a charset one
	dodir /usr/share/locale/xx/LC_MESSAGES
	echo mo > "${ED}"/usr/share/locale/xx/LC_MESSAGES/porttest.mo

	# an empty dir that only existed to hold a dropped file
	dodir /usr/share/porttest/im/onlydropped
	echo drop > "${ED}"/usr/share/porttest/im/onlydropped/x.log
}
