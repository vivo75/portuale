# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
DESCRIPTION="porttest: filenames with spaces, UTF-8, shell metachars (CONTENTS quoting)"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_install() {
	local d="${ED}"/usr/share/porttest/u
	mkdir -p "${d}"
	echo a > "${d}/with space.txt"
	echo b > "${d}/with	tab.txt"
	echo c > "${d}/dollar\$sign.txt"
	echo d > "${d}/café-utf8-ünïcödé.txt"
	echo e > "${d}/paren(and)bracket[x].txt"
	echo f > "${d}/hash#and%percent.txt"
	# a symlink whose name AND target both have spaces
	ln -s "with space.txt" "${d}/link to space.txt"
	# a subdir with a space, holding a file
	mkdir -p "${d}/sub dir"
	echo g > "${d}/sub dir/inner file.txt"
}
