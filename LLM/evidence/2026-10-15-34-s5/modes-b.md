# Task #34 S5b re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `188ecb57335d243dd8f323622d342dad061f8a59`, dirty
(`M Cargo.lock`,
`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/Cargo.toml`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s5/g2/`; S5b change in place:
G2; binary below already contains it).
pmtest HEAD: `2492d4b2e99bc5d7af351fb882cf12335d125bf8`
(`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-15 05:33 UTC (before the runs;
already contains the S5b change; the runner's own rebuild said
`Finished release profile [optimized] in 0.09s/0.10s` — a no-op — in every run;
no cargo build/test was run during the bed runs).
Image: `localhost/test-portuale:latest` (`5616d41d00cd`), Portage 3.0.82.2.

Each pubgrub run (from `/home/vivo/repo/PORTUALE/pmtest`, sequentially):
`export GENTOO_MIRRORS=http://10.48.0.229:8080`
`L0_SKIP_MULTI=1 L0_SKIP_INVARIANTS=1 L0_PORTUALE_OPTS=--solver=pubgrub`
`L0_EMERGE_OPTS="<mode> --getbinpkg=n --usepkg=n" differential-test-bed/run/l0-resolver.sh`
with `<mode>` = `-pv`, `-pvu`, `-pvuD`, `-pvuDN`, `-pve` in that order.
Default run: same env except `L0_PORTUALE_OPTS` unset and
`L0_EMERGE_OPTS="-pvuDN --getbinpkg=n --usepkg=n"`.
Counts:
`python3 differential-test-bed/compare/resolve-compare.py --count-categories missing,extra,version,action,exit <rundir> differential-test-bed/compare/known-divergences.yaml > <rundir>/counts.txt`
(runner exit 1 in all 6 runs — the final comparator exiting nonzero is expected, not a
failure; the count command also rewrites `<rundir>/l0-report.txt`/`.json` with the action
split; count exit 1 in all 6 runs for the same reason.)

Environment note: same gentoo tree `11c58b7a`, same buildovl `3b1df681`, same porttest
`b4b5d17a`, same VDB snapshot (320 installed packages, `vdb-list.txt` byte-identical to
the S5a default run), same `world.txt`, same image as the S5a runs
(headers of all six S5b `counts.txt` confirm the tree/overlay hashes).

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S5b run dir | S5b missing | S5b extra | S5b version | S5b action | S5b exit | S5a missing | S5a extra | S5a version | S5a action | S5a exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261015T053625Z/` | 1 | 1213 | 0 | 0 | 30 | 23 | 5 | 0 | 0 | 30 | -22 | +1208 | 0 | 0 | 0 |
| `-pvu` | `differential-test-bed/logs/l0-20261015T054453Z/` | 1 | 1103 | 0 | 0 | 30 | 23 | 5 | 0 | 0 | 30 | -22 | +1098 | 0 | 0 | 0 |
| `-pvuD` | `differential-test-bed/logs/l0-20261015T055257Z/` | 1 | 3273 | 0 | 0 | 30 | 23 | 4 | 0 | 0 | 30 | -22 | +3269 | 0 | 0 | 0 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261015T060359Z/` | 0 | 4 | 0 | 0 | 30 | 0 | 4 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |
| `-pve` | `differential-test-bed/logs/l0-20261015T061552Z/` | 0 | 10 | 0 | 0 | 30 | 0 | 10 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |

(S5a totals and S5a run dirs per mode are quoted from the S5a modes-a.md:
S5a run dirs `-pv` `l0-20261015T040457Z/`, `-pvu` `l0-20261015T041110Z/`,
`-pvuD` `l0-20261015T041711Z/`, `-pvuDN` `l0-20261015T042557Z/`, `-pve`
`l0-20261015T043546Z/`. The operative delta in this S5b re-measure is S5b vs S5a.)

Notes: in `-pv`/`-pvu`/`-pvuD` every UP finding is `extra` (all UP breakdowns are
extra-only); the single remaining `missing=1` in each of the three modes is
`kde-apps_kdecore-meta`. `-pvuDN` and `-pve` are per-probe identical to S5a
(0 up, 0 down).
Context (not counted): S5b clean probes 49/54/16/12/81
(S5a: 57/60/64/12/81).
explained findings 11/11/11/8/8 (identical to S5a in every mode).

## Probes whose counted totals went UP versus S5a (name, S5a total, S5b total)

Per-probe totals are the sums of the five counted categories from the
`## counts by probe` section of each run's `counts.txt`; S5a per-probe totals from
`counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`, `counts-pvuDN.txt`,
`counts-pve.txt` in this directory (copies of each S5a run dir's `counts.txt`).

### -pv (`differential-test-bed/logs/l0-20261015T053625Z/`)

39 probes went UP vs S5a (S5a run dir `l0-20261015T040457Z/`). All UP findings are
extra-only.

| probe | S5a total | S5b total |
|---|---|---|
| `_system` | 0 | 26 |
| `_world` | 0 | 26 |
| `app-admin_sudo` | 0 | 44 |
| `app-editors_emacs` | 0 | 44 |
| `app-editors_neovim` | 0 | 28 |
| `app-editors_vim` | 0 | 2 |
| `app-misc_tmux` | 0 | 44 |
| `app-shells_starship` | 0 | 48 |
| `app-text_texlive` | 0 | 12 |
| `app-text_texlive-core` | 0 | 12 |
| `dev-build_cmake` | 0 | 48 |
| `dev-db_mariadb` | 0 | 48 |
| `dev-db_postgresql` | 0 | 43 |
| `dev-lang_ruby` | 0 | 48 |
| `dev-lang_rust` | 4 | 44 |
| `dev-lang_rust-bin` | 0 | 48 |
| `dev-libs_glib` | 0 | 28 |
| `dev-python_lxml` | 0 | 2 |
| `dev-qt_qtbase_6` | 0 | 13 |
| `dev-ruby_rake` | 0 | 48 |
| `dev-texlive_texlive-latex` | 0 | 12 |
| `kde-frameworks_kcoreaddons` | 5 | 7 |
| `media-libs_gstreamer` | 0 | 25 |
| `media-libs_mesa` | 0 | 2 |
| `media-video_ffmpeg` | 0 | 15 |
| `media-video_vlc` | 5 | 7 |
| `net-libs_nodejs` | 0 | 44 |
| `net-misc_networkmanager` | 1 | 15 |
| `sys-apps_bat` | 0 | 48 |
| `sys-apps_dbus` | 0 | 32 |
| `sys-apps_fd` | 0 | 48 |
| `sys-apps_ripgrep` | 0 | 48 |
| `sys-apps_systemd` | 0 | 20 |
| `sys-apps_util-linux` | 0 | 44 |
| `sys-auth_polkit` | 1 | 20 |
| `virtual_libudev` | 0 | 44 |
| `virtual_mta` | 0 | 44 |
| `www-servers_apache` | 0 | 44 |
| `x11-base_xorg-server` | 0 | 31 |

1 probe went DOWN vs S5a (context, not required):

| probe | S5a total | S5b total | S5b breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 11 | 7 | missing=1 extra=6 |

### -pvu (`differential-test-bed/logs/l0-20261015T054453Z/`)

34 probes went UP vs S5a (S5a run dir `l0-20261015T041110Z/`). All UP findings are
extra-only.

| probe | S5a total | S5b total |
|---|---|---|
| `_world` | 0 | 44 |
| `app-admin_sudo` | 0 | 44 |
| `app-editors_emacs` | 0 | 44 |
| `app-editors_neovim` | 0 | 28 |
| `app-editors_vim` | 0 | 2 |
| `app-misc_tmux` | 0 | 44 |
| `app-shells_starship` | 0 | 48 |
| `app-text_texlive` | 0 | 12 |
| `app-text_texlive-core` | 0 | 12 |
| `dev-db_mariadb` | 0 | 48 |
| `dev-db_postgresql` | 0 | 43 |
| `dev-lang_ruby` | 0 | 48 |
| `dev-lang_rust` | 4 | 44 |
| `dev-lang_rust-bin` | 0 | 48 |
| `dev-libs_glib` | 0 | 28 |
| `dev-qt_qtbase_6` | 0 | 13 |
| `dev-ruby_rake` | 0 | 48 |
| `dev-texlive_texlive-latex` | 0 | 12 |
| `kde-frameworks_kcoreaddons` | 5 | 7 |
| `media-libs_gstreamer` | 0 | 25 |
| `media-libs_mesa` | 0 | 2 |
| `media-video_ffmpeg` | 0 | 15 |
| `media-video_vlc` | 5 | 7 |
| `net-libs_nodejs` | 0 | 44 |
| `net-misc_networkmanager` | 1 | 15 |
| `sys-apps_bat` | 0 | 48 |
| `sys-apps_dbus` | 0 | 44 |
| `sys-apps_fd` | 0 | 48 |
| `sys-apps_ripgrep` | 0 | 48 |
| `sys-apps_systemd` | 0 | 44 |
| `sys-auth_polkit` | 1 | 20 |
| `virtual_mta` | 0 | 44 |
| `www-servers_apache` | 0 | 44 |
| `x11-base_xorg-server` | 0 | 31 |

1 probe went DOWN vs S5a (context, not required):

| probe | S5a total | S5b total | S5b breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 11 | 7 | missing=1 extra=6 |

### -pvuD (`differential-test-bed/logs/l0-20261015T055257Z/`)

76 probes went UP vs S5a (S5a run dir `l0-20261015T041711Z/`). All UP findings are
extra-only. No probe went DOWN vs S5a.

| probe | S5a total | S5b total |
|---|---|---|
| `_dev-lang_python-3.15` | 0 | 48 |
| `_system` | 0 | 48 |
| `_world` | 0 | 48 |
| `app-admin_perl-cleaner` | 0 | 48 |
| `app-admin_sudo` | 0 | 48 |
| `app-crypt_gnupg` | 0 | 48 |
| `app-editors_emacs` | 0 | 48 |
| `app-editors_neovim` | 0 | 32 |
| `app-editors_vim` | 0 | 37 |
| `app-misc_tmux` | 0 | 48 |
| `app-portage_eix` | 0 | 48 |
| `app-portage_gentoolkit` | 0 | 48 |
| `app-shells_starship` | 0 | 48 |
| `app-text_texlive` | 0 | 16 |
| `app-text_texlive-core` | 0 | 16 |
| `dev-build_autoconf` | 0 | 48 |
| `dev-build_automake` | 0 | 48 |
| `dev-build_cmake` | 0 | 49 |
| `dev-build_libtool` | 0 | 48 |
| `dev-build_meson` | 0 | 48 |
| `dev-db_mariadb` | 0 | 48 |
| `dev-db_postgresql` | 0 | 47 |
| `dev-lang_perl` | 0 | 48 |
| `dev-lang_python_3.14` | 0 | 48 |
| `dev-lang_ruby` | 0 | 48 |
| `dev-lang_rust` | 4 | 44 |
| `dev-lang_rust-bin` | 0 | 48 |
| `dev-libs_boost` | 0 | 47 |
| `dev-libs_glib` | 0 | 32 |
| `dev-libs_icu` | 0 | 47 |
| `dev-perl_Locale-gettext` | 0 | 48 |
| `dev-perl_XML-Parser` | 0 | 48 |
| `dev-python_certifi` | 0 | 48 |
| `dev-python_cython` | 0 | 48 |
| `dev-python_jinja2` | 0 | 48 |
| `dev-python_lxml` | 0 | 48 |
| `dev-python_pip` | 0 | 47 |
| `dev-python_pygments` | 0 | 47 |
| `dev-python_pyyaml` | 0 | 48 |
| `dev-python_setuptools` | 0 | 48 |
| `dev-qt_qtbase_6` | 0 | 17 |
| `dev-ruby_rake` | 0 | 48 |
| `dev-texlive_texlive-latex` | 0 | 16 |
| `kde-frameworks_kcoreaddons` | 5 | 11 |
| `media-libs_freetype` | 0 | 48 |
| `media-libs_gstreamer` | 0 | 29 |
| `media-libs_mesa` | 0 | 38 |
| `media-video_ffmpeg` | 0 | 19 |
| `media-video_vlc` | 5 | 11 |
| `net-libs_nodejs` | 0 | 45 |
| `net-misc_networkmanager` | 1 | 15 |
| `net-misc_openssh` | 0 | 48 |
| `sys-apps_bat` | 0 | 48 |
| `sys-apps_dbus` | 0 | 48 |
| `sys-apps_fd` | 0 | 48 |
| `sys-apps_pciutils` | 0 | 48 |
| `sys-apps_portage` | 0 | 48 |
| `sys-apps_ripgrep` | 0 | 48 |
| `sys-apps_systemd` | 0 | 48 |
| `sys-apps_systemd-utils` | 0 | 48 |
| `sys-apps_util-linux` | 0 | 48 |
| `sys-auth_polkit` | 1 | 24 |
| `sys-devel_binutils` | 0 | 49 |
| `sys-devel_gcc` | 0 | 48 |
| `sys-fs_lvm2` | 0 | 48 |
| `sys-libs_glibc` | 0 | 48 |
| `sys-libs_glibc_multilib_` | 0 | 48 |
| `sys-process_htop` | 0 | 48 |
| `virtual_cron` | 0 | 48 |
| `virtual_jpeg` | 0 | 46 |
| `virtual_libc` | 0 | 48 |
| `virtual_libudev` | 0 | 48 |
| `virtual_mta` | 0 | 48 |
| `virtual_perl-ExtUtils-MakeMaker` | 0 | 48 |
| `www-servers_apache` | 0 | 47 |
| `x11-base_xorg-server` | 0 | 35 |

### -pvuDN (`differential-test-bed/logs/l0-20261015T060359Z/`)

No probe went UP vs S5a (S5a run dir `l0-20261015T042557Z/`).
No probe went DOWN vs S5a — per-probe counted totals are identical to S5a.

### -pve (`differential-test-bed/logs/l0-20261015T061552Z/`)

No probe went UP vs S5a (S5a run dir `l0-20261015T043546Z/`).
No probe went DOWN vs S5a — per-probe counted totals are identical to S5a.

## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261015T062807Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0 — equal to the S5a default run (`l0-20261015T044752Z`: 0/448/0/0/0).
Context (not counted): clean probes 14, explained findings 0 (same as S5a).

No probe's counted total changed vs the S5a default run (0 up, 0 down).

Per-probe fingerprints of the portuale side: `diff -rq` of the `portuale/` output
directories between the S5a and S5b default runs reports no differences — the
portuale side is byte-identical file-for-file (117/117). `fingerprint.tsv` differs
only in the `date_utc` line (`2026-10-15T04:47:53Z` vs `2026-10-15T06:28:07Z`).
The real (Portage) side differs in 0 files when the
`Dependency resolution took ...s (backtrack: ...)` timing line is excluded (0/117
with a non-timing diff). `vdb-list.txt` (320 packages) and `world.txt` are
byte-identical between the two default runs.
