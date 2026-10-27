#!/bin/bash
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); . "$HERE/run-pair.sh"
mkcell fxw dev-libs/p590wpar
runpair fxw q19-build-default dev-libs/p590wpull
runpair fxw q19-build-bdeps --with-bdeps=y dev-libs/p590wpull
