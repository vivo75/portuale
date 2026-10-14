# #354 S0 probe: image shape is selected by T354_V (1 or 2) from the caller env,
# so the same version can be rebuilt with a different image.
EAPI=8
DESCRIPTION="t354 reinstall stale-files probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	local v=${T354_V:-1}
	einfo "t354: image variant ${v}"
	insinto /usr/share/t354
	echo same > "${T}"/keep; doins "${T}"/keep
	echo "content v${v}" > "${T}"/chg; doins "${T}"/chg
	echo "content v${v}" > "${T}"/chgmod; doins "${T}"/chgmod
	insinto /usr/share/t354/d
	insinto /etc/t354
	echo "conf same" > "${T}"/same.conf; doins "${T}"/same.conf
	echo "conf v${v}" > "${T}"/both.conf; doins "${T}"/both.conf
	echo "conf v${v}" > "${T}"/bothmod.conf; doins "${T}"/bothmod.conf
	if [[ ${v} == 1 ]]; then
		insinto /usr/share/t354
		echo drop > "${T}"/drop; doins "${T}"/drop
		echo dropmod > "${T}"/dropmod; doins "${T}"/dropmod
		echo droptouch > "${T}"/droptouch; doins "${T}"/droptouch
		echo f2s > "${T}"/f2s; doins "${T}"/f2s
		dosym keep /usr/share/t354/s2f
		dosym keep /usr/share/t354/symdrop
		insinto /usr/share/t354/d
		echo x > "${T}"/x; doins "${T}"/x
		insinto /usr/share/t354/e/f
		echo z > "${T}"/z; doins "${T}"/z
		insinto /usr/share/t354/g
		echo y > "${T}"/y; doins "${T}"/y
		insinto /etc/t354
		echo unmod > "${T}"/unmod.conf; doins "${T}"/unmod.conf
		echo mod > "${T}"/mod.conf; doins "${T}"/mod.conf
		echo touch > "${T}"/touch.conf; doins "${T}"/touch.conf
	else
		insinto /usr/share/t354
		echo new > "${T}"/new; doins "${T}"/new
		dosym keep /usr/share/t354/f2s
		echo s2f > "${T}"/s2f; doins "${T}"/s2f
		insinto /usr/share/t354/d
		echo x2 > "${T}"/x2; doins "${T}"/x2
	fi
}
