# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="porttest: relative / absolute / dangling / chained symlinks"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_install() {
	dodir /usr/share/porttest
	echo real > "${ED}"/usr/share/porttest/pt-target

	# a live relative symlink
	dosym pt-target /usr/share/porttest/pt-rel
	# a live absolute symlink
	dosym /usr/share/porttest/pt-target /usr/share/porttest/pt-abs
	# a deliberately dangling symlink (target never installed)
	dosym ../nonexistent/pt-nowhere /usr/share/porttest/pt-dangle
	# a two-hop chain
	dosym pt-rel /usr/share/porttest/pt-chain
	# a symlink into a bin dir pointing at a system path
	dosym /bin/true /usr/bin/pt-true-link
	# many uniform links (exercise CONTENTS sym-line volume)
	local i
	for i in $(seq 1 20); do
		dosym pt-target "/usr/share/porttest/pt-link-${i}"
	done
}
