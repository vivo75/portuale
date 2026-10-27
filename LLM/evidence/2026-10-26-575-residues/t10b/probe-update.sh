#!/bin/bash
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); . "$HERE/run-pair.sh"
mkcell fx
for a in p569pull p589apull p589bpull p589cpull; do runpair fx u-$a --update dev-libs/$a; done
