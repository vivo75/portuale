#!/bin/bash
# setup.sh: build the scratch overlay + hermetic configroot for #354 S0.
# Everything lives under /var/tmp/t354; nothing in pmtest/fixtures is touched.
set -eu
B=/var/tmp/t354
rm -rf "$B"; mkdir -p "$B"
R=$B/repo; CFG=$B/cfg
mkdir -p $R/metadata $R/profiles/default $R/eclass
echo t354 > $R/profiles/repo_name
printf 'masters =\nthin-manifests = true\ncache-formats = md5-dict\nprofile-formats = portage-2\n' > $R/metadata/layout.conf
echo amd64 > $R/profiles/arch.list
echo 5 > $R/profiles/default/eapi
cat > $R/profiles/default/make.defaults <<'MD'
ARCH="amd64"
ACCEPT_KEYWORDS="amd64"
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
cp -a /home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-14-354-s0/scripts/ebuilds/. $R/
mkdir -p $CFG/etc/portage
ln -s $R/profiles/default $CFG/etc/portage/make.profile
cat > $CFG/etc/portage/repos.conf <<RC
[DEFAULT]
main-repo = t354
[t354]
location = $R
RC
cat > $CFG/etc/portage/make.conf <<'MC'
FEATURES="-sandbox -usersandbox -network-sandbox -ipc-sandbox -pid-sandbox -mount-sandbox -news -merge-wait"
CFLAGS="-O2"
LDFLAGS=""
BINPKG_FORMAT="gpkg"
BINPKG_COMPRESS="zstd"
GENTOO_MIRRORS=""
MC
# regenerate md5-cache via Portage's own egencache (real md5 entries)
egencache --config-root=$CFG --repo=t354 --update --jobs=1 >/dev/null
find $R/metadata/md5-cache -type f | sort
