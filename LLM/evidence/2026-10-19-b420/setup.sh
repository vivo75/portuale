#!/bin/bash
# Setup for #420 design cells: throwaway ROOT reusing pmtest fixtures read-only.
S=/var/tmp/w0/s10
R=$S/R
FIX=/home/vivo/repo/PORTUALE/pmtest/fixtures
cat > $R/etc/portage/repos.conf/repos.conf <<EOF
[DEFAULT]
main-repo = testrepo

[testrepo]
location = $FIX/repo

[overlay]
location = $FIX/overlay
priority = 10

[independentoverlay]
location = $FIX/independentoverlay
priority = 20
masters = overlay

[layoutmasteroverlay]
location = $FIX/layoutmasteroverlay
priority = 30

[repnamesection]
location = $FIX/repnamerepo
priority = 40

[t341]
location = $FIX/repo-341
priority = 50

[cellrepo]
location = $S/cellrepo
priority = 60
EOF
cat > $R/etc/portage/make.conf <<EOF
PKGDIR="$R/pkgdir"
FEATURES="-news"
EOF
ln -sfn $FIX/repo/profiles/default $R/etc/portage/make.profile
mkdir -p $R/tmp $R/dist $R/logs $R/root/var/db/pkg $R/pkgdir
