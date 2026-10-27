#!/bin/bash
# s0e-run-all.sh: portuale-vs-Portage pairs for the S0e cells. Needs, under /var/tmp/claude-s0e:
#   fx0       cp -a of pmtest/fixtures;  fx0.cfg = s0e-mkcfg.sh fx0 (absolute repos.conf, make.profile)
#   bt/       pytest --basetemp of a suite run through the tracing shim (hermetic per-test roots)
# Hermetic roots are copied first (the suite's trees are never run against in place).
set -u
S=/var/tmp/claude-s0e; P=$(dirname "$0")/s0e-pair.sh; export OUT=${OUT:?}
cp_root() { # cp_root <glob of source dir> <name> -> $S/h-<name> plus its mkcfg
  local src; src=$(ls -d $1 | head -1); rm -rf "${S:?}/h-$2"; cp -a "$src" "$S/h-$2"
  [ -d "$S/h-$2/repo" ] && "$S/mkcfg.sh" "$S/h-$2" "$S/h-$2.cfg"
}
cp_root "$S/bt/popen-*/test_keeper_reachable_pin_brea0/fixtures" keeper-arg
cp_root "$S/bt/popen-*/test_keeper_reachable_pin_brea1/fixtures" keeper-hard
cp_root "$S/bt/popen-*/test_keeper_reachable_needer_t0/fixtures" triangle
cp_root "$S/bt/popen-*/test_ignore_world_contrast_aga0/fixtures" igwc
cp_root "$S/bt/popen-*/test_complete_graph_use_break_0/fixtures" cgpuse
cp_root "$S/bt/popen-*/test_complete_graph_ver_break_0/fixtures" cgpver
cp_root "$S/bt/popen-*/test_or_choice_avoids_downgrad0/b1root" orchoice
H=$S/h
$P keeper-arg $H-keeper-arg $H-keeper-arg.cfg --pretend =dev-libs/paired-2.0
$P keeper-hard $H-keeper-hard $H-keeper-hard.cfg --pretend dev-libs/needer
$P keeper-triangle $H-triangle $H-triangle.cfg --pretend dev-libs/needer dev-libs/othermod
for a in '>=dev-libs/igw0x-2' '<dev-libs/igw0x-1'; do
  n=$(echo "$a" | tr -c 'a-z0-9\n' _)
  $P igwc-plain$n $H-igwc $H-igwc.cfg --pretend "$a"
  $P igwc-ignore$n $H-igwc $H-igwc.cfg --pretend --ignore-world "$a"
  $P igwfix-ignore$n $S/fx0 $S/fx0.cfg --pretend --ignore-world "$a"
done
$P cgpuse-plain $H-cgpuse $H-cgpuse.cfg --pretend dev-libs/cgp0x
$P cgpuse-igw $H-cgpuse $H-cgpuse.cfg --pretend --ignore-world dev-libs/cgp0x
$P cgpver-plain-ge $H-cgpver $H-cgpver.cfg --pretend '>=dev-libs/cgp1x-2'
$P cgpver-plain-lt $H-cgpver $H-cgpver.cfg --pretend '<dev-libs/cgp1x-1'
# ROOT differs from the config root here: portuale and Portage both use fixtures as config root
PCFG=$S/fx0 $P orchoice $H-orchoice $S/fx0.cfg --pretend --update --deep @world
PCFG=$S/fx0 $P orchoice-bt0 $H-orchoice $S/fx0.cfg --pretend --update --deep --backtrack=0 @world
F=$S/fx0; C=$S/fx0.cfg
for a in p563pull p569pull p589apull p589bpull p589cpull; do $P fx-$a $F $C --pretend dev-libs/$a; done
$P fx-p569pull_t8x $F $C --pretend dev-libs/p569pull dev-libs/t8x
$P fx-t8x $F $C --pretend dev-libs/t8x
# corpus-stored needer/paired cells on the shared fixtures root
$P fx-plainuser-needer $F $C --pretend dev-libs/plainuser dev-libs/needer
$P fx-needer-plainuser $F $C --pretend dev-libs/needer dev-libs/plainuser
$P fx-needer $F $C --pretend dev-libs/needer
$P fx-paired-up $F $C --pretend --update dev-libs/paired
$P fx-paired-2 $F $C --pretend =dev-libs/paired-2.0
$P fx-triangle $F $C --pretend dev-libs/needer dev-libs/othermod
$P fx-triangle-rev $F $C --pretend dev-libs/othermod dev-libs/needer
# -pv forms used by the t8 plan
for a in p589apull p589bpull p589cpull p569pull; do $P pv-$a $F $C -pv dev-libs/$a; done
$P pv-p569pull_t8x $F $C -pv dev-libs/p569pull dev-libs/t8x
$P pv-t8x $F $C -pv dev-libs/t8x
# -- second batch: --update --deep forms (the invariants' puD cells) and the --backtrack=0 oracle-209 cell
for a in p569pull p589apull p589bpull p589cpull; do $P fx-puD-$a $F $C --pretend --update --deep dev-libs/$a; done
cp_root "$S/bt2/popen-*/test_oracle_209_backtrack_zero1/fixtures" o209
$P o209 $S/h-o209 $S/h-o209.cfg --pretend --backtrack=0 --update --deep --newuse --oneshot =dev-libs/r25lib-2.0
