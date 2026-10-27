#!/bin/bash
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); . "$HERE/run-pair.sh"
mkcell fx
mkcell fxw dev-libs/p590vpar
runpair fxw q19-order dev-libs/p590vpull
