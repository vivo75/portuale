#!/bin/bash
# setup.sh: scratch overlay + hermetic configroot for #385.
# Everything lives under /var/tmp/s385; nothing in pmtest/fixtures is touched.
# Overlay holds throwaway ebuilds only (no md5-cache needed for `ebuild <f> merge`;
# layout.conf declares thin-manifests so no Manifest is required either).
set -eu
B=/var/tmp/s385
EV=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-19-385
rm -rf "$B"
R=$B/repo; CFG=$B/cfg
mkdir -p "$R/metadata" "$R/profiles/default" "$R/eclass"
mkdir -p "$CFG/etc/portage" "$EV/scripts" "$EV/cells"
echo t385 > "$R/profiles/repo_name"
printf 'masters =\nthin-manifests = true\ncache-formats = md5-dict\nprofile-formats = portage-2\n' > "$R/metadata/layout.conf"
echo amd64 > "$R/profiles/arch.list"
echo 5 > "$R/profiles/default/eapi"
cat > "$R/profiles/default/make.defaults" <<'MD'
ARCH="amd64"
ACCEPT_KEYWORDS="amd64"
ACCEPT_LICENSE="*"
CHOST="x86_64-pc-linux-gnu"
CONFIG_PROTECT="/etc"
CONFIG_PROTECT_MASK="/etc/env.d"
ELIBC="glibc"
KERNEL="linux"
USERLAND="GNU"
IUSE_IMPLICIT="prefix prefix-guest prefix-stack"
USE_EXPAND_IMPLICIT="ARCH ELIBC KERNEL"
USE_EXPAND_UNPREFIXED="ARCH"
USE_EXPAND_VALUES_ARCH="amd64"
USE_EXPAND_VALUES_ELIBC="glibc"
USE_EXPAND_VALUES_KERNEL="linux"
MD

emit() { # $1=shape $2=eapi $3=body-file-content via stdin
    local shape=$1 eapi=$2
    local d=$R/app-misc/t385-$shape
    mkdir -p "$d"
    {
        echo "EAPI=$eapi"
        echo 'DESCRIPTION="t385 minstrel empty-dir probe (throwaway)"'
        echo 'HOMEPAGE="https://example.invalid"'
        echo 'LICENSE="MIT"'
        echo 'SLOT="0"'
        echo 'KEYWORDS="amd64"'
        echo 'S=${WORKDIR}'
        echo
        cat
    } > "$d/t385-$shape-$eapi.ebuild"
}

# (a) bare dodir of an empty directory.
for eapi in 7 8; do emit a $eapi <<'EB'
src_install() {
	dodir /opt/t385/empty
}
EB
done
# (b) keepdir of an empty directory.
for eapi in 7 8; do emit b $eapi <<'EB'
src_install() {
	keepdir /opt/t385/kept
}
EB
done
# (c) dodir + keepdir under one nested parent.
for eapi in 7 8; do emit c $eapi <<'EB'
src_install() {
	dodir /opt/t385/nest/empty
	keepdir /opt/t385/nest/kept
}
EB
done
# (d) an empty dodir plus a doins'd file in a sibling directory.
for eapi in 7 8; do emit d $eapi <<'EB'
src_install() {
	dodir /opt/t385/dirempty
	insinto /opt/t385/withfile
	echo "t385-d payload" > "${T}/payload.txt" || die
	doins "${T}/payload.txt"
}
EB
done

ln -sfn "$R/profiles/default" "$CFG/etc/portage/make.profile"
cat > "$CFG/etc/portage/repos.conf" <<RC
[DEFAULT]
main-repo = t385
[t385]
location = $R
RC
cat > "$CFG/etc/portage/make.conf" <<'MC'
FEATURES="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -mount-sandbox -news -merge-wait"
CFLAGS="-O2"
LDFLAGS=""
BINPKG_FORMAT="gpkg"
BINPKG_COMPRESS="zstd"
GENTOO_MIRRORS=""
MC
find "$R" -name '*.ebuild' | LC_ALL=C sort
echo "setup done: $B"
