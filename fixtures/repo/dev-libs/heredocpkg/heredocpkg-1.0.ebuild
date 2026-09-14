# portuale fixture: a quoted here-document in `src_compile` -- the
# `<<-'EOF'` form the real `porttest/splitdebug` overlay uses (and the
# fourth `declare -f` bug's own repro shape). The ebuild inherits
# nothing and uses `${CC:-gcc}`, so the result depends only on the
# shell backend, not on any eclass. See docs/brush-pin.md (B1/B4).
EAPI=8
DESCRIPTION="portuale fixture: quoted here-document src_compile"
HOMEPAGE="https://example.invalid/portuale"
SLOT="0"
KEYWORDS="amd64"
LICENSE="GPL-2"
S="${WORKDIR}"

src_compile() {
	cat > pt-heredoc.c <<-'EOF'
		#include <stdio.h>
		int helper(int x){ return x * 3; }
		int main(void){ printf("%d\n", helper(7)); return 0; }
	EOF
	${CC:-gcc} ${CFLAGS} -g -O0 ${LDFLAGS} -o pt-heredoc pt-heredoc.c || die
	cat > pt-heredoc-lib.c <<-'EOF'
		int libfn(int x){ return x + 1; }
	EOF
	${CC:-gcc} ${CFLAGS} -g -O0 -fPIC -shared ${LDFLAGS} \
		-Wl,-soname,libptheredoc.so.0 -o libptheredoc.so.0.0.0 pt-heredoc-lib.c || die
}

src_install() {
	exeinto /usr/bin
	doexe pt-heredoc
	dolib.so libptheredoc.so.0.0.0
}
