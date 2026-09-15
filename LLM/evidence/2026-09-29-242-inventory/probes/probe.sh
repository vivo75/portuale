#!/bin/bash
# Track X Slice A S0: fresh real-Portage probes of the folded symptoms
# (#207 cyc0z, #208(a) cyc0w, #230 blk0) plus the g216 shape, under the
# default staging (ROOT=$FX) and under FX_HOST_ROOTS=1 as control.
# Real only; portuale is not executed here.
set -u
export LC_ALL=C.UTF-8 TZ=UTC
umask 022
STAGE=/tmp/xa-inv-fx
OUT=/out
/TEST/layers/l0-fixture-oracle/stage.sh "$STAGE" > "$OUT/stage.log" 2>&1 \
  || { echo "STAGING FAILED"; tail -20 "$OUT/stage.log"; exit 2; }
FX="$STAGE/fixtures"
rm -rf /var/db/pkg
cp -r "$FX/var/db/pkg" /var/db/pkg
if [ -f "$FX/var/lib/portage/world" ]; then
  mkdir -p /var/lib/portage
  cp "$FX/var/lib/portage/world" /var/lib/portage/world
fi
export PORTAGE_CONFIGROOT="$FX" DISTDIR="$FX/distfiles"
export PORTAGE_REPOSITORIES="$(cat "$FX/etc/portage/repos.conf/repos.conf")"
export PYTHONHASHSEED=0
{
  echo "date_utc $(date -u +%FT%TZ)"
  echo "real_portage $(/usr/sbin/emerge --version 2>/dev/null | head -1)"
  echo "staged_at $FX"
} > "$OUT/fingerprint.tsv"
run() {
  local file="$1"; shift
  echo "### emerge -p --color=n $*" >> "$OUT/run.log"
  timeout 600 /usr/sbin/emerge -p --color=n "$@" > "$OUT/$file" 2>&1
  echo "rc=$?" >> "$OUT/$file"
  echo "  $*  ($(tail -1 "$OUT/$file"))" >> "$OUT/run.log"
}
run_debug() {
  local file="$1"; shift
  timeout 600 /usr/sbin/emerge -p --debug --color=n "$@" > "$OUT/$file" 2>&1
  echo "rc=$?" >> "$OUT/$file"
}
# --- default staging: ROOT=$FX ---
export ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX"
echo "== default staging ROOT=$FX ==" >> "$OUT/run.log"
run default-cyc0z-1.txt "=dev-libs/cyc0z-1"
run default-cyc0z-2.txt "=dev-libs/cyc0z-2"
run default-cyc0w-1.txt "=dev-libs/cyc0w-1"
run default-cyc0w-2.txt "=dev-libs/cyc0w-2"
run default-cyc0w-3.txt "=dev-libs/cyc0w-3"
run default-blk0-acb.txt --backtrack=0 dev-libs/blk0a dev-libs/blk0c dev-libs/blk0b
run default-blk0-abc.txt --backtrack=0 dev-libs/blk0a dev-libs/blk0b dev-libs/blk0c
run default-blk0-bac.txt --backtrack=0 dev-libs/blk0b dev-libs/blk0a dev-libs/blk0c
run default-blk0-bca.txt --backtrack=0 dev-libs/blk0b dev-libs/blk0c dev-libs/blk0a
run default-blk0-cab.txt --backtrack=0 dev-libs/blk0c dev-libs/blk0a dev-libs/blk0b
run default-blk0-cba.txt --backtrack=0 dev-libs/blk0c dev-libs/blk0b dev-libs/blk0a
run default-g216top.txt app-misc/g216top
run default-g216comp.txt dev-lang/g216comp
run default-g216top-b0.txt --backtrack=0 app-misc/g216top
run default-g216comp-b0.txt --backtrack=0 dev-lang/g216comp
run_debug default-debug-cyc0z-1.txt "=dev-libs/cyc0z-1"
run_debug default-debug-cyc0w-3.txt "=dev-libs/cyc0w-3"
# --- control: host-exact roots ---
export ROOT="/" PORTAGE_RUNNING_ROOT="/"
echo "== control ROOT=/ ==" >> "$OUT/run.log"
run hostroots-cyc0z-1.txt "=dev-libs/cyc0z-1"
run hostroots-cyc0z-2.txt "=dev-libs/cyc0z-2"
run hostroots-cyc0w-1.txt "=dev-libs/cyc0w-1"
run hostroots-cyc0w-2.txt "=dev-libs/cyc0w-2"
run hostroots-cyc0w-3.txt "=dev-libs/cyc0w-3"
run hostroots-blk0-acb.txt --backtrack=0 dev-libs/blk0a dev-libs/blk0c dev-libs/blk0b
run hostroots-blk0-abc.txt --backtrack=0 dev-libs/blk0a dev-libs/blk0b dev-libs/blk0c
run hostroots-blk0-bac.txt --backtrack=0 dev-libs/blk0b dev-libs/blk0a dev-libs/blk0c
run hostroots-blk0-bca.txt --backtrack=0 dev-libs/blk0b dev-libs/blk0c dev-libs/blk0a
run hostroots-blk0-cab.txt --backtrack=0 dev-libs/blk0c dev-libs/blk0a dev-libs/blk0b
run hostroots-blk0-cba.txt --backtrack=0 dev-libs/blk0c dev-libs/blk0b dev-libs/blk0a
run hostroots-g216top.txt app-misc/g216top
run hostroots-g216comp.txt dev-lang/g216comp
run hostroots-g216top-b0.txt --backtrack=0 app-misc/g216top
run hostroots-g216comp-b0.txt --backtrack=0 dev-lang/g216comp
run_debug hostroots-debug-cyc0z-1.txt "=dev-libs/cyc0z-1"
run_debug hostroots-debug-cyc0w-3.txt "=dev-libs/cyc0w-3"
echo ALLDONE >> "$OUT/run.log"
