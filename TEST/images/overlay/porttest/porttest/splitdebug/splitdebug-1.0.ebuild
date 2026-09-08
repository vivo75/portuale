# synthetic L1 merge-parity fixture -- see ../../README.md
EAPI=8
inherit toolchain-funcs
DESCRIPTION="porttest: FEATURES=splitdebug .debug files + .build-id links"
HOMEPAGE="https://example.invalid/porttest"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_compile() {
	cat > pt-sd.c <<-'EOF'
		#include <stdio.h>
		int helper(int x){ return x * 3; }
		int main(void){ printf("%d\n", helper(7)); return 0; }
	EOF
	$(tc-getCC) ${CFLAGS} -g -O0 ${LDFLAGS} -o pt-splitdebug pt-sd.c || die
	# a shared lib too (its own .debug + build-id)
	cat > pt-lib.c <<-'EOF'
		int libfn(int x){ return x + 1; }
	EOF
	$(tc-getCC) ${CFLAGS} -g -O0 -fPIC -shared ${LDFLAGS} \
		-Wl,-soname,libptsd.so.0 -o libptsd.so.0.0.0 pt-lib.c || die
}

src_install() {
	dobin pt-splitdebug
	dolib.so libptsd.so.0.0.0
	dosym libptsd.so.0.0.0 /usr/lib64/libptsd.so.0
	dosym libptsd.so.0.0.0 /usr/lib64/libptsd.so
}
