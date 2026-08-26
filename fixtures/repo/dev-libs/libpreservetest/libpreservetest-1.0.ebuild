EAPI=8
DESCRIPTION="fixture package: real preserve-libs registration -- the library half"
SLOT="0"
KEYWORDS="amd64"

src_compile() {
	echo 'int preservetest_value(void) { return 42; }' > "${T}/libpreservetest.c" || die
	gcc -shared -fPIC -Wl,-soname,libpreservetest.so.1 \
		-o "${T}/libpreservetest.so.1" "${T}/libpreservetest.c" || die
}

src_install() {
	insinto /usr/lib
	doins "${T}/libpreservetest.so.1"
}
