# B1 (R8): #354 cell B shape. v1 ships keep/dropmod/droptouch; v2 ships
# keep only. Reinstalled (B1a) or unmerged (B1b) with -unmerge-orphans.
EAPI=8
DESCRIPTION="t355 unmerge-orphans probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	local v=${T355_V:-1}
	einfo "t355-drop: image variant ${v}"
	insinto /usr/share/t355
	echo keep > "${T}"/keep; doins "${T}"/keep
	if [[ ${v} == 1 ]]; then
		echo dropmod > "${T}"/dropmod; doins "${T}"/dropmod
		echo droptouch > "${T}"/droptouch; doins "${T}"/droptouch
	fi
}
