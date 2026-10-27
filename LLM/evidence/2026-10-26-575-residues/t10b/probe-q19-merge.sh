#!/bin/bash
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); . "$HERE/run-pair.sh"
mkcell fx
runpair fx q19-merge dev-libs/p590xpull
