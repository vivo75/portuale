#!/bin/bash
# run-counts.sh <outdir> : portuale-only BT-trace run of the 10a count list plus the 10b cells
# on scratch roots under /var/tmp/claude-10b (cp -a of pmtest/fixtures; world atoms appended to the
# h-* and w-* roots as the pytest _world_extra_env helper does). BIN picks the binary.
set -u
OUT=$1; mkdir -p "$OUT"; S=/var/tmp/claude-10b
BIN=${BIN:-/home/vivo/repo/PORTUALE/portuale/target/release/portuale}
SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
mkroot() { local d=$S/$1; shift; [ -d "$d" ] && return; mkdir -p "$S/ptmp"; cp -a "$SRC" "$d"; for a in "$@"; do echo "$a" >> "$d/var/lib/portage/world"; done; }
mkroot fx0
mkroot h-keeper dev-libs/keeper
mkroot h-igw dev-libs/igw0a
mkroot h-cgpver dev-libs/cgp1a
mkroot h-cgpuse dev-libs/cgp0q
mkroot w-avirt dev-libs/p589avirt
mkroot w-vpar dev-libs/p590vpar
run() { # name root args...
  local name=$1 root=$S/$2; shift 2
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$root ROOT=$root PORTAGE_RUNNING_ROOT=$root \
    DISTDIR=$root/distfiles PORTAGE_TMPDIR=$S/ptmp PORTUALE_BT_TRACE=1 \
    "$BIN" emerge "$@" > "$OUT/$name.out" 2> "$OUT/$name.err"; echo $? > "$OUT/$name.rc"
  local bt; bt=$(grep -ho 'backtrack: [0-9]*/[0-9]*' "$OUT/$name.out" "$OUT/$name.err" | head -1)
  echo "$name | rc $(cat "$OUT/$name.rc") | ${bt:-no-backtrack-line} | passes=$(grep -c '^BT try=' "$OUT/$name.err")"
}
N=(--complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n)
for a in p563pull p569pull p589apull p589bpull p589cpull; do run fx-$a fx0 --pretend dev-libs/$a; done
run fx-p569pull_t8x fx0 --pretend dev-libs/p569pull dev-libs/t8x
run fx-t8x fx0 --pretend dev-libs/t8x
run keeper-arg h-keeper --pretend =dev-libs/paired-2.0
run keeper-hard h-keeper --pretend dev-libs/needer
run keeper-triangle h-keeper --pretend dev-libs/needer dev-libs/othermod
run fx-triangle fx0 --pretend dev-libs/needer dev-libs/othermod
run fx-triangle-rev fx0 --pretend dev-libs/othermod dev-libs/needer
run igw-ge h-igw --pretend '>=dev-libs/igw0x-2'
run igw-lt h-igw --pretend '<dev-libs/igw0x-1'
run cgpver-ge h-cgpver --pretend '>=dev-libs/cgp1x-2'
run cgpver-lt h-cgpver --pretend '<dev-libs/cgp1x-1'
run cgpuse h-cgpuse --pretend dev-libs/cgp0x
for a in p569pull p589apull p589bpull p589cpull; do run puD-$a fx0 --pretend --update --deep dev-libs/$a; done
# 10b cells
run t1-deep fx0 --pretend --deep dev-libs/p589apull
run t1-nocomplete fx0 --pretend "${N[@]}" dev-libs/p589apull
run t1-cg fx0 --pretend "${N[@]}" --complete-graph dev-libs/p589apull
run t1-revdep-deep-inworld w-avirt --pretend --deep =dev-libs/p589aprov-5.44
run t2-nocomplete fx0 --pretend "${N[@]}" dev-libs/p589bpull
run t3-nocomplete fx0 --pretend "${N[@]}" dev-libs/p589cpull
run q19-order w-vpar --pretend dev-libs/p590vpull
