#!/bin/bash
# #509 merge oracle (#541/#542): real `-K` merges of the host-built patched
# gpkgs, on both managers, each into a fresh ROOT.
#   14-merge-match: `emerge -K app-misc/up-a`, local patches match the index
#     -> table on stdout, no warning, VDB gains USER_PATCHES+digests.
#   16-merge-mismatch: changed up-b patches + `--binpkg-respect-user-patches=n`,
#     `emerge -K app-misc/up-b` -> table on stdout, mismatch warning on stderr.
# Usage: probe-merge.sh <portage|portuale> <outdir>   (tree built on demand)
# Each shape writes <id>.<pm>.{out,err,rc,vdb} (streams apart) and <id>.<pm>.cast.
set -u
pm=$1; out=$(readlink -f "$2"); here=$(cd "$(dirname "$0")" && pwd)
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev509-tree
mkdir -p "$out"
if [ ! -f $T/.built ]; then
  bash $here/probe.sh portage /tmp/ev509-build-only >/dev/null 2>&1
fi
reset_tree() {
  cp $T/Packages.pristine $T/cfg/pkgdir/Packages
  rm -rf $T/cfg/etc/portage/patches; cp -r $T/patches.pristine $T/cfg/etc/portage/patches
}
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev509-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev509-bin/emerge; bin=/var/tmp/ev509-bin/emerge; fi

run_merge() { # id, root, args...
  local id=$1 root=$2; shift 2
  rm -rf $root; mkdir -p $root
  (cd $T && env ROOT=$root PORTAGE_CONFIGROOT=$T/cfg "$bin" "$@" >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
  (cd $T && env ROOT=$root PORTAGE_CONFIGROOT=$T/cfg asciinema rec --headless --overwrite -q -c "$bin $*" "$out/$id.$pm.cast" >/dev/null 2>&1)
  { echo "### VDB after $id ($pm)"; find $root/var/db/pkg -type f | sort; } >"$out/$id.$pm.vdb" 2>&1
}

export PORTAGE_CONFIGROOT=$T/cfg
# 14: matching patches -> binary merge with table, no warning
reset_tree
run_merge 14-merge-match $T/root-14 -K --color=n app-misc/up-a
for f in USER_PATCHES user_patch.digests; do
  echo "--- $f:" >>"$out/14-merge-match.$pm.vdb"
  cat "$T/root-14/var/db/pkg/app-misc/up-a-1.0/$f" >>"$out/14-merge-match.$pm.vdb" 2>&1
done
# 16: changed patches + =n -> binary merge with table + mismatch warning
reset_tree
printf -- '--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-up-b-CHANGED\n' > $T/cfg/etc/portage/patches/app-misc/up-b/user.patch
run_merge 16-merge-mismatch $T/root-16 -K --color=n --binpkg-respect-user-patches=n app-misc/up-b
reset_tree
echo MERGES-DONE
