#!/bin/bash
# usage: run.sh <name> <portage|portuale|portuale-old>
set -e
n=$1; kind=$2; W=/var/tmp/ev505; E=$W/repo/app-misc/ev505/ev505-1.ebuild
rm -rf $W/t-$n $W/r-$n; mkdir -p $W/t-$n $W/r-$n
B=(env -i HOME=$HOME PATH=/usr/sbin:/usr/bin:/sbin:/bin LANG=C.UTF-8 LOCPATH=/var/tmp/ev505/loc USERLAND=BSD PORTAGE_CONFIGROOT=$W/cfg ROOT=$W/r-$n/ PORTAGE_TMPDIR=$W/t-$n)
case $kind in
portage) "${B[@]}" ebuild $E clean merge ;;
portuale) "${B[@]}" /home/vivo/repo/PORTUALE/portuale/target/release/portuale ebuild $E merge ;;
portuale-old) "${B[@]}" PORTUALE_BIN_DIR=$W/oldbin/bin /home/vivo/repo/PORTUALE/portuale/target/release/portuale ebuild $E merge ;;
esac 2>&1 | tail -3
bzcat $W/r-$n/var/db/pkg/app-misc/ev505-1/environment.bz2 > $W/$n.env
cp $W/t-$n/portage/app-misc/ev505-1/temp/environment $W/$n.T.env 2>/dev/null || true
