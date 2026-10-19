# bed2 — L0 resolver, L0 fixture oracle, L1 merge gate (runs 3–5)

Date (UTC): 2026-10-19. All runs sequential, each prefixed with
`PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1` (frozen binary `/var/tmp/bedbin/portuale`).

- pmtest HEAD: `07ef58293ec9e7f9c65469a86d19d2025b39e5e3`, dirty:
  `M pytests-contract-suite/test_emerge_pretend_contract.py`,
  untracked `fixtures/repo-355/app-misc/t381-k/`,
  `fixtures/repo-355/app-misc/t381-rep/`, 4 untracked md5-cache files.
- portuale HEAD: `9a9097494c1f2af8b0617fc45d8796137971bdea`, dirty
  (8 modified code-notes/crate files, untracked `LLM/evidence/2026-10-19-381|391/`).
  Registry `portuale-bed` resolves version `9a909749-dirty`, emerge/ebuild/mrg =
  `/var/tmp/bedbin/portuale`. Runner rebuilds the PM itself; no build ran here
  (`PMTEST_NO_BUILD=1`).
- No file in either repo was edited; no cargo ran; no commit.

Reference (previous green runs, per task brief): l0-resolver 120 probes /
clean 101 / UNEXPLAINED 31 (18 rows); fixture oracle UNEXPLAINED 4 =
`--backtrack_0_dev-libs_blk0*` rows only; merge gate 0 hard / 0 UNEXPLAINED /
payload diffs 0 / merged_count 2 in runs 4–5.

## Run 1 — l0-resolver.sh (full atom list, no env overrides)

- Transcript: `/var/tmp/s10/bed2-l0r.txt`. Runner exit code: **1**.
- Report: `differential-test-bed/logs/l0-20261019T015228Z/`
  (`l0-report.txt` + `l0-report.json`; `logs/l0-report.txt` symlinked).
- Summary: probes 120, clean 101, parity 0.842, explained 6, UNEXPLAINED 31
  (advice 9, error 6, order 14, truncated 2). Invariants: 120 probes,
  0 portuale + 0 control violations.
- Explained (allowlisted): gnome-base_gnome-shell x4
  (`gnome-shell-cluster-a-abort`); gnome-base_nautilus + app-editors_gedit
  order lines (`gedit-nautilus-cluster-a-rewalk-abort`). Allowlist entries
  matching nothing: 6 `pubgrub-*` entries.
- Delta vs previous same-level report (`logs/l0-20261019T005107Z`,
  same 120/101/6/31 incl. category split): **zero — row set identical**
  (`diff` of `###` headers empty).
- Default argv both sides: `emerge -pv <atom>` (no `L0_EMERGE_OPTS` /
  `L0_PORTUALE_OPTS` overrides, so portuale argv byte-identical).

UNEXPLAINED rows (18; rc = portage_rc / portuale_rc from `l0-report.json`):

1. `dev-lang_ruby` (atom `dev-lang/ruby`), rc 0/0:
   `[order] merge order diverges at #20: real dev-ruby/did_you_mean vs portuale dev-ruby/matrix`
2. `dev-ruby_rake` (atom `dev-ruby/rake`), rc 0/0:
   `[order] merge order diverges at #20: real dev-ruby/did_you_mean vs portuale dev-ruby/matrix`
3. `app-containers_podman` (atom `app-containers/podman`), rc 1/1:
   `[truncated] autounmask backtracking terminated early (real Total 8 / 8 lines, portuale Total 43 / 43 lines) -- merge-list set/order/totals comparison suppressed`
4. `kde-plasma_plasma-meta` (atom `kde-plasma/plasma-meta`), rc 1/1:
   `[advice] real-only: masked-atom: >=x11-base/xwayland-23.1.0[libei]`
   `[advice] real-only: masked: !!! All ebuilds that could satisfy ">=x11-base/xwayland-23.1.0[libei]" have been masked.`
   `[advice] real-only: masked: !!! One of the following masked packages is required to complete your request:`
   `[truncated] autounmask backtracking terminated early (real Total 4 / 4 lines, portuale Total 465 / 465 lines) -- merge-list set/order/totals comparison suppressed`
   `[error] real-only message: !!! All ebuilds that could satisfy ">=x11-base/xwayland-23.1.0[libei]" have been masked.`
   `[error] real-only message: !!! One of the following masked packages is required to complete your request:`
5. `kde-apps_kdecore-meta` (atom `kde-apps/kdecore-meta`), rc 0/0:
   `[order] merge order diverges at #95: real x11-libs/libXau vs portuale dev-python/pyproject-metadata`
6. `gui-libs_gtk_4` (atom `gui-libs/gtk:4`), rc 1/1:
   `[order] merge order diverges at #30: real x11-base/xcb-proto vs portuale dev-python/pyproject-metadata`
7. `x11-libs_gtk__3` (atom `x11-libs/gtk+:3`), rc 1/1:
   `[order] merge order diverges at #30: real dev-libs/libgit2 vs portuale dev-python/pyproject-metadata`
8. `gnome-base_nautilus` (atom `gnome-base/nautilus`), rc 1/1:
   `[advice] real-only: masked-atom: >=app-crypt/gcr-3.27.90:0=[gtk]`
   `[advice] real-only: masked: !!! All ebuilds that could satisfy ">=app-crypt/gcr-3.27.90:0=[gtk]" have been masked.`
   `[advice] real-only: masked: !!! One of the following masked packages is required to complete your request:`
   `[error] real-only message: !!! All ebuilds that could satisfy ">=app-crypt/gcr-3.27.90:0=[gtk]" have been masked.`
   `[error] real-only message: !!! One of the following masked packages is required to complete your request:`
9. `app-editors_gedit` (atom `app-editors/gedit`), rc 1/1: same five
   gcr/gtk advice+error lines as row 8.
10. `net-misc_networkmanager` (atom `net-misc/networkmanager`), rc 0/0:
    `[order] merge order diverges at #26: real x11-libs/pixman vs portuale dev-python/pyproject-metadata`
11. `www-client_firefox` (atom `www-client/firefox`), rc 1/1:
    `[order] merge order diverges at #37: real dev-lang/nasm vs portuale media-libs/freetype`
12. `mail-client_thunderbird` (atom `mail-client/thunderbird`), rc 1/1:
    `[order] merge order diverges at #38: real dev-lang/nasm vs portuale media-libs/freetype`
13. `app-office_libreoffice` (atom `app-office/libreoffice`), rc 1/1:
    `[order] merge order diverges at #28: real app-arch/cpio vs portuale media-gfx/graphite2`
14. `media-gfx_gimp` (atom `media-gfx/gimp`), rc 1/1:
    `[order] merge order diverges at #54: real dev-libs/libyaml vs portuale media-libs/freetype`
15. `net-analyzer_wireshark` (atom `net-analyzer/wireshark`), rc 1/1:
    `[order] merge order diverges at #47: real dev-libs/libgit2 vs portuale dev-python/pyproject-metadata`
16. `media-video_vlc` (atom `media-video/vlc`), rc 0/0:
    `[order] merge order diverges at #7: real sys-libs/mtdev vs portuale x11-libs/xtrans`
17. `x11-base_xorg-server` (atom `x11-base/xorg-server`), rc 0/0:
    `[order] merge order diverges at #74: real x11-drivers/xf86-video-vesa vs portuale x11-drivers/xf86-video-amdgpu`
18. `MULTI_emptytree-system` (kind multi, argv both sides `emerge -pe @system`), rc 0/0:
    `[order] merge order diverges at #304: real sys-auth/pambase vs portuale sys-apps/portage`

Counts match the reference exactly (120 / 101 / 6 / 31, 18 rows).

## Run 2 — l0-fixture-oracle.sh

- Transcript: `/var/tmp/s10/bed2-l0fx.txt`. Runner exit code: **1**.
- Report: `differential-test-bed/logs/l0-fx-20261019T020404Z/`.
- Summary: probes 48, clean 38, parity 0.792, explained 19, UNEXPLAINED 4
  (all `skipped-updates`).
- Delta vs previous same-level report (`logs/l0-fx-20261019T010113Z`,
  same 48/38/19/4): **zero — row set identical**.
- Argv both sides: `emerge -p --color=n <case-args>` (case args = shell words
  of the atom-list line).

UNEXPLAINED rows (4; rc 0/0 all):

1. `--backtrack_0_dev-libs_blk0a_dev-libs_blk0b_dev-libs_blk0c`
   (`--backtrack=0 dev-libs/blk0a dev-libs/blk0b dev-libs/blk0c`):
   `[skipped-updates] dev-libs/blk0x:0: skipped-update explanation differs: real-only (1): (dev-libs/blk0x-2:0/0::testrepo, ebuild scheduled for merge) USE="" ABI_X86="(64)" conflicts with | portuale-only (1): (dev-libs/blk0x-2:0/0::testrepo, ebuild scheduled for merge) USE="" ELIBC="glibc" conflicts with`
2. `--backtrack_0_dev-libs_blk0b_dev-libs_blk0a_dev-libs_blk0c`
   (`--backtrack=0 dev-libs/blk0b dev-libs/blk0a dev-libs/blk0c`):
   same shape, blk0x-2, real `ABI_X86="(64)"` vs portuale `ELIBC="glibc"`.
3. `--backtrack_0_dev-libs_blk0b_dev-libs_blk0c_dev-libs_blk0a`
   (`--backtrack=0 dev-libs/blk0b dev-libs/blk0c dev-libs/blk0a`):
   same shape, blk0x-3, real `ABI_X86="(64)"` vs portuale `ELIBC="glibc"`.
4. `--backtrack_0_dev-libs_blk0c_dev-libs_blk0b_dev-libs_blk0a`
   (`--backtrack=0 dev-libs/blk0c dev-libs/blk0b dev-libs/blk0a`):
   same shape, blk0x-3, real `ABI_X86="(64)"` vs portuale `ELIBC="glibc"`.

Matches the reference (UNEXPLAINED 4 = only these rows).

## Run 3 — l1-merge-from-binpkg.sh gate list, default env

- Transcript: `/var/tmp/s10/bed2-g1.txt`. Runner exit code: **0**.
- Report: `differential-test-bed/logs/l1-20261019T020435Z/`.
- Summary: hard findings 0, explained 0, UNEXPLAINED 0, payload diffs 0,
  mtime-only diffs 7 (non-fatal).
- glibc/bash merged: **no** — both sides `merged_count 0`, `merge_rc 0`
  (gate atoms already installed in the image; without `L1_CONSUME_REINSTALL`
  there was nothing to re-merge). Portage snapshot 9 files, portuale 9 files.
- Delta: no same-env prior L1 report tonight (both earlier L1 reports are
  reinstall-mode with merged_count 2); counts match the gate reference
  (0 / 0 / 0).

## Run 4 — same + `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1`

- Transcript: `/var/tmp/s10/bed2-g2.txt`. Runner exit code: **0**.
- Report: `differential-test-bed/logs/l1-20261019T030302Z/`.
- Summary: hard findings 0, explained 0, UNEXPLAINED 0, payload diffs 0,
  mtime-only diffs 1416 (non-fatal).
- glibc/bash merged: **yes** — both sides `merged_count 2`, `merge_rc 0`
  (`sys-libs/glibc-2.43-r2`, `app-shells/bash-5.3_p15`).
- Delta vs prior reinstall-mode report `logs/l1-20261019T000743Z`
  (also 0/0/0, merged 2/2, mtime-only 1416): **zero**.

## Run 5 — same as run 4 + `L1_CONSUME_INSTALL_MASK="/usr/share/doc /usr/share/info /usr/share/man"`

- Transcript: `/var/tmp/s10/bed2-g3.txt`. Runner exit code: **0**.
- Report: `differential-test-bed/logs/l1-20261019T031051Z/`.
- Summary: hard findings 0, explained 0, UNEXPLAINED 0, payload diffs 0,
  mtime-only diffs 1418 (non-fatal).
- glibc/bash merged: **yes** — both sides `merged_count 2`, `merge_rc 0`
  (same `glibc-2.43-r2` + `bash-5.3_p15`).
- Delta vs prior report `logs/l1-20261019T001000Z`
  (also 0/0/0, merged 2/2, mtime-only 1418): **zero**.

## Totals vs reference

- L0 resolver: 120 probes / 101 clean / explained 6 / UNEXPLAINED 31 =
  reference. Delta vs previous: none.
- Fixture oracle: 48 / 38 / 19 / UNEXPLAINED 4 (backtrack rows only) =
  reference. Delta vs previous: none.
- Merge gate runs 3–5: 0 hard, 0 UNEXPLAINED, 0 payload diffs in all three;
  merged_count 0 (run 3, no reinstall) and 2+2 with glibc+bash on both sides
  (runs 4–5) = reference. No new findings anywhere.
