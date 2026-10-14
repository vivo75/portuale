# Task S0 Step 7 — full baseline: 5 modes × corpus, `--solver=pubgrub`

PM HEAD (portuale): `8772601ef5701805cc5d1d6009e4c487f5ca9d9a`, dirty
(`M crates/portage-repo/src/solver_bridge.rs`,
`M LLM/code-notes/portage-repo/solver_bridge.md`; Step 5 trace work in progress).
pmtest HEAD: `9b54dfd1aede3f75bb70b3dd44bdb20ebe44c7f9` (S0 Steps 3–4 bed/comparator
changes uncommitted, as expected pre-Step-8).

Binary: `portuale/target/release/portuale`, built 2026-10-14 09:59 UTC (before the runs).
Image: `localhost/test-portuale:latest` (`5616d41d00cd`), Portage 3.0.82.2.

Each run:
`export GENTOO_MIRRORS=http://10.48.0.229:8080`
`L0_SKIP_MULTI=1 L0_SKIP_INVARIANTS=1 L0_PORTUALE_OPTS=--solver=pubgrub`
`L0_EMERGE_OPTS="<mode> --getbinpkg=n --usepkg=n" run/l0-resolver.sh`
Counts:
`python3 differential-test-bed/compare/resolve-compare.py --count-categories missing,extra,version,action,exit <rundir> differential-test-bed/compare/known-divergences.yaml > <rundir>/counts.txt`
(runner exit 1 in all 5 modes — the final comparator exiting nonzero is expected, not a failure;
the count command also rewrites `<rundir>/l0-report.txt`/`.json` with the action split.)

Sanity: `-pv` run reproduces §0.1 — `net-libs/nodejs` 8 real rows vs 48 portuale rows (40 extra).

## Totals per mode (unexplained only, 117 probes each)

| mode | run dir | missing | extra | version | action | exit | probes |
|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261014T100015Z/` | 203 | 1679 | 0 | 0 | 34 | 117 |
| `-pvu` | `differential-test-bed/logs/l0-20261014T100547Z/` | 9 | 1698 | 0 | 0 | 34 | 117 |
| `-pvuD` | `differential-test-bed/logs/l0-20261014T101050Z/` | 82 | 1698 | 0 | 0 | 34 | 117 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261014T101728Z/` | 2516 | 137 | 0 | 0 | 34 | 117 |
| `-pve` | `differential-test-bed/logs/l0-20261014T102449Z/` | 26078 | 137 | 0 | 0 | 16 | 117 |

Notes: `version`/`action` are 0 in every mode because almost no `cat/pkg` row exists on
both sides with a differing version or bracket letter — the sets themselves diverge, so
everything lands in `missing`/`extra`. `exit` counts per-probe exit-code findings
(34 in `-pv`/`-pvu`/`-pvuD`/`-pvuDN`, 16 in `-pve`). Context (not counted):
clean probes 3/37/15/10/0; explained findings 11/11/11/8/8.

## Top 15 probes per mode (name + counted findings)

### `-pv` (`counts-pv.txt`)

| probe | total | breakdown |
|---|---|---|
| dev-lang_rust | 101 | missing=1 extra=100 |
| _world | 85 | missing=50 extra=35 |
| _system | 75 | missing=69 extra=6 |
| dev-lang_rust-bin | 67 | extra=67 |
| app-editors_emacs | 47 | extra=47 |
| dev-db_postgresql | 46 | extra=46 |
| dev-lang_ruby | 44 | extra=44 |
| virtual_mta | 43 | extra=43 |
| virtual_cron | 43 | extra=43 |
| sys-process_htop | 43 | extra=43 |
| sys-fs_lvm2 | 43 | extra=43 |
| sys-apps_systemd-utils | 43 | extra=43 |
| sys-apps_ripgrep | 43 | extra=43 |
| sys-apps_pciutils | 43 | extra=43 |
| sys-apps_fd | 43 | extra=43 |

### `-pvu` (`counts-pvu.txt`)

| probe | total | breakdown |
|---|---|---|
| dev-lang_rust | 101 | missing=1 extra=100 |
| dev-lang_rust-bin | 67 | extra=67 |
| _world | 54 | extra=54 |
| app-editors_emacs | 47 | extra=47 |
| dev-db_postgresql | 46 | extra=46 |
| dev-lang_ruby | 44 | extra=44 |
| virtual_mta | 43 | extra=43 |
| virtual_cron | 43 | extra=43 |
| sys-process_htop | 43 | extra=43 |
| sys-fs_lvm2 | 43 | extra=43 |
| sys-apps_systemd-utils | 43 | extra=43 |
| sys-apps_ripgrep | 43 | extra=43 |
| sys-apps_pciutils | 43 | extra=43 |
| sys-apps_fd | 43 | extra=43 |
| sys-apps_bat | 43 | extra=43 |

### `-pvuD` (`counts-pvuD.txt`)

| probe | total | breakdown |
|---|---|---|
| dev-lang_rust | 102 | missing=2 extra=100 |
| dev-lang_rust-bin | 68 | missing=1 extra=67 |
| _world | 55 | missing=1 extra=54 |
| app-editors_emacs | 48 | missing=1 extra=47 |
| dev-db_postgresql | 47 | missing=1 extra=46 |
| dev-lang_ruby | 45 | missing=1 extra=44 |
| virtual_mta | 44 | missing=1 extra=43 |
| virtual_cron | 44 | missing=1 extra=43 |
| sys-process_htop | 44 | missing=1 extra=43 |
| sys-fs_lvm2 | 44 | missing=1 extra=43 |
| sys-apps_systemd-utils | 44 | missing=1 extra=43 |
| sys-apps_ripgrep | 44 | missing=1 extra=43 |
| sys-apps_pciutils | 44 | missing=1 extra=43 |
| sys-apps_fd | 44 | missing=1 extra=43 |
| sys-apps_bat | 44 | missing=1 extra=43 |

### `-pvuDN` (`counts-pvuDN.txt`)

| probe | total | breakdown |
|---|---|---|
| dev-lang_rust | 73 | missing=13 extra=60 |
| _system | 71 | missing=65 extra=6 |
| _dev-lang_python-3.15 | 63 | missing=56 extra=7 |
| sys-devel_gcc | 60 | missing=56 extra=4 |
| dev-build_autoconf | 59 | missing=56 extra=3 |
| sys-devel_binutils | 58 | missing=56 extra=2 |
| dev-build_automake | 57 | missing=56 extra=1 |
| virtual_perl-ExtUtils-MakeMaker | 56 | missing=56 |
| virtual_pager | 56 | missing=56 |
| virtual_libudev | 56 | missing=56 |
| virtual_libc | 56 | missing=56 |
| sys-libs_zlib | 56 | missing=56 |
| sys-libs_glibc_multilib_ | 56 | missing=56 |
| sys-libs_glibc | 56 | missing=56 |
| sys-apps_util-linux | 56 | missing=56 |

### `-pve` (`counts-pve.txt`)

| probe | total | breakdown |
|---|---|---|
| _system | 374 | missing=368 extra=6 |
| dev-lang_rust | 338 | missing=278 extra=60 |
| _dev-lang_python-3.15 | 328 | missing=321 extra=7 |
| _world | 326 | missing=320 extra=6 |
| sys-devel_gcc | 325 | missing=321 extra=4 |
| net-misc_openssh | 325 | missing=325 |
| virtual_pager | 324 | missing=324 |
| dev-build_autoconf | 324 | missing=321 extra=3 |
| virtual_editor | 323 | missing=323 |
| sys-devel_binutils | 323 | missing=321 extra=2 |
| virtual_libc | 322 | missing=322 |
| dev-build_automake | 322 | missing=321 extra=1 |
| virtual_perl-ExtUtils-MakeMaker | 321 | missing=321 |
| virtual_libudev | 321 | missing=321 |
| sys-libs_zlib | 321 | missing=321 |

Per-mode full counts: `counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`,
`counts-pvuDN.txt`, `counts-pve.txt` in this directory (copies of each run dir's
`counts.txt`, which also holds the `## counts by probe` section).
