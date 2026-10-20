#!/bin/bash
# usage: sample.sh OUT ; samples eu-stack of the emerge every 0.5 s
out=$1; : > $out
cd /tmp
PORTUALE_PROFILE=1 /home/vivo/repo/PORTUALE/portuale/target/release/portuale emerge -puDN @world >/var/tmp/c452/world.out 2>/var/tmp/c452/world.err &
pid=$!
while kill -0 $pid 2>/dev/null; do
  echo "=== $(date +%s.%N)" >> $out
  eu-stack -n 60 -p $pid 2>/dev/null >> $out
  sleep 0.4
done
wait $pid; echo "rc $?" >> $out
