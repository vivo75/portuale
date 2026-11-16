#!/bin/bash
# usage: mkroots.sh DEST ; scratch ROOTs for plan 02.764: world = the B atom of a shape, empty VDB (test_740 style).
D=$1
mk() { R=$D/$1; rm -rf "$R"; mkdir -p "$R/var/lib/portage" "$R/var/db/pkg"; [ -n "$2" ] && echo "$2" > "$R/var/lib/portage/world" || : > "$R/var/lib/portage/world"; }
mk v1 dev-libs/w764cb; mk v2 dev-libs/w764ab; mk v3 dev-libs/w764db; mk ctl ""
