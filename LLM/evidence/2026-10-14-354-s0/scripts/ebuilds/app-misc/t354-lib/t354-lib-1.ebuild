# #354 S0 cell f: libt354a.so.1 kept in both variants, libt354b.so.1 only in T354_V=1.
EAPI=8
DESCRIPTION="t354 preserve-libs probe library"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}
RESTRICT="strip"

src_compile() {
	echo 'int t354a(void){return 1;}' > a.c
	echo 'int t354b(void){return 2;}' > b.c
	gcc -shared -fPIC -Wl,-soname,libt354a.so.1 -o libt354a.so.1 a.c || die
	gcc -shared -fPIC -Wl,-soname,libt354b.so.1 -o libt354b.so.1 b.c || die
}

src_install() {
	local v=${T354_V:-1}
	einfo "t354-lib: image variant ${v}"
	exeinto /usr/lib64
	doexe libt354a.so.1
	dosym libt354a.so.1 /usr/lib64/libt354a.so
	if [[ ${v} == 1 ]]; then
		doexe libt354b.so.1
		dosym libt354b.so.1 /usr/lib64/libt354b.so
	fi
}
