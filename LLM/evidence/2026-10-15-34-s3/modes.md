# Task S3 re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `942a0ebd3281af75d4d505de9d8993ddd573016b`, dirty
(`M LLM/backlog-tasks-2026-10.md`,
`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s3/`; S3 change in progress).
pmtest HEAD: `f4cbd929f349d579bae1e52cf927eb14eae37b8f`
(`M fixtures/etc/portage/package.use`,
`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-15 00:56 UTC (before the runs;
already contains the S3 change; no cargo build/test was run during the bed runs).
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
`b4b5d17a`, same VDB snapshot (320 installed packages), same image as the S2 runs
(headers of all six S3 `counts.txt` confirm the tree/overlay hashes).

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S3 run dir | S3 missing | S3 extra | S3 version | S3 action | S3 exit | S2 missing | S2 extra | S2 version | S2 action | S2 exit | delta missing | delta extra | delta version | delta action | delta exit | S1 missing | S1 extra | S1 version | S1 action | S1 exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261015T005937Z/` | 24 | 5 | 0 | 0 | 30 | 24 | 5 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 | 204 | 1589 | 0 | 0 | 30 |
| `-pvu` | `differential-test-bed/logs/l0-20261015T010549Z/` | 24 | 5 | 0 | 0 | 30 | 24 | 5 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 | 9 | 1608 | 0 | 0 | 30 |
| `-pvuD` | `differential-test-bed/logs/l0-20261015T011152Z/` | 98 | 4 | 0 | 0 | 30 | 83 | 2903 | 0 | 0 | 30 | +15 | -2899 | 0 | 0 | 0 | 83 | 1608 | 0 | 0 | 30 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261015T012001Z/` | 4148 | 4 | 0 | 0 | 30 | 1234 | 4 | 0 | 0 | 30 | +2914 | 0 | 0 | 0 | 0 | 2529 | 4 | 0 | 0 | 30 |
| `-pve` | `differential-test-bed/logs/l0-20261015T012855Z/` | 3932 | 10 | 86 | 86 | 30 | 2044 | 10 | 0 | 0 | 30 | +1888 | 0 | +86 | +86 | 0 | 26363 | 4 | 0 | 0 | 12 |

(S1 totals and S1 run dirs per mode are quoted from the S2 modes.md for reference:
S1 run dirs `-pv` `l0-20261014T212554Z/`, `-pvu` `l0-20261014T213106Z/`,
`-pvuD` `l0-20261014T213610Z/`, `-pvuDN` `l0-20261014T214249Z/`, `-pve` `l0-20261014T215013Z/`.
The operative delta in this S3 re-measure is S3 vs S2.)

Notes: `-pv` and `-pvu` are per-probe identical to S2 (0 probes up, 0 down).
Context (not counted): S3 clean probes 54/59/18/12/3 (S2: 54/59/15/12/3);
explained findings 11/11/11/8/8 (identical to S2 in every mode).
`-pve` `version`/`action` findings are new in S3 (86 each, one pair per affected
probe): e.g. `_system` has `[version] sys-apps/portage: real 3.0.81.3 vs
portuale 3.0.82.2` and `[action] sys-apps/portage: real action [UD] vs
portuale [R]`.

## Probes whose counted totals went UP versus S2 (name, S2 total, S3 total)

Per-probe totals are the sums of the five counted categories from the
`## counts by probe` section of each run's `counts.txt`; S2 per-probe totals from
`counts-{pv,pvu,pvuD,pvuDN,pve}.txt` in `LLM/evidence/2026-10-15-34-s2/`.

### -pv (`differential-test-bed/logs/l0-20261015T005937Z/`)

No probe went UP vs S2.

### -pvu (`differential-test-bed/logs/l0-20261015T010549Z/`)

No probe went UP vs S2.

### -pvuD (`differential-test-bed/logs/l0-20261015T011152Z/`)

1 probe(s) went UP vs S2:

| probe | S2 total | S3 total | S3 breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 11 | 12 | extra=4 missing=8 |

76 probe(s) went DOWN vs S2 (context, not required):

| probe | S2 total | S3 total | S3 breakdown |
|---|---|---|---|
| `_dev-lang_python-3.15` | 44 | 1 | missing=1 |
| `_system` | 49 | 1 | missing=1 |
| `_world` | 49 | 1 | missing=1 |
| `app-admin_perl-cleaner` | 44 | 1 | missing=1 |
| `app-admin_sudo` | 44 | 1 | missing=1 |
| `app-crypt_gnupg` | 44 | 1 | missing=1 |
| `app-editors_emacs` | 44 | 1 | missing=1 |
| `app-editors_neovim` | 28 | 1 | missing=1 |
| `app-editors_vim` | 33 | 1 | missing=1 |
| `app-misc_tmux` | 44 | 1 | missing=1 |
| `app-portage_eix` | 44 | 1 | missing=1 |
| `app-portage_gentoolkit` | 44 | 1 | missing=1 |
| `app-shells_starship` | 44 | 1 | missing=1 |
| `app-text_texlive` | 12 | 1 | missing=1 |
| `app-text_texlive-core` | 12 | 1 | missing=1 |
| `dev-build_autoconf` | 44 | 1 | missing=1 |
| `dev-build_automake` | 44 | 1 | missing=1 |
| `dev-build_cmake` | 43 | 0 | - |
| `dev-build_libtool` | 44 | 1 | missing=1 |
| `dev-build_meson` | 44 | 1 | missing=1 |
| `dev-db_mariadb` | 44 | 1 | missing=1 |
| `dev-db_postgresql` | 43 | 1 | missing=1 |
| `dev-db_sqlite` | 43 | 0 | - |
| `dev-lang_perl` | 44 | 1 | missing=1 |
| `dev-lang_python_3.14` | 44 | 1 | missing=1 |
| `dev-lang_ruby` | 44 | 1 | missing=1 |
| `dev-lang_rust` | 42 | 5 | missing=5 |
| `dev-lang_rust-bin` | 44 | 1 | missing=1 |
| `dev-libs_boost` | 43 | 1 | missing=1 |
| `dev-libs_glib` | 28 | 1 | missing=1 |
| `dev-libs_icu` | 43 | 1 | missing=1 |
| `dev-perl_Locale-gettext` | 44 | 1 | missing=1 |
| `dev-perl_XML-Parser` | 44 | 1 | missing=1 |
| `dev-python_certifi` | 44 | 1 | missing=1 |
| `dev-python_cython` | 44 | 1 | missing=1 |
| `dev-python_jinja2` | 44 | 1 | missing=1 |
| `dev-python_lxml` | 44 | 1 | missing=1 |
| `dev-python_pip` | 43 | 1 | missing=1 |
| `dev-python_pygments` | 43 | 1 | missing=1 |
| `dev-python_pyyaml` | 44 | 1 | missing=1 |
| `dev-python_setuptools` | 44 | 1 | missing=1 |
| `dev-qt_qtbase_6` | 13 | 1 | missing=1 |
| `dev-ruby_rake` | 44 | 1 | missing=1 |
| `dev-texlive_texlive-latex` | 12 | 1 | missing=1 |
| `kde-frameworks_kcoreaddons` | 9 | 6 | missing=6 |
| `media-libs_freetype` | 44 | 1 | missing=1 |
| `media-libs_gstreamer` | 25 | 1 | missing=1 |
| `media-libs_mesa` | 34 | 1 | missing=1 |
| `media-video_ffmpeg` | 15 | 1 | missing=1 |
| `media-video_vlc` | 9 | 6 | missing=6 |
| `net-libs_nodejs` | 41 | 1 | missing=1 |
| `net-misc_networkmanager` | 13 | 2 | missing=2 |
| `net-misc_openssh` | 44 | 1 | missing=1 |
| `sys-apps_bat` | 44 | 1 | missing=1 |
| `sys-apps_dbus` | 44 | 1 | missing=1 |
| `sys-apps_fd` | 44 | 1 | missing=1 |
| `sys-apps_pciutils` | 44 | 1 | missing=1 |
| `sys-apps_ripgrep` | 44 | 1 | missing=1 |
| `sys-apps_systemd` | 44 | 1 | missing=1 |
| `sys-apps_systemd-utils` | 44 | 1 | missing=1 |
| `sys-apps_util-linux` | 44 | 1 | missing=1 |
| `sys-auth_polkit` | 22 | 2 | missing=2 |
| `sys-devel_binutils` | 43 | 0 | - |
| `sys-devel_gcc` | 44 | 1 | missing=1 |
| `sys-fs_lvm2` | 44 | 1 | missing=1 |
| `sys-libs_glibc` | 44 | 1 | missing=1 |
| `sys-libs_glibc_multilib_` | 44 | 1 | missing=1 |
| `sys-process_htop` | 44 | 1 | missing=1 |
| `virtual_cron` | 44 | 1 | missing=1 |
| `virtual_jpeg` | 42 | 1 | missing=1 |
| `virtual_libc` | 44 | 1 | missing=1 |
| `virtual_libudev` | 44 | 1 | missing=1 |
| `virtual_mta` | 44 | 1 | missing=1 |
| `virtual_perl-ExtUtils-MakeMaker` | 44 | 1 | missing=1 |
| `www-servers_apache` | 43 | 1 | missing=1 |
| `x11-base_xorg-server` | 31 | 1 | missing=1 |

### -pvuDN (`differential-test-bed/logs/l0-20261015T012001Z/`)

77 probe(s) went UP vs S2:

| probe | S2 total | S3 total | S3 breakdown |
|---|---|---|---|
| `_dev-lang_python-3.15` | 13 | 56 | missing=56 |
| `_system` | 17 | 65 | missing=65 |
| `_world` | 17 | 65 | missing=65 |
| `app-admin_perl-cleaner` | 13 | 56 | missing=56 |
| `app-admin_sudo` | 13 | 56 | missing=56 |
| `app-crypt_gnupg` | 13 | 56 | missing=56 |
| `app-editors_emacs` | 13 | 56 | missing=56 |
| `app-editors_neovim` | 13 | 40 | missing=40 |
| `app-editors_vim` | 13 | 45 | missing=45 |
| `app-misc_tmux` | 13 | 56 | missing=56 |
| `app-portage_eix` | 13 | 56 | missing=56 |
| `app-portage_gentoolkit` | 13 | 56 | missing=56 |
| `app-shells_starship` | 13 | 56 | missing=56 |
| `app-text_texlive` | 13 | 24 | missing=24 |
| `app-text_texlive-core` | 13 | 24 | missing=24 |
| `dev-build_autoconf` | 13 | 56 | missing=56 |
| `dev-build_automake` | 13 | 56 | missing=56 |
| `dev-build_cmake` | 13 | 56 | missing=56 |
| `dev-build_libtool` | 13 | 56 | missing=56 |
| `dev-build_meson` | 13 | 56 | missing=56 |
| `dev-db_mariadb` | 13 | 56 | missing=56 |
| `dev-db_postgresql` | 13 | 55 | missing=55 |
| `dev-db_sqlite` | 13 | 56 | missing=56 |
| `dev-lang_perl` | 13 | 56 | missing=56 |
| `dev-lang_python_3.14` | 13 | 56 | missing=56 |
| `dev-lang_ruby` | 13 | 56 | missing=56 |
| `dev-lang_rust` | 13 | 56 | missing=56 |
| `dev-lang_rust-bin` | 13 | 56 | missing=56 |
| `dev-libs_boost` | 13 | 55 | missing=55 |
| `dev-libs_glib` | 13 | 40 | missing=40 |
| `dev-libs_icu` | 13 | 55 | missing=55 |
| `dev-perl_Locale-gettext` | 13 | 56 | missing=56 |
| `dev-perl_XML-Parser` | 13 | 56 | missing=56 |
| `dev-python_certifi` | 13 | 56 | missing=56 |
| `dev-python_cython` | 13 | 56 | missing=56 |
| `dev-python_jinja2` | 13 | 56 | missing=56 |
| `dev-python_lxml` | 13 | 56 | missing=56 |
| `dev-python_pip` | 13 | 55 | missing=55 |
| `dev-python_pygments` | 13 | 55 | missing=55 |
| `dev-python_pyyaml` | 13 | 56 | missing=56 |
| `dev-python_setuptools` | 13 | 56 | missing=56 |
| `dev-qt_qtbase_6` | 13 | 25 | missing=25 |
| `dev-ruby_rake` | 13 | 56 | missing=56 |
| `dev-texlive_texlive-latex` | 13 | 24 | missing=24 |
| `kde-apps_kdecore-meta` | 17 | 24 | extra=4 missing=20 |
| `kde-frameworks_kcoreaddons` | 13 | 24 | missing=24 |
| `media-libs_freetype` | 13 | 56 | missing=56 |
| `media-libs_gstreamer` | 13 | 37 | missing=37 |
| `media-libs_mesa` | 13 | 46 | missing=46 |
| `media-video_ffmpeg` | 13 | 27 | missing=27 |
| `media-video_vlc` | 13 | 24 | missing=24 |
| `net-libs_nodejs` | 13 | 53 | missing=53 |
| `net-misc_networkmanager` | 14 | 25 | missing=25 |
| `net-misc_openssh` | 13 | 56 | missing=56 |
| `sys-apps_bat` | 13 | 56 | missing=56 |
| `sys-apps_dbus` | 13 | 56 | missing=56 |
| `sys-apps_fd` | 13 | 56 | missing=56 |
| `sys-apps_pciutils` | 13 | 56 | missing=56 |
| `sys-apps_ripgrep` | 13 | 56 | missing=56 |
| `sys-apps_systemd` | 13 | 56 | missing=56 |
| `sys-apps_systemd-utils` | 13 | 56 | missing=56 |
| `sys-apps_util-linux` | 13 | 56 | missing=56 |
| `sys-auth_polkit` | 13 | 33 | missing=33 |
| `sys-devel_binutils` | 13 | 56 | missing=56 |
| `sys-devel_gcc` | 13 | 56 | missing=56 |
| `sys-fs_lvm2` | 13 | 56 | missing=56 |
| `sys-libs_glibc` | 13 | 56 | missing=56 |
| `sys-libs_glibc_multilib_` | 13 | 56 | missing=56 |
| `sys-process_htop` | 13 | 56 | missing=56 |
| `virtual_cron` | 13 | 56 | missing=56 |
| `virtual_jpeg` | 13 | 54 | missing=54 |
| `virtual_libc` | 13 | 56 | missing=56 |
| `virtual_libudev` | 13 | 56 | missing=56 |
| `virtual_mta` | 13 | 56 | missing=56 |
| `virtual_perl-ExtUtils-MakeMaker` | 13 | 56 | missing=56 |
| `www-servers_apache` | 13 | 55 | missing=55 |
| `x11-base_xorg-server` | 13 | 43 | missing=43 |

### -pve (`differential-test-bed/logs/l0-20261015T012855Z/`)

78 probe(s) went UP vs S2:

| probe | S2 total | S3 total | S3 breakdown |
|---|---|---|---|
| `_dev-lang_python-3.15` | 20 | 45 | action=1 missing=43 version=1 |
| `_dev-libs_libevent-2.1.12` | 20 | 45 | action=1 missing=43 version=1 |
| `_system` | 15 | 50 | action=1 missing=48 version=1 |
| `_world` | 15 | 50 | action=1 missing=48 version=1 |
| `app-admin_perl-cleaner` | 20 | 45 | action=1 missing=43 version=1 |
| `app-admin_sudo` | 20 | 45 | action=1 missing=43 version=1 |
| `app-alternatives_awk` | 20 | 45 | action=1 missing=43 version=1 |
| `app-crypt_gnupg` | 20 | 45 | action=1 missing=43 version=1 |
| `app-editors_emacs` | 21 | 46 | action=1 extra=1 missing=43 version=1 |
| `app-editors_neovim` | 20 | 29 | action=1 missing=27 version=1 |
| `app-editors_vim` | 20 | 34 | action=1 missing=32 version=1 |
| `app-misc_tmux` | 20 | 45 | action=1 missing=43 version=1 |
| `app-portage_eix` | 20 | 45 | action=1 missing=43 version=1 |
| `app-portage_gentoolkit` | 20 | 45 | action=1 missing=43 version=1 |
| `app-shells_starship` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-build_autoconf` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-build_automake` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-build_cmake` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-build_libtool` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-build_make` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-build_meson` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-build_ninja` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-db_mariadb` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-db_postgresql` | 20 | 44 | action=1 missing=42 version=1 |
| `dev-db_sqlite` | 20 | 311 | missing=311 |
| `dev-lang_perl` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-lang_python_3.14` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-lang_ruby` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-lang_rust` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-lang_rust-bin` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-libs_boost` | 20 | 44 | action=1 missing=42 version=1 |
| `dev-libs_glib` | 20 | 29 | action=1 missing=27 version=1 |
| `dev-libs_icu` | 20 | 44 | action=1 missing=42 version=1 |
| `dev-libs_openssl` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-perl_Locale-gettext` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-perl_XML-Parser` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-python_certifi` | 19 | 45 | action=1 missing=43 version=1 |
| `dev-python_cython` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-python_jinja2` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-python_lxml` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-python_pip` | 14 | 44 | action=1 missing=42 version=1 |
| `dev-python_pygments` | 20 | 44 | action=1 missing=42 version=1 |
| `dev-python_pyyaml` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-python_setuptools` | 20 | 45 | action=1 missing=43 version=1 |
| `dev-ruby_rake` | 20 | 45 | action=1 missing=43 version=1 |
| `media-libs_freetype` | 20 | 45 | action=1 missing=43 version=1 |
| `media-libs_gstreamer` | 20 | 26 | action=1 missing=24 version=1 |
| `media-libs_libpng` | 20 | 45 | action=1 missing=43 version=1 |
| `media-libs_mesa` | 20 | 35 | action=1 missing=33 version=1 |
| `net-libs_nodejs` | 20 | 42 | action=1 missing=40 version=1 |
| `net-misc_openssh` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_bat` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_coreutils` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_dbus` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_fd` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_pciutils` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_ripgrep` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_systemd` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_systemd-utils` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-apps_util-linux` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-auth_polkit` | 20 | 22 | action=1 missing=20 version=1 |
| `sys-devel_binutils` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-devel_gcc` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-fs_lvm2` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-libs_glibc` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-libs_glibc_multilib_` | 20 | 45 | action=1 missing=43 version=1 |
| `sys-libs_zlib` | 20 | 320 | missing=320 |
| `sys-process_htop` | 20 | 45 | action=1 missing=43 version=1 |
| `virtual_cron` | 20 | 45 | action=1 missing=43 version=1 |
| `virtual_editor` | 20 | 45 | action=1 missing=43 version=1 |
| `virtual_jpeg` | 20 | 43 | action=1 missing=41 version=1 |
| `virtual_libc` | 20 | 45 | action=1 missing=43 version=1 |
| `virtual_libudev` | 20 | 45 | action=1 missing=43 version=1 |
| `virtual_mta` | 20 | 45 | action=1 missing=43 version=1 |
| `virtual_pager` | 20 | 45 | action=1 missing=43 version=1 |
| `virtual_perl-ExtUtils-MakeMaker` | 20 | 45 | action=1 missing=43 version=1 |
| `www-servers_apache` | 20 | 44 | action=1 missing=42 version=1 |
| `x11-base_xorg-server` | 20 | 32 | action=1 missing=30 version=1 |

10 probe(s) went DOWN vs S2 (context, not required):

| probe | S2 total | S3 total | S3 breakdown |
|---|---|---|---|
| `app-text_texlive` | 20 | 13 | action=1 missing=11 version=1 |
| `app-text_texlive-core` | 20 | 13 | action=1 missing=11 version=1 |
| `dev-qt_qtbase_6` | 20 | 14 | action=1 missing=12 version=1 |
| `dev-texlive_texlive-latex` | 20 | 13 | action=1 missing=11 version=1 |
| `kde-apps_kdecore-meta` | 29 | 18 | action=1 extra=9 missing=7 version=1 |
| `kde-frameworks_kcoreaddons` | 20 | 13 | action=1 missing=11 version=1 |
| `media-video_ffmpeg` | 20 | 16 | action=1 missing=14 version=1 |
| `media-video_vlc` | 20 | 13 | action=1 missing=11 version=1 |
| `net-misc_networkmanager` | 20 | 13 | action=1 missing=11 version=1 |
| `sys-apps_portage` | 321 | 45 | action=1 missing=43 version=1 |
## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261015T014043Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0 — equal to the S2 default run (`l0-20261014T235819Z`: 0/448/0/0/0).
Context (not counted): clean probes 14, explained findings 0 (same as S2).

No probe's counted total changed vs the S2 default run (0 up, 0 down).

Per-probe fingerprints of the portuale side: `diff -rq` of the `portuale/` output
directories between the S2 and S3 default runs reports no differences — the
portuale side is byte-identical file-for-file (117/117). `fingerprint.tsv` differs
only in the `date_utc` line (`2026-10-14T23:58:19Z` vs `2026-10-15T01:40:43Z`).
The real (Portage) side differs in 75 files, every hunk being only the
`Dependency resolution took ...s (backtrack: ...)` timing line (spot-checked
`_system.txt`: `2.10 s` vs `2.13 s`; all other hunks filtered out by excluding that
line are empty).

Per-mode full counts: `counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`,
`counts-pvuDN.txt`, `counts-pve.txt`, `counts-default.txt` in this directory (copies
of each run dir's `counts.txt`, which also holds the `## counts by probe` section).
