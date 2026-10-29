#!/bin/sh
# Capture Portage 3.0.83 and portuale under `--pretend --debug` (#614).
# Portage prints the banner but neither the "Calculating dependencies ... done!"
# notice nor the "Dependency resolution took ..." report; portuale prints both.
PORTUALE_DIR=${PORTUALE_DIR:-/home/vivo/repo/PORTUALE/portuale}
PMTEST_DIR=${PMTEST_DIR:-$PORTUALE_DIR/../pmtest}
OUT=${OUT:-/tmp/opencode}

mkdir -p "$PORTUALE_DIR/LLM/evidence/2026-10-29-e614"
rm -rf "$OUT/ph-e614" && cp -a "$PMTEST_DIR/fixtures" "$OUT/ph-e614"
cd "$OUT/ph-e614"
for atom in dev-libs/diamond dev-libs/abort-masked-mid; do
  env PORTAGE_CONFIGROOT=$PWD ROOT=$PWD PORTAGE_RUNNING_ROOT=$PWD \
      DISTDIR=$PWD/distfiles emerge --pretend --debug "$atom" \
      >"$OUT/portage-$(basename $atom).txt" 2>&1
  echo "portage $atom rc=$?"
done
for atom in dev-libs/diamond dev-libs/abort-masked-mid; do
  env PORTAGE_CONFIGROOT=$PWD ROOT=$PWD PORTAGE_RUNNING_ROOT=$PWD \
      DISTDIR=$PWD/distfiles "$PORTUALE_DIR/target/release/portuale" \
      emerge --pretend --debug "$atom" >"$OUT/portuale-$(basename $atom).txt" 2>&1
  echo "portuale $atom rc=$?"
done
