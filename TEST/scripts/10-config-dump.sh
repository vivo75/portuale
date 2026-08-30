#!/bin/bash
set +e
LOG=/TEST/logs/10-config-dump.log
{
  echo "=== EMERGE_DEFAULT_OPTS ==="
  /usr/sbin/emerge --info 2>/dev/null | grep -E "EMERGE_DEFAULT_OPTS|^FEATURES"
  echo "=== make.conf ==="
  cat /etc/portage/make.conf 2>/dev/null
  echo "=== env EMERGE_DEFAULT_OPTS ==="
  env | grep -i emerge
  echo "=== plain vs verbose real emerge (coreutils) ==="
  /usr/sbin/emerge --pretend --color=n --verbose=n sys-apps/coreutils 2>&1 | grep -nE "^\[ebuild" | cat -A
  /usr/sbin/emerge -p --color=n -v sys-apps/coreutils 2>&1 | grep -nE "^\[ebuild" | cat -A
} | tee "$LOG"
