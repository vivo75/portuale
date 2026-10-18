#!/bin/bash
# Compare Portage (out/) with portuale (out-portuale/) per cell, modulo the
# global-updates banner, resolution seconds, license-group warning and ROOT.
norm() { sed -e 's#/tmp/st/fixtures/\?#ROOT#;s#/var/tmp/s8/stage/fixtures/\?#ROOT#' -e '/^Dependency resolution took/d;/^Undefined license/d;/^### delta/d' "$1" | awk '/^Performing Global Updates/{skip=1} skip&&/^These are|^Calculating|^\[|^emerge|^!!!|^rc=/{skip=0} !skip' | cat -s; }
for f in out/c*.txt; do n=$(basename $f); echo "=== $n"; diff <(norm $f) <(norm out-portuale/$n) && echo same; done
