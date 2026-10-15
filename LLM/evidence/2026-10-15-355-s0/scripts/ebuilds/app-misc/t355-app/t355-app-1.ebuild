# P1: NEEDs only libt355mid.so.1. P1c (T355_V=2): NEEDs libt355mid.so.1
# and libt355base.so.1, so base.so.1 has a second consumer.
EAPI=8
DESCRIPTION="t355 preserve-libs probe app"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}
RESTRICT="strip"

src_compile() {
	local v=${T355_V:-1}
	einfo "t355-app: image variant ${v}"
	echo 'int t355mid(void){return 0;}' > m.c
	echo 'int t355base(void){return 0;}' > b.c
	gcc -shared -fPIC -Wl,-soname,libt355mid.so.1 -o libt355mid.so m.c || die
	gcc -shared -fPIC -Wl,-soname,libt355base.so.1 -o libt355base.so b.c || die
	if [[ ${v} == 1 ]]; then
		echo 'int t355mid(void); int main(void){return t355mid();}' > a.c
		gcc -o t355app a.c -L. -lt355mid || die
	else
		echo 'int t355mid(void); int t355base(void); int main(void){return t355mid()+t355base();}' > a.c
		gcc -o t355app a.c -L. -lt355mid -lt355base || die
	fi
}

src_install() {
	exeinto /usr/bin
	doexe t355app
}
