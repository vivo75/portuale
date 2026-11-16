#!/bin/bash
# usage: mkroots.sh FROZEN_FIXTURES DEST ; builds the two scratch ROOTs the plan 02.765 probes need,
# mirroring pmtest's _698_root (shared vdb + installed holders that recorded the p569prov 5.42 pins).
F=$1; D=$2
mk() { # name "world extras" holder-lines...
  local name=$1 world=$2; shift 2
  local r=$D/$name; rm -rf "$r"; mkdir -p "$r"; cp -a "$F/var" "$r/var"
  find "$r/var" \( -name '*.lock' -o -name '.portage_lockfile' -o -name '.vdb*' \) -prune -exec rm -rf {} + 2>/dev/null
  for w in $world; do echo "$w" >> "$r/var/lib/portage/world"; done
  while [ $# -gt 0 ]; do
    local cpv=$1 rd=$2; shift 2
    local e=$r/var/db/pkg/$cpv; mkdir -p "$e"
    echo "${cpv%%/*}" > "$e/CATEGORY"; echo 8 > "$e/EAPI"; echo 0 > "$e/SLOT"; echo testrepo > "$e/repository"; echo "$rd" > "$e/RDEPEND"
  done
}
mk r698 "dev-libs/p698rebuild dev-libs/p698reinst dev-libs/p698upg" \
  dev-libs/p698rebuild-1 "dev-libs/p569prov:0/5.42=" \
  dev-libs/p698reinst-1 "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=" \
  dev-libs/p698upg-1 "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42="
mk r703 "dev-libs/p698rebuild dev-libs/p573virt" \
  dev-libs/p698rebuild-1 "dev-libs/p569prov:0/5.42=" \
  dev-libs/p698reinst-1 "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=" \
  dev-libs/p698upg-1 "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=" \
  dev-libs/p573virt-1 "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42="
