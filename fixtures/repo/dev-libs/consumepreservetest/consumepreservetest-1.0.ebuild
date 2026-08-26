EAPI=8
DESCRIPTION="fixture package: real preserve-libs registration -- a real consumer of libpreservetest.so.1"
SLOT="0"
KEYWORDS="amd64"

src_compile() {
	echo 'int preservetest_value(void) { return 42; }' > "${T}/libpreservetest.c" || die
	gcc -shared -fPIC -Wl,-soname,libpreservetest.so.1 \
		-o "${T}/libpreservetest.so.1" "${T}/libpreservetest.c" || die
	ln -sf libpreservetest.so.1 "${T}/libpreservetest.so" || die
	echo 'extern int preservetest_value(void); int main(void) { return preservetest_value() == 42 ? 0 : 1; }' \
		> "${T}/consumepreservetest.c" || die
	gcc -o "${T}/consumepreservetest" "${T}/consumepreservetest.c" -L"${T}" -lpreservetest || die
}

src_install() {
	exeinto /usr/bin
	doexe "${T}/consumepreservetest"
}
