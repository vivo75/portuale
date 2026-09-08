# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="porttest: keepdir / .keep naming / a genuinely empty owned dir"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_install() {
	# keepdir -> creates a .keep_<CATEGORY>_<PN>-<SLOT> file
	keepdir /var/lib/porttest/kept
	keepdir /var/lib/porttest/kept/nested
	# a dir with real content (control)
	dodir /var/lib/porttest/full
	echo x > "${ED}"/var/lib/porttest/full/f
	# a genuinely empty owned dir, no keepdir (portage QA: kept, but no
	# .keep -- the merge must still create it and record it in CONTENTS)
	dodir /var/lib/porttest/bare
	# an odd-permission dir
	dodir /var/lib/porttest/mode0700
	fperms 0700 /var/lib/porttest/mode0700
}
