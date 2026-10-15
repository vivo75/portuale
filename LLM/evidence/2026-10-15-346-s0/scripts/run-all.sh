#!/bin/bash
# S0 master runner (plan LLM/02.346-ebuild-config-options-parity.opus.md, slice S0).
# Runs every family in order; each script is idempotent (mkcfg wipes its tag).
# Families U/K/B/A/C/G are quick; E/S/M/L do real builds/merges and take longer.
# Total wall time ~30-45 min. Scratch /var/tmp/s346 is removed at the end;
# host /var/tmp/portage is restored to its pre-run (empty) state.
# The binary target/release/portuale is NEVER rebuilt here.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"
bash "$H/u.sh"
bash "$H/k.sh"
bash "$H/b.sh"
bash "$H/e.sh"
bash "$H/s.sh"
bash "$H/m.sh"
bash "$H/a.sh"
bash "$H/c.sh"
bash "$H/g.sh"
bash "$H/l.sh"
rm -f "$EVDIR"/m/*.basepath "$EVDIR"/l/*.basepath
rm -rf /var/tmp/portage/app-misc /var/tmp/s346
