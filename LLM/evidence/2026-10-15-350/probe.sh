#!/bin/bash
# Probe for #350: does Portage's `ebuild <f> <phase>` re-run pretend/setup?
# Usage: probe.sh   (uses the host Portage; scratch dir under /var/tmp, removed at the end)
set -u
H=$(cd "$(dirname "$0")" && pwd); S=$(mktemp -d /var/tmp/p350.XXXXXX)
cp -a "$H/repo" "$H/cfg" "$S/"; mkdir "$S/tmp"
ln -sfn "$S/repo/profiles/base" "$S/cfg/etc/portage/make.profile"
printf '[DEFAULT]\nmain-repo = probe\n[probe]\nlocation = %s/repo\n' "$S" > "$S/cfg/etc/portage/repos.conf"
# the ebuild logs each phase body into $S/log
sed -i "s|LOGFILE|$S/log|" "$S/repo/t/p/p-1.ebuild"
export PORTAGE_CONFIGROOT=$S/cfg PORTAGE_TMPDIR=$S/tmp
F="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox"
FEATURES=-userpriv ebuild "$S/repo/t/p/p-1.ebuild" manifest >/dev/null 2>&1
run(){ rm -f "$S/log"; rm -rf "$S/tmp/portage"; echo "== $1 FEATURES=$2"
  FEATURES="$2 $F" ebuild "$S/repo/t/p/p-1.ebuild" $1 >/dev/null 2>&1; echo "bodies: $(tr '\n' ' ' < "$S/log")"; }
run unpack ""; run "setup unpack" ""; run "setup unpack" keepwork
rm -f "$S/log"; rm -rf "$S/tmp/portage"
for i in 1 2 3; do FEATURES="$F" ebuild "$S/repo/t/p/p-1.ebuild" unpack >/dev/null 2>&1; echo "unpack run $i bodies so far: $(tr '\n' ' ' < "$S/log")"; done
rm -rf "$S"
