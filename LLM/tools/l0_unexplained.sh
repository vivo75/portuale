#!/bin/sh
# Print an L0 report's unexplained findings as sorted "probe<TAB>finding" lines,
# for a by-name comparison between two bed runs.
# Usage: l0_unexplained.sh <l0-report.txt>
awk '/^## unexplained findings/{on=1; next} /^## /{on=0} on && /^### /{p=$2; next} on && /^  \[/{sub(/^  /,""); print p "\t" $0}' "$1" | sort
