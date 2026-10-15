# S0 provenance

- Date: 2026-10-15/16 (UTC). Executor: unattended S0 worker.
- Oracle ("Portage", never "real"): host `emerge` — Portage 3.0.82.2
  (python 3.14.8-final-0, default/linux/amd64/23.0/desktop/plasma/systemd,
  gcc-16, glibc-2.44-r3, 6.18.39-serv x86_64).
- portuale: `/home/vivo/repo/PORTUALE/portuale/target/release/portuale`,
  mtime 2026-10-15 19:18:07 UTC, **never rebuilt during this slice**
  (invoked via `/var/tmp/s346/bin/{emerge,ebuild}` symlinks, recorded in each `.argv`).
- portuale git rev: `f221c03f53b69ac686dc270cbae995cfc329f40e`
  (`git rev-parse HEAD`, both repos untouched — no commits, no branches, no
  product-code/test/backlog/plan edits; see SUMMARY.md).
- Hermeticity: per-cell `PORTAGE_CONFIGROOT` under `/var/tmp/s346/<tag>/cfg`
  (layout mirrors pmtest `_t341_configroot`: fixture repos + `repo-341` +
  `repo-348`, `make.profile` → fixture `repo/profiles/default`), `ROOT` =
  cfg (emerge cells) or scratch `$base/root` (real merges), `CLEAN_DELAY=0`,
  `FEATURES=-news` base, `GENTOO_MIRRORS=http://10.48.0.229:8080` exported,
  `EMERGE_DEFAULT_OPTS` empty, `PYTHONHASHSEED=0` (except C `*-noseed` runs).
  Known constant noise, identical for fixture-shape cells in pmtest itself:
  (H1) the stock `/usr/share/portage/config/repos.conf` adds a host `gentoo`
  repo (`/.gentoo/repos/gentoo`) to every Portage run; (H2) host
  `/etc/portage/binrepos.conf/*.conf` (`[gentoo]`, `[f1r]`) is *also* loaded
  by Portage (`bintree.py:965-979`: install-path config first, configroot
  second), so G-family Portage runs consult cached host binhost indexes;
  (H3) fixture overlay repos print `missing masters attribute` /
  `missing repo_name` warnings on Portage stderr (verbatim, not stripped).
  Portage reads `make.globals` from its install path whatever the
  configroot is; portuale reads it only as `<cfg>/usr/share/portage/config/make.globals`
  (e06 carries a copy) — that asymmetry is itself an S4 cell.
- Every `emerge` cell runs with `--package-moves=n` on both sides (351-UPD
  cut, `actions.py:3671-3691`), except: g00 + l00 (the dedicated coupling
  cells) and c12 (`--frobnicate`, dies in arg parsing before resolution;
  moves flag would be dead text). `ebuild` cells take no moves flag.
- Capture shape: `<cell>-<pm>.{argv,rc,stdout,stderr}` (verbatim, untrimmed;
  `.argv` holds cwd + full env assignments + command), plus `.stagelog`
  (phase env dumps), `.pkgfind`/`.distfind`/`.tmpfind`/`.buildlogs`/
  `.plogfind`/`.vdbfind`/`.scratchfind` where relevant, and
  `e/e03-host-var-tmp-portage.txt` (host pollution the 349-TMP bug causes).
- Scratch: `/var/tmp/s346` (+ transient `/var/tmp/s346/probe`,
  `/var/tmp/dbg`, `/var/tmp/dbg-l`), all deleted at slice end; host
  `/var/tmp/portage` restored to its pre-run empty state. Never `/dev/shm`.
- No new fixtures were needed: scratch overlays/repos/profiles carry the
  synthetic packages (`s346-fetch`, `s346-gbin-*`, `s346m` Manifest copy,
  chain profiles); every script rebuilds its world from fixtures + scratch.
