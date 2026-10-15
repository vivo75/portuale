# Task #34 S5a re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `22ba8c3d527852fb434d5c45b0d30d77a23684a0`, dirty
(`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s5/`; S5a change in place:
G1; binary below already contains it).
pmtest HEAD: `bd22d426f08ec8c887c5e6dd66cb6f46e9c5a278`
(`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-15 04:03 UTC (before the runs;
already contains the S5a change; the runner's own rebuild said
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
`b4b5d17a`, same VDB snapshot (320 installed packages), same image as the S2/S3/S4 runs
(headers of all six S5a `counts.txt` confirm the tree/overlay hashes).

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S5a run dir | S5a missing | S5a extra | S5a version | S5a action | S5a exit | S4 missing | S4 extra | S4 version | S4 action | S4 exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261015T040457Z/` | 23 | 5 | 0 | 0 | 30 | 24 | 5 | 0 | 0 | 30 | -1 | 0 | 0 | 0 | 0 |
| `-pvu` | `differential-test-bed/logs/l0-20261015T041110Z/` | 23 | 5 | 0 | 0 | 30 | 24 | 5 | 0 | 0 | 30 | -1 | 0 | 0 | 0 | 0 |
| `-pvuD` | `differential-test-bed/logs/l0-20261015T041711Z/` | 23 | 4 | 0 | 0 | 30 | 98 | 4 | 0 | 0 | 30 | -75 | 0 | 0 | 0 | 0 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261015T042557Z/` | 0 | 4 | 0 | 0 | 30 | 81 | 4 | 0 | 0 | 30 | -81 | 0 | 0 | 0 | 0 |
| `-pve` | `differential-test-bed/logs/l0-20261015T043546Z/` | 0 | 10 | 0 | 0 | 30 | 785 | 10 | 88 | 88 | 30 | -785 | 0 | -88 | -88 | 0 |

(S4 totals and S4 run dirs per mode are quoted from the S4 modes.md for reference:
S4 run dirs `-pv` `l0-20261015T022535Z/`, `-pvu` `l0-20261015T023148Z/`,
`-pvuD` `l0-20261015T023755Z/`, `-pvuDN` `l0-20261015T024616Z/`, `-pve` `l0-20261015T025613Z/`.
The operative delta in this S5a re-measure is S5a vs S4.)

Notes: no probe went UP in any mode (0 up in all five; see per-mode sections).
`-pv`/`-pvu`: the single drop is `sys-apps_portage` `missing=1` → 0 (1 down, 0 up).
`-pvuD`: 0 up, 75 down — the remaining counted findings are exactly
`missing=23` (`dev-lang_rust` 4, `kde-apps_kdecore-meta` 7, `kde-frameworks_kcoreaddons` 5,
`media-video_vlc` 5, `net-misc_networkmanager` 1, `sys-auth_polkit` 1) plus
`extra=4` (`kde-apps_kdecore-meta` 4).
`-pvuDN`: 0 up, 81 down — every S4 counted `missing` is gone; the only remaining
counted findings are `kde-apps_kdecore-meta` `extra=4` (S4 was `missing=1 extra=4`,
now `extra=4`) plus unchanged `exit=30`.
`-pve`: 0 up, 88 down — all S4 counted `missing`/`version`/`action` are gone; the only
remaining counted findings are `kde-apps_kdecore-meta` `extra=9` (S4 was
`missing=9 extra=9 version=1 action=1`) and `app-editors_emacs` `extra=1`, plus
unchanged `exit=30`.
Context (not counted): S5a clean probes 57/60/64/12/81
(S4: 56/59/18/12/3).
explained findings 11/11/11/8/8 (identical to S4 in every mode).

## Probes whose counted totals went UP versus S4 (name, S4 total, S5a total)

Per-probe totals are the sums of the five counted categories from the
`## counts by probe` section of each run's `counts.txt`; S4 per-probe totals from
`counts-{pv,pvu,pvuD,pvuDN,pve}.txt` in `LLM/evidence/2026-10-15-34-s4/`.

### -pv (`differential-test-bed/logs/l0-20261015T040457Z/`)

No probe went UP vs S4.

1 probe went DOWN vs S4 (context, not required):

| probe | S4 total | S5a total | S5a breakdown |
|---|---|---|---|
| `sys-apps_portage` | 1 | 0 | — (no counted findings) |

### -pvu (`differential-test-bed/logs/l0-20261015T041110Z/`)

No probe went UP vs S4.

1 probe went DOWN vs S4 (context, not required):

| probe | S4 total | S5a total | S5a breakdown |
|---|---|---|---|
| `sys-apps_portage` | 1 | 0 | — (no counted findings) |

### -pvuD (`differential-test-bed/logs/l0-20261015T041711Z/`)

No probe went UP vs S4.

75 probes went DOWN vs S4 (context, not required): 69 probes `1` → `0`
(`_dev-lang_python-3.15`, `_system`, `_world`, `app-admin_perl-cleaner`,
`app-admin_sudo`, `app-crypt_gnupg`, `app-editors_emacs`, `app-editors_neovim`,
`app-editors_vim`, `app-misc_tmux`, `app-portage_eix`, `app-portage_gentoolkit`,
`app-shells_starship`, `app-text_texlive`, `app-text_texlive-core`,
`dev-build_autoconf`, `dev-build_automake`, `dev-build_libtool`, `dev-build_meson`,
`dev-db_mariadb`, `dev-db_postgresql`, `dev-lang_perl`, `dev-lang_python_3.14`,
`dev-lang_ruby`, `dev-lang_rust-bin`, `dev-libs_boost`, `dev-libs_glib`, `dev-libs_icu`,
`dev-perl_Locale-gettext`, `dev-perl_XML-Parser`, `dev-python_certifi`,
`dev-python_cython`, `dev-python_jinja2`, `dev-python_lxml`, `dev-python_pip`,
`dev-python_pygments`, `dev-python_pyyaml`, `dev-python_setuptools`,
`dev-qt_qtbase_6`, `dev-ruby_rake`, `dev-texlive_texlive-latex`, `media-libs_freetype`,
`media-libs_gstreamer`, `media-libs_mesa`, `media-video_ffmpeg`, `net-libs_nodejs`,
`net-misc_openssh`, `sys-apps_bat`, `sys-apps_dbus`, `sys-apps_fd`, `sys-apps_pciutils`,
`sys-apps_portage`, `sys-apps_ripgrep`, `sys-apps_systemd`, `sys-apps_systemd-utils`,
`sys-apps_util-linux`, `sys-devel_gcc`, `sys-fs_lvm2`, `sys-libs_glibc`,
`sys-libs_glibc_multilib_`, `sys-process_htop`, `virtual_cron`, `virtual_jpeg`,
`virtual_libc`, `virtual_libudev`, `virtual_mta`, `virtual_perl-ExtUtils-MakeMaker`,
`www-servers_apache`, `x11-base_xorg-server`), plus:

| probe | S4 total | S5a total | S5a breakdown |
|---|---|---|---|
| `dev-lang_rust` | 5 | 4 | missing=4 |
| `kde-apps_kdecore-meta` | 12 | 11 | missing=7 extra=4 |
| `kde-frameworks_kcoreaddons` | 6 | 5 | missing=5 |
| `media-video_vlc` | 6 | 5 | missing=5 |
| `net-misc_networkmanager` | 2 | 1 | missing=1 |
| `sys-auth_polkit` | 2 | 1 | missing=1 |

(69 × 1 + 6 × 1 = 75, exactly the missing delta 98 − 23.)

### -pvuDN (`differential-test-bed/logs/l0-20261015T042557Z/`)

No probe went UP vs S4.

81 probes went DOWN vs S4 (context, not required): 80 probes `1` → `0`
(`_dev-lang_python-3.15`, `_system`, `_world`, `app-admin_perl-cleaner`,
`app-admin_sudo`, `app-crypt_gnupg`, `app-editors_emacs`, `app-editors_neovim`,
`app-editors_vim`, `app-misc_tmux`, `app-portage_eix`, `app-portage_gentoolkit`,
`app-shells_starship`, `app-text_texlive`, `app-text_texlive-core`,
`dev-build_autoconf`, `dev-build_automake`, `dev-build_cmake`, `dev-build_libtool`,
`dev-build_meson`, `dev-db_mariadb`, `dev-db_postgresql`, `dev-db_sqlite`,
`dev-lang_perl`, `dev-lang_python_3.14`, `dev-lang_ruby`, `dev-lang_rust`,
`dev-lang_rust-bin`, `dev-libs_boost`, `dev-libs_glib`, `dev-libs_icu`,
`dev-perl_Locale-gettext`, `dev-perl_XML-Parser`, `dev-python_certifi`,
`dev-python_cython`, `dev-python_jinja2`, `dev-python_lxml`, `dev-python_pip`,
`dev-python_pygments`, `dev-python_pyyaml`, `dev-python_setuptools`,
`dev-qt_qtbase_6`, `dev-ruby_rake`, `dev-texlive_texlive-latex`,
`kde-frameworks_kcoreaddons`, `media-libs_freetype`, `media-libs_gstreamer`,
`media-libs_libpng`, `media-libs_mesa`, `media-video_ffmpeg`, `media-video_vlc`,
`net-libs_nodejs`, `net-misc_networkmanager`, `net-misc_openssh`, `sys-apps_bat`,
`sys-apps_dbus`, `sys-apps_fd`, `sys-apps_pciutils`, `sys-apps_portage`,
`sys-apps_ripgrep`, `sys-apps_systemd`, `sys-apps_systemd-utils`,
`sys-apps_util-linux`, `sys-auth_polkit`, `sys-devel_binutils`, `sys-devel_gcc`,
`sys-fs_lvm2`, `sys-libs_glibc`, `sys-libs_glibc_multilib_`, `sys-libs_zlib`,
`sys-process_htop`, `virtual_cron`, `virtual_jpeg`, `virtual_libc`,
`virtual_libudev`, `virtual_mta`, `virtual_pager`, `virtual_perl-ExtUtils-MakeMaker`,
`www-servers_apache`, `x11-base_xorg-server`), plus:

| probe | S4 total | S5a total | S5a breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 5 | 4 | extra=4 |

(S4 `kde-apps_kdecore-meta` was `missing=1 extra=4`; the `missing=1` is gone.)

### -pve (`differential-test-bed/logs/l0-20261015T043546Z/`)

No probe went UP vs S4.

88 probes went DOWN vs S4 (context, not required): all remaining counted
`missing`/`version`/`action` findings are gone. Every down probe went to `0`
except:

| probe | S4 total | S5a total | S5a breakdown |
|---|---|---|---|
| `app-editors_emacs` | 12 | 1 | extra=1 |
| `kde-apps_kdecore-meta` | 20 | 9 | extra=9 |

(S4 `kde-apps_kdecore-meta` was `missing=9 extra=9 version=1 action=1`; S4
`app-editors_emacs` was `missing=9 extra=1 version=1 action=1`. All other 86
down probes went to `0`, including `dev-db_sqlite` `11` → `0`, `sys-libs_zlib`
`11` → `0`, `dev-python_certifi` `10` → `0`, `dev-python_pip` `5` → `0`, and
eighty-two `11` → `0` probes.)

## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261015T044752Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0 — equal to the S4 default run (`l0-20261015T030838Z`: 0/448/0/0/0).
Context (not counted): clean probes 14, explained findings 0 (same as S4).

No probe's counted total changed vs the S4 default run (0 up, 0 down).

Per-probe fingerprints of the portuale side: `diff -rq` of the `portuale/` output
directories between the S4 and S5a default runs reports no differences — the
portuale side is byte-identical file-for-file (117/117). `fingerprint.tsv` differs
only in the `date_utc` line (`2026-10-15T03:08:38Z` vs `2026-10-15T04:47:53Z`).
The real (Portage) side differs in 105 files; every file was checked with the
`Dependency resolution took ...s (backtrack: ...)` timing line excluded and no
file has any other difference (0/117 with a non-timing diff).

Per-mode full counts: `counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`,
`counts-pvuDN.txt`, `counts-pve.txt`, `counts-default.txt` in this directory (copies
of each run dir's `counts.txt`, which also holds the `## counts by probe` section).
