# #354 S0 cell f: consumer linked against both t354-lib sonames.
EAPI=8
DESCRIPTION="t354 preserve-libs probe consumer"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}
RESTRICT="strip"
# no DEPEND: with ROOT!=/ it would target the host BROOT
RDEPEND="app-misc/t354-lib"

src_compile() {
	echo 'int t354a(void); int t354b(void); int main(void){return t354a()+t354b();}' > m.c
	# link against local stubs with the same sonames: DT_NEEDED is by soname
	echo 'int t354a(void){return 0;}' > a.c; echo 'int t354b(void){return 0;}' > b.c
	gcc -shared -fPIC -Wl,-soname,libt354a.so.1 -o libt354a.so a.c || die
	gcc -shared -fPIC -Wl,-soname,libt354b.so.1 -o libt354b.so b.c || die
	gcc -o t354cons m.c -L. -lt354a -lt354b || die
}

src_install() {
	dobin t354cons
}
