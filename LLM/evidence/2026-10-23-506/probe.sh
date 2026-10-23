#!/bin/bash
# #506 oracle: circular-dependency report text, Portage 3.0.83-r1 (host emerge)
# versus portuale, on the pmtest fixture repo through a hermetic config root.
# Usage: probe.sh <portage|portuale> <outdir> [shapes.tsv]
# shapes.tsv rows: <id>\t<env assignments or ->\t<emerge args>
# Each shape writes <id>.<pm>.{out,err,rc} (streams apart) and <id>.<pm>.cast
# (asciinema, both streams on one pty).
set -u
pm=$1; out=$2; here=$(cd "$(dirname "$0")" && pwd); shapes=${3:-$here/shapes.tsv}
FX=${FX:-$(readlink -f $here/../../../fixtures)}; T=/var/tmp/ev506-tree
rm -rf $T; mkdir -p $T/cfg/etc/portage/repos.conf $T/profile
printf '%s\n' "$FX/repo/profiles/base" "$FX/repo/profiles/arch/amd64" > $T/profile/parent
ln -sfn $T/profile $T/cfg/etc/portage/make.profile
printf '[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = %s/repo\n' "$FX" > $T/cfg/etc/portage/repos.conf/r.conf
printf 'FEATURES="-news"\nACCEPT_LICENSE="*"\n' > $T/cfg/etc/portage/make.conf
cp $FX/etc/portage/package.use $FX/etc/portage/package.mask $T/cfg/etc/portage/
export PORTAGE_CONFIGROOT=$T/cfg ROOT=/
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev506-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev506-bin/emerge; bin=/var/tmp/ev506-bin/emerge; fi
mkdir -p "$out"
while IFS=$'\t' read -r id envs args; do
  [ -z "$id" ] || [ "${id:0:1}" = "#" ] && continue
  [ "$envs" = - ] && envs=
  cmd="env $envs $bin $args"
  (cd $T && $cmd >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
  (cd $T && asciinema rec --headless --overwrite -q -c "$cmd" "$out/$id.$pm.cast" >/dev/null 2>&1)
done < "$shapes"
