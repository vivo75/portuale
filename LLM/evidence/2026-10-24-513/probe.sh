#!/bin/bash
# #513 oracle: deprecated `virtuals` files, Portage 3.0.83 (host emerge) versus portuale.
# Each shape gets its own PORTAGE_CONFIGROOT (user /etc/portage/virtuals) and
# leaf profile (profile virtuals), chained onto the pmtest fixture profiles.
# Usage: probe.sh <portage|portuale> <outdir> [shapes.tsv]
# Writes <id>.<pm>.{out,err,rc} (streams apart) and <id>.<pm>.cast.
set -u
pm=$1; out=$2; here=$(cd "$(dirname "$0")" && pwd); shapes=${3:-$here/shapes.tsv}
FX=${FX:-$(readlink -f $here/../../../fixtures)}; B=/var/tmp/ev513-tree
rm -rf $B; mkdir -p $B
# Merges need a Manifest that lists the ebuild (digestcheck is strict), so every
# shape reads a copy of the fixture repo with Manifests written by host `ebuild`.
REPO=/var/tmp/ev513-repo; rm -rf $REPO; mkdir -p $REPO; cp -r $FX/repo/. $REPO/
for p in dccyca dccycb; do (cd $REPO/dev-libs/$p && ebuild $p-1.0.ebuild manifest >/dev/null 2>&1); done
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev513-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev513-bin/emerge; bin=/var/tmp/ev513-bin/emerge; fi
mkdir -p "$out"; out=$(readlink -f "$out")
while IFS=$'\t' read -r id user prof args; do
  [ -z "$id" ] || [ "${id:0:1}" = "#" ] && continue
  C=$B/$id; cfg=$C/cfg; R=$C/root
  mkdir -p $cfg/etc/portage/repos.conf $C/profile $C/pparent $R/var/lib/portage $R/var/db/pkg
  printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $C/profile/parent
  [ "$prof" = parent ] && printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" "$C/pparent" > $C/profile/parent
  ln -sfn $C/profile $cfg/etc/portage/make.profile
  printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s\n' "$REPO" > $cfg/etc/portage/repos.conf/r.conf
  printf 'FEATURES="-news"\nACCEPT_LICENSE="*"\n' > $cfg/etc/portage/make.conf
  cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $cfg/etc/portage/ 2>/dev/null
  case $user in nonempty) printf 'virtual/editor app-editors/vim\n' > $cfg/etc/portage/virtuals;; empty) : > $cfg/etc/portage/virtuals;; esac
  case $prof in
    both)  printf 'virtual/editor app-editors/vim\n' > $C/profile/virtuals; printf 'virtual/editor app-editors/vim\n' > $C/pparent/virtuals; printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" "$C/pparent" > $C/profile/parent;;
    nonempty) printf 'virtual/editor app-editors/vim\n' > $C/profile/virtuals;;
    comment) printf '# nothing to see\n' > $C/profile/virtuals;;
    oneword) printf 'virtual/editor\n' > $C/profile/virtuals;;
    parent) printf 'virtual/editor app-editors/vim\n' > $C/pparent/virtuals;;
  esac
  # the user profile dir (/etc/portage/profile) is not in the profile chain
  case $id in 19-*) mkdir -p $cfg/etc/portage/profile; printf 'virtual/editor app-editors/vim\n' > $cfg/etc/portage/profile/virtuals;; esac
  # ordering shapes: an invalid package.mask line next to the virtuals warning
  case $id in 16-*) printf '!!!invalid-atom\n' >> $C/profile/package.mask;; 17-*) printf '!!!invalid-atom\n' >> $cfg/etc/portage/package.mask;; esac
  export PORTAGE_CONFIGROOT=$cfg ROOT=$R
  cmd="env ROOT=$R PORTAGE_CONFIGROOT=$cfg $bin $args"
  (cd $C && $cmd >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
  (cd $C && asciinema rec --headless --overwrite -q -c "$cmd" "$out/$id.$pm.cast" >/dev/null 2>&1)
done < "$shapes"
