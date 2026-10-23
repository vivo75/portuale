#!/bin/bash
# #509 oracle: user patches end to end, Portage 3.0.83-r1 (host emerge)
# versus portuale, on a scratch repo with real --buildpkgonly binaries.
# Tree ($T): repo `up509` (up-a..up-e, SLOT 0 except up-e SLOT 1), PKGDIR
# with five real gpkgs whose Packages index carries USER_PATCHES for
# a/b/c/e (built under patches) and no key for up-d (built without
# patches), local patches for a/b/c/e.
# Usage: probe.sh <portage|portuale> <outdir>
# Each shape writes <id>.<pm>.{out,err,rc} (streams apart) and <id>.<pm>.cast.
# Env: REBUILD=1 forces the repo+binpkg rebuild (default reuses $T if built).
set -u
pm=$1; out=$(readlink -f "$2"); here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev509-tree
mkdir -p "$out"

build_tree() {
rm -rf $T; mkdir -p $T/cfg/etc/portage/repos.conf $T/profile $T/repo/app-misc
for p in up-a up-b up-c up-d up-e; do mkdir -p $T/repo/app-misc/$p; done
for p in up-a up-b up-c up-d up-e; do
slot=0; [ "$p" = up-e ] && slot=1
cat > $T/repo/app-misc/$p/$p-1.0.ebuild <<EB
EAPI=8
DESCRIPTION="oracle: user-patch $p case"
HOMEPAGE="https://example.com"
S="\${WORKDIR}/src"
LICENSE="MIT"
SLOT="$slot"
KEYWORDS="amd64"
src_unpack() { mkdir -p "\${S}"; }
src_install() { :; }
EB
done
mkdir -p $T/repo/profiles $T/repo/metadata
printf 'app-misc\n' > $T/repo/profiles/categories
printf 'up509\n' > $T/repo/profiles/repo_name
printf 'masters = testrepo\nprofile-formats = portage-2\n' > $T/repo/metadata/layout.conf
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $T/profile/parent
ln -sfn $T/profile $T/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n\n[up509]\nlocation = %s/repo\n' "$FX" "$T" > $T/cfg/etc/portage/repos.conf/r.conf
printf 'FEATURES="-news -sandbox -usersandbox"\nACCEPT_LICENSE="*"\nPKGDIR="%s"\n' "$T/cfg/pkgdir" > $T/cfg/etc/portage/make.conf
mkdir -p $T/cfg/pkgdir $T/root
mkdir -p $T/cfg/etc/portage/patches/app-misc/up-a $T/cfg/etc/portage/patches/app-misc/up-b $T/cfg/etc/portage/patches/app-misc/up-c $T/cfg/etc/portage/patches/app-misc/up-e
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-a\n' > $T/cfg/etc/portage/patches/app-misc/up-a/user.patch
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-b\n' > $T/cfg/etc/portage/patches/app-misc/up-b/user.patch
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-c\n' > $T/cfg/etc/portage/patches/app-misc/up-c/user.patch
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-e\n' > $T/cfg/etc/portage/patches/app-misc/up-e/user.patch
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
for p in up-a up-b up-c up-d up-e; do ebuild $T/repo/app-misc/$p/$p-1.0.ebuild manifest >/dev/null 2>&1; done
emerge --buildpkgonly --color=n app-misc/up-a app-misc/up-b app-misc/up-c app-misc/up-d app-misc/up-e >$T/build.log 2>&1 || { echo "BUILD FAILED, see $T/build.log"; tail -30 $T/build.log; exit 1; }
[ -f $T/cfg/pkgdir/Packages ] || { echo "BUILD PRODUCED NO Packages"; exit 1; }
cp $T/cfg/pkgdir/Packages $T/Packages.pristine
cp -r $T/cfg/etc/portage/patches $T/patches.pristine
touch $T/.built
}

reset_tree() {
  cp $T/Packages.pristine $T/cfg/pkgdir/Packages
  rm -rf $T/cfg/etc/portage/patches; cp -r $T/patches.pristine $T/cfg/etc/portage/patches
}

if [ "${REBUILD:-0}" = 1 ] || [ ! -f $T/.built ]; then build_tree; fi

export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/root
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev509-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev509-bin/emerge; bin=/var/tmp/ev509-bin/emerge; fi

run_shape() { # id, args...
  local id=$1; shift
  (cd $T && env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg "$bin" "$@" >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
  (cd $T && env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg asciinema rec --headless --overwrite -q -c "$bin $*" "$out/$id.$pm.cast" >/dev/null 2>&1)
}

# 01: local patches match the index -> binary row, no block
reset_tree
run_shape 01-match-k -p --color=n -k app-misc/up-a
# 02/03: local patches differ -> ebuild row + block (-k); abort (-K)
reset_tree
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-b-CHANGED\n' > $T/cfg/etc/portage/patches/app-misc/up-b/user.patch
run_shape 02-differ-k -p --color=n -k app-misc/up-b
run_shape 03-differ-K -p --color=n -K app-misc/up-b
# 04/05: index entry without the key, local patches present -> masked
reset_tree
python3 - "$T/cfg/pkgdir/Packages" <<'EOF'
import sys
p = sys.argv[1]
out = []
skip = False
for line in open(p):
    if line.startswith('CPV: app-misc/up-c-1.0'):
        skip = True
    if skip and line.startswith('USER_PATCHES:'):
        skip = False
        continue
    if skip and line.strip() == '':
        pass
    out.append(line)
open(p, 'w').writelines(out)
EOF
run_shape 04-lack-k -p --color=n -k app-misc/up-c
run_shape 05-lack-K -p --color=n -K app-misc/up-c
# 06/07: index entry without the key, no local patches -> trusted binary
reset_tree
run_shape 06-trusted-k -p --color=n -k app-misc/up-d
run_shape 07-trusted-K -p --color=n -K app-misc/up-d
# 08/09: =n disables the filter
reset_tree
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-b-CHANGED\n' > $T/cfg/etc/portage/patches/app-misc/up-b/user.patch
run_shape 08-disablen-k -p --color=n -k --binpkg-respect-user-patches=n app-misc/up-b
run_shape 09-disablen-K -p --color=n -K --binpkg-respect-user-patches=n app-misc/up-b
# 10: explicit =y keeps the filter but silences the block
reset_tree
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-b-CHANGED\n' > $T/cfg/etc/portage/patches/app-misc/up-b/user.patch
run_shape 10-explicit-y-k -p --color=n -k --binpkg-respect-user-patches=y app-misc/up-b
# 11: --verbose names the triggering patch files
run_shape 11-verbose-k -p --color=n -k --verbose app-misc/up-b
# 12: full -K stderr on the differing shape (mask reason text)
run_shape 12-differ-K-stderr -p --color=n -K app-misc/up-b
# 13: slot-qualified binary row (oracle: `app-misc/up-e-1.0:1-1`)
reset_tree
python3 - "$T/cfg/pkgdir/Packages" <<'PYEOF'
import sys
p = sys.argv[1]
lines = open(p).read().split('\n')
in_e = False
for i, l in enumerate(lines):
    if l == 'CPV: app-misc/up-e-1.0':
        in_e = True
    elif l.startswith('CPV: '):
        in_e = False
    if in_e and l.startswith('USER_PATCHES:'):
        lines[i] = 'USER_PATCHES: ' + '0' * 64
open(p, 'w').write('\n'.join(lines))
PYEOF
run_shape 13-slot-K -p --color=n -K app-misc/up-e
# 15: stray files outside cat/pkg do not crash (7964f58e2)
reset_tree
printf 'not a patch\n' > $T/cfg/etc/portage/patches/stray.patch
mkdir -p $T/cfg/etc/portage/patches/app-misc-stray
printf 'x\n' > $T/cfg/etc/portage/patches/app-misc-stray/user.patch
run_shape 15-stray-k -p --color=n -k app-misc/up-a
reset_tree
echo SHAPES-DONE
