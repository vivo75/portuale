# S0.1 + S0.2 + S0.3 — worker `a` (attrs table, vendored drift, predicate parity)

Status: captures complete 2026-10-12. Branch `backlog/341-eapi-atom-grammar`
(portuale + pmtest). Oracle: host Portage 3.0.82.2
(`python3 -c 'import portage'` → `/usr/lib/python3.14/site-packages/portage/`,
`portage.VERSION == 3.0.82.2`); pinned tag `portage-3.0.82.2`
(`1d95fc2c552a2c7d347e54093fa0e5b172a57004`). EAPIs in scope: 7, 8 (+99
unsupported for S0.3). No product code touched; no fixtures created
(S0.1–S0.3 need none); probes live in `/var/tmp/t348-a/` (hermetic
`PORTAGE_TMPDIR` per run, fresh builddirs so phase-marker skips cannot bite).

Row ids: `A<n>` = `_eapi_attrs` field, `C<n>` = Python-side consumer,
`D-<file>` = vendored file, `P-<pred>-<eapi>` = predicate cell.
Verdicts: `same` / `diverges (fix)` / `diverges (cut, owner decides)` /
`not reproducible (why)`. For S0.1/S0.2 the verdict column uses the
inventory statuses the plan orders
(`implemented` / `implemented elsewhere (vendored bash)` / `missing` /
`not applicable at floor 7`).

Method notes (S0.3):

- Predicates are pure functions of `$EAPI` (`bin/eapi.sh:7-92`,
  `[[ ${1-${EAPI-0}} == <class> ]]`); every in-tree call site calls them
  with no args (`grep ___eapi_ bin/*.sh`: all no-arg), so probes call them
  with no args, one probe ebuild per EAPI
  (`/var/tmp/t348-a/t348-a-pred/t348-a-pred-{7,8,99}.ebuild`, `pkg_setup`
  loops the 74 names in `eapi.sh` definition order, `rc=0; "${p}" || rc=$?`).
- Runners: `host-bash` = host bash sourcing the HOST `eapi.sh` (byte-identical
  to `bin/eapi.sh` — S0.2 `cmp` clean); `portuale-bash` /
  `portuale-brush` = `target/release/portuale ebuild <probe> setup
  --shell <bash|brush>` (exactly how portuale executes phases; never rebuilt).
- `EAPI` is readonly inside the phase env on BOTH shells (override probes
  `R-portuale-{bash,brush}-override99`: `EAPI: readonly variable` /
  `cannot mutate readonly variable`), so no-arg EAPI-99 under brush is
  unreachable through any phase path — the canonical `P-*-99` brush column
  is therefore `not reproducible`, with body parity proven by the explicit-arg
  supplementary runs (`R-{host-bash,portuale-bash,portuale-brush}-arg99`:
  `"${p}" 99`, all 74 equal on all three shells and equal to host no-arg 99).
- Live values below come from the captures (`a/P/R-*.stdout`), not from
  reading `eapi.sh`.

Correction to the plan: `_eapi_attrs` has **38** fields live
(`eapi.py:163-205`, `len(...) == 38`), not 39. Rows A1–A38.

## S0.1 attribute table (live values in `a/s01-live-dump.txt`)

| id | field | 7 | 8 | Portage threshold (`eapi.py:289-328`) → effect at 7/8 | portuale site | status |
|----|-------|---|---|------------------------------------------------------|---------------|--------|
| A1 | allows_package_provided | F | F | `<=6`; `config.py:974` gates `package.provided` | `portage-dep/src/eapi.rs:49,89` (table only; no provided-gating user found in `crates/`) | missing |
| A2 | bdepend | T | T | `>=7`; BDEPEND metadata/phase var | `portage-dep/src/eapi.rs:50,90` (dep side only); no `BROOT/BDEPEND` phase-env handling in `ebuild_phases/` | missing |
| A3 | broot | T | T | `>=7`; `config.environ` keeps `BROOT` (`config.py:3345`) | none (no `BROOT` setter in `ebuild_phases/`) | missing |
| A4 | dosed_dohard | F | F | `<=3`; `dosed`/`dohard` die via `___eapi_has_dosed/dohard` (`phase-helpers.sh`) | vendored bash (`bin/phase-helpers.sh`, `bin/eapi.sh:29,34`) | implemented elsewhere (vendored bash) |
| A5 | empty_groups_always_true | F | F | `<=6`; dep `use_reduce` empty-group rule | `portage-dep/src/eapi.rs:51,91` | implemented |
| A6 | exports_AA | F | F | `<=3`; `config.environ` pops `AA` (`config.py:3332`) | `ebuild_phases/commands.rs:73-74` (sets `A`, never `AA`) | implemented |
| A7 | exports_EBUILD_PHASE_FUNC | T | T | `>=5`; `config.environ` sets `EBUILD_PHASE_FUNC` | none (`EBUILD_PHASE_FUNC` never set in `ebuild_phases/`) | missing |
| A8 | exports_ECLASSDIR | F | F | `<=6`; pops `ECLASSDIR` | none (never set → absent on both sides; S0.4 cell confirms) | implemented |
| A9 | exports_KV | F | F | `<=3`; `doebuild.py:655` pops `KV` | none (`KV` never set in `ebuild_phases/` → absent on both sides) | implemented |
| A10 | exports_merge_type | T | T | `>=4`; `MERGE_TYPE=source/binary` (`doebuild.py:1282`) | `remote/merge_script.rs:10-14` (merge path) + `ebuild_phases/single_phase.rs:149` (`binary` only); source phases omit `MERGE_TYPE` (Portage: `source`) | missing |
| A11 | exports_pms_vars | T | T | `<=8`; unexport only for 9+ (`doebuild.py:2211`) | none (no filtering → all exported, same as Portage at 7/8) | implemented |
| A12 | exports_PORTDIR | F | F | `<=6`; pops `PORTDIR` | none (never set → absent on both sides) | implemented |
| A13 | exports_replace_vars | T | T | `>=4`; `REPLACING_VERSIONS` (`doebuild.py:1300`, `EbuildExecuter.py:75`) | `ebuild_phases/single_phase.rs:266-267` (+ `ebuild_merge::replacing_versions`) | implemented |
| A14 | idepend | F | T | `>=8`; IDEPEND metadata key (`doebuild`, `post_install_meta`) | `portage-dep/src/eapi.rs:52,92` (table); `post_install_meta.rs:94-100` reduces IDEPEND unconditionally (7/8 differ: 7 must ignore) | missing |
| A15 | iuse_defaults | T | T | `>=1`; `+/-` IUSE defaults | `portage-dep/src/eapi.rs:53,93` | implemented |
| A16 | iuse_effective | T | T | `>=5`; `IUSE_EFFECTIVE` build-info (`doebuild.py:2710`) | `ebuild_phases/post_install_meta.rs:77,143-147` (writes when config supplies it) | implemented |
| A17 | posixish_locale | T | T | `>=6`; `split_LC_ALL` + `LC_ALL` fan-out (`EbuildPhase.py:54`, `config.environ` assert) | `ebuild_phases/phase_env.rs:239-266` (unconditional fan-out; correct at floor 7) + `remote/merge_script.rs:208` (`eapi_is_posixish`) | implemented |
| A18 | path_variables_end_with_trailing_slash | F | F | `<=6`; `D/ED/ROOT/EROOT/ESYSROOT/BROOT` stripped (`config.py:3391`) | `ebuild_phases/phase_env.rs:84-85,296-303` (`eapi_path_var`, applied to `D/ED/ROOT/EROOT`; `ESYSROOT/BROOT/SYSROOT` unset anyway) | implemented |
| A19 | pkg_pretend | T | T | `>=4`; `pretend` runs (`doebuild.py:2393`) | `ebuild_phases/mod.rs:142` (`pretend` in chain) | implemented |
| A20 | prefix | T | T | `>=3`; keeps `ED/EPREFIX/EROOT/ESYSROOT` | `phase_env.rs:116,124,167` (`ED=D`, `EROOT=ROOT`, `EPREFIX=""` unconditionally — correct iff prefix unused) | implemented |
| A21 | profile_file_dirs | T | T | `>=7`; per-EAPI profile dirs | none found in `portage-profile/` (no `profile_file_dirs` reference) | missing |
| A22 | profile_eapi_default | F | F | `>=9`; `profiles/eapi` ignored below 9 | `portage-profile/src/atom_lines.rs:65-72` (`eapi_has_profile_eapi_default`) | implemented |
| A23 | rdepend_depend | F | F | `<=3`; no RDEPEND→DEPEND fallback | dep side (no fallback implemented anywhere; correct at floor 7) | not applicable at floor 7 |
| A24 | repo_deps | F | F | always `False`; `::repo` atoms invalid | `portage-dep/src/eapi.rs:56,96` + `portuale/src/portageq.rs:92-94,364-366` (`eapi_has_repo_deps`) | implemented |
| A25 | required_use | T | T | `>=4`; gate (`doebuild.py:1943`) | `portage-dep/src/eapi.rs:57,97` (table); phase/merge-time gate: none found (`REQUIRED_USE` only in `regen.rs:36` key list) | missing |
| A26 | required_use_at_most_one_of | T | T | `>=5`; `??` groups | `portage-dep/src/eapi.rs:58,98` | implemented |
| A27 | selective_src_uri_restriction | F | T | `>=8`; selective `SRC_URI` `?()` restriction | `portage-dep/src/eapi.rs:59,99` (table only; no fetch/dep consumer found) | missing |
| A28 | slot_deps | T | T | `>=1`; `:` slot deps | `portage-dep/src/eapi.rs:60,100` | implemented |
| A29 | slot_operator | T | T | `>=5`; `:=`/`newcommand` | `portage-dep/src/eapi.rs:61,101` | implemented |
| A30 | src_prepare_src_configure | T | T | `>=2`; `prepare`/`configure` run (`doebuild.py:2388`) | none (no skip code in `run_phase.rs`; always runs → same at floor 7) | not applicable at floor 7 |
| A31 | src_uri_arrows | T | T | `>=2`; `->` renames | `portage-dep/src/eapi.rs:62,102` | implemented |
| A32 | stablemask | T | T | `>=5`; `use.stable.mask/force` apply (`UseManager.py:95-103`, via `eapi_supports_stable_use_forcing_and_masking = stablemask`, `eapi.py:119`) | `portage-profile/src/resolve/use_force_mask.rs:114-126` (reads unconditionally — correct at 7/8 since gate is True) | implemented |
| A33 | use_stable | F | F | `>=9`; `package.use.stable.*` gated (`UseManager.py:90`, via `eapi_supports_use_stable = use_stable`, `eapi.py:115`) | profile `use.stable.force/mask` read; `package.use.stable.*` gate: none found (only repo+chain `package.use.stable` stacking at `use_force_mask.rs:130`, no EAPI gate) | missing |
| A34 | strong_blocks | T | T | `>=2`; `!`/`!!` blocks | `portage-dep/src/eapi.rs:63,103` | implemented |
| A35 | symlink_rewrite | T | T | `<=8`; absolute-symlink rewrite under `ED` (`vartree.py:5529,5667` via `eapi_rewrites_symlinks`) | none found in `ebuild_merge/` (only vdb/prune `rewrite` hits, unrelated) | missing |
| A36 | sysroot | T | T | `>=7`; `ESYSROOT` kept (`config.py:3341`) | none (no `ESYSROOT`/`SYSROOT` in phase env) | missing |
| A37 | use_deps | T | T | `>=2`; `[]` use deps | `portage-dep/src/eapi.rs:64,104` | implemented |
| A38 | use_dep_defaults | T | T | `>=4`; `[-]`/`[+]` defaults | `portage-dep/src/eapi.rs:65,105` | implemented |

Tally S0.1 fields: implemented 21, implemented elsewhere (vendored bash) 1,
missing 14 (A1, A2, A3, A7, A10, A14, A21, A25, A27, A33, A35, A36 + profile
dirs/sysroot/broot class), not applicable at floor 7: 2 (A23, A30).
Only two fields differ 7→8 live: `idepend` (F→T) and
`selective_src_uri_restriction` (F→T) — both `missing` above.

## S0.1 consumers (Python-side `eapi_*` outside `dep/`; grep log `a/s01-consumers-grep.txt`)

| id | Portage site | effect | portuale site | status |
|----|--------------|--------|---------------|--------|
| C1 | `eapi_exports_KV`, `doebuild.py:655` | pops `KV` for EAPI>3 | none (`KV` never set) | implemented |
| C2 | `eapi_exports_merge_type`, `doebuild.py:1282`, `config.py:3335` | sets `MERGE_TYPE` (source/binary) | `remote/merge_script.rs:10` (merge), `single_phase.rs:149` (binary only) | missing (source-phase export) |
| C3 | `eapi_exports_replace_vars`, `doebuild.py:1300`, `EbuildExecuter.py:75`, `Binpkg.py:94`, `config.py:3363-3366` | `REPLACING_VERSIONS` / `REPLACED_BY_VERSION` gating by phase | `single_phase.rs:266-267`, `ebuild_merge::replacing_versions` | implemented |
| C4 | `eapi_exports_pms_vars`, `doebuild.py:2211` | unexports PMS vars only for 9+ | none (no filtering; same at 7/8) | implemented |
| C5 | `eapi_has_src_prepare_and_src_configure`, `doebuild.py:2388`, `EbuildExecuter.py:135` | skips `prepare`/`configure` below EAPI 2 | none (always runs) | not applicable at floor 7 |
| C6 | `eapi_has_pkg_pretend`, `doebuild.py:2393` | skips `pretend` below EAPI 4 | `mod.rs:142` chain (always runs) | not applicable at floor 7 |
| C7 | `eapi_has_required_use`, `doebuild.py:1943` (+ depgraph) | validates REQUIRED_USE at phase time | none found (see A25) | missing |
| C8 | `eapi_exports_AA`, `config.py:3332` | pops `AA` for EAPI>3 | `commands.rs:73-74` (never sets `AA`) | implemented |
| C9 | `eapi_supports_prefix`, `config.py:3351` | pops `ED/EPREFIX/EROOT/ESYSROOT` below EAPI 3 | `phase_env.rs:116,124,167` (constant) | implemented |
| C10 | `_get_eapi_attrs` direct: `config.py:974` (`allows_package_provided`), `config.py:1888` (USE/`repo_env` — field-neutral at 7/8), `config.py:3271` (`environ`), `doebuild.py:2710` (`iuse_effective`/`slot_operator`), `EbuildPhase.py:54` (`posixish_locale`), `dbapi/__init__.py:255` (`iuse_effective`), `vartree.py:969` + `dbapi` SLOT check (`slot_operator`), `Package.py:109` | assorted (see A-rows) | assorted (see A-rows) | see A-rows |
| C11 | `eapi_is_supported` gates (`doebuild.py:571`, `porttree.py`, `depgraph.py`, `EbuildMetadataPhase.py`) | refuse/mask unsupported EAPI | `ebuild_phases/depend_phase.rs:140-150` (metadata time only; other entry points unchecked — S0.7 territory) | missing (partial) |
| C12 | `eapi_rewrites_symlinks`, `vartree.py:5667` | merge-time symlink rewrite (EAPI≤8) | none found | missing |
| C13 | `eapi_has_repo_deps`, `QueryCommand.py:61` | `::repo` atoms in `has_version`/`best_version` | `portuale/src/portageq.rs:92-94,364-366` | implemented |
| C14 | `eapi_supports_use_stable` / `eapi_supports_stable_use_forcing_and_masking`, `UseManager.py:90-174` | gate `package.use.stable.*` (off at 7/8) vs `use.stable.*` (on at 7/8) | `use_force_mask.rs:114-130` (no gate) | missing (the `use_stable` half; see A33) |
| C15 | `versions.py:296,441` (`_get_pv_re` / slot re) | `_get_pv_re` takes attrs but ignores them (`versions.py:96-103` — no-op); slot re is dep-side | none / dep side | not applicable (Portage itself ignores the attr) |

## S0.2 vendored drift (script `scripts/a-drift.sh`, output `a/drift-tag.txt` + `a/drift-host.txt`)

| id | file | vs tag `portage-3.0.82.2` | vs host `/usr/lib/portage/python3.14` | verdict |
|----|------|---------------------------|---------------------------------------------|---------|
| D-README.md | `bin/README.md` | OWNED: README (portuale doc, no upstream counterpart) | n/a (portuale-only) | same (intentional) |
| D-bashrc-functions.sh | `bin/bashrc-functions.sh` | identical | identical | same |
| D-chmod-lite | `bin/chmod-lite` | OWNED: shim execing $PORTUALE_BIN __helper chmod-lite (#326 D1/D2) | n/a (portuale-only) | same (intentional) |
| D-eapi.sh | `bin/eapi.sh` | identical | identical | same |
| D-sed | `bin/ebuild-helpers/bsd/sed` | identical | identical | same |
| D-die | `bin/ebuild-helpers/die` | identical | identical | same |
| D-dobin | `bin/ebuild-helpers/dobin` | identical | identical | same |
| D-doconfd | `bin/ebuild-helpers/doconfd` | identical | identical | same |
| D-dodir | `bin/ebuild-helpers/dodir` | identical | identical | same |
| D-dodoc | `bin/ebuild-helpers/dodoc` | identical | identical | same |
| D-doenvd | `bin/ebuild-helpers/doenvd` | identical | identical | same |
| D-doexe | `bin/ebuild-helpers/doexe` | identical | identical | same |
| D-dohard | `bin/ebuild-helpers/dohard` | identical | identical | same |
| D-doheader | `bin/ebuild-helpers/doheader` | identical | identical | same |
| D-dohtml | `bin/ebuild-helpers/dohtml` | identical | identical | same |
| D-doinfo | `bin/ebuild-helpers/doinfo` | identical | identical | same |
| D-doinitd | `bin/ebuild-helpers/doinitd` | identical | identical | same |
| D-doins | `bin/ebuild-helpers/doins` | identical | identical | same |
| D-dolib | `bin/ebuild-helpers/dolib` | identical | identical | same |
| D-dolib.a | `bin/ebuild-helpers/dolib.a` | identical | identical | same |
| D-dolib.so | `bin/ebuild-helpers/dolib.so` | identical | identical | same |
| D-doman | `bin/ebuild-helpers/doman` | identical | identical | same |
| D-domo | `bin/ebuild-helpers/domo` | identical | identical | same |
| D-dosbin | `bin/ebuild-helpers/dosbin` | identical | identical | same |
| D-dosed | `bin/ebuild-helpers/dosed` | identical | identical | same |
| D-dosym | `bin/ebuild-helpers/dosym` | identical | identical | same |
| D-elog | `bin/ebuild-helpers/elog` | identical | identical | same |
| D-emake | `bin/ebuild-helpers/emake` | identical | identical | same |
| D-fowners | `bin/ebuild-helpers/fowners` | identical | identical | same |
| D-fperms | `bin/ebuild-helpers/fperms` | identical | identical | same |
| D-keepdir | `bin/ebuild-helpers/keepdir` | identical | identical | same |
| D-newins | `bin/ebuild-helpers/newins` | identical | identical | same |
| D-nonfatal | `bin/ebuild-helpers/nonfatal` | identical | identical | same |
| D-prepall | `bin/ebuild-helpers/prepall` | identical | identical | same |
| D-chown | `bin/ebuild-helpers/unprivileged/chown` | identical | identical | same |
| D-install | `bin/ebuild-helpers/xattr/install` | identical | identical | same |
| D-ebuild-ipc | `bin/ebuild-ipc` | OWNED: always exits 127, no IPC daemon (#326 D6) | n/a (portuale-only) | same (intentional) |
| D-ebuild.sh | `bin/ebuild.sh` | identical | identical | same |
| D-ecompress | `bin/ecompress` | identical | identical | same |
| D-ecompress-file | `bin/ecompress-file` | identical | identical | same |
| D-estrip | `bin/estrip` | identical | identical | same |
| D-filter-bash-environment.py | `bin/filter-bash-environment.py` | identical | DIFF vs host: shebang `env python` vs host `python3.14` (distro rewrite; tag has `env python`) | same (intentional) |
| D-helper-functions.sh | `bin/helper-functions.sh` | identical | identical | same |
| D-05double-D | `bin/install-qa-check.d/05double-D` | identical | identical | same |
| D-05prefix | `bin/install-qa-check.d/05prefix` | identical | identical | same |
| D-10executable-issues | `bin/install-qa-check.d/10executable-issues` | identical | identical | same |
| D-10ignored-flags | `bin/install-qa-check.d/10ignored-flags` | identical | identical | same |
| D-20deprecated-directories | `bin/install-qa-check.d/20deprecated-directories` | identical | identical | same |
| D-20runtime-directories | `bin/install-qa-check.d/20runtime-directories` | identical | identical | same |
| D-60bash-completion | `bin/install-qa-check.d/60bash-completion` | identical | identical | same |
| D-60openrc | `bin/install-qa-check.d/60openrc` | identical | identical | same |
| D-60pkgconfig | `bin/install-qa-check.d/60pkgconfig` | identical | identical | same |
| D-60pngfix | `bin/install-qa-check.d/60pngfix` | identical | identical | same |
| D-60systemd | `bin/install-qa-check.d/60systemd` | identical | identical | same |
| D-60udev | `bin/install-qa-check.d/60udev` | identical | identical | same |
| D-80libraries | `bin/install-qa-check.d/80libraries` | identical | identical | same |
| D-80multilib-strict | `bin/install-qa-check.d/80multilib-strict` | identical | identical | same |
| D-90bad-bin-group-write | `bin/install-qa-check.d/90bad-bin-group-write` | identical | identical | same |
| D-90bad-bin-owner | `bin/install-qa-check.d/90bad-bin-owner` | identical | identical | same |
| D-90cmake-warnings | `bin/install-qa-check.d/90cmake-warnings` | identical | identical | same |
| D-90config-impl-decl | `bin/install-qa-check.d/90config-impl-decl` | identical | identical | same |
| D-90cython-dep | `bin/install-qa-check.d/90cython-dep` | identical | identical | same |
| D-90gcc-warnings | `bin/install-qa-check.d/90gcc-warnings` | identical | identical | same |
| D-90world-writable | `bin/install-qa-check.d/90world-writable` | identical | identical | same |
| D-95empty-dirs | `bin/install-qa-check.d/95empty-dirs` | identical | identical | same |
| D-isolated-functions.sh | `bin/isolated-functions.sh` | identical | identical | same |
| D-misc-functions.sh | `bin/misc-functions.sh` | identical | identical | same |
| D-phase-functions.sh | `bin/phase-functions.sh` | DIFF: header comment + `filtered_vars+=( "PORTUALE_.*" )` + `___save_and_filter_ebuild_env` (brush strategy #2, header-documented) | same DIFF (host lacks it too) | same (intentional) |
| D-phase-helpers.sh | `bin/phase-helpers.sh` | identical | identical | same |
| D-portageq-wrapper | `bin/portageq-wrapper` | OWNED: execs $PORTUALE_BIN portageq (#326 D5, feat#157 S6) | n/a (portuale-only) | same (intentional) |
| D-portuale-python | `bin/portuale-python` | OWNED: execs $PORTUALE_BIN __helper python (#326 D1) | n/a (portuale-only) | same (intentional) |
| D-50xdg-utils | `bin/postinst-qa-check.d/50xdg-utils` | identical | identical | same |
| D-save-ebuild-env.sh | `bin/save-ebuild-env.sh` | identical | identical | same |
| D-version-functions.sh | `bin/version-functions.sh` | identical | identical | same |

Tally S0.2: 74 files under `bin/`: 68 byte-identical to tag AND host,
1 intentional local change (`phase-functions.sh`: brush strategy #2 +
`PORTUALE_.*` filter, both header-documented), 5 portuale-owned shims/doc.
No unintended drift. `diff -r` shape matches plan fact 1.

## S0.3 predicate parity (script `scripts/a-predicates.sh`, captures `a/P/R-*`, table `a/P/predicates-table.tsv`)

Predicate line numbers: `bin/eapi.sh:<line>` per row (`grep -n '^___eapi_'`).
`host-bash` = host bash + host `eapi.sh`; `p-brush` = portuale `--shell brush`;
`p-bash` = portuale `--shell bash`.

| cell | `eapi.sh:line` | host-bash | p-brush | p-bash | verdict |
|------|----------------|-----------|---------|--------|---------|
| P-___eapi_default_src_test_disables_parallel_jobs-7 | :7 | 1 | 1 | 1 | same |
| P-___eapi_has_S_WORKDIR_fallback-7 | :8 | 1 | 1 | 1 | same |
| P-___eapi_has_pkg_pretend-7 | :9 | 0 | 0 | 0 | same |
| P-___eapi_has_src_configure-7 | :10 | 0 | 0 | 0 | same |
| P-___eapi_has_src_prepare-7 | :11 | 0 | 0 | 0 | same |
| P-___eapi_has_BDEPEND-7 | :15 | 0 | 0 | 0 | same |
| P-___eapi_has_BROOT-7 | :16 | 0 | 0 | 0 | same |
| P-___eapi_has_IDEPEND-7 | :17 | 1 | 1 | 1 | same |
| P-___eapi_has_PORTDIR_ECLASSDIR-7 | :18 | 1 | 1 | 1 | same |
| P-___eapi_has_RDEPEND_DEPEND_fallback-7 | :19 | 1 | 1 | 1 | same |
| P-___eapi_has_SYSROOT-7 | :20 | 0 | 0 | 0 | same |
| P-___eapi_has_accumulated_PROPERTIES-7 | :21 | 1 | 1 | 1 | same |
| P-___eapi_has_accumulated_RESTRICT-7 | :22 | 1 | 1 | 1 | same |
| P-___eapi_has_prefix_variables-7 | :23 | 0 | 0 | 0 | same |
| P-___eapi_has_assert-7 | :27 | 0 | 0 | 0 | same |
| P-___eapi_has_docompress-7 | :28 | 0 | 0 | 0 | same |
| P-___eapi_has_dohard-7 | :29 | 1 | 1 | 1 | same |
| P-___eapi_has_doheader-7 | :30 | 0 | 0 | 0 | same |
| P-___eapi_has_dohtml-7 | :31 | 1 | 1 | 1 | same |
| P-___eapi_has_dolib_libopts-7 | :32 | 1 | 1 | 1 | same |
| P-___eapi_has_domo-7 | :33 | 0 | 0 | 0 | same |
| P-___eapi_has_dosed-7 | :34 | 1 | 1 | 1 | same |
| P-___eapi_has_dostrip-7 | :35 | 0 | 0 | 0 | same |
| P-___eapi_has_eapply-7 | :36 | 0 | 0 | 0 | same |
| P-___eapi_has_eapply_user-7 | :37 | 0 | 0 | 0 | same |
| P-___eapi_has_edo-7 | :38 | 1 | 1 | 1 | same |
| P-___eapi_has_einstall-7 | :39 | 1 | 1 | 1 | same |
| P-___eapi_has_einstalldocs-7 | :40 | 0 | 0 | 0 | same |
| P-___eapi_has_get_libdir-7 | :41 | 0 | 0 | 0 | same |
| P-___eapi_has_hasq-7 | :42 | 0 | 0 | 0 | same |
| P-___eapi_has_hasv-7 | :43 | 0 | 0 | 0 | same |
| P-___eapi_has_in_iuse-7 | :44 | 0 | 0 | 0 | same |
| P-___eapi_has_nonfatal-7 | :45 | 0 | 0 | 0 | same |
| P-___eapi_has_pipestatus-7 | :46 | 1 | 1 | 1 | same |
| P-___eapi_has_useq-7 | :47 | 0 | 0 | 0 | same |
| P-___eapi_has_usex-7 | :48 | 0 | 0 | 0 | same |
| P-___eapi_has_ver_replacing-7 | :49 | 1 | 1 | 1 | same |
| P-___eapi_has_version_functions-7 | :50 | 0 | 0 | 0 | same |
| P-___eapi_best_version_and_has_version_support_--host-root-7 | :54 | 1 | 1 | 1 | same |
| P-___eapi_best_version_and_has_version_support_-b_-d_-r-7 | :55 | 0 | 0 | 0 | same |
| P-___eapi_die_can_respect_nonfatal-7 | :56 | 0 | 0 | 0 | same |
| P-___eapi_doconfd_respects_insopts-7 | :57 | 0 | 0 | 0 | same |
| P-___eapi_dodoc_supports_-r-7 | :58 | 0 | 0 | 0 | same |
| P-___eapi_doenvd_respects_insopts-7 | :59 | 0 | 0 | 0 | same |
| P-___eapi_doheader_respects_insopts-7 | :60 | 0 | 0 | 0 | same |
| P-___eapi_doinitd_respects_exeopts-7 | :61 | 0 | 0 | 0 | same |
| P-___eapi_doins_and_newins_preserve_symlinks-7 | :62 | 0 | 0 | 0 | same |
| P-___eapi_domo_respects_into-7 | :63 | 1 | 1 | 1 | same |
| P-___eapi_econf_passes_--datarootdir-7 | :64 | 1 | 1 | 1 | same |
| P-___eapi_econf_passes_--disable-dependency-tracking-7 | :65 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--disable-silent-rules-7 | :66 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--disable-static-7 | :67 | 1 | 1 | 1 | same |
| P-___eapi_econf_passes_--docdir_and_--htmldir-7 | :68 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--with-sysroot-7 | :69 | 0 | 0 | 0 | same |
| P-___eapi_has_DESTTREE_INSDESTTREE-7 | :70 | 1 | 1 | 1 | same |
| P-___eapi_has_dosym_r-7 | :71 | 1 | 1 | 1 | same |
| P-___eapi_helpers_can_die-7 | :72 | 0 | 0 | 0 | same |
| P-___eapi_newins_supports_reading_from_standard_input-7 | :73 | 0 | 0 | 0 | same |
| P-___eapi_unpack_is_case_sensitive-7 | :74 | 1 | 1 | 1 | same |
| P-___eapi_unpack_supports_7z-7 | :75 | 0 | 0 | 0 | same |
| P-___eapi_unpack_supports_absolute_paths-7 | :76 | 0 | 0 | 0 | same |
| P-___eapi_unpack_supports_lha-7 | :77 | 0 | 0 | 0 | same |
| P-___eapi_unpack_supports_rar-7 | :78 | 0 | 0 | 0 | same |
| P-___eapi_unpack_supports_txz-7 | :79 | 0 | 0 | 0 | same |
| P-___eapi_unpack_supports_xz-7 | :80 | 0 | 0 | 0 | same |
| P-___eapi_use_enable_and_use_with_support_empty_third_argument-7 | :81 | 0 | 0 | 0 | same |
| P-___eapi_usev_has_second_arg-7 | :82 | 1 | 1 | 1 | same |
| P-___eapi_bash_3_2-7 | :86 | 1 | 1 | 1 | same |
| P-___eapi_bash_4_2-7 | :87 | 0 | 0 | 0 | same |
| P-___eapi_bash_5_0-7 | :88 | 1 | 1 | 1 | same |
| P-___eapi_bash_5_3-7 | :89 | 1 | 1 | 1 | same |
| P-___eapi_enables_failglob_in_global_scope-7 | :90 | 0 | 0 | 0 | same |
| P-___eapi_has_ENV_UNSET-7 | :91 | 0 | 0 | 0 | same |
| P-___eapi_has_strict_keepdir-7 | :92 | 1 | 1 | 1 | same |
| P-___eapi_default_src_test_disables_parallel_jobs-8 | :7 | 1 | 1 | 1 | same |
| P-___eapi_has_S_WORKDIR_fallback-8 | :8 | 1 | 1 | 1 | same |
| P-___eapi_has_pkg_pretend-8 | :9 | 0 | 0 | 0 | same |
| P-___eapi_has_src_configure-8 | :10 | 0 | 0 | 0 | same |
| P-___eapi_has_src_prepare-8 | :11 | 0 | 0 | 0 | same |
| P-___eapi_has_BDEPEND-8 | :15 | 0 | 0 | 0 | same |
| P-___eapi_has_BROOT-8 | :16 | 0 | 0 | 0 | same |
| P-___eapi_has_IDEPEND-8 | :17 | 0 | 0 | 0 | same |
| P-___eapi_has_PORTDIR_ECLASSDIR-8 | :18 | 1 | 1 | 1 | same |
| P-___eapi_has_RDEPEND_DEPEND_fallback-8 | :19 | 1 | 1 | 1 | same |
| P-___eapi_has_SYSROOT-8 | :20 | 0 | 0 | 0 | same |
| P-___eapi_has_accumulated_PROPERTIES-8 | :21 | 0 | 0 | 0 | same |
| P-___eapi_has_accumulated_RESTRICT-8 | :22 | 0 | 0 | 0 | same |
| P-___eapi_has_prefix_variables-8 | :23 | 0 | 0 | 0 | same |
| P-___eapi_has_assert-8 | :27 | 0 | 0 | 0 | same |
| P-___eapi_has_docompress-8 | :28 | 0 | 0 | 0 | same |
| P-___eapi_has_dohard-8 | :29 | 1 | 1 | 1 | same |
| P-___eapi_has_doheader-8 | :30 | 0 | 0 | 0 | same |
| P-___eapi_has_dohtml-8 | :31 | 1 | 1 | 1 | same |
| P-___eapi_has_dolib_libopts-8 | :32 | 1 | 1 | 1 | same |
| P-___eapi_has_domo-8 | :33 | 0 | 0 | 0 | same |
| P-___eapi_has_dosed-8 | :34 | 1 | 1 | 1 | same |
| P-___eapi_has_dostrip-8 | :35 | 0 | 0 | 0 | same |
| P-___eapi_has_eapply-8 | :36 | 0 | 0 | 0 | same |
| P-___eapi_has_eapply_user-8 | :37 | 0 | 0 | 0 | same |
| P-___eapi_has_edo-8 | :38 | 1 | 1 | 1 | same |
| P-___eapi_has_einstall-8 | :39 | 1 | 1 | 1 | same |
| P-___eapi_has_einstalldocs-8 | :40 | 0 | 0 | 0 | same |
| P-___eapi_has_get_libdir-8 | :41 | 0 | 0 | 0 | same |
| P-___eapi_has_hasq-8 | :42 | 1 | 1 | 1 | same |
| P-___eapi_has_hasv-8 | :43 | 1 | 1 | 1 | same |
| P-___eapi_has_in_iuse-8 | :44 | 0 | 0 | 0 | same |
| P-___eapi_has_nonfatal-8 | :45 | 0 | 0 | 0 | same |
| P-___eapi_has_pipestatus-8 | :46 | 1 | 1 | 1 | same |
| P-___eapi_has_useq-8 | :47 | 1 | 1 | 1 | same |
| P-___eapi_has_usex-8 | :48 | 0 | 0 | 0 | same |
| P-___eapi_has_ver_replacing-8 | :49 | 1 | 1 | 1 | same |
| P-___eapi_has_version_functions-8 | :50 | 0 | 0 | 0 | same |
| P-___eapi_best_version_and_has_version_support_--host-root-8 | :54 | 1 | 1 | 1 | same |
| P-___eapi_best_version_and_has_version_support_-b_-d_-r-8 | :55 | 0 | 0 | 0 | same |
| P-___eapi_die_can_respect_nonfatal-8 | :56 | 0 | 0 | 0 | same |
| P-___eapi_doconfd_respects_insopts-8 | :57 | 1 | 1 | 1 | same |
| P-___eapi_dodoc_supports_-r-8 | :58 | 0 | 0 | 0 | same |
| P-___eapi_doenvd_respects_insopts-8 | :59 | 1 | 1 | 1 | same |
| P-___eapi_doheader_respects_insopts-8 | :60 | 1 | 1 | 1 | same |
| P-___eapi_doinitd_respects_exeopts-8 | :61 | 1 | 1 | 1 | same |
| P-___eapi_doins_and_newins_preserve_symlinks-8 | :62 | 0 | 0 | 0 | same |
| P-___eapi_domo_respects_into-8 | :63 | 1 | 1 | 1 | same |
| P-___eapi_econf_passes_--datarootdir-8 | :64 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--disable-dependency-tracking-8 | :65 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--disable-silent-rules-8 | :66 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--disable-static-8 | :67 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--docdir_and_--htmldir-8 | :68 | 0 | 0 | 0 | same |
| P-___eapi_econf_passes_--with-sysroot-8 | :69 | 0 | 0 | 0 | same |
| P-___eapi_has_DESTTREE_INSDESTTREE-8 | :70 | 1 | 1 | 1 | same |
| P-___eapi_has_dosym_r-8 | :71 | 0 | 0 | 0 | same |
| P-___eapi_helpers_can_die-8 | :72 | 0 | 0 | 0 | same |
| P-___eapi_newins_supports_reading_from_standard_input-8 | :73 | 0 | 0 | 0 | same |
| P-___eapi_unpack_is_case_sensitive-8 | :74 | 1 | 1 | 1 | same |
| P-___eapi_unpack_supports_7z-8 | :75 | 1 | 1 | 1 | same |
| P-___eapi_unpack_supports_absolute_paths-8 | :76 | 0 | 0 | 0 | same |
| P-___eapi_unpack_supports_lha-8 | :77 | 1 | 1 | 1 | same |
| P-___eapi_unpack_supports_rar-8 | :78 | 1 | 1 | 1 | same |
| P-___eapi_unpack_supports_txz-8 | :79 | 0 | 0 | 0 | same |
| P-___eapi_unpack_supports_xz-8 | :80 | 0 | 0 | 0 | same |
| P-___eapi_use_enable_and_use_with_support_empty_third_argument-8 | :81 | 0 | 0 | 0 | same |
| P-___eapi_usev_has_second_arg-8 | :82 | 0 | 0 | 0 | same |
| P-___eapi_bash_3_2-8 | :86 | 1 | 1 | 1 | same |
| P-___eapi_bash_4_2-8 | :87 | 1 | 1 | 1 | same |
| P-___eapi_bash_5_0-8 | :88 | 0 | 0 | 0 | same |
| P-___eapi_bash_5_3-8 | :89 | 1 | 1 | 1 | same |
| P-___eapi_enables_failglob_in_global_scope-8 | :90 | 0 | 0 | 0 | same |
| P-___eapi_has_ENV_UNSET-8 | :91 | 0 | 0 | 0 | same |
| P-___eapi_has_strict_keepdir-8 | :92 | 0 | 0 | 0 | same |
| P-___eapi_default_src_test_disables_parallel_jobs-99 | :7 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_S_WORKDIR_fallback-99 | :8 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_pkg_pretend-99 | :9 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_src_configure-99 | :10 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_src_prepare-99 | :11 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_BDEPEND-99 | :15 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_BROOT-99 | :16 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_IDEPEND-99 | :17 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_PORTDIR_ECLASSDIR-99 | :18 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_RDEPEND_DEPEND_fallback-99 | :19 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_SYSROOT-99 | :20 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_accumulated_PROPERTIES-99 | :21 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_accumulated_RESTRICT-99 | :22 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_prefix_variables-99 | :23 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_assert-99 | :27 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_docompress-99 | :28 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_dohard-99 | :29 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_doheader-99 | :30 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_dohtml-99 | :31 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_dolib_libopts-99 | :32 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_domo-99 | :33 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_dosed-99 | :34 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_dostrip-99 | :35 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_eapply-99 | :36 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_eapply_user-99 | :37 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_edo-99 | :38 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_einstall-99 | :39 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_einstalldocs-99 | :40 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_get_libdir-99 | :41 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_hasq-99 | :42 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_hasv-99 | :43 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_in_iuse-99 | :44 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_nonfatal-99 | :45 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_pipestatus-99 | :46 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_useq-99 | :47 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_usex-99 | :48 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_ver_replacing-99 | :49 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_version_functions-99 | :50 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_best_version_and_has_version_support_--host-root-99 | :54 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_best_version_and_has_version_support_-b_-d_-r-99 | :55 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_die_can_respect_nonfatal-99 | :56 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_doconfd_respects_insopts-99 | :57 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_dodoc_supports_-r-99 | :58 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_doenvd_respects_insopts-99 | :59 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_doheader_respects_insopts-99 | :60 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_doinitd_respects_exeopts-99 | :61 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_doins_and_newins_preserve_symlinks-99 | :62 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_domo_respects_into-99 | :63 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_econf_passes_--datarootdir-99 | :64 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_econf_passes_--disable-dependency-tracking-99 | :65 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_econf_passes_--disable-silent-rules-99 | :66 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_econf_passes_--disable-static-99 | :67 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_econf_passes_--docdir_and_--htmldir-99 | :68 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_econf_passes_--with-sysroot-99 | :69 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_DESTTREE_INSDESTTREE-99 | :70 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_dosym_r-99 | :71 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_helpers_can_die-99 | :72 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_newins_supports_reading_from_standard_input-99 | :73 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_unpack_is_case_sensitive-99 | :74 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_unpack_supports_7z-99 | :75 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_unpack_supports_absolute_paths-99 | :76 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_unpack_supports_lha-99 | :77 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_unpack_supports_rar-99 | :78 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_unpack_supports_txz-99 | :79 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_unpack_supports_xz-99 | :80 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_use_enable_and_use_with_support_empty_third_argument-99 | :81 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_usev_has_second_arg-99 | :82 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_bash_3_2-99 | :86 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_bash_4_2-99 | :87 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_bash_5_0-99 | :88 | 1 | - | 1 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_bash_5_3-99 | :89 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_enables_failglob_in_global_scope-99 | :90 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_ENV_UNSET-99 | :91 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |
| P-___eapi_has_strict_keepdir-99 | :92 | 0 | - | 0 | not reproducible (brush `__check_bash_version` gate; bodies equal per arg99 runs) |

Tally S0.3: 222 cells — `same` 148 (all of EAPI 7 and 8 on all three runners,
`diff` clean both ways), `not reproducible` 74 (all of EAPI 99: brush dies in
`__check_bash_version`, `bin/ebuild.sh:15-51`, before any phase body runs),
`diverges` 0.

The EAPI-99 brush finding, in full (brush-vs-bash, NOT EAPI semantics):

- `R-portuale-brush-99` (rc 1, 0 PRED lines): `EAPI 99 requires >=bash-5.3
  but only 5.2 was found` (`ebuild.sh:75 Called __check_bash_version`).
  `___eapi_bash_5_3` (`bin/eapi.sh:89`, true for any EAPI not in 0–8) raises
  the floor to 5.3; embedded brush reports `BASH_VERSINFO` 5.2 while host
  bash is 5.3 (`bash -c 'echo ${BASH_VERSINFO[*]:0:2}'` → `5 3`).
  On a bash-5.2 host real Portage would die identically (same gate), so this
  is brush's emulated bash version showing through, not an EAPI rule portuale
  got wrong. At floor 7/8 the gate never fires (EAPI 8 needs ≥5.0).
- Body parity for 99 proven anyway: `R-{host-bash,portuale-bash,
  portuale-brush}-arg99` (explicit `"${p}" 99` form, 74/74 PREDs each) are
  mutually identical AND identical to host no-arg `R-host-bash-99`. The
  `${1-${EAPI-0}}` default-expansion path is covered by the 7/8 no-arg runs.
- Side observation (both shells agree): `EAPI` is readonly in the phase env
  (`R-portuale-bash-override99`: `EAPI: readonly variable`;
  `R-portuale-brush-override99`: `cannot mutate readonly variable`).

## Owner decisions (recorded, not asked — default proposals)

1. S0.2 shebang (`filter-bash-environment.py`: `env python` vs host
   `python3.14`): no action — matches the tag; host line is a distro
   rewrite. Default: record as intentional, never "sync to host".
2. `P-*-99` brush gate: brush emulates bash 5.2, host bash is 5.3. If brush
   ever reports 5.3, the 99 gate stops firing under brush. Default: no
   portuale-side fix (out of EAPI scope; belongs to `docs/brush-pr/`
   version-emulation tracking, if anywhere); keep the 74 cells
   `not reproducible` with the arg99 body-parity evidence.
3. S0.1 `missing` attrs that are behaviorally silent at floor 7/8 either way
   (A7 `EBUILD_PHASE_FUNC`, A8/A12 unset-vars, A9 `KV`): default is a
   dedicated S1 slice that adds the fields + the two real exports
   (`MERGE_TYPE=source` for source phases — A10/C2; `EBUILD_PHASE_FUNC` —
   A7) while leaving the always-absent ones absent with a test pinning the
   absence. Owner decides whether absence-pins are worth it.
4. A14 (`idepend`: 7 ignores, 8 reduces) vs `post_install_meta.rs` reducing
   IDEPEND unconditionally; A25 (no REQUIRED_USE gate found); A35 (no
   symlink-rewrite site found); A3/A36 (no BROOT/SYSROOT/ESYSROOT in phase
   env): these need S0.4/S0.6 capture cells before any fix slice — default:
   S0.8 routes each to its cluster (S2/S4/S6) rather than fixing from this
   inventory.

## Not reproducible (this worker)

- `P-<pred>-99` on `portuale --shell brush` via any phase path: `EAPI` is
  readonly in-phase (both shells) and `ebuild.sh:75` dies first (brush bash
  5.2 < required 5.3). Reason recorded above; bodies covered by arg99 runs.
- Nothing else: every other cell ran green first try (host `ebuild(1)` repo
  scaffolding deliberately NOT used — pure predicates need no repo; the
  `portuale ebuild <file> setup` path needs only the `<pn>/<pn>-<ver>.ebuild`
  layout, verified with the smoke probe).

## Files created (no commits, no fixture/repo touches)

- `LLM/evidence/2026-10-12-348-s0/a-attrs-drift-predicates.md` (this file)
- `LLM/evidence/2026-10-12-348-s0/scripts/a-drift.sh` (+ `a-predicates.sh`)
- `LLM/evidence/2026-10-12-348-s0/a/s01-live-dump.txt` (live 7/8 values),
  `a/s01-consumers-grep.txt` (consumer grep log), `a/drift-tag.txt`,
  `a/drift-host.txt`, `a/s02-table.md`, `a/P/R-*` (11 runs ×
  `.argv/.rc/.stdout/.stderr`), `a/P/predicates-table.tsv` (222 rows)
- Scratch only in `/var/tmp/t348-a/` (probes, per-run tmpdirs, loop scripts)
