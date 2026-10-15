# P1/P1c: mid library. v1 ships libt355mid.so.1 (NEEDED libt355base.so.1,
# linked against a stub of the same soname built in WORKDIR); v2 drops it.
EAPI=8
DESCRIPTION="t355 preserve-libs probe mid library"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}
RESTRICT="strip"

src_compile() {
	echo 'int t355base(void); int t355mid(void){return t355base();}' > m.c
	echo 'int t355base(void){return 0;}' > b.c
	gcc -shared -fPIC -Wl,-soname,libt355base.so.1 -o libt355base.so b.c || die
	gcc -shared -fPIC -Wl,-soname,libt355mid.so.1 -o libt355mid.so.1 m.c -L. -lt355base || die
}

src_install() {
	local v=${T355_V:-1}
	einfo "t355-mid: image variant ${v}"
	if [[ ${v} == 1 ]]; then
		exeinto /usr/lib64
		doexe libt355mid.so.1
		dosym libt355mid.so.1 /usr/lib64/libt355mid.so
	else
		insinto /usr/share/t355
		echo mid-v2 > "${T}"/mid-v2; doins "${T}"/mid-v2
	fi
}
