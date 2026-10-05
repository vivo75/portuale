#!/bin/sh
# #317 S0: capture every syscall real Portage makes on the VDB while it
# merges, replaces and unmerges a package, on a plain files VDB.
# Run as root from the portuale repo root:  sudo sh docs/evidence/317-s0-capture.sh
# Output: docs/evidence/317-s0-strace/<step>.log (+ <step>.vdb, the VDB lines only)
set -eu
PI=
F=$(cd ../pmtest/fixtures && pwd)
OUT=$(pwd)/docs/evidence/317-s0-strace
R=$(mktemp -d /var/tmp/317-s0-root.XXXXXX)
T=$(mktemp -d /var/tmp/317-s0-tmp.XXXXXX)
mkdir -p "$R/var/db/pkg" "$R/var/lib/portage" "$R/var/cache/edb" "$OUT"
touch "$R/var/lib/portage/world"
E="$F/repo/dev-libs/binpkgrmpkg"
run() {  # step-name command...
  name=$1; shift
  env ROOT="$R" PORTAGE_CONFIGROOT="$F" PORTAGE_TMPDIR="$T" DISTDIR="$F/distfiles" \
      FEATURES="${PI:+parallel-install }-sandbox -usersandbox -userpriv -usersync -ipc-sandbox -network-sandbox -pid-sandbox" \
      strace -f -y -tt -s 256 -e trace=%file,%desc,fcntl,flock -o "$OUT/$name.log" "$@" \
      > "$OUT/$name.out" 2>&1 || { echo "$name failed:"; tail -20 "$OUT/$name.out"; exit 1; }
  grep -F "$R/var/db/pkg" "$OUT/$name.log" | sed "s#$R##g" > "$OUT/$name.vdb" || true
  echo "$name: $(wc -l < "$OUT/$name.vdb") VDB calls"
}
run 1-merge-new      ebuild --skip-manifest "$E/binpkgrmpkg-1.0.ebuild" merge
run 2-merge-same-pf  ebuild --skip-manifest "$E/binpkgrmpkg-1.0.ebuild" merge
run 3-merge-other-pf ebuild --skip-manifest "$E/binpkgrmpkg-2.0.ebuild" merge
run 4-unmerge        ebuild --skip-manifest "$E/binpkgrmpkg-2.0.ebuild" unmerge
# The in-tree slot lock (vartree.py:533-552) is taken only with
# FEATURES=parallel-install (the _slot_locked decorator, vartree.py:2088-2105,
# on dblink.unmerge 2481 and dblink.merge 6123 -- the path both `ebuild`
# and emerge's MergeProcess use). Same steps again with it on.
PI=1
run 5-pi-merge-new      ebuild --skip-manifest "$E/binpkgrmpkg-1.0.ebuild" merge
run 6-pi-merge-other-pf ebuild --skip-manifest "$E/binpkgrmpkg-2.0.ebuild" merge
run 7-pi-unmerge        ebuild --skip-manifest "$E/binpkgrmpkg-2.0.ebuild" unmerge
echo "ROOT kept at $R"
