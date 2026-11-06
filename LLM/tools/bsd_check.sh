#!/bin/sh
# Type-check the workspace for a BSD target (backlog #637). No BSD rust-std
# is installed, so std is built from `rust-src` (needs network for std's
# own crates.io deps and RUSTC_BOOTSTRAP=1 on a stable toolchain).
# Usage: LLM/tools/bsd_check.sh [triple]   (default x86_64-unknown-freebsd)
# Prints every error with its file; exit 0 only if the crate checks green.
set -u
target="${1:-x86_64-unknown-freebsd}"
dir="$(mktemp -d /var/tmp/bsd-check-XXXXXX)"
trap 'rm -rf "$dir"' EXIT
CARGO_TARGET_DIR="$dir" RUSTC_BOOTSTRAP=1 \
  cargo check -Zbuild-std=std,panic_abort --target "$target" -p portuale 2>&1 | tee "$dir/log" \
  | grep -aE '^error|^ +--> ' | paste - - | sed 's/ *--> */ @ /'
grep -aq '^error' "$dir/log" && exit 1
exit 0
