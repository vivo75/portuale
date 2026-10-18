# S404 provenance (backlog #404, 02.346 S11: auto-clean)

- Date: 2026-10-18 (UTC). Executor: implementation agent.
- Oracle ("Portage", never "real"): host `emerge`/`ebuild` — Portage
  3.0.82.2 (python 3.14.8-final-0, default/linux/amd64/23.0/desktop/plasma,
  gcc-16, glibc-2.44-r3, 6.18.39-serv x86_64).
- portuale: `/home/vivo/repo/PORTUALE/portuale/target/release/portuale`,
  mtime 2026-10-18 12:17:53 UTC, rev `5674bc11`, **never rebuilt during
  this slice's capture** (invoked via `/var/tmp/s404/bin/{emerge,ebuild}`
  symlinks, recorded in each `.argv`). Pre-fix binary: prints nothing
  after the merge in any autoclean cell (re-pins the S0 finding).
- Hermeticity: per-cell `PORTAGE_CONFIGROOT` under `/var/tmp/s404/<tag>/cfg`
  (layout mirrors the S0/S403 harnesses: fixture repos + `repo-341` +
  `repo-348`, `make.profile` → fixture `repo/profiles/default`), `ROOT`
  = scratch `$base/root`, `FEATURES`
  `-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news`,
  `GENTOO_MIRRORS=http://10.48.0.229:8080`, `EMERGE_DEFAULT_OPTS` empty,
  `PYTHONHASHSEED=0`. All `emerge` cells carry `--package-moves=n` on both
  sides (351-UPD cut: the forced-yes after applied global updates is NOT
  modelled). Known constant noise, identical to S0/S403: (H1) the stock
  `/usr/share/portage/config/repos.conf` adds a host `gentoo` repo to
  every Portage run; (H2) host `/etc/portage/binrepos.conf` entries are
  also loaded by Portage; (H3) fixture overlay repos print
  `missing masters attribute` warnings on Portage stderr. Portage reads
  `make.globals` from its install path whatever the configroot is
  (AUTOCLEAN default `no`, `make.globals:103`); portuale reads it only as
  `<cfg>/usr/share/portage/config/make.globals`, so the hermetic cfgs
  (no copy) behave as AUTOCLEAN unset.
- **No `CLEAN_DELAY` anywhere** (neither env nor make.conf): the
  5-second default applies, so any countdown line in a capture is a
  finding, not harness noise.
- Capture shape: `<cell>-<pm>.{argv,rc,stdout,stderr}` (verbatim,
  untrimmed; `.argv` holds cwd + full env assignments + command), plus
  `.vdbfind` (post-run `var/db/pkg` listing, maxdepth 2) and `.basepath`
  (scratch base, for re-inspection) where relevant.
- Scratch: `/var/tmp/s404` (kept for re-inspection until the slice
  lands; never `/dev/shm`). Never the host `/var/db/pkg` or host root.
- cl05 caveat (learned mid-capture): Portage consolidates vdb fields
  into `<entry>/metadata` and serves it while its stamp is valid, so a
  same-second `SLOT` edit alone is invisible to it (first cl05 run kept
  selecting 7). The cell now deletes the stale `metadata` file after the
  SLOT edit, falling back to the individual field files (documented
  Portage behavior, `_explode_metadata_file`).
