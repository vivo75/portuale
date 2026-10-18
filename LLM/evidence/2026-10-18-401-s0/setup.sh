#!/bin/bash
# Builds the throwaway config root the #401 cells run on (default /var/tmp/s7-oracle):
# one package whose distfile a FETCHCOMMAND copies from a local file. Needs Portage's
# `ebuild` only to write the Manifest (already stored here as ./Manifest).
S=${S7:-/var/tmp/s7-oracle}
rm -rf "$S"
mkdir -p "$S"/etc/portage/repos.conf "$S"/repo/{profiles/base,metadata,dev-util/fetchprobe} \
         "$S"/tmp "$S"/dist "$S"/root "$S"/src
HERE=$(cd "$(dirname "$0")" && pwd)
echo fetchrepo > "$S/repo/profiles/repo_name"
printf 'masters =\nthin-manifests = true\n' > "$S/repo/metadata/layout.conf"
echo 8 > "$S/repo/profiles/base/eapi"
printf 'ARCH="amd64"\nACCEPT_KEYWORDS="amd64"\nIUSE_IMPLICIT="prefix"\n' > "$S/repo/profiles/base/make.defaults"
echo "default/linux/amd64 amd64 stable" > "$S/repo/profiles/profiles.desc"
ln -s "$S/repo/profiles/base" "$S/etc/portage/make.profile"
printf '[DEFAULT]\nmain-repo = fetchrepo\n[fetchrepo]\nlocation = %s/repo\n' "$S" > "$S/etc/portage/repos.conf/r.conf"
printf 'ACCEPT_KEYWORDS="amd64"\nFEATURES="-news -sandbox -usersandbox -network-sandbox -pid-sandbox -ipc-sandbox -userpriv"\nGENTOO_MIRRORS=""\n' > "$S/etc/portage/make.conf"
echo "probe payload" > "$S/src/probe-1.tar.gz"
cp "$HERE/fetchprobe-1.ebuild" "$HERE/Manifest" "$S/repo/dev-util/fetchprobe/"
