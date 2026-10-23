#!/bin/bash
# Hermetic `emerge --info` probe for backlog #504. Usage: probe.sh <portage|portuale> <outdir>
# Builds a tree with: a git repo (head commit + timestamp.chk), a
# timestamp-only repo, a repo whose sync-uri carries user:password@, and a
# binrepo with credentials. Records via asciinema.
set -eu
pm=$1; out=$2; T=/var/tmp/ev504-tree
here=$(cd "$(dirname "$0")" && pwd)
if [ ! -d $T ]; then
  mkdir -p $T/cfg/etc/portage/repos.conf $T/repo1/profiles/pfx $T/repo1/metadata $T/repo2/profiles $T/repo3/profiles $T/bh
  echo main > $T/repo1/profiles/repo_name; echo second > $T/repo2/profiles/repo_name; echo third > $T/repo3/profiles/repo_name
  printf 'ARCH="amd64"\nACCEPT_KEYWORDS="amd64"\n' > $T/repo1/profiles/pfx/make.defaults
  ln -s $T/repo1/profiles/pfx $T/cfg/etc/portage/make.profile
  echo "Mon, 01 Jan 2026 00:00:00 +0000" > $T/repo1/metadata/timestamp.chk
  mkdir -p $T/repo2/metadata; printf '# c\nTue, 02 Feb 2026 01:02:03 +0000  \n' > $T/repo2/metadata/timestamp.chk
  git -C $T/repo1 init -q; git -C $T/repo1 -c user.name=a -c user.email=a@b add -A
  git -C $T/repo1 -c user.name=a -c user.email=a@b commit -qm init
  cat > $T/cfg/etc/portage/repos.conf/r.conf <<EOF
[DEFAULT]
main-repo = main

[main]
location = $T/repo1
sync-type = git
sync-uri = https://alice:s3cret@example.invalid/main.git

[second]
location = $T/repo2
sync-type = rsync
sync-uri = rsync://bob:hunter2@example.invalid/second
priority = 5

[third]
location = $T/repo3
priority = 6
EOF
  cat > $T/cfg/etc/portage/binrepos.conf <<EOF
[bh]
sync-uri = https://carol:pw123@binhost.example.invalid/pkgs
priority = 7
location = $T/bh
verify-signature = false
EOF
fi
export PORTAGE_CONFIGROOT=$T/cfg ROOT=$T/cfg PORTAGE_TMPDIR=/var/tmp/ev504-tmp
mkdir -p "$out"
if [ "$pm" = portage ]; then bin=$(command -v emerge); else mkdir -p /var/tmp/ev504-bin; ln -sf $(readlink -f ${PORTUALE_BIN:-$here/../../../target/release/portuale}) /var/tmp/ev504-bin/emerge; bin=/var/tmp/ev504-bin/emerge; fi
asciinema rec --headless --overwrite -q -c "$bin --info" "$out/$pm.cast"
"$bin" --info > "$out/$pm.txt" 2>"$out/$pm.err" || true
