#!/bin/sh
# #317 S5: real Portage 3.0.82.2 `ebuild` merge / same-pf replace / other-pf
# replace / unmerge, once on a plain files ROOT and once with the ROOT's
# var/db/pkg a `portuale vdb mount --rw` over sqlite, then over redb; the
# database is converted back to files and compared after each step with
# 317-s5-compare.py, plus the counter.
# Run from the portuale root with a built rust/target/release/portuale:
#   sh docs/evidence/317-s5-host.sh            (uses sudo for ebuild and the mount)
set -eu
B=$(pwd)/rust/target/release/portuale
C=$(pwd)/docs/evidence/317-s5-compare.py
F=$(cd ../pmtest/fixtures && pwd)
E=$F/repo/dev-libs/binpkgrmpkg
W=$(mktemp -d /var/tmp/317-s5.XXXXXX)
mkroot() { mkdir -p "$1/var/db/pkg" "$1/var/lib/portage" "$1/var/cache/edb" "$1.tmp"; touch "$1/var/lib/portage/world"; }
eb() {  # root ebuild phase
  sudo env ROOT="$1" PORTAGE_CONFIGROOT="$F" PORTAGE_TMPDIR="$1.tmp" DISTDIR="$F/distfiles" \
    FEATURES="-sandbox -usersandbox -userpriv -ipc-sandbox -network-sandbox -pid-sandbox" \
    ebuild --skip-manifest "$2" "$3" > "$1.log" 2>&1
  n=$(grep -c 'ERROR\|FAILED\|Traceback\|not permitted' "$1.log" || true)
  echo "    $(basename "$2") $3: rc 0, $n error lines"
}
for kind in sqlite redb; do
  echo "== $kind"
  A=$W/$kind-files; D=$W/$kind-db; mkroot "$A"; mkroot "$D"
  $B vdb convert --from "files:$D" --to "$kind:$W/v.$kind" > /dev/null
  step=0
  for cmd in "binpkgrmpkg-1.0.ebuild merge" "binpkgrmpkg-1.0.ebuild merge" \
             "binpkgrmpkg-2.0.ebuild merge" "binpkgrmpkg-2.0.ebuild unmerge"; do
    step=$((step + 1)); set -- $cmd
    echo "  step $step: ebuild $1 $2"
    eb "$A" "$E/$1" "$2"
    sudo "$B" vdb mount --rw --root "$D" "$kind:$W/v.$kind" "$D/var/db/pkg"
    eb "$D" "$E/$1" "$2"
    sudo umount "$D/var/db/pkg"; sleep 1
    sudo rm -rf "$W/back"; sudo "$B" vdb convert --from "$kind:$W/v.$kind" --to "files:$W/back" > /dev/null
    printf '    compare: '; sudo python3 "$C" "$A/var/db/pkg" "$W/back/var/db/pkg"
    printf '    counter: files %s, %s %s\n' "$(sudo cat "$A/var/cache/edb/counter")" "$kind" \
      "$(sudo "$B" vdb status "$kind:$W/v.$kind" | sed -n 's/^counter: *//p')"
  done
done
echo "work dir: $W"
