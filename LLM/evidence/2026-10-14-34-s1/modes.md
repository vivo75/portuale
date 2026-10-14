# Task S1 re-measure — 5 modes × corpus, `--solver=pubgrub`

PM HEAD (portuale): `9a86600e0f7d63eb168f46593a87b092fab73410`, dirty
(`M LLM/backlog-tasks-2026-10.md`,
`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M LLM/evidence/2026-10-14-34-s0/buckets.md`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-14-34-s0/buckets-unfiltered.md`,
`?? LLM/evidence/2026-10-14-34-s1/`; S1 change in progress).
pmtest HEAD: `ecaf0262e022a2e768eaf532653c273a097e69d2`
(`M differential-test-bed/compare/pg-buckets.py`,
`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-14 21:25 UTC (before the runs;
already contains the S1 change; no cargo build/test was run during the bed runs).
Image: `localhost/test-portuale:latest` (`5616d41d00cd`), Portage 3.0.82.2.

Each run (from `/home/vivo/repo/PORTUALE/pmtest`, sequentially):
`export GENTOO_MIRRORS=http://10.48.0.229:8080`
`L0_SKIP_MULTI=1 L0_SKIP_INVARIANTS=1 L0_PORTUALE_OPTS=--solver=pubgrub`
`L0_EMERGE_OPTS="<mode> --getbinpkg=n --usepkg=n" differential-test-bed/run/l0-resolver.sh`
with `<mode>` = `-pv`, `-pvu`, `-pvuD`, `-pvuDN`, `-pve` in that order.
Counts:
`python3 differential-test-bed/compare/resolve-compare.py --count-categories missing,extra,version,action,exit <rundir> differential-test-bed/compare/known-divergences.yaml > <rundir>/counts.txt`
(runner exit 1 in all 5 modes — the final comparator exiting nonzero is expected, not a
failure; the count command also rewrites `<rundir>/l0-report.txt`/`.json` with the action
split; count exit 1 in all 5 modes for the same reason.)

## Totals per mode (unexplained only, 117 probes each)

| mode | S1 run dir | S1 missing | S1 extra | S1 version | S1 action | S1 exit | S0 missing | S0 extra | S0 version | S0 action | S0 exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261014T212554Z/` | 204 | 1589 | 0 | 0 | 30 | 203 | 1679 | 0 | 0 | 34 | +1 | -90 | 0 | 0 | -4 |
| `-pvu` | `differential-test-bed/logs/l0-20261014T213106Z/` | 9 | 1608 | 0 | 0 | 30 | 9 | 1698 | 0 | 0 | 34 | 0 | -90 | 0 | 0 | -4 |
| `-pvuD` | `differential-test-bed/logs/l0-20261014T213610Z/` | 83 | 1608 | 0 | 0 | 30 | 82 | 1698 | 0 | 0 | 34 | +1 | -90 | 0 | 0 | -4 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261014T214249Z/` | 2529 | 4 | 0 | 0 | 30 | 2516 | 137 | 0 | 0 | 34 | +13 | -133 | 0 | 0 | -4 |
| `-pve` | `differential-test-bed/logs/l0-20261014T215013Z/` | 26363 | 4 | 0 | 0 | 12 | 26078 | 137 | 0 | 0 | 16 | +285 | -133 | 0 | 0 | -4 |

S0 run dirs (for reference): `-pv` `l0-20261014T100015Z/`, `-pvu` `l0-20261014T100547Z/`,
`-pvuD` `l0-20261014T101050Z/`, `-pvuDN` `l0-20261014T101728Z/`, `-pve` `l0-20261014T102449Z/`.

Notes: `version`/`action` are 0 in every mode on both sides, same as S0. Context (not counted):
S1 clean probes 3/45/18/12/0 (S0: 3/37/15/10/0); explained findings 11/11/11/8/8 (same as S0).
The exit delta (-4) is the same two probes in every mode: `dev-db_mariadb` and
`__app-shells_bash-5.2` no longer have exit-code findings (each carried 2); both now show
merge-list findings instead (see "probes up" below). No new exit probes in any mode.

## Counted `extra` findings whose cp equals the probe's own target cp

| mode | S1 self-cp extras | S0 self-cp extras | delta |
|---|---|---|---|
| `-pv` | 0 of 1589 | 85 of 1679 | -85 |
| `-pvu` | 0 of 1608 | 85 of 1698 | -85 |
| `-pvuD` | 0 of 1608 | 85 of 1698 | -85 |
| `-pvuDN` | 0 of 4 | 85 of 137 | -85 |
| `-pve` | 0 of 4 | 85 of 137 | -85 |

How computed (`/tmp/opencode/bed-runner/analyze.py`): for each of the 117 atoms in
`differential-test-bed/atomlists/l0-resolve.txt`, the probe slug is the atom with every
character outside `[A-Za-z0-9._-]` replaced by `_` (the bed's own `slug()` in
`layers/l0/in-container.sh`; the mapping slug<->atom is bijective over all 117 probes in
all 10 runs). The probe's target cp is the atom with version operators (`<>=~`), trailing
`*`, USE brackets and slot (`:...`) stripped, and the package cut at the first `-`
followed by a digit (sets `@system`/`@world` have no target cp). Each counted `extra`
finding (`category == 'extra'`, no `explained_by` key, from `<rundir>/l0-report.json`)
has detail `ebuild <cpv[:slot]> present for portuale, absent for real`; its cp is derived
with the same cut rule and compared to the probe's target cp. Sanity checks: the JSON
extra count equals the `counts.txt` extra total in all 10 runs (e.g. S1 `-pv` 1589 = 1589);
no counted extra finding carries `explained_by` in any run. Spot check: S0 `-pv`
`dev-lang_rust` extras include `dev-lang/rust-1.94.1:1.94.1`,
`dev-lang/rust-bin-1.87.0:1.87.0`, ... (24 self-cp rows under `dev-lang_rust`,
24 under `dev-lang_rust-bin`, 11 under `app-shells_bash`, 7 under `_dev-lang_python-3.15`,
etc., totalling 85 in every S0 mode); S1 `-pv` `dev-lang_rust` has 43 findings and none
is a `dev-lang/rust*` row.

## Probes whose counted totals went UP versus S0 (name, S0 total, S1 total)

| mode | probe | S0 total | S1 total | S1 breakdown |
|---|---|---|---|---|
| `-pv` | `dev-db_mariadb` | 2 | 43 | extra=43 |
| `-pvu` | `dev-db_mariadb` | 2 | 43 | extra=43 |
| `-pvuD` | `dev-db_mariadb` | 2 | 44 | missing=1 extra=43 |
| `-pvuDN` | `dev-db_mariadb` | 2 | 13 | missing=13 |
| `-pve` | `dev-db_mariadb` | 2 | 279 | missing=279 |
| `-pve` | `__app-shells_bash-5.2` | 2 | 6 | missing=6 |

(S0 totals for these probes were `exit=2` in every mode; the S1 totals are merge-list
findings. No other probe's counted total went up in any mode. Per-probe totals are the
sums of the five counted categories from the `## counts by probe` section of each run's
`counts.txt`.)

## Top 15 probes per mode, S1 (name + counted findings)

### `-pv` (`counts-pv.txt`)

| probe | total | breakdown |
|---|---|---|
| _world | 79 | extra=29 missing=50 |
| _system | 69 | missing=69 |
| app-admin_sudo | 43 | extra=43 |
| app-editors_emacs | 43 | extra=43 |
| app-misc_tmux | 43 | extra=43 |
| app-portage_gentoolkit | 43 | extra=43 |
| app-shells_starship | 43 | extra=43 |
| dev-db_mariadb | 43 | extra=43 |
| dev-lang_ruby | 43 | extra=43 |
| dev-lang_rust-bin | 43 | extra=43 |
| dev-perl_XML-Parser | 43 | extra=43 |
| dev-python_pyyaml | 43 | extra=43 |
| dev-ruby_rake | 43 | extra=43 |
| media-libs_freetype | 43 | extra=43 |
| sys-apps_bat | 43 | extra=43 |

### `-pvu` (`counts-pvu.txt`)

| probe | total | breakdown |
|---|---|---|
| _world | 48 | extra=48 |
| app-admin_sudo | 43 | extra=43 |
| app-editors_emacs | 43 | extra=43 |
| app-misc_tmux | 43 | extra=43 |
| app-portage_gentoolkit | 43 | extra=43 |
| app-shells_starship | 43 | extra=43 |
| dev-db_mariadb | 43 | extra=43 |
| dev-lang_ruby | 43 | extra=43 |
| dev-lang_rust-bin | 43 | extra=43 |
| dev-perl_XML-Parser | 43 | extra=43 |
| dev-python_pyyaml | 43 | extra=43 |
| dev-ruby_rake | 43 | extra=43 |
| media-libs_freetype | 43 | extra=43 |
| sys-apps_bat | 43 | extra=43 |
| sys-apps_fd | 43 | extra=43 |

### `-pvuD` (`counts-pvuD.txt`)

| probe | total | breakdown |
|---|---|---|
| _world | 49 | extra=48 missing=1 |
| app-admin_sudo | 44 | extra=43 missing=1 |
| app-editors_emacs | 44 | extra=43 missing=1 |
| app-misc_tmux | 44 | extra=43 missing=1 |
| app-portage_gentoolkit | 44 | extra=43 missing=1 |
| app-shells_starship | 44 | extra=43 missing=1 |
| dev-db_mariadb | 44 | extra=43 missing=1 |
| dev-lang_ruby | 44 | extra=43 missing=1 |
| dev-lang_rust-bin | 44 | extra=43 missing=1 |
| dev-perl_XML-Parser | 44 | extra=43 missing=1 |
| dev-python_pyyaml | 44 | extra=43 missing=1 |
| dev-ruby_rake | 44 | extra=43 missing=1 |
| media-libs_freetype | 44 | extra=43 missing=1 |
| sys-apps_bat | 44 | extra=43 missing=1 |
| sys-apps_fd | 44 | extra=43 missing=1 |

### `-pvuDN` (`counts-pvuDN.txt`)

| probe | total | breakdown |
|---|---|---|
| _system | 65 | missing=65 |
| _dev-lang_python-3.15 | 56 | missing=56 |
| app-admin_perl-cleaner | 56 | missing=56 |
| app-crypt_gnupg | 56 | missing=56 |
| app-portage_eix | 56 | missing=56 |
| dev-build_autoconf | 56 | missing=56 |
| dev-build_automake | 56 | missing=56 |
| dev-build_cmake | 56 | missing=56 |
| dev-build_libtool | 56 | missing=56 |
| dev-build_meson | 56 | missing=56 |
| dev-db_sqlite | 56 | missing=56 |
| dev-lang_perl | 56 | missing=56 |
| dev-lang_python_3.14 | 56 | missing=56 |
| dev-perl_Locale-gettext | 56 | missing=56 |
| dev-python_certifi | 56 | missing=56 |

### `-pve` (`counts-pve.txt`)

| probe | total | breakdown |
|---|---|---|
| _system | 368 | missing=368 |
| net-misc_openssh | 325 | missing=325 |
| virtual_pager | 324 | missing=324 |
| virtual_editor | 323 | missing=323 |
| virtual_libc | 322 | missing=322 |
| _dev-lang_python-3.15 | 321 | missing=321 |
| _dev-libs_libevent-2.1.12 | 321 | missing=321 |
| app-admin_perl-cleaner | 321 | missing=321 |
| app-alternatives_awk | 321 | missing=321 |
| app-crypt_gnupg | 321 | missing=321 |
| app-portage_eix | 321 | missing=321 |
| dev-build_autoconf | 321 | missing=321 |
| dev-build_automake | 321 | missing=321 |
| dev-build_cmake | 321 | missing=321 |
| dev-build_libtool | 321 | missing=321 |

Per-mode full counts: `counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`,
`counts-pvuDN.txt`, `counts-pve.txt` in this directory (copies of each run dir's
`counts.txt`, which also holds the `## counts by probe` section).
