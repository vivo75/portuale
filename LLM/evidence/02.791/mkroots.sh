#!/bin/bash
# usage: mkroots.sh FROZEN_FIXTURES DEST ; builds the scratch ROOTs plan 02.766 probes, mirroring pmtest's _698_root:
# a copy of the fixture var/ (shared vdb), extra world atoms, extra installed entries recording a stale binding.
# Every extra entry is: vdb CPV RDEPEND [DEPEND]   (SLOT 0, EAPI 8, repo testrepo).
F=$1; D=$2
mk() { # name "world extras"
  R=$D/$1; rm -rf "$R"; mkdir -p "$R"; cp -a "$F/var" "$R/var"
  find "$R/var" \( -name '*.lock' -o -name '.portage_lockfile' -o -name '.vdb*' \) -prune -exec rm -rf {} + 2>/dev/null
  python3 "$(dirname "$0")/seed_updates.py" "$R" "$F/repo/profiles/updates" "$F"/repo*/profiles/updates >/dev/null
  for w in $2; do echo "$w" >> "$R/var/lib/portage/world"; done
}
vdb() { # cpv rdepend [depend] [slot]
  local e=$R/var/db/pkg/$1; mkdir -p "$e"
  echo "${1%%/*}" > "$e/CATEGORY"; echo 8 > "$e/EAPI"; echo "${4:-0}" > "$e/SLOT"; echo testrepo > "$e/repository"; echo "$2" > "$e/RDEPEND"
  [ -n "$3" ] && echo "$3" > "$e/DEPEND"
  return 0
}
PIN='=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42='
BIND='dev-libs/p569prov:0/5.42='
# R0 control: nothing added
mk ctl ""
# R1 #761 shape: p569virt (installed fixture, outside the @world walk) has a stale binding once p569prov moves
mk prov "dev-libs/p569prov"
# R2 #698 root
mk r698 "dev-libs/p698rebuild dev-libs/p698reinst dev-libs/p698upg"
vdb dev-libs/p698rebuild-1 "$BIND"; vdb dev-libs/p698reinst-1 "$PIN"; vdb dev-libs/p698upg-1 "$PIN"
# R3 #703 root
mk r703 "dev-libs/p698rebuild dev-libs/p573virt"
vdb dev-libs/p698rebuild-1 "$BIND"; vdb dev-libs/p698reinst-1 "$PIN"; vdb dev-libs/p698upg-1 "$PIN"; vdb dev-libs/p573virt-1 "$PIN"
# bdw: consumer in the world, DEPEND-only binding (resolver reads DEPEND for a walked consumer that can build)
mk bdw "dev-libs/p569prov dev-libs/p698rebuild"
vdb dev-libs/p698rebuild-1 "" "$BIND"
# pull: p569pull-2 forces the provider to 5.44; p573virt (outside the walk) recorded the 5.42 pin
mk pull "dev-libs/p569pull"
vdb dev-libs/p573virt-1 "$PIN"
# gone: a consumer with no ebuild in the tree (outside the walk)
mk gone "dev-libs/p569prov"
vdb dev-libs/p766gone-1 "$BIND"
# unred: consumer in the world whose recorded text does not reduce (unbalanced ||)
mk unred "dev-libs/p569prov dev-libs/p698rebuild"
vdb dev-libs/p698rebuild-1 '|| ( dev-libs/p569prov:0/5.42='
# bdout: DEPEND-only binding, consumer outside the walk (record only: D1 hides it)
mk bdout "dev-libs/p569prov"
vdb dev-libs/p698rebuild-1 "" "$BIND"
# version-choice records (G3): p569virt in the world
mk virtw "dev-libs/p569virt"
mk vw2 "dev-libs/p569prov dev-libs/p569virt"
# --- plan 02.768-769 roots ---
# outr: consumer outside the walk whose hold-back passes (Q1(a) + seeding witness)
mk outr "dev-libs/p569prov"
vdb dev-libs/p698rebuild-1 "$BIND"
# outpin: consumer outside the walk with a recorded pin; its tree would accept the provider (Q-D)
mk outpin "dev-libs/p569prov"
vdb dev-libs/p698reinst-1 "$PIN"
# a2: masked-only consumer
mk a2 "dev-libs/p569prov"
vdb dev-libs/hardmaskedpkg-1.0 "$BIND"
# a5or: hold-back atom inside ||
mk a5or "dev-libs/p569prov"
vdb dev-libs/p698rebuild-1 "|| ( =dev-libs/p569prov-5.42* dev-libs/p569core ) $BIND"
# a7: downgrade (installed provider newer than the tree's best)
mk a7 "dev-libs/p569prov"
rm -rf "$R"/var/db/pkg/dev-libs/p569prov-*
vdb dev-libs/p569prov-5.46 "" "" "0/5.46"
vdb dev-libs/p698rebuild-1 "dev-libs/p569prov:0/5.46="
# a8: the installed provider has an empty SLOT file
mk a8 "dev-libs/p569prov"
vdb dev-libs/p698rebuild-1 "$BIND"
for s in "$R"/var/db/pkg/dev-libs/p569prov-*/SLOT; do : > "$s"; done
# a10: the consumer's tree pins the old provider (backlog #773, recorded only)
mk a10 "dev-libs/p569prov"
vdb dev-libs/p573virt-1 "$BIND"
# a808: the accepting || arm names a package the plan does not merge (backlog #808)
mk a808 "dev-libs/p569prov"
vdb dev-libs/p808virt-1 "$BIND"
