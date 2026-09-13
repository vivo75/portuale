# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
inherit toolchain-funcs
DESCRIPTION="porttest: RESTRICT=strip keeps the binary unstripped (no .debug)"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
RESTRICT="strip"
S="${WORKDIR}"

src_install() {
	printf 'int main(void){return 0;}\n' > "${T}"/pt-rs.c
	$(tc-getCC) ${CFLAGS} -g ${LDFLAGS} -o "${T}"/pt-restrict "${T}"/pt-rs.c || die
	newbin "${T}"/pt-restrict pt-restrict
}
