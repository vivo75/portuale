#!/bin/bash
# #380: a merge whose image has a path with \n or \r. Run INSIDE the test
# container, as: cell.sh <driver> <variant>   (driver: "ebuild" or
# "/usr/local/bin/portuale ebuild"; variant: file | link | dir | ok)
driver=$1; variant=$2
R=/var/db/repos/nlrepo
rm -rf $R /var/db/pkg/app-misc/nlpkg-* /var/tmp/portage/app-misc /usr/share/nlpkg
mkdir -p $R/profiles $R/metadata $R/app-misc/nlpkg /etc/portage/repos.conf
echo nlrepo > $R/profiles/repo_name
echo "masters = gentoo" > $R/metadata/layout.conf
printf '[nlrepo]\nlocation = %s\n' $R > /etc/portage/repos.conf/nlrepo.conf
E=$R/app-misc/nlpkg/nlpkg-1.ebuild
cat > $E <<'EBUILD'
EAPI=8
DESCRIPTION="newline path probe"
SLOT="0"
KEYWORDS="amd64"
IUSE="test"
S="${WORKDIR}"
src_unpack() { :; }
src_install() {
	mkdir -p "${ED}/usr/share/nlpkg"
	: > "${ED}/usr/share/nlpkg/plain"
EBUILD
case $variant in
 file) cat >> $E <<'EBUILD'
	: > "${ED}/usr/share/nlpkg/a"$'\n'"b"
	: > "${ED}/usr/share/nlpkg/c"$'\r'"d"
}
EBUILD
 ;;
 link) cat >> $E <<'EBUILD'
	ln -s $'x\ny' "${ED}/usr/share/nlpkg/lnk"
}
EBUILD
 ;;
 dir) cat >> $E <<'EBUILD'
	mkdir -p "${ED}/usr/share/nlpkg/d"$'\n'"e"
}
EBUILD
 ;;
 ok) echo '}' >> $E ;;
esac
ebuild $E manifest >/dev/null 2>&1
$driver $E merge 2>&1 | grep -v "cgroup\|Unable to unshare"
echo ">>> rc=${PIPESTATUS[0]}"
ls -d /var/db/pkg/app-misc/nlpkg-* 2>&1
find /usr/share/nlpkg 2>/dev/null | cat -v
cat /var/db/pkg/app-misc/nlpkg-1/CONTENTS 2>/dev/null | cat -v
