#!/bin/sh
# Portage: portageq envvar. portuale has no envvar query and `emerge --info` shows neither
# variable (host Portage prints neither either), so read the strings embedded in the binary.
case "$1" in
portage) portageq envvar FETCHCOMMAND RESUMECOMMAND ;;
portuale) grep -a -o 'wget [^"]*-U \\*"Portage ([^)]*) distfile-fetch' target/release/portuale | sort -u ;;
esac
