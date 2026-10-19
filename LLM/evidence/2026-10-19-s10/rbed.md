# Bed runs 2026-10-19 (UTC)

Env for all runs: `PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1`
(frozen binary `/var/tmp/bedbin/portuale`, no cargo build).
Workdir: `/home/vivo/repo/PORTUALE/pmtest`.

## PM HEADs

- portuale (`/home/vivo/repo/PORTUALE/portuale`): `971186f4e86117665d084d30738d79675a09dc3`,
  branch `batch/2026-10-18`, dirty (modified: `LLM/code-notes/INDEX.md`,
  `LLM/code-notes/portage-repo/masked_deps.md`,
  `LLM/code-notes/portage-repo/resolve/pass/top_level_unsatisfied.md`,
  `LLM/code-notes/portuale/remote/merge_script.md`,
  `crates/portage-repo/src/masked_deps.rs`,
  `crates/portage-repo/src/resolve/pass/top_level_unsatisfied.rs`,
  `crates/portage-repo/src/tests/binpkg_use.rs`,
  `crates/portuale/src/remote/merge_flow.sh`,
  `crates/portuale/src/remote/merge_script.rs`,
  `crates/portuale/src/remote/tests/stages.rs`;
  untracked: `LLM/code-notes/portuale/remote/merge_flow.sh.md`).
- pmtest (`/home/vivo/repo/PORTUALE/pmtest`): `964fb4422b9f0495458e0f9625eee738f594809d`,
  branch `batch/2026-10-18`, dirty (`M pytests-contract-suite/test_emerge_pretend_contract.py`).
- Reports below belong to those revs (the bed stamps `pm.json` per run dir).

## Run 1 — l0-resolver.sh (default `atomlists/l0-resolve.txt`)

- Runner exit code: **1**
- Raw log: `/var/tmp/s10/bed-l0r.txt`
- Report: `differential-test-bed/logs/l0-20261019T005107Z/l0-report.txt`
  (JSON: same dir `l0-report.json`; `logs/l0-report.txt` symlinked to it)
- Summary: probes 120, clean 101, parity_rate 0.842, explained 6,
  UNEXPLAINED 31 (advice 9, error 6, order 14, truncated 2)
- Invariants: 120 probes, 0 portuale violation(s), 0 control violation(s)
- Delta vs previous (`logs/l0-20261018T225931Z`): **none** —
  full-file diff of `l0-report.txt` is only the `date_utc` line.
  Matches reference 120/101/31.
- UNEXPLAINED row ids (18 rows under `## unexplained findings`):
  `dev-lang_ruby`, `dev-ruby_rake`, `app-containers_podman`,
  `kde-plasma_plasma-meta`, `kde-apps_kdecore-meta`, `gui-libs_gtk_4`,
  `x11-libs_gtk__3`, `gnome-base_nautilus`, `app-editors_gedit`,
  `net-misc_networkmanager`, `www-client_firefox`,
  `mail-client_thunderbird`, `app-office_libreoffice`, `media-gfx_gimp`,
  `net-analyzer_wireshark`, `media-video_vlc`, `x11-base_xorg-server`,
  `MULTI_emptytree-system`
  (plus 3 explained rows: `gnome-base_gnome-shell`, `gnome-base_nautilus`,
  `app-editors_gedit`; unmatched-allowlist section empty)

## Run 2 — l0-fixture-oracle.sh (default `atomlists/l0-fixture-oracle.txt`)

- Runner exit code: **1**
- Raw log: `/var/tmp/s10/bed-l0fx.txt`
- Report: `differential-test-bed/logs/l0-fx-20261019T010113Z/l0-report.txt`
  (JSON: same dir `l0-report.json`)
- Summary: probes 48, clean 38, parity_rate 0.792, explained 19,
  UNEXPLAINED 4 (skipped-updates 4)
- Delta vs previous (`logs/l0-fx-20261018T225818Z`): **none** —
  full-file diff of `l0-report.txt` is only the `date_utc` line.
- UNEXPLAINED row ids (4 rows, all `[skipped-updates]`, nothing else):
  `--backtrack_0_dev-libs_blk0a_dev-libs_blk0b_dev-libs_blk0c`,
  `--backtrack_0_dev-libs_blk0b_dev-libs_blk0a_dev-libs_blk0c`,
  `--backtrack_0_dev-libs_blk0b_dev-libs_blk0c_dev-libs_blk0a`,
  `--backtrack_0_dev-libs_blk0c_dev-libs_blk0b_dev-libs_blk0a`
- Explained rows (6): `dev-libs_autounmaskkeywordpkg`, `dev-libs_kwneedpkg`,
  `dev-libs_requiredusebadpkg`, `-D_dev-libs_deepusedepfconsumer`,
  `-D_--autounmask-use_n_dev-libs_r135consumer`, `dev-libs_condanyof`
- Allowlist entries that matched nothing: `slotop-world-complete-graph-order`
  (identical in previous report)

## Run 3 — l31-remote-merge.sh (default `atomlists/l31-s0.txt`, default `L31_MODE=control`)

- Runner exit code: **0**
- Raw log: `/var/tmp/s10/bed-l31.txt`
- Report: `differential-test-bed/logs/l31-20261019T010140Z/l31-report.txt`
  (`logs/l31-report.txt` symlinked to it)
- Summary: hard findings 0, explained 0, UNEXPLAINED 0,
  payload diffs 0 (tolerated), mtime-only diffs 21 (non-fatal)
- Both sides: pm portage, Portage 3.0.82.2, merge_rc 0, merged_count 3, files 41
- Delta vs previous: previous run (`logs/l31-20261013T191637Z`) was
  **mode=candidate** (hard 0, UNEXPLAINED 0, mtime-only 5); this run is
  **mode=control**, so headline counts are not directly comparable.
  No prior control-mode baseline cited here. No UNEXPLAINED rows.
