#!/bin/bash
# #508 oracle: `emerge --depclean` cycle suggestion, Portage 3.0.83-r1 (host emerge)
# versus portuale, on the pmtest fixture repo. Each shape gets its own ROOT with
# a hand-written VDB (installed list) and world file, through the 507 config.
# Usage: probe.sh <portage|portuale> <outdir> [shapes.tsv]
# Each shape writes <id>.<pm>.{out,err,rc} (streams apart) and <id>.<pm>.cast.
set -u
pm=$1; out=$2; here=$(cd "$(dirname "$0")" && pwd); shapes=${3:-$here/shapes.tsv}
FX=${FX:-$(readlink -f $here/../../../fixtures)}; B=/var/tmp/ev508-tree
rm -rf $B; mkdir -p $B/cfg/etc/portage/repos.conf $B/profile
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $B/profile/parent
ln -sfn $B/profile $B/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n' "$FX" > $B/cfg/etc/portage/repos.conf/r.conf
printf 'FEATURES="-news"\nACCEPT_LICENSE="*"\n' > $B/cfg/etc/portage/make.conf
cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $B/cfg/etc/portage/ 2>/dev/null
export PORTAGE_CONFIGROOT=$B/cfg
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev508-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev508-bin/emerge; bin=/var/tmp/ev508-bin/emerge; fi
mkdir -p "$out"
while IFS=$'\t' read -r id inst world args; do
  [ -z "$id" ] || [ "${id:0:1}" = "#" ] && continue
  R=$B/root-$id; rm -rf $R; mkdir -p $R/var/lib/portage
  IFS=, read -ra inst_a <<< "$inst"
  for tok in "${inst_a[@]}"; do p=${tok%%=*}; rd=; [[ $tok == *=* ]] && rd=${tok#*=}
    d=$R/var/db/pkg/dev-libs/$p-1.0; mkdir -p $d
    printf 'dev-libs\n' > $d/CATEGORY; printf '0\n' > $d/SLOT; printf '8\n' > $d/EAPI
    printf 'amd64\n' > $d/KEYWORDS; printf '\n' > $d/USE; printf '\n' > $d/IUSE
    if [[ $tok == *=* ]]; then printf '%s\n' "$rd" > $d/RDEPEND
    else printf '%s\n' "$(sed -n 's/^RDEPEND="\(.*\)"$/\1/p' $FX/repo/dev-libs/$p/$p-1.0.ebuild)" > $d/RDEPEND; fi
  done
  printf '%s\n' "${world//,/$'\n'}" | sed 's#^#dev-libs/#' > $R/var/lib/portage/world
  export ROOT=$R
  cmd="env ROOT=$R $bin $args"
  (cd $B && $cmd >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
  (cd $B && asciinema rec --headless --overwrite -q -c "$cmd" "$out/$id.$pm.cast" >/dev/null 2>&1)
done < "$shapes"
