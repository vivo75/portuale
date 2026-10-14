# Task S0 Step 7 — bucket counts (pubgrub vs Portage)

Inputs: the 5 run dirs in `modes.md` (117 probes each), per-probe
`PORTUALE_PG_TRACE=1` captures in `<rundir>/pgtrace/` (one container
per mode, same mounts/env as `run/lib.sh podman_run_pm` +
`layers/l0/in-container.sh`, `GENTOO_MIRRORS` forwarded, stderr per
probe into `pgtrace/<probe>.txt`; loop over only the probes with
extras; driver `trace-run.sh` + probe list `pgtrace-probes.tsv` kept
in each run dir). Parsing reused from `resolve-compare.py` by import
(`parse`, `split_flags`, `split_cpv`); key rule `(type, cp, slot)` as
in `compare()`. Counting keys are comparator-identical (totals
reconcile 1:1 with `l0-report.json`); bucket attribution additionally
repairs the comparator's `-rN:slot` artifact (`split_cpv_slot` turns
e.g. `sys-devel/binutils-2.44-r4:2.44` into cp=whole-token, ver='': a
cp never contains ':', so the script re-splits such tokens with the
same version grammar for cp identity vs trace roots/graph only).

Bucket rules (exactly one bucket per counted finding). MISSING:
Portage action `R` -> H-no-R; same cp in portuale output with another
slot/version -> H-slot; else H-other. EXTRA: shortest chain from a
pg-target root over pg-edge; (1) extra cp == target cp ->
H-target-all-slots; (2) first chain edge whose parent cpv is pg-installed: no-D ->
H-installed-expanded, D + DEPEND/BDEPEND -> H-installed-bdeps, D +
RDEPEND/PDEPEND/IDEPEND -> H-installed-config-USE; (3) chain through
an edge whose child is pg-closure shallow with an installed parent
-> H-closure; (4) H-installed-only skipped (not detectable from the
trace); (5) rest -> H-other. Chain ties: shortest wins; roots in
lexicographic order, then trace order in the adjacency lists
(deterministic across runs).

## Bucket table (cells = unexplained missing+extra findings)

| bucket | -pv | -pvu | -pvuD | -pvuDN | -pve |
|---|---|---|---|---|---|
| H-target-all-slots | 97 | 97 | 97 | 97 | 97 |
| H-installed-expanded | 294 | 305 | 0 | 0 | 2 |
| H-installed-config-USE | 0 | 0 | 43 | 2 | 0 |
| H-installed-bdeps | 0 | 0 | 262 | 0 | 0 |
| H-closure | 0 | 0 | 0 | 0 | 0 |
| H-no-R | 148 | 8 | 8 | 969 | 24223 |
| H-installed-only | 0 | 0 | 0 | 0 | 0 |
| H-switches | 0 | 0 | 0 | 0 | 0 |
| H-slot | 0 | 0 | 0 | 0 | 0 |
| H-or-choice | 0 | 0 | 0 | 0 | 0 |
| H-cycle | 0 | 0 | 0 | 0 | 0 |
| H-pdepend-idepend | 0 | 0 | 0 | 0 | 0 |
| H-other | 1343 | 1297 | 1370 | 1585 | 1893 |
| TOTAL | 1882 | 1707 | 1780 | 2653 | 26215 |
| modes.md missing+extra | 1882 | 1707 | 1780 | 2653 | 26215 |

Verification: ALL column totals equal the modes.md missing+extra totals.
- -pv: buckets total 1882 vs missing+extra 1882 (missing 203/203, extra 1679/1679)
- -pvu: buckets total 1707 vs missing+extra 1707 (missing 9/9, extra 1698/1698)
- -pvuD: buckets total 1780 vs missing+extra 1780 (missing 82/82, extra 1698/1698)
- -pvuDN: buckets total 2653 vs missing+extra 2653 (missing 2516/2516, extra 137/137)
- -pve: buckets total 26215 vs missing+extra 26215 (missing 26078/26078, extra 137/137)

Zero buckets (value 0, not distinguishable by script): H-installed-only (rule 4 skipped: repo absence of an installed parent's version is not visible in the trace), H-switches, H-or-choice (`||` choice leaves no trace mark), H-cycle, H-pdepend-idepend.

Diagnostics: `-rN:slot` repaired attributions: 4043; extra==target with pg-target slot!=* : 0; first-edge parent cp installed but cpv not exact (near-miss, counted by fallthrough rules, not rule 2): 46; extras with no chain from any root (-> H-other): 0.

## Example probes (up to 3 per bucket per mode)

### H-target-all-slots
- -pv: `sys-devel_gcc`, `sys-devel_binutils`, `dev-build_autoconf` (97 total)
- -pvu: `sys-devel_gcc`, `sys-devel_binutils`, `dev-build_autoconf` (97 total)
- -pvuD: `sys-devel_gcc`, `sys-devel_binutils`, `dev-build_autoconf` (97 total)
- -pvuDN: `sys-devel_gcc`, `sys-devel_binutils`, `dev-build_autoconf` (97 total)
- -pve: `sys-devel_gcc`, `sys-devel_binutils`, `dev-build_autoconf` (97 total)

### H-installed-expanded
- -pv: `sys-apps_systemd-utils`, `dev-python_pip`, `dev-python_pyyaml` (294 total)
- -pvu: `sys-apps_systemd-utils`, `dev-python_pip`, `dev-python_pyyaml` (305 total)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: `kde-apps_kdecore-meta` (2 total)

### H-installed-config-USE
- -pv: (0)
- -pvu: (0)
- -pvuD: `app-portage_gentoolkit`, `dev-perl_XML-Parser`, `app-admin_sudo` (43 total)
- -pvuDN: `kde-apps_kdecore-meta` (2 total)
- -pve: (0)

### H-installed-bdeps
- -pv: (0)
- -pvu: (0)
- -pvuD: `sys-apps_systemd-utils`, `dev-python_pip`, `dev-python_pyyaml` (262 total)
- -pvuDN: (0)
- -pve: (0)

### H-closure
- -pv: (0)
- -pvu: (0)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: (0)

### H-no-R
- -pv: `sys-devel_gcc`, `sys-libs_glibc`, `sys-devel_binutils` (148 total)
- -pvu: `dev-lang_rust`, `kde-frameworks_kcoreaddons`, `kde-apps_kdecore-meta` (8 total)
- -pvuD: `dev-lang_rust`, `kde-frameworks_kcoreaddons`, `kde-apps_kdecore-meta` (8 total)
- -pvuDN: `sys-apps_portage`, `sys-devel_gcc`, `sys-libs_glibc` (969 total)
- -pve: `sys-apps_portage`, `sys-devel_gcc`, `sys-libs_glibc` (24223 total)

### H-installed-only
- -pv: (0)
- -pvu: (0)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: (0)

### H-switches
- -pv: (0)
- -pvu: (0)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: (0)

### H-slot
- -pv: (0)
- -pvu: (0)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: (0)

### H-or-choice
- -pv: (0)
- -pvu: (0)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: (0)

### H-cycle
- -pv: (0)
- -pvu: (0)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: (0)

### H-pdepend-idepend
- -pv: (0)
- -pvu: (0)
- -pvuD: (0)
- -pvuDN: (0)
- -pve: (0)

### H-other
- -pv: `sys-apps_portage`, `sys-apps_systemd`, `sys-apps_systemd-utils` (1343 total)
- -pvu: `sys-apps_portage`, `sys-apps_systemd-utils`, `dev-python_pip` (1297 total)
- -pvuD: `sys-apps_portage`, `sys-devel_gcc`, `sys-libs_glibc` (1370 total)
- -pvuDN: `sys-apps_portage`, `sys-devel_gcc`, `sys-libs_glibc` (1585 total)
- -pve: `sys-apps_portage`, `sys-devel_gcc`, `sys-libs_glibc` (1893 total)

## Sample chains

1. probe `net-libs_nodejs` (-pv), extra `acct-group/polkitd`, bucket H-other
   chain: net-libs/nodejs --[DEPEND via net-libs/nodejs-20.18.2]--> net-libs/nghttp2 --[DEPEND via net-libs/nghttp2-1.69.0]--> sys-apps/systemd --[PDEPEND via sys-apps/systemd-259.4-r2]--> sys-auth/polkit --[RDEPEND via sys-auth/polkit-126-r3]--> acct-user/polkitd --[RDEPEND via acct-user/polkitd-0-r3]--> acct-group/polkitd
2. probe `net-libs_nodejs` (-pv), extra `acct-user/polkitd`, bucket H-other
   chain: net-libs/nodejs --[DEPEND via net-libs/nodejs-20.18.2]--> net-libs/nghttp2 --[DEPEND via net-libs/nghttp2-1.69.0]--> sys-apps/systemd --[PDEPEND via sys-apps/systemd-259.4-r2]--> sys-auth/polkit --[RDEPEND via sys-auth/polkit-126-r3]--> acct-user/polkitd
3. probe `dev-lang_rust` (-pv), extra `dev-lang/nasm`, bucket H-installed-expanded
   chain: dev-lang/rust --[DEPEND via dev-lang/rust-1.74.1-r101]--> net-misc/curl --[DEPEND via net-misc/curl-8.19.0]--> net-libs/libpsl --[BDEPEND via net-libs/libpsl-0.21.5]--> dev-build/meson --[DEPEND via dev-build/meson-1.10.2]--> dev-lang/nasm
4. probe `app-shells_bash` (-pv), extra `app-shells/bash`, bucket H-target-all-slots
   chain: app-shells/bash == target root
5. probe `dev-build_autoconf` (-pv), extra `dev-build/autoconf`, bucket H-target-all-slots
   chain: dev-build/autoconf == target root
