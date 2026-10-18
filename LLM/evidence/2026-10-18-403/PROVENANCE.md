# S403 provenance (backlog #403, slice S10 of 02.346)

- Date: 2026-10-18 (UTC). Executor: implementation agent.
- Oracle ("Portage", never "real"): host `emerge`/`ebuild` — Portage
  3.0.82.2 (python 3.14.8-final-0, default/linux/amd64/23.0/desktop/plasma,
  gcc-16, glibc-2.44-r3, 6.18.39-serv x86_64). Vendored checkout
  `3rdparty/portage` is byte-identical to the host install for
  `dbapi/vartree.py` (md5-verified for `output.py`; `vartree.py` differs
  by one trailing newline only), so cited line numbers apply to both.
- portuale: `/home/vivo/repo/PORTUALE/portuale/target/release/portuale`,
  rebuilt 2026-10-18 (`cargo build --release`, before any capture) and
  **never rebuilt during the capture run** (invoked via
  `/var/tmp/s403/bin/{emerge,ebuild}` symlinks, recorded in each `.argv`).
- Hermeticity: per-cell `PORTAGE_CONFIGROOT` under `/var/tmp/s403/<tag>/cfg`
  (layout mirrors the S0 harness: fixture repos + `repo-341` + `repo-348` +
  synthetic `s403` overlay, `make.profile` → fixture `repo/profiles/default`),
  `ROOT` = scratch `$base/root`, `CLEAN_DELAY=0`, `FEATURES`
  `-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news`
  (+ explicit `protect-owned` / `-protect-owned` / `CONFIG_PROTECT` where the
  cell says so), `GENTOO_MIRRORS=http://10.48.0.229:8080`,
  `EMERGE_DEFAULT_OPTS` empty, `PYTHONHASHSEED=0`. All `emerge` cells carry
  `--package-moves=n` on both sides. Known constant noise, identical to S0:
  (H1) the stock `/usr/share/portage/config/repos.conf` adds a host `gentoo`
  repo to every Portage run; (H2) host `/etc/portage/binrepos.conf` entries
  are also loaded by Portage; (H3) fixture overlay repos print
  `missing masters attribute` warnings on Portage stderr.
  Portage reads `make.globals` from its install path whatever the
  configroot is; portuale reads it only as
  `<cfg>/usr/share/portage/config/make.globals` (the S4 asymmetry: the
  hermetic cfg has no copy, so `protect-owned` — default-on in
  make.globals — is off in portuale unless the cell sets it explicitly;
  l08 sets it, l08b negates it; this is documented S4 behaviour, not new).
- Capture shape: `<cell>-<pm>.{argv,rc,stdout,stderr}` (verbatim, untrimmed;
  `.argv` holds cwd + full env assignments + command), plus
  `<cell>-<pm>.basepath` (the scratch base, kept for build-log inspection).
- Scratch: `/var/tmp/s403` (+ transient `/var/tmp/s403/probe`); `/var/tmp`
  is a 48G tmpfs, kept clean. Never `/dev/shm`. No host `/var/db/pkg` or
  host-root writes. The `l-l02b-logdir-portage` and `l-l02c-noclean`
  scratch bases are ad-hoc (not script cells) and documented in matrix.md.
- Scripts: `scripts/lib.sh` (adapted S0 harness) + `scripts/l.sh` (the whole
  recipe; re-runnable, idempotent per cell via `mkcfg`'s `rm -rf`).
  Synthetic packages live in the scratch overlay only
  (`$S403/s403-overlay`, repo `s403`, `thin-manifests`, no Manifest files —
  thin-manifest repos skip the check like `repo-348` in S0); no pmtest
  fixtures were added for the oracle.
- No product code changed during capture; no commits, no pushes.
  Untouched: the untracked `pg359*` fixture files in `../pmtest`
  (another worker's).
