#!/bin/bash
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); . "$HERE/run-pair.sh"
mkcell fx
mkcell fxw dev-libs/p589avirt
N=(--complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n)
runpair fx t1-plain dev-libs/p589apull
runpair fx t1-deep --deep dev-libs/p589apull
runpair fx t1-nocomplete "${N[@]}" dev-libs/p589apull
runpair fx t1-cg "${N[@]}" --complete-graph dev-libs/p589apull
runpair fxw t1-revdep-deep-inworld --deep =dev-libs/p589aprov-5.44
runpair fx t2-plain dev-libs/p589bpull
runpair fx t3-plain dev-libs/p589cpull
runpair fx t3-nocomplete "${N[@]}" dev-libs/p589cpull
runpair fx t2-nocomplete "${N[@]}" dev-libs/p589bpull
