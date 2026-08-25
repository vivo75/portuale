EAPI=8
DESCRIPTION="fixture package: real NEEDED.ELF.2 generation (install_qa_check + scanelf) and vdb copy"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	exeinto /usr/bin
	doexe /bin/true
}
