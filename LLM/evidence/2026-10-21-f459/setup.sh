#!/bin/bash
# #459 oracle root: binary merge whose pkg_postinst dies. Ebuilds mirror
# the #444 source shape (f459- prefix per worktree rule); binpkgs are
# built with real Portage `emerge --buildpkgonly` into $S/pkgdir, then
# the cells merge them with `--usepkgonly`.
S=${S459:-/var/tmp/f459/oracle}
rm -rf "$S"
mkdir -p "$S"/etc/portage/repos.conf "$S"/repo/{profiles/base,metadata,dev-util/f459-postinstfail,dev-util/f459-good,dev-util/f459-top} \
         "$S"/tmp "$S"/dist "$S"/root "$S"/pkgdir
echo fetchrepo > "$S/repo/profiles/repo_name"
printf 'masters =\nthin-manifests = true\n' > "$S/repo/metadata/layout.conf"
echo 8 > "$S/repo/profiles/base/eapi"
printf 'ARCH="amd64"\nACCEPT_KEYWORDS="amd64"\nIUSE_IMPLICIT="prefix"\n' > "$S/repo/profiles/base/make.defaults"
echo "default/linux/amd64 amd64 stable" > "$S/repo/profiles/profiles.desc"
ln -s "$S/repo/profiles/base" "$S/etc/portage/make.profile"
printf '[DEFAULT]\nmain-repo = fetchrepo\n[fetchrepo]\nlocation = %s/repo\n' "$S" > "$S/etc/portage/repos.conf/r.conf"
printf 'ACCEPT_KEYWORDS="amd64"\nFEATURES="-news -sandbox -usersandbox -network-sandbox -pid-sandbox -userpriv"\nGENTOO_MIRRORS=""\nPKGDIR="%s/pkgdir"\n' "$S" > "$S/etc/portage/make.conf"
mk() { mkdir -p "$S/repo/dev-util/$1"; cat > "$S/repo/dev-util/$1/$1-1.ebuild"; }
mk f459-postinstfail <<'E'
EAPI=8
DESCRIPTION="binary postinst dies"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_install() { keepdir /opt/f459-postinstfail; }
pkg_postinst() { die "postinst exploded"; }
E
mk f459-good <<'E'
EAPI=8
DESCRIPTION="independent"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_install() { keepdir /opt/f459-good; }
E
mk f459-top <<'E'
EAPI=8
DESCRIPTION="needs f459-postinstfail"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-util/f459-postinstfail"
S="${WORKDIR}"
src_install() { keepdir /opt/f459-top; }
E
# No Manifests: `thin-manifests = true` with no SRC_URI gives `ebuild
# manifest` nothing to record, and real Portage resolves this root without one.
