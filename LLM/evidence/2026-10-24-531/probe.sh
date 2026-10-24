#!/bin/bash
# #531 capture: the message `emerge` prints for a package whose RDEPEND holds
# a use dependency the atom grammar rejects (`cat/pkg[a(+),-a]`, the #510
# probe atom), host Portage 3.0.83 versus portuale.
#
# The visible text is NOT the C reducer message (`invalid use dep token:
# '-a'`): `Package._validate_deps` passes `is_valid_flag`, so
# `use_reduce`'s C fast path is skipped (it requires `is_valid_flag is None`)
# and the Python path's `Invalid atom (%s), token %s` wrapper is what a user
# sees (lib/portage/dep/__init__.py:974, :1079-1086).
#
# Usage: probe.sh <portage|portuale> <outdir>
set -u
pm=$1; out=$2; here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}
B=/var/tmp/ev531-tree
rm -rf $B; mkdir -p $B/fx $B/cfg/etc/portage $B/root/var/lib/portage
cp -a "$FX"/. $B/fx/
D=$B/fx/repo/dev-libs/r531bad; mkdir -p $D
cat > $D/r531bad-1.0.ebuild <<'EOF'
EAPI=8
DESCRIPTION="scratch package for #531"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="cat/pkg[a(+),-a]"
EOF
mkdir -p $B/fx/repo/metadata/md5-cache/dev-libs
md5=$(md5sum $D/r531bad-1.0.ebuild | cut -d' ' -f1)
printf '_md5_=%s\nEAPI=8\nKEYWORDS=amd64\nRDEPEND=cat/pkg[a(+),-a]\nSLOT=0\n_eclasses_=\n_mtime_=0\n' "$md5" \
  > $B/fx/repo/metadata/md5-cache/dev-libs/r531bad-1.0
mkdir -p $B/cfg/etc/portage/repos.conf
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n[overlay]\nlocation = %s/overlay\n' "$B/fx" "$B/fx" \
  > $B/cfg/etc/portage/repos.conf/r.conf
ln -sfn $B/fx/repo/profiles/default $B/cfg/etc/portage/make.profile
printf 'FEATURES="-news"\nACCEPT_LICENSE="*"\n' > $B/cfg/etc/portage/make.conf
case $pm in
  portage) bin=$(command -v emerge) ;;
  portuale) bin=${PORTUALE_BIN:-$(readlink -f $here/../../../target/release/portuale)}; bin="env $bin" ;;
  *) echo "usage: probe.sh <portage|portuale> <outdir>" >&2; exit 2 ;;
esac
mkdir -p "$out"
(cd $B && env PORTAGE_CONFIGROOT=$B/cfg ROOT=$B/root $bin emerge -p --color=n dev-libs/r531bad \
  >"$out/01-rdepend-bad-dep.$pm.out" 2>"$out/01-rdepend-bad-dep.$pm.err"; echo $? >"$out/01-rdepend-bad-dep.$pm.rc")
