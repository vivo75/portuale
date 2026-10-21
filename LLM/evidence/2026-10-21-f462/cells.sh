#!/bin/bash
# #462 oracle cells: the autounmask-changes path on host Portage 3.0.82.2 vs
# portuale, over the staged fixture root (`setup.sh` first).
# usage: cells.sh   (writes portage.txt / portuale.txt captures beside itself)
set -u
S=${S7:-/var/tmp/f462/stage}
FX=$S/fixtures
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PORTUALE=/home/vivo/worktrees/f462/portuale/target/release/portuale

run_portage() { local name=$1; shift
  rm -rf "$S/root" "$S/dist" "$S/tmp"; mkdir -p "$S/root" "$S/dist" "$S/tmp"
  out=$(env -u PORTAGE_REPOSITORIES \
    PORTAGE_CONFIGROOT=$FX ROOT=$S/root DISTDIR=$S/dist PORTAGE_TMPDIR=$S/tmp \
    PYTHONHASHSEED=0 EMERGE_DEFAULT_OPTS= \
    emerge "$@" </dev/null 2>$S/stderr.txt); rc=$?
  { echo "=== $name [emerge $*] rc=$rc"; echo "--- stdout:"; printf '%s\n' "$out";
    echo "--- stderr:"; cat "$S/stderr.txt"; } >> "$HERE/portage.txt"
}
run_portuale() { local name=$1; shift
  out=$(PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX DISTDIR=$FX/distfiles \
    PYTHONHASHSEED=0 EMERGE_DEFAULT_OPTS= \
    "$PORTUALE" emerge "$@" </dev/null 2>$S/stderr.txt); rc=$?
  { echo "=== $name [portuale emerge $*] rc=$rc"; echo "--- stdout:"; printf '%s\n' "$out";
    echo "--- stderr:"; cat "$S/stderr.txt"; } >> "$HERE/portuale.txt"
}
cell() { run_portage "$@"; run_portuale "$@"; }

rm -f "$HERE/portage.txt" "$HERE/portuale.txt"
cell "c1-maskneedpkg" --pretend --autounmask dev-libs/maskneedpkg
cell "c2-keepmasks-n" --pretend --autounmask-keep-masks=n dev-libs/maskmaskedconsumer
cell "c3-hardmasked" --pretend --autounmask dev-libs/hardmaskedpkg
cell "c4-keyword" --pretend --autounmask dev-libs/autounmaskkeywordpkg
cell "c5-use" --pretend dev-libs/usedeprejectedpkg
cell "c6-license" --pretend --autounmask dev-libs/licensemaskedconsumer
cell "c7-quiet" --quiet --pretend --autounmask dev-libs/maskneedpkg
cell "c8-only" --pretend --autounmask --autounmask-only dev-libs/maskneedpkg
echo "wrote $HERE/portage.txt $HERE/portuale.txt"
