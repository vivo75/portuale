#!/bin/bash
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); . "$HERE/run-pair.sh"
mkcell fx
N=(--complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n)
runpair fx t3-nocomplete "${N[@]}" dev-libs/p589cpull
runpair fx t2-nocomplete "${N[@]}" dev-libs/p589bpull
