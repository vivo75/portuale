# Task S2 re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `f2d29237c1d8fed780e6dcefd1e350c782b9db78`, dirty
(`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/src/resolve/outcome.rs`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s2/`; S2 change in progress).
pmtest HEAD: `543c40cb2931d36bab6bfbe4a78a6322cf0d24c2`
(`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-14 23:15 UTC (before the runs;
already contains the S2 change; no cargo build/test was run during the bed runs).
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

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S2 run dir | S2 missing | S2 extra | S2 version | S2 action | S2 exit | S1 missing | S1 extra | S1 version | S1 action | S1 exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261014T231744Z/` | 24 | 5 | 0 | 0 | 30 | 204 | 1589 | 0 | 0 | 30 | -180 | -1584 | 0 | 0 | 0 |
| `-pvu` | `differential-test-bed/logs/l0-20261014T232409Z/` | 24 | 5 | 0 | 0 | 30 | 9 | 1608 | 0 | 0 | 30 | +15 | -1603 | 0 | 0 | 0 |
| `-pvuD` | `differential-test-bed/logs/l0-20261014T233009Z/` | 83 | 2903 | 0 | 0 | 30 | 83 | 1608 | 0 | 0 | 30 | 0 | +1295 | 0 | 0 | 0 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261014T233802Z/` | 1234 | 4 | 0 | 0 | 30 | 2529 | 4 | 0 | 0 | 30 | -1295 | 0 | 0 | 0 | 0 |
| `-pve` | `differential-test-bed/logs/l0-20261014T234638Z/` | 2044 | 10 | 0 | 0 | 30 | 26363 | 4 | 0 | 0 | 12 | -24319 | +6 | 0 | 0 | +18 |

S1 run dirs (for reference): `-pv` `l0-20261014T212554Z/`, `-pvu` `l0-20261014T213106Z/`,
`-pvuD` `l0-20261014T213610Z/`, `-pvuDN` `l0-20261014T214249Z/`, `-pve` `l0-20261014T215013Z/`.

Notes: `version`/`action` are 0 in every mode on both sides. Context (not counted):
S2 clean probes 54/59/15/12/3 (S1: 3/45/18/12/0); explained findings 11/11/11/8/8
(identical to S1 in every mode). The S2 `-pve` exit total (30) keeps all 6 S1 `-pve`
exit probes (`app-containers_runc`, `app-emulation_wine-vanilla`, `dev-lang_go`,
`gnome-base_gnome-shell`, `www-servers_nginx`, `x11-wm_i3`, 2 findings each) and adds
the 9 probes listed under `-pve` below (2 findings each: 12 + 18 = 30).

## Probes whose counted totals went UP versus S1 (name, S1 total, S2 total)

| mode | probe | S1 total | S2 total | S2 breakdown |
|---|---|---|---|---|
| `-pv` | `kde-apps_kdecore-meta` | 10 | 11 | missing=7 extra=4 |
| `-pvu` | `kde-apps_kdecore-meta` | 10 | 11 | missing=7 extra=4 |
| `-pvuD` | `_system` | 1 | 49 | missing=1 extra=48 |
| `-pvuD` | `_dev-lang_python-3.15` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `app-admin_perl-cleaner` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `app-crypt_gnupg` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `app-portage_eix` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-build_autoconf` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-build_automake` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-build_libtool` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-build_meson` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-lang_perl` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-lang_python_3.14` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-perl_Locale-gettext` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-python_certifi` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-python_cython` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-python_jinja2` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-python_lxml` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-python_setuptools` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `net-misc_openssh` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `sys-apps_dbus` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `sys-apps_systemd` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `sys-apps_util-linux` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `sys-devel_gcc` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `sys-libs_glibc` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `sys-libs_glibc_multilib_` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `virtual_libc` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `virtual_libudev` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `virtual_perl-ExtUtils-MakeMaker` | 1 | 44 | missing=1 extra=43 |
| `-pvuD` | `dev-build_cmake` | 0 | 43 | extra=43 |
| `-pvuD` | `dev-db_sqlite` | 0 | 43 | extra=43 |
| `-pvuD` | `sys-devel_binutils` | 0 | 43 | extra=43 |
| `-pve` | `app-editors_gedit` | 0 | 2 | exit=2 |
| `-pve` | `app-office_libreoffice` | 0 | 2 | exit=2 |
| `-pve` | `gnome-base_nautilus` | 0 | 2 | exit=2 |
| `-pve` | `gui-libs_gtk_4` | 0 | 2 | exit=2 |
| `-pve` | `mail-client_thunderbird` | 0 | 2 | exit=2 |
| `-pve` | `media-gfx_gimp` | 0 | 2 | exit=2 |
| `-pve` | `net-analyzer_wireshark` | 0 | 2 | exit=2 |
| `-pve` | `www-client_firefox` | 0 | 2 | exit=2 |
| `-pve` | `x11-libs_gtk__3` | 0 | 2 | exit=2 |

(No probe's counted total went up in `-pvuDN`. Per-probe totals are the sums of the
five counted categories from the `## counts by probe` section of each run's
`counts.txt`; S1 per-probe totals from `counts-{pv,pvu,pvuD,pvuDN,pve}.txt` in
`LLM/evidence/2026-10-14-34-s1/`. The S1 totals of 1 in `-pvuD` are `missing=1`;
the S1 totals of 0 mean the probe had no counted findings in that mode.)

## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261014T235819Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0. Context (not counted): clean probes 14, explained findings 0
(both allowlist entries matched nothing: `gedit-nautilus-cluster-a-rewalk-abort`,
`gnome-shell-cluster-a-abort`).

Most recent earlier default-mode run with the same options: none exists under
`differential-test-bed/logs/`. Method: the run's real (Portage) side is
byte-identical (modulo `Dependency resolution took` timing lines) to the real side
of the three `-pvuDN --getbinpkg=n --usepkg=n` runs on record — S0
`l0-20261014T101728Z/`, S1 `l0-20261014T214249Z/`, S2 `l0-20261014T233802Z/`
(117/117 files equal in each case; same gentoo tree `11c58b7a`, same VDB snapshot,
same image) — but all three ran portuale with `--solver=pubgrub` (documented in
the S0/S1 modes.md). Every other `l0-*` run dir was checked the same way: all 28
`l0-20261013T*` runs and all 66 pre-Oct-13 `l0-*` runs differ on the real side
(the Oct 12/13 runs show the `-pv` signature, e.g. `@world` = 76 packages, 26 new,
50 reinstalls, no updates), so none of them used the same emerge options; runs
before pmtest `ecaf026` (2026-10-14 12:19 UTC) additionally could not pass solver
options to portuale at all.

Per-mode full counts: `counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`,
`counts-pvuDN.txt`, `counts-pve.txt`, `counts-default.txt` in this directory (copies
of each run dir's `counts.txt`, which also holds the `## counts by probe` section).
