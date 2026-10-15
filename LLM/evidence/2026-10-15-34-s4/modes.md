# Task S4 re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `a0c58e365d959a54bf4dd2118a4ff51a74102426`, dirty
(`M LLM/backlog-tasks-2026-10.md`,
`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s4/`,
`?? THIRD-PARTY.md`; S4 change in progress:
pubgrub bridge — installed packages use their recorded USE and deps under `-D`).
pmtest HEAD: `80184a9d7d0f69f5c6349954169ac27d282d0389`
(`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-15 02:23 UTC (before the runs;
already contains the S4 change; the runner's own rebuild said
`Finished release profile [optimized] in 0.10s` — a no-op — in every run;
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
`b4b5d17a`, same VDB snapshot (320 installed packages), same image as the S2/S3 runs
(headers of all six S4 `counts.txt` confirm the tree/overlay hashes).

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S4 run dir | S4 missing | S4 extra | S4 version | S4 action | S4 exit | S3 missing | S3 extra | S3 version | S3 action | S3 exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261015T022535Z/` | 24 | 5 | 0 | 0 | 30 | 24 | 5 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |
| `-pvu` | `differential-test-bed/logs/l0-20261015T023148Z/` | 24 | 5 | 0 | 0 | 30 | 24 | 5 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |
| `-pvuD` | `differential-test-bed/logs/l0-20261015T023755Z/` | 98 | 4 | 0 | 0 | 30 | 98 | 4 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261015T024616Z/` | 81 | 4 | 0 | 0 | 30 | 4148 | 4 | 0 | 0 | 30 | -4067 | 0 | 0 | 0 | 0 |
| `-pve` | `differential-test-bed/logs/l0-20261015T025613Z/` | 785 | 10 | 88 | 88 | 30 | 3932 | 10 | 86 | 86 | 30 | -3147 | 0 | +2 | +2 | 0 |

(S3 totals and S3 run dirs per mode are quoted from the S3 modes.md for reference:
S3 run dirs `-pv` `l0-20261015T005937Z/`, `-pvu` `l0-20261015T010549Z/`,
`-pvuD` `l0-20261015T011152Z/`, `-pvuDN` `l0-20261015T012001Z/`, `-pve` `l0-20261015T012855Z/`.
The operative delta in this S4 re-measure is S4 vs S3.)

Notes: `-pv`, `-pvu` and `-pvuD` are per-probe identical to S3 in the counted
categories (0 probes up, 0 down). `-pvuDN` drops from 4148 to 81 counted missing
(0 up, 81 down — every affected probe is now down to a single `missing=1`,
except `kde-apps_kdecore-meta` at `missing=1 extra=4`). `-pve` drops from 3932 to
785 counted missing (1 up, 87 down).
Context (not counted): S4 clean probes 56/59/18/12/3
(S3: 54/59/18/12/3; the two newly clean probes in `-pv` are `dev-db_sqlite` and
`sys-libs_zlib`, each of which loses a single non-counted `[use]` finding —
`-pv` counted totals are unchanged).
explained findings 11/11/11/8/8 (identical to S3 in every mode).
The `-pve` `version`/`action` +2 is not new breakage: `dev-db_sqlite` (S3
`missing=311`, no version/action) and `sys-libs_zlib` (S3 `missing=320`, no
version/action) each gain the usual `sys-apps/portage` version/action pair while
shedding ~300 missing findings each (both now `missing=9 version=1 action=1`).

## Probes whose counted totals went UP versus S3 (name, S3 total, S4 total)

Per-probe totals are the sums of the five counted categories from the
`## counts by probe` section of each run's `counts.txt`; S3 per-probe totals from
`counts-{pv,pvu,pvuD,pvuDN,pve}.txt` in `LLM/evidence/2026-10-15-34-s3/`.

### -pv (`differential-test-bed/logs/l0-20261015T022535Z/`)

No probe went UP vs S3.

### -pvu (`differential-test-bed/logs/l0-20261015T023148Z/`)

No probe went UP vs S3.

### -pvuD (`differential-test-bed/logs/l0-20261015T023755Z/`)

No probe went UP vs S3.

### -pvuDN (`differential-test-bed/logs/l0-20261015T024616Z/`)

No probe went UP vs S3.

81 probe(s) went DOWN vs S3 (context, not required):

| probe | S3 total | S4 total | S4 breakdown |
|---|---|---|---|
| `_dev-lang_python-3.15` | 56 | 1 | missing=1 |
| `_system` | 65 | 1 | missing=1 |
| `_world` | 65 | 1 | missing=1 |
| `app-admin_perl-cleaner` | 56 | 1 | missing=1 |
| `app-admin_sudo` | 56 | 1 | missing=1 |
| `app-crypt_gnupg` | 56 | 1 | missing=1 |
| `app-editors_emacs` | 56 | 1 | missing=1 |
| `app-editors_neovim` | 40 | 1 | missing=1 |
| `app-editors_vim` | 45 | 1 | missing=1 |
| `app-misc_tmux` | 56 | 1 | missing=1 |
| `app-portage_eix` | 56 | 1 | missing=1 |
| `app-portage_gentoolkit` | 56 | 1 | missing=1 |
| `app-shells_starship` | 56 | 1 | missing=1 |
| `app-text_texlive` | 24 | 1 | missing=1 |
| `app-text_texlive-core` | 24 | 1 | missing=1 |
| `dev-build_autoconf` | 56 | 1 | missing=1 |
| `dev-build_automake` | 56 | 1 | missing=1 |
| `dev-build_cmake` | 56 | 1 | missing=1 |
| `dev-build_libtool` | 56 | 1 | missing=1 |
| `dev-build_meson` | 56 | 1 | missing=1 |
| `dev-db_mariadb` | 56 | 1 | missing=1 |
| `dev-db_postgresql` | 55 | 1 | missing=1 |
| `dev-db_sqlite` | 56 | 1 | missing=1 |
| `dev-lang_perl` | 56 | 1 | missing=1 |
| `dev-lang_python_3.14` | 56 | 1 | missing=1 |
| `dev-lang_ruby` | 56 | 1 | missing=1 |
| `dev-lang_rust` | 56 | 1 | missing=1 |
| `dev-lang_rust-bin` | 56 | 1 | missing=1 |
| `dev-libs_boost` | 55 | 1 | missing=1 |
| `dev-libs_glib` | 40 | 1 | missing=1 |
| `dev-libs_icu` | 55 | 1 | missing=1 |
| `dev-perl_Locale-gettext` | 56 | 1 | missing=1 |
| `dev-perl_XML-Parser` | 56 | 1 | missing=1 |
| `dev-python_certifi` | 56 | 1 | missing=1 |
| `dev-python_cython` | 56 | 1 | missing=1 |
| `dev-python_jinja2` | 56 | 1 | missing=1 |
| `dev-python_lxml` | 56 | 1 | missing=1 |
| `dev-python_pip` | 55 | 1 | missing=1 |
| `dev-python_pygments` | 55 | 1 | missing=1 |
| `dev-python_pyyaml` | 56 | 1 | missing=1 |
| `dev-python_setuptools` | 56 | 1 | missing=1 |
| `dev-qt_qtbase_6` | 25 | 1 | missing=1 |
| `dev-ruby_rake` | 56 | 1 | missing=1 |
| `dev-texlive_texlive-latex` | 24 | 1 | missing=1 |
| `kde-apps_kdecore-meta` | 24 | 5 | missing=1 extra=4 |
| `kde-frameworks_kcoreaddons` | 24 | 1 | missing=1 |
| `media-libs_freetype` | 56 | 1 | missing=1 |
| `media-libs_gstreamer` | 37 | 1 | missing=1 |
| `media-libs_libpng` | 56 | 1 | missing=1 |
| `media-libs_mesa` | 46 | 1 | missing=1 |
| `media-video_ffmpeg` | 27 | 1 | missing=1 |
| `media-video_vlc` | 24 | 1 | missing=1 |
| `net-libs_nodejs` | 53 | 1 | missing=1 |
| `net-misc_networkmanager` | 25 | 1 | missing=1 |
| `net-misc_openssh` | 56 | 1 | missing=1 |
| `sys-apps_bat` | 56 | 1 | missing=1 |
| `sys-apps_dbus` | 56 | 1 | missing=1 |
| `sys-apps_fd` | 56 | 1 | missing=1 |
| `sys-apps_pciutils` | 56 | 1 | missing=1 |
| `sys-apps_portage` | 56 | 1 | missing=1 |
| `sys-apps_ripgrep` | 56 | 1 | missing=1 |
| `sys-apps_systemd` | 56 | 1 | missing=1 |
| `sys-apps_systemd-utils` | 56 | 1 | missing=1 |
| `sys-apps_util-linux` | 56 | 1 | missing=1 |
| `sys-auth_polkit` | 33 | 1 | missing=1 |
| `sys-devel_binutils` | 56 | 1 | missing=1 |
| `sys-devel_gcc` | 56 | 1 | missing=1 |
| `sys-fs_lvm2` | 56 | 1 | missing=1 |
| `sys-libs_glibc` | 56 | 1 | missing=1 |
| `sys-libs_glibc_multilib_` | 56 | 1 | missing=1 |
| `sys-libs_zlib` | 56 | 1 | missing=1 |
| `sys-process_htop` | 56 | 1 | missing=1 |
| `virtual_cron` | 56 | 1 | missing=1 |
| `virtual_jpeg` | 54 | 1 | missing=1 |
| `virtual_libc` | 56 | 1 | missing=1 |
| `virtual_libudev` | 56 | 1 | missing=1 |
| `virtual_mta` | 56 | 1 | missing=1 |
| `virtual_pager` | 56 | 1 | missing=1 |
| `virtual_perl-ExtUtils-MakeMaker` | 56 | 1 | missing=1 |
| `www-servers_apache` | 55 | 1 | missing=1 |
| `x11-base_xorg-server` | 43 | 1 | missing=1 |

### -pve (`differential-test-bed/logs/l0-20261015T025613Z/`)

1 probe(s) went UP vs S3:

| probe | S3 total | S4 total | S4 breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 18 | 20 | missing=9 extra=9 version=1 action=1 |

87 probe(s) went DOWN vs S3 (context, not required):

| probe | S3 total | S4 total | S4 breakdown |
|---|---|---|---|
| `_dev-lang_python-3.15` | 45 | 11 | missing=9 version=1 action=1 |
| `_dev-libs_libevent-2.1.12` | 45 | 11 | missing=9 version=1 action=1 |
| `_system` | 50 | 11 | missing=9 version=1 action=1 |
| `_world` | 50 | 11 | missing=9 version=1 action=1 |
| `app-admin_perl-cleaner` | 45 | 11 | missing=9 version=1 action=1 |
| `app-admin_sudo` | 45 | 11 | missing=9 version=1 action=1 |
| `app-alternatives_awk` | 45 | 11 | missing=9 version=1 action=1 |
| `app-crypt_gnupg` | 45 | 11 | missing=9 version=1 action=1 |
| `app-editors_emacs` | 46 | 12 | missing=9 extra=1 version=1 action=1 |
| `app-editors_neovim` | 29 | 11 | missing=9 version=1 action=1 |
| `app-editors_vim` | 34 | 11 | missing=9 version=1 action=1 |
| `app-misc_tmux` | 45 | 11 | missing=9 version=1 action=1 |
| `app-portage_eix` | 45 | 11 | missing=9 version=1 action=1 |
| `app-portage_gentoolkit` | 45 | 11 | missing=9 version=1 action=1 |
| `app-shells_starship` | 45 | 11 | missing=9 version=1 action=1 |
| `app-text_texlive` | 13 | 11 | missing=9 version=1 action=1 |
| `app-text_texlive-core` | 13 | 11 | missing=9 version=1 action=1 |
| `dev-build_autoconf` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-build_automake` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-build_cmake` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-build_libtool` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-build_make` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-build_meson` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-build_ninja` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-db_mariadb` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-db_postgresql` | 44 | 11 | missing=9 version=1 action=1 |
| `dev-db_sqlite` | 311 | 11 | missing=9 version=1 action=1 |
| `dev-lang_perl` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-lang_python_3.14` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-lang_ruby` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-lang_rust` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-lang_rust-bin` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-libs_boost` | 44 | 11 | missing=9 version=1 action=1 |
| `dev-libs_glib` | 29 | 11 | missing=9 version=1 action=1 |
| `dev-libs_icu` | 44 | 11 | missing=9 version=1 action=1 |
| `dev-libs_openssl` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-perl_Locale-gettext` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-perl_XML-Parser` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-python_certifi` | 45 | 10 | missing=8 version=1 action=1 |
| `dev-python_cython` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-python_jinja2` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-python_lxml` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-python_pip` | 44 | 5 | missing=3 version=1 action=1 |
| `dev-python_pygments` | 44 | 11 | missing=9 version=1 action=1 |
| `dev-python_pyyaml` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-python_setuptools` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-qt_qtbase_6` | 14 | 11 | missing=9 version=1 action=1 |
| `dev-ruby_rake` | 45 | 11 | missing=9 version=1 action=1 |
| `dev-texlive_texlive-latex` | 13 | 11 | missing=9 version=1 action=1 |
| `kde-frameworks_kcoreaddons` | 13 | 11 | missing=9 version=1 action=1 |
| `media-libs_freetype` | 45 | 11 | missing=9 version=1 action=1 |
| `media-libs_gstreamer` | 26 | 11 | missing=9 version=1 action=1 |
| `media-libs_libpng` | 45 | 11 | missing=9 version=1 action=1 |
| `media-libs_mesa` | 35 | 11 | missing=9 version=1 action=1 |
| `media-video_ffmpeg` | 16 | 11 | missing=9 version=1 action=1 |
| `media-video_vlc` | 13 | 11 | missing=9 version=1 action=1 |
| `net-libs_nodejs` | 42 | 11 | missing=9 version=1 action=1 |
| `net-misc_networkmanager` | 13 | 11 | missing=9 version=1 action=1 |
| `net-misc_openssh` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_bat` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_coreutils` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_dbus` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_fd` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_pciutils` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_portage` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_ripgrep` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_systemd` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_systemd-utils` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-apps_util-linux` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-auth_polkit` | 22 | 11 | missing=9 version=1 action=1 |
| `sys-devel_binutils` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-devel_gcc` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-fs_lvm2` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-libs_glibc` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-libs_glibc_multilib_` | 45 | 11 | missing=9 version=1 action=1 |
| `sys-libs_zlib` | 320 | 11 | missing=9 version=1 action=1 |
| `sys-process_htop` | 45 | 11 | missing=9 version=1 action=1 |
| `virtual_cron` | 45 | 11 | missing=9 version=1 action=1 |
| `virtual_editor` | 45 | 11 | missing=9 version=1 action=1 |
| `virtual_jpeg` | 43 | 11 | missing=9 version=1 action=1 |
| `virtual_libc` | 45 | 11 | missing=9 version=1 action=1 |
| `virtual_libudev` | 45 | 11 | missing=9 version=1 action=1 |
| `virtual_mta` | 45 | 11 | missing=9 version=1 action=1 |
| `virtual_pager` | 45 | 11 | missing=9 version=1 action=1 |
| `virtual_perl-ExtUtils-MakeMaker` | 45 | 11 | missing=9 version=1 action=1 |
| `www-servers_apache` | 44 | 11 | missing=9 version=1 action=1 |
| `x11-base_xorg-server` | 32 | 11 | missing=9 version=1 action=1 |
## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261015T030838Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0 — equal to the S3 default run (`l0-20261015T014043Z`: 0/448/0/0/0).
Context (not counted): clean probes 14, explained findings 0 (same as S3).

No probe's counted total changed vs the S3 default run (0 up, 0 down).

Per-probe fingerprints of the portuale side: `diff -rq` of the `portuale/` output
directories between the S3 and S4 default runs reports no differences — the
portuale side is byte-identical file-for-file (117/117). `fingerprint.tsv` differs
only in the `date_utc` line (`2026-10-15T01:40:43Z` vs `2026-10-15T03:08:38Z`).
The real (Portage) side differs in 103 files; every file was checked with the
`Dependency resolution took ...s (backtrack: ...)` timing line excluded and no
file has any other difference (0/117 with a non-timing diff).

Per-mode full counts: `counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`,
`counts-pvuDN.txt`, `counts-pve.txt`, `counts-default.txt` in this directory (copies
of each run dir's `counts.txt`, which also holds the `## counts by probe` section).
