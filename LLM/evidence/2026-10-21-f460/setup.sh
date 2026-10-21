#!/bin/bash
# #444 oracle root: bad (src_compile dies), good (independent), top (needs
# bad) mirrored from the #423 shape, plus postinstfail whose pkg_postinst
# dies after an otherwise successful merge, and posttop (needs postinstfail).
S=${S7:-/var/tmp/f460/probe}
rm -rf "$S"
mkdir -p "$S"/etc/portage/repos.conf "$S"/repo/{profiles/base,metadata,dev-util/bad,dev-util/good,dev-util/top,dev-util/postinstfail,dev-util/posttop} \
         "$S"/tmp "$S"/dist "$S"/root
echo fetchrepo > "$S/repo/profiles/repo_name"
printf 'masters =\nthin-manifests = true\n' > "$S/repo/metadata/layout.conf"
echo 8 > "$S/repo/profiles/base/eapi"
printf 'ARCH="amd64"\nACCEPT_KEYWORDS="amd64"\nIUSE_IMPLICIT="prefix"\n' > "$S/repo/profiles/base/make.defaults"
echo "default/linux/amd64 amd64 stable" > "$S/repo/profiles/profiles.desc"
ln -s "$S/repo/profiles/base" "$S/etc/portage/make.profile"
printf '[DEFAULT]\nmain-repo = fetchrepo\n[fetchrepo]\nlocation = %s/repo\n' "$S" > "$S/etc/portage/repos.conf/r.conf"
printf 'ACCEPT_KEYWORDS="amd64"\nFEATURES="-news -sandbox -usersandbox -network-sandbox -pid-sandbox -userpriv"\nGENTOO_MIRRORS=""\n' > "$S/etc/portage/make.conf"
mk() { mkdir -p "$S/repo/dev-util/$1"; cat > "$S/repo/dev-util/$1/$1-1.ebuild"; }
mk bad <<'E'
EAPI=8
DESCRIPTION="fails to compile"
SLOT="0"
KEYWORDS="amd64"
src_unpack() { mkdir -p "${S}"; }
src_compile() { die "compile exploded"; }
E
mk good <<'E'
EAPI=8
DESCRIPTION="independent"
SLOT="0"
KEYWORDS="amd64"
src_unpack() { mkdir -p "${S}"; }
src_install() { keepdir /opt/good; }
E
mk top <<'E'
EAPI=8
DESCRIPTION="needs bad"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-util/bad"
src_unpack() { mkdir -p "${S}"; }
src_install() { keepdir /opt/top; }
E
mk postinstfail <<'E'
EAPI=8
DESCRIPTION="postinst dies"
SLOT="0"
KEYWORDS="amd64"
src_unpack() { mkdir -p "${S}"; }
src_install() { keepdir /opt/postinstfail; }
pkg_postinst() { die "postinst exploded"; }
E
mk posttop <<'E'
EAPI=8
DESCRIPTION="needs postinstfail"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-util/postinstfail"
src_unpack() { mkdir -p "${S}"; }
src_install() { keepdir /opt/posttop; }
E
# No Manifests: `thin-manifests = true` with no SRC_URI gives `ebuild
# manifest` nothing to record, and real Portage resolves this root without one.
