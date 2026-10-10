#!/bin/bash
# In-container half. Cells file: one cell per line, '|'-separated:
#   name | installed (';'-separated: cat/pkg-ver:slot[:RDEPEND]) | world atoms (space) | emerge args
# For each cell runs real and portuale with IDENTICAL argv (--ignore-default-opts --color=n appended first).
set -u
CELLS=$1; OUT=$2
REAL=/usr/sbin/emerge; PTL=/usr/local/bin/emerge
export LC_ALL=C.UTF-8 TZ=UTC PYTHONHASHSEED=0
umask 022
/stage.sh /tmp/stage > "$OUT/stage.log" 2>&1 || { echo "staging failed"; exit 2; }
FX=/tmp/stage/fixtures
$REAL --version | head -1 > "$OUT/real-version.txt"
: > "$OUT/meta.tsv"
while IFS='|' read -r name inst world args; do
  name=$(echo "$name" | xargs); case $name in ''|\#*) continue ;; esac
  ROOT_=/tmp/root-$name; rm -rf "$ROOT_"; mkdir -p "$ROOT_/var/lib/portage"
  for w in $world; do echo "$w"; done > "$ROOT_/var/lib/portage/world"
  IFS=';' read -ra items <<< "$inst"
  for it in "${items[@]}"; do
    it=$(echo "$it" | xargs); [ -z "$it" ] && continue
    pv=${it%%:*}; rest=${it#*:}; slot=${rest%%:*}; rdep=""; case $rest in *:*) rdep=${rest#*:} ;; esac
    cat_=${pv%%/*}; pf=${pv#*/}; d="$ROOT_/var/db/pkg/$cat_/$pf"; mkdir -p "$d"
    echo "$cat_" > "$d/CATEGORY"; echo "$slot" > "$d/SLOT"; echo testrepo > "$d/repository"; echo 8 > "$d/EAPI"
    [ -n "$rdep" ] && echo "$rdep" > "$d/RDEPEND"
  done
  export PORTAGE_CONFIGROOT="$FX" ROOT="$ROOT_" PORTAGE_RUNNING_ROOT="$ROOT_" DISTDIR="$FX/distfiles"
  export PORTAGE_REPOSITORIES="$(cat "$FX/etc/portage/repos.conf/repos.conf")"
  for side in real portuale; do
    bin=$REAL; [ $side = portuale ] && bin=$PTL
    mkdir -p "$OUT/$side"
    echo "$bin --ignore-default-opts --color=n $args" > "$OUT/$side/$name.argv"
    # shellcheck disable=SC2086
    (cd "$ROOT_" && $bin --ignore-default-opts --color=n $args) > "$OUT/$side/$name.out" 2> "$OUT/$side/$name.err"
    echo $? > "$OUT/$side/$name.rc"
  done
  printf '%s\t%s\t%s\n' "$name" "$(cat "$OUT/real/$name.rc")" "$(cat "$OUT/portuale/$name.rc")" >> "$OUT/meta.tsv"
  diff -u "$OUT/real/$name.out" "$OUT/portuale/$name.out" > "$OUT/$name.diff"
done < "$CELLS"
