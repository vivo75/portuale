EAPI=8
DESCRIPTION="fixture package: real merge/filesystem mutation (task #55) -- a real file plus a real symlink"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "hello from mergepkg" > "${T}/hello.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/hello.txt"
	dosym hello.txt /usr/share/${PN}/hello-link.txt
}

pkg_preinst() {
	# Real ordering proof: pkg_preinst runs before anything is merged
	# into ${ROOT}, so the file src_install created must not be visible
	# there yet.
	if [[ ! -e "${ROOT}"/usr/share/${PN}/hello.txt ]]; then
		touch "${T}"/preinst-ran-before-merge || die
	fi
}

pkg_postinst() {
	# Real ordering proof: pkg_postinst runs only after the merge (and
	# this pilot's own vdb write) has completed.
	if [[ -e "${ROOT}"/usr/share/${PN}/hello.txt && -e "${ROOT}"/var/db/pkg/${CATEGORY}/${PF}/CONTENTS ]]; then
		touch "${T}"/postinst-ran-after-merge || die
	fi
}
