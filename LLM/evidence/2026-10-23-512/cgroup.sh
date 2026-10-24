#!/bin/bash
# #512 oracle, item (5): cgroup resource summary text.
# Upstream: 55beac58c (`=== Resource usage for {build,install} of {cpv}: ...`,
# was `=== Resource usage for {cpv} [...]`) + a6fca880a (background mode
# with --verbose echoes it to stdout). Needs FEATURES=cgroup and a
# writable cgroupfs; when the environment cannot provide one the shape
# records that instead.
# Usage: cgroup.sh <portage|portuale> <outdir>
set -u
pm=$1; out=$(readlink -f "$2")
here=$(cd "$(dirname "$0")" && pwd)
T=/var/tmp/ev512-tree
[ -f $T/.built ] || { echo "run probe.sh first"; exit 2; }
mkdir -p "$out"
printf 'FEATURES="-news -sandbox -usersandbox cgroup"\nACCEPT_LICENSE="*"\nACCEPT_KEYWORDS="amd64"\nPORTAGE_TMPDIR="%s/tmp"\n' "$T" > "$T/cfg/etc/portage/make.conf"
rm -rf "$T/root" "$T/tmp"; mkdir -p "$T/root" "$T/tmp"
mount | grep -i cgroup | head -3 >"$out/cgroup-mounts.txt" || true
export ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp
if [ "$pm" = portage ]; then bin=$(command -v emerge)
else mkdir -p /var/tmp/ev512-bin; ln -sf "$(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale})" /var/tmp/ev512-bin/emerge; bin=/var/tmp/ev512-bin/emerge; fi
id=14-cgroup
(cd "$T" && env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp \
  timeout 120 stdbuf -o0 -e0 "$bin" --oneshot --color=n --jobs=2 --verbose \
    =app-misc/sysextra-1.0 \
    >"$out/$id.$pm.out" 2>"$out/$id.$pm.err"; echo $? >"$out/$id.$pm.rc")
echo "rc $(cat $out/$id.$pm.rc)"
grep -h "Resource usage" "$out/$id.$pm.out" "$out/$id.$pm.err" 2>/dev/null || echo "no Resource usage on terminal"
find "$T/tmp/portage" -name build.log | head -3 | xargs grep -h "Resource usage" 2>/dev/null || echo "no Resource usage in build logs"
(cd "$T" && env ROOT=$T/root PORTAGE_CONFIGROOT=$T/cfg PORTAGE_TMPDIR=$T/tmp \
  asciinema rec --headless --overwrite -q -c "$bin --oneshot --color=n --jobs=2 --verbose =app-misc/sysextra-1.0" "$out/$id.$pm.cast" >/dev/null 2>&1) || true
