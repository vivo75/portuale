#!/bin/bash
# #458 oracle root: the long-clean-multi shape (dev-libs/schedparent plus its
# two leaves), copied verbatim from the pmtest fixture tree -- the same root
# as #456's `LLM/evidence/2026-10-21-b456/`, plus a PKGDIR for the
# --buildpkgonly binpkg output. A clean multi-package `--buildpkgonly` build
# with no SRC_URI, so the terminal shows the build phases themselves and
# nothing else (no fetch, no Manifest lines).
S=${S7:-/var/tmp/f458}
HERE=$(cd "$(dirname "$0")" && pwd)
FIX=$HERE/../../../fixtures/repo/dev-libs
rm -rf "$S"
mkdir -p "$S"/etc/portage/repos.conf "$S"/repo/{profiles/base,metadata,dev-libs/schedparent,dev-libs/schedleaf-a,dev-libs/schedleaf-b} \
         "$S"/tmp "$S"/dist "$S"/root "$S"/pkgdir
echo qbuild > "$S/repo/profiles/repo_name"
printf 'masters =\nthin-manifests = true\n' > "$S/repo/metadata/layout.conf"
echo 8 > "$S/repo/profiles/base/eapi"
printf 'ARCH="amd64"\nACCEPT_KEYWORDS="amd64"\nIUSE_IMPLICIT="prefix"\n' > "$S/repo/profiles/base/make.defaults"
echo "default/linux/amd64 amd64 stable" > "$S/repo/profiles/profiles.desc"
ln -s "$S/repo/profiles/base" "$S/etc/portage/make.profile"
printf '[DEFAULT]\nmain-repo = qbuild\n[qbuild]\nlocation = %s/repo\n' "$S" > "$S/etc/portage/repos.conf/r.conf"
printf 'ACCEPT_KEYWORDS="amd64"\nFEATURES="-news -sandbox -usersandbox -network-sandbox -pid-sandbox -ipc-sandbox -userpriv"\nGENTOO_MIRRORS=""\nPKGDIR="%s/pkgdir"\n' "$S" > "$S/etc/portage/make.conf"
for p in schedparent schedleaf-a schedleaf-b; do
  cp "$FIX/$p/$p-1.0.ebuild" "$S/repo/dev-libs/$p/" || exit 1
done
# No Manifests: `thin-manifests = true` with no SRC_URI gives `ebuild
# manifest` nothing to record (it writes no file), and real Portage
# resolves and builds this root without one.
