# P1/P1c/P2: base library. -1 ships only .so.1, -2 ships only .so.2.
EAPI=8
DESCRIPTION="t355 preserve-libs probe base library"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}
RESTRICT="strip"

src_compile() {
	echo 'int t355base(void){return 1;}' > a.c
	echo 'int t355base2(void){return 2;}' > b.c
	gcc -shared -fPIC -Wl,-soname,libt355base.so.1 -o libt355base.so.1 a.c || die
	gcc -shared -fPIC -Wl,-soname,libt355base.so.2 -o libt355base.so.2 b.c || die
}

src_install() {
	exeinto /usr/lib64
	if [[ ${PV} == 1 ]]; then
		doexe libt355base.so.1
		dosym libt355base.so.1 /usr/lib64/libt355base.so
	else
		doexe libt355base.so.2
		dosym libt355base.so.2 /usr/lib64/libt355base.so
	fi
}
