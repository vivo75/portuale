#!/bin/bash
# Runs the #335 and #328 container oracles with this tree's portuale; prints
# one labelled section per case. Usage: m1-cells.sh > baseline/m-cells/m1.log
PORT=$(cd "$(dirname "$0")/../../.." && pwd)
BED=$PORT/../pmtest/differential-test-bed
cd "$BED" || exit 2
run() { podman run --rm --entrypoint /bin/bash -v "$PWD/scripts:/o:ro" -v "$PORT/target/release:/pm:ro" localhost/test-portuale:latest "$@"; }
for c in env makeconf; do echo "=== 335 $c"; run /o/335-unmerge-features.sh "/pm/portuale emerge" $c 2>&1; done
echo "=== 328"; run /o/328-quickpkg-makeopts.sh "/pm/portuale emerge" 2>&1
