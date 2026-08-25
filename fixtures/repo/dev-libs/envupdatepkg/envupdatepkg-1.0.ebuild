EAPI=8
DESCRIPTION="fixture package: real env_update()/ldconfig triggering -- installs its own env.d entry and lib dir"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /etc/env.d
	echo 'LDPATH="/usr/lib/envupdatetest"' > "${T}/50-envupdatetest" || die
	echo 'ENVUPDATETEST_VAR="hello from envupdatetest"' >> "${T}/50-envupdatetest" || die
	doins "${T}/50-envupdatetest"

	keepdir /usr/lib/envupdatetest
}
