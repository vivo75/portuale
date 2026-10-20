#!/bin/bash
# #437 oracle root: the #423 root (bad/good/top).
S=${S7:-/var/tmp/b437}
HERE=$(cd "$(dirname "$0")" && pwd)
S7=$S bash "$HERE/../2026-10-19-b423/setup.sh"
