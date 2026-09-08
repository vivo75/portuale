# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
inherit toolchain-funcs
DESCRIPTION="porttest: a setuid-root ELF + a setgid one + odd perms"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_install() {
	printf 'int main(void){return 0;}\n' > "${T}"/pt.c
	$(tc-getCC) ${CFLAGS} ${LDFLAGS} -o "${T}"/pt-setuid "${T}"/pt.c || die
	newbin "${T}"/pt-setuid pt-setuid
	newbin "${T}"/pt-setuid pt-setgid
	newbin "${T}"/pt-setuid pt-sticky
	fperms 4711 /usr/bin/pt-setuid
	fperms 2755 /usr/bin/pt-setgid
	fperms 1750 /usr/bin/pt-sticky
	# a plain data file with a deliberately unusual mode
	dodir /usr/share/porttest
	echo secret > "${ED}"/usr/share/porttest/pt-0600
	fperms 0600 /usr/share/porttest/pt-0600
}
