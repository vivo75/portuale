# P2 control: different-version mid upgrade ships libt355mid.so.2
# (NEEDED libt355base.so.1), so the preserve keys differ.
EAPI=8
DESCRIPTION="t355 preserve-libs probe mid library v2"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}
RESTRICT="strip"

src_compile() {
	echo 'int t355base(void); int t355mid2(void){return t355base();}' > m.c
	echo 'int t355base(void){return 0;}' > b.c
	gcc -shared -fPIC -Wl,-soname,libt355base.so.1 -o libt355base.so b.c || die
	gcc -shared -fPIC -Wl,-soname,libt355mid.so.2 -o libt355mid.so.2 m.c -L. -lt355base || die
}

src_install() {
	exeinto /usr/lib64
	doexe libt355mid.so.2
	dosym libt355mid.so.2 /usr/lib64/libt355mid.so
}
