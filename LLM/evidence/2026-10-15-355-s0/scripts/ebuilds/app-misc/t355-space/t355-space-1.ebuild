# S1/S2/S3 (#357): paths with spaces. v1 ships "a b.txt",
# "c d -> a b.txt" and "sp dir/f". v2 drops "a b.txt" (S2).
# v3 ships only a symlink whose target contains " -> " (S3).
EAPI=8
DESCRIPTION="t355 spaces-in-CONTENTS probe"
HOMEPAGE="none"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
S=${WORKDIR}

src_install() {
	local v=${T355_V:-1}
	einfo "t355-space: image variant ${v}"
	case ${v} in
	1)
		insinto /usr/share/t355
		echo ab > "${T}/a b.txt"; doins "${T}/a b.txt"
		dosym "a b.txt" "/usr/share/t355/c d"
		insinto "/usr/share/t355/sp dir"
		echo f > "${T}"/f; doins "${T}"/f
		;;
	2)
		insinto /usr/share/t355
		dosym keep2 /usr/share/t355/keep2link
		echo keep2 > "${T}"/keep2; doins "${T}"/keep2
		dosym "a b.txt" "/usr/share/t355/c d"
		insinto "/usr/share/t355/sp dir"
		echo f > "${T}"/f; doins "${T}"/f
		;;
	3)
		insinto /usr/share/t355
		echo keep > "${T}"/keep; doins "${T}"/keep
		dosym "x -> y" /usr/share/t355/arrow
		;;
	esac
}
