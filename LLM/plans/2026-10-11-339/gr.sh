#!/bin/bash
# G-R gate: usage gr.sh OUTDIR [nofx]
cd /home/vivo/repo/PORTUALE/portuale
O=$1; mkdir -p $O; O=$(cd $O && pwd)
export GENTOO_MIRRORS=http://10.48.0.229:8080
rm -rf ../pmtest/fixtures/var/db/pkg/dev-libs/pkginfopkg-1.0
sh LLM/tools/gate.sh $O/gate > $O/gate.summary 2>&1
BED=$PWD/../pmtest/differential-test-bed
( cd ../pmtest
  $BED/run/l0-resolver.sh > $O/l0.log 2>&1
  R=$(grep -o '/[^ ]*l0-report.txt' $O/l0.log | tail -1); echo "$R" > $O/l0-report-path
  cd ../portuale; sh LLM/tools/l0_unexplained.sh $R > $O/l0-unexplained.tsv
  diff LLM/plans/2026-10-11-339/baseline/l0-unexplained.tsv $O/l0-unexplained.tsv > $O/l0.diff; echo "l0diff rc=$?" >> $O/summary.txt
  cd ../pmtest
  if [ "$2" != nofx ]; then $BED/run/l0-fixture-oracle-all.sh > $O/fx.log 2>&1; tail -1 $O/fx.log >> $O/summary.txt; fi
)
echo done >> $O/summary.txt
