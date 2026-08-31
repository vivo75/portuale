# Scope backlog

This is **not** a Python-vs-Rust parity backlog. Every slice in this pilot is
implemented on both sides in the same commit and verified byte-for-byte
identical via the shared contract suite before being considered done
(`PORTING/PROMPT.md`'s own "portability of change, not of source" hard goal).
733 contract tests pass as of this writing; an inventory scan (CLI flag
tables, function-level architecture, `--json` fields, git history) still
finds zero Rust-vs-Python gaps.

What this file inventories is real portage behavior this pilot hasn't ported
to **either** side yet — deliberate, documented scope cuts or explicit
`PROMPT.md` architecture boundaries.

> **Re-derived 2026-08-27** against current source (`portage-repo`,
> `portage-profile`, `portage-use-reduce`, `portuale/src/fetch.rs`,
> `git log`), not carried forward from the previous version. The original
> file was written 2026-08-17 (commit `578246278`) and never updated across
> the ~90 `PORTING/` commits since; **almost every item it listed has since
> shipped**. Part 1 records what closed; Part 2 is the genuinely-remaining
> work; Part 3 is the explicit non-goals. Re-verify against
> `README.md`/`git log`/the actual source before trusting even this version.

---

## Part 1 — shipped since 2026-08-17 (the original 21 items)

| # | Original item | Status | Landed by |
|---|---|---|---|
| 1 | Sub-slot modeling (`SLOT="0/5"`) | **shipped** | `9c926033f` (`Candidate::sub_slot`, real `_match_slot` sub-slot check; fixed a silent dependency-match bug) |
| 2 | Structured (non-flat) `use_reduce` | **shipped for the depgraph** | `59237ccbb` (`||` groups: resolve only the first satisfiable alternative) + `3ca7a66b4` (`subset=` for `--with-test-deps`). `DepNode`/`build_dep_tree`/`use_reduce_flat_disjunctive` wired into both real dep-walk sites (`lib.rs:5266`/`5569`); real `flat=False`/`opconvert=False` (`use_reduce_structured`) shipped 2026-08-27 for `--changed-deps` (see #4). `opconvert` (operator-into-arg-list shape) genuinely never needed. |
| 3 | `repos.conf` `masters` (repo inheritance) | **shipped** | `04601e1a9` (implicit main-repo default) + explicit `masters =` chain resolution (`RepoConfig::masters`, `find_repos`) + `f7057b159`/`5a7bbeff7` (eclass `inherit()` across the masters chain). *Residual:* `layout.conf`'s own `masters =` key and `profile-formats` gating — see Part 2. |
| 4 | Per-level/per-source config precedence (real `USE_ORDER`) | **partly shipped** | `6fa34677f`/`992a82117`/`5f7c6f059` (real `USE_ORDER` precedence for global force/mask, implicit IUSE, `+/-` defaults). `package.mask`/`.unmask`/`.accept_keywords` now stack per-source (`stack_mask_lines`, `[repo, profile-chain, user]`). *Residual:* `package.use`'s full `configdict["repo"]`/`["defaults"]` per-level interleaving with each level's `make.defaults`, and the `env`/`pkginternal`/`features`/`env.d` `USE_ORDER` layers — see Part 2. |
| 5 | `--changed-slot` | **shipped** | `97a27a317` (`slot_changed`, real `_changed_slot`, as an independent `Reinstall` trigger) |
| 6 | `--with-test-deps` | **shipped** | `3ca7a66b4` (real `use_reduce` `subset={"test"}` via `use_reduce_flat_subset`) |
| 7 | Overlay repos' own `package.mask`/`.unmask`/`profiles`/`license_groups` | **shipped** | `b6c386ef6` + `6e368f28f` (overlay `package.mask`/`.unmask`, `::repo`-auto-scoped) + `9a03b8734` (overlay `package.use`/`.mask`/`.force`/`.stable.*`). `license_groups` **fixed 2026-08-30** (real-tree finding): was read per-profile-chain-level, but real `LicenseManager` reads `<repo>/profiles/license_groups` for the main repo + every overlay (`LocationsManager.profile_locations`, `:432`) — real gentoo never puts the file in a `profiles/<foo>/` dir, so `@FREE`/`@EULA` expanded to nothing and every ebuild failed its license check on a real tree. `resolve_config` now reads it repo-level (main then overlays). |
| 8 | `package.use`'s own full `USE_ORDER` precedence | **partly shipped** — same residual as #4 | `9a03b8734` (all three sources stacked, flat model) |
| 9 | `--deselect` world_sets/custom-set integration | **shipped** | `2ba3c8a5f` (`emerge --deselect @set` against the combined `world_set`) |
| 9b | Real `Atom.intersects()` algebra for `--deselect` | **shipped** | `7406bae50` (dropped the narrower category/package check + a bogus installed-check) |
| 10 | Cross-repo profile parents (`reponame:path` / bare `:path`) | **shipped** | `afd1a210c` (`expand_parent_colon`/`repo_containing`, real `LocationsManager._expand_parent_colon`) |
| 11 | `USE_EXPAND` corners | **shipped** | `66a8a7703` (`USE_EXPAND_UNPREFIXED`) + `USE_EXPAND_IMPLICIT`/`IUSE_IMPLICIT`/`IUSE_EFFECTIVE` (2026-08-27) + IUSE-aware `_*` wildcard expansion (2026-08-27) + `USE_EXPAND` grouping in `emerge -pv` + `USE_EXPAND_HIDDEN` + installed-vs-new `*`/`%` markers + the `( … )` forced/masked wrap (2026-08-27, `Config::use_expand_hidden` + `build_use_expand_display` + `forced_or_masked_flags`). No residual (real `-pv` ANSI color / `--all-flags` line tracked under A.2). |
| 12 | `accept_keywords_defaults` bare-atom substitution | **shipped** | `743cd9b4a` (bare `package.accept_keywords` atom → implicit `~arch` at both profile and user level) |
| 13 | `strip_libc_deps` in `--changed-deps` | **shipped** | `b29600063` |
| 14 | `--changed-deps-report` | **shipped** | `69ca60846` (real cosmetic "you might want `--changed-deps`" notice, its own `--json` `changed_deps_report` array) |
| 15 | `--with-bdeps-auto` | **shipped** | `c505df6eb` |
| 16 | Real atom-grammar wildcards/build-ids | **descoped** (not a gap) | Decision recorded: the bounded `*/*`/`category/*`/`*/package` matcher is sufficient for `package.mask`-style matching; full wildcard/glob/build-id atoms never reach `DEPEND`/`RDEPEND` parsing. Not on the backlog anymore. |
| 17 | `--autounmask*` family | **keyword + USE resolution shipped** | `2003e020d` + `927402f3f` (suggestion mode) → superseded 2026-08-30 by real *resolution* for both the **keyword** (`--autounmask <kwmasked>` → `The following keyword changes are necessary to proceed:` block, exit 0) and **USE** (`--autounmask-use`, on by default → `The following USE changes are necessary to proceed:` + `-pv` USE line reflects the flip) kinds (`resolve_pretend` `autounmask_keywords`/`autounmask_use` params, `GraphResult::autounmask_keyword_changes`/`autounmask_use_changes`). The **`opt=` parent flip** (increment 3) **shipped 2026-08-30**: when a dep's `opt?`/`opt=` use-dep can't be satisfied AND the child's flag is `use.mask`'d (so no child flip), `resolve_pretend_graph` flips the *requesting parent's* flag, re-resolves the freed dep, records `>=<parent-cpv> -flag` (new `dev-libs/parentflip{childpkg,eqpkg}` fixtures). Cuts: re-resolves only the freed dep not the whole graph; parent's one-level dep chain; a non-`New` parent re-renders as `New`. *Residual:* `--autounmask-license`, `--autounmask-write` (writes files — `PROMPT.md` "never writes", Part 3). |
| 18 | `--root-deps`/cross-`ROOT` dependency resolution | **substantially shipped** | Real `ESYSROOT`-vs-`ROOT` distinction, `running_root_satisfies_atom`, `||` branch selection fed by running-root satisfiability, `93327d274`, `356088e6c` (recursive build-entry, first increment), `678a8875d` (output marking), top-level `IDEPEND` vs running root. *Residual:* see Part 2. |
| 19 | Binary package support | **local `PKGDIR` shipped** | `3099d9adf` (`--usepkg`/`--usepkgonly`/`--binpkg-respect-use`) + `96d8fbccb` (`--usepkg-exclude`/`-include`) + `0ae1f8be6` (`--rebuilt-binaries`) + `0b18b2140` (downgrade detection) + `7e5a380d7` (real `ebuild … package` builds an xpak binpkg) + real `PORTAGE_COMPRESSION_COMMAND` + `--pretend` half of `--getbinpkg`/`--getbinpkgonly` (`binrepos.conf` + `PORTAGE_BINHOST` parsing, remote binhost `Packages`-index candidates, the `g` bracket column, `Size of downloads:` from the index `SIZE`). *Residual (all since shipped or narrowed — see Part 2 item 6):* remote download + merge, `--getbinpkg` mixed source+binary merge, `gpkg` format all done; still open: `layout.conf` negotiation, `Packages.gz`, `BUILD_ID`/splitdebug/packdebug/RPM, PKGDIR-index locking. |
| 20 | Real ebuild phase execution | **shipped** | `eeecd96cd` (the `actionmap_deps` phase chain via embedded `brush`) + `2f5a3ddad`/`39907fee6` |
| 21 | Real merge/install/filesystem mutation | **shipped** | `2f5a3ddad` (`merge`) + `2a52f7d88` (`unmerge`) + `qmerge`/`config`/`info`/`prerm`/`postrm` + real `CONFIG_PROTECT`/`collision-protect`/`preserve-libs`/`env_update`; **`emerge <atom>` source build + merge shipped 2026-08-31** (`emerge_build::run_source_merge` → `ebuild_merge::run_merge` per resolved entry; `MergeOptions::from_env`) — **`New` + `Upgrade`/`Downgrade`/`Reinstall`** (an in-place same-slot replace unmerges the old version: `ebuild_merge::unmerge_replaced_same_slot`, factored out of `merge_binpkg`, also wired into `merge_after_install` so `ebuild <file> merge` of v2 over v1 stops orphaning v1's files); v1 cuts: no preserve-libs/reverse-dep check on the replace. **`--getbinpkg` mixed source+binary merge** + **world-file recording** (real `Scheduler._world_atom` — a successful `emerge <atom>` records the target as `cat/pkg` in `var/lib/portage/world`; `--oneshot`/`-1` now implemented, suppresses that + the `--pretend` world colour) both shipped 2026-08-31. World-file v1 cuts: `cat/pkg`-granular (no slot atoms), `@set` targets not added to `world_sets`. **`--keep-going` for `emerge <atom>` / `--getbinpkg` shipped 2026-08-31** (`emerge_build::run_merge_loop`: on a merge failure BFS-drop the failed entry's transitive dependents via `GraphEntry.required_by`, merge the rest, exit non-zero with a combined failed+skipped report — real `Scheduler._calc_resume_list`) |

---

## Part 2 — genuinely still open

Ranked roughly by how self-contained each is.

### 0. `emerge` cleanup actions

- **`emerge --pretend --unmerge` / `-pC`** **shipped 2026-08-27**
  (`run_unmerge_pretend`, real `_emerge/unmerge.py::_unmerge_display` for
  `unmerge_action == "unmerge"`): atom → vdb match → `selected`, other
  installed versions → `omitted`, `sys-apps/portage` self-`protected`,
  the "is part of your system profile" + "still listed in the following
  package sets" warnings (`collect_installed_sets`) -- `_unmerge_display`
  is complete for `unmerge_action == "unmerge"`.
  **Real (non-`--pretend`) `emerge -C <atom>` removal shipped 2026-08-31**
  (`pretend.rs::execute_unmerge` + `ebuild_merge::unmerge_one_installed`,
  factored out of `unmerge_replaced_same_slot`): after the
  `_unmerge_display` preview (its `>>> These are the packages that would
  be unmerged:` header now correctly `--pretend`/`--ask`-gated), each
  `selected` version's `pkg_prerm` (from its own vdb-saved env) → files
  → `pkg_postrm` → vdb dir removal, `>>> Unmerging (N of M) <cpv>...`
  per package, then `deselect_from_world` (real
  `WorldSelectedPackagesSet.cleanPackage`). The old
  `--unmerge requires --pretend` gate is gone. v1 cuts: no `CLEAN_DELAY`
  countdown, no `--ask`.
  **`--depclean`/`--prune`/`--prune --nodeps` real removal shipped the
  same day** -- see their own bullets below.
  **`FEATURES=unmerge-backup` shipped 2026-08-31**: real `dblink._pre_
  unmerge_backup` -> `ebuild_package::quickpkg_from_vdb` (real
  `dblink.quickpkg` -- stage the vdb `CONTENTS` files from `${ROOT}` into
  an `image/` dir, copy the vdb dir as `build-info/`, run the same real
  `bin/misc-functions.sh __dyn_package` `ebuild <file> package` uses, via
  the new `invoke_dyn_package`; `$PKGDIR/Packages` entry from the vdb's
  own build-info). Wired into `unmerge_one_installed` (`backup:
  Option<&PackageOptions>`) for the standalone `-C`/`--depclean`/`--prune`
  paths; a quickpkg failure aborts that unmerge. v1 cuts: the
  `treewalk()` replace-loop `_pre_merge_backup`/`FEATURES=downgrade-backup`
  path, the real `BUILD_TIME` idempotency check (narrowed to file
  existence), `fif`/`dev` `CONTENTS` nodes.
- **`emerge --pretend --depclean` / `-pc`** **shipped 2026-08-27**
  (`depclean_cleanlist`: the reachability closure over the installed
  `RDEPEND`/`PDEPEND`/`DEPEND`/`BDEPEND` graph; `run_depclean_pretend`:
  advisory block,
  `>>> Calculating removal order...`, the `-pC` per-package block on the
  cleanlist, the `Number to remove:` stats block). Both the no-args
  full form and the **`--depclean <atoms>` narrowing** (world atoms
  dropped, non-arg installed packages protected, `--- Couldn't find`,
  bare-name resolution). Build-time-dep edges (`bdeps="auto"` for remove
  mode -- `DEPEND`/`BDEPEND` kept unless `--with-bdeps=n`) **shipped
  2026-08-27**. The **topological removal-order sort**
  (`topological_removal_order`, real `actions.py:1591-1731` -- each
  package unmerged before the ones it depends on; `run_unmerge_pretend`
  gained a `preserve_order` flag) **shipped 2026-08-27** too, bar the
  slot-operator-built priority bump and the cycle-breaking single pop.
  The **`--verbose` reverse-dependency display** (real `show_parents`,
  `create_cleanlist:1324`/`1331` -- `<cpv> pulled in by: <parent>
  requires <atom>` for every kept package; `DepcleanResult.kept_parents`
  from a `_parent_atoms`-recording BFS; also suppresses the "To see
  reverse dependencies" hint) **shipped 2026-08-28**.
  `package.provided` (the general resolver behavior -- a listed CPV
  satisfies a dependency atom silently / triggers the `WARNING:` block
  for a direct target) **shipped 2026-08-29** (`Config::package_provided`,
  `GraphResult::pprovided_atoms`, real `config.py:970-1027` +
  `dep_check.py:1052` + `depgraph.py:5497-5615`/`11192-11235`); the
  depclean-specific corner (a provided entry as a depclean root, and the
  advisory's "will be removed by depclean even if in world" claim) is a
  separate, minor residual.
  `--depclean-lib-check` (the `NEEDED.ELF.2` soname-consumer scan that
  keeps a cleanlist package a surviving binary still links against, plus
  the second-pass graph re-closure and the `* ...will not be removed`
  WARNING; wires up the previously-dead `needed_elf` module; `=n` skips
  it and shows the `Depclean may break link level dependencies`
  advisory) **shipped 2026-08-29** for both `--depclean` and `--prune`.
  The "dependencies could not be completely resolved" safety halt
  (real `unresolved_deps()`, `actions.py:1137-1248` -- a kept package's
  unsatisfiable hard runtime dep prints the `bad(" * ")` block and exits
  1 without removing anything; `DepcleanResult::unresolved` via
  `unresolved_runtime_deps`; `||`-group and libc-provider atoms narrowed
  out) **shipped 2026-08-29** for both. `--deselect=n` in args mode
  (real `action_depclean`'s `deselect = myopts.get("--deselect") !=
  "n"` -- `-pc <atoms> --deselect=n` keeps `world` as a protection
  root; `depclean_cleanlist` `deselect` param; also fixed `--deselect`
  wrongly triggering the standalone action alongside `--depclean`/
  `--prune`/`--unmerge`) **shipped 2026-08-30**. **Real (non-`--pretend`)
  `emerge --depclean` removal shipped 2026-08-31**: `run_depclean_pretend`
  gained a `pretend: bool` and passes the real flag to the shared
  `run_unmerge_pretend`, which runs `execute_unmerge` (the `-C` slice's
  machinery) after the preview -- real `action_depclean` feeds its
  cleanlist to the very same `unmerge()`. Safety halt + `--depclean-lib-
  check` still gate removal; stats block reads `Number removed:`. **Still
  open:** the depclean-specific slot-operator rebuild interaction (see
  A.7 for the general resolver rebuild), the exact `@selected`-vs-`@world`
  set nesting (approximated), the "Broken soname dependencies found"
  *warning* half of `unresolved_deps()` (no soname deps in this pilot's
  RDEPEND).
- **`emerge -p --prune` / `-pP`** **shipped 2026-08-27**
  (`prune_cleanlist`: seed the closure from every installed package
  except the non-highest-in-cp ones an `args_set` matches -- `args_set`
  auto-fills with every multi-version cp; `run_prune_pretend`: no
  advisory block, no stats block, the `--nodeps` hint line, shared
  `resolve_cleanup_args`). The `--verbose` `show_parents` display
  (real `create_cleanlist`'s prune branch, `actions.py:1339` -- only for
  an `args_set`-matched kept version with a real `Package` parent;
  shared `render_show_parents`) **shipped 2026-08-28**. `--prune
  --nodeps` (routes to `_unmerge_display`'s own prune branch instead of
  `_calc_depclean` -- no dep check at all; `prune_nodeps_selection` +
  `run_prune_nodeps_pretend`; best-version `COUNTER` tiebreak narrowed
  out) **shipped 2026-08-28**. `--depclean-lib-check` (shared with
  `--depclean` -- see above) **shipped 2026-08-29**. **Real
  (non-`--pretend`) `emerge --prune` / `--prune --nodeps` removal shipped
  2026-08-31** (same `pretend: bool` wiring as `--depclean`;
  `run_prune_nodeps_pretend` builds its own `removal_list` and calls
  `execute_unmerge` directly, real `actions.py:2684` routing `prune
  --nodeps` through the same `unmerge()` `-C` uses). **Still open:** the depclean-specific slot-operator rebuild interaction (see A.7 for the general resolver rebuild).
- Minor `-pC` narrowings: ~~the higher-slot refinement on the
  set-protection warning~~ **shipped 2026-08-30** (real
  `unmerge.py:421-441`'s `higher_slot`: the "still listed in package
  sets" warning is suppressed for a set when an installed newer version
  of the same cp in a *different slot* also matches the set atom; shared
  `still_listed_parents` used by both `-pC` and `-pP`; new
  `dev-libs/dualslotpkg` dual-slot fixture). The "currently used Python
  interpreter" self-skip is a **non-gap** for this pilot -- its `emerge`
  is a Rust binary with no Python interpreter of its own to protect.
  The literal vdb-path argument (`emerge -C /var/db/pkg/cat/pkg-ver`,
  `unmerge.py:137-182` -- `resolve_vdb_path_arg`) **shipped 2026-08-28**.
- **`emerge --config <atom>`** (real `action_config`) **shipped
  2026-08-31**: `pretend.rs::run_config_action` -- exactly one atom,
  vdb-matched like `--unmerge`; 0 -> `No packages found.` exit 0, >1 ->
  `The following packages available:` exit 1, 1 -> `Configuring pkg...`
  + `pkg_config` from the vdb-saved env
  (`ebuild_merge::run_vdb_saved_env_phase`, factored out of
  `unmerge_one_installed`) + a best-effort builddir clean. Ignores
  `--pretend`. v1 cuts: `--ask` interactive picker/prompt, `elog`. New
  `dev-libs/emergeconfigpkg` fixture. **Still open:** `emerge --info`
  (real `action_info` -- the big config/env dump), `emerge --search`/
  `-s`, `--regen`, `--sync`, `--metadata`, `--check-news`, `--clean`.

### A. Small, self-contained dry-run/config slices

1. ~~**`layout.conf`'s own `masters =` key** (and `profile-formats`
   gating, and `repo-name`)~~ **shipped 2026-08-27**: `find_repos` now
   parses `<repo>/metadata/layout.conf` -- `masters =` is a middle tier
   (repos.conf wins, layout.conf next, implicit main-repo default last);
   `repo-name` overrides the section name (`RepoConfig::name`);
   `profile-formats = portage-2` gates the cross-repo `parent`-colon
   syntax (`RepoConfig::profile_formats` -> `resolve_config`'s own
   `repo_profile_formats` read -> `expand_parent_colon`). Real portage's
   EAPI-conditional `profile-formats` default when the key is absent is
   not modeled (absent = "no portage-2"). **Also shipped, same day**:
   `profiles/repo_name` as the canonical name source (precedence
   `layout.conf repo-name` > `profiles/repo_name` > section), `aliases`
   (`repos.conf` + `layout.conf`), and the real section-name mismatch
   **drop** (`config.py:1121` -- a repo whose name != its section name,
   with no alias covering it, is dropped with a `!!! Section ...` error).
   ~~**Residual:** `::alias` atom matching / `alias:path` profile
   parents still use the canonical name only.~~ **Resolved 2026-08-30**:
   `alias:path` **profile parents** now resolve through the alias
   (`resolve_config` gained a `repo_aliases` param -> `expand_parent_
   colon`, real `repositories.get_location_for_name`). `::alias` in an
   **atom** was NOT a gap -- real `match_from_list` does a straight
   `pkg.repo == atom.repo` name comparison with no alias step
   (`dep/__init__.py:3201`), and the pilot (both sides, via the real
   `portage.dep.match_from_list` the Python reference calls) already
   matched that (fixture `dev-libs/repnamepkg::repnamesection`).

2. ~~**`USE_EXPAND_HIDDEN` / `USE_EXPAND_IMPLICIT`.**~~ `USE_EXPAND_IMPLICIT`
   **shipped 2026-08-27** (`Config::iuse_effective`, real EAPI 5+
   `_calc_iuse_effective`): `elibc_*`/`kernel_*`/... derived from
   `USE_EXPAND_IMPLICIT` + `USE_EXPAND_VALUES_*` + `IUSE_IMPLICIT` are now
   valid implicit IUSE for every package, wired into `use_deps_satisfied`
   (`valid_iuse`) and the `REQUIRED_USE` path (`implicit_iuse_set`). It
   was **not** display-only — it drives `is_valid_flag`.
   `USE_EXPAND_HIDDEN` **shipped 2026-08-27** too, once `emerge -pv` grew
   real `USE_EXPAND` grouping to hide from (`Config::use_expand_hidden` +
   `portage_repo::build_use_expand_display`). ~~**Residual:** (a) an
   installed package's USE-dep check uses raw vdb `IUSE`~~ **shipped
   2026-08-29**: `dependency_avoid_update_candidate`'s installed-vdb
   USE-dep check now uses the real `dbapi._iuse_implicit_cnstr` built-
   package domain (recorded `IUSE` ∪ profile `IUSE_EFFECTIVE` ∪ the
   package's own recorded `USE` — real `_iuse_implicit_built`'s `flag in
   use` clause, bug 640318), not raw vdb `IUSE`. New
   `dev-libs/builtusedivergedep`/`needsbuiltusediverge` fixtures. Real
   `_match_use` recomputes this domain rather than reading a vdb
   `IUSE_EFFECTIVE` file, so not persisting one is not a gap.
   ~~**Residual:** (b) `-pv` USE display still lacks real portage's
   *natural* within-group sort (`_alnum_sort_key`)~~ **shipped
   2026-08-30**: `portage_repo::alnum_sort_key` (real
   `output_helpers.py::_alnum_sort_key` -- split on digit runs, compare
   them as numbers) applied at the 3 flag-sort sites (`display`,
   `removed`, `pretend.rs` `--alphabetical`), so `python3_9` precedes
   `python3_12`. New `dev-libs/naturalsortpkg` fixture. No residual left
   for this item -- ANSI colour across *all* of `-pv` shipped as the
   multi-increment buildout, see item 14. The enabled-first
   within-group order + `emerge --alphabetical` **shipped 2026-08-27**
   (`build_use_expand_display` enabled-first split, `pretend.rs::
   use_suffix` `alphabetical` param + `use_flag_sort_key`). `all_flags`
   (always on for `emerge -pv` -- the diff shows *every* flag, plain for
   unchanged, plus `(-flag%)` for a flag dropped from IUSE) **shipped
   2026-08-28** (`render_flag` three-state `FlagState`;
   `build_use_expand_display` walks `old_iuse \ cur_iuse`). The installed-vs-new
   `*`/`%` diff markers (`build_use_expand_display`'s `installed` param),
   the `( … )` forced/masked wrap (`forced_or_masked_flags` +
   `build_use_expand_display`'s `forced` param), and the `[ebuild N ~]`
   bracket-mask marker (`keyword_mask_marker` + `GraphEntry::keyword_
   mask` + `pretend.rs::mask_suffix`) all **shipped 2026-08-27**, real
   `_create_use_string` / `gen_mask_str`. The `[ebuild NS]` new-slot
   marker (`GraphEntry::new_slot`, real `_get_installed_best`) **shipped
   2026-08-27** too — rendered unconditionally (not `-v`-gated), and it
   carried a correctness fix: `resolve_pretend`'s "already installed"
   checks are now filtered to the resolved candidate's own main slot, so
   a cross-slot request resolves as `New` instead of a bogus
   `Upgrade`/`Downgrade`. **Residual:** `dependency_avoid_update_candidate`
   (dependency-atom `avoid_update`) still matches version-only across
   slots.
   The `[ebuild I..]` interactive column (`GraphEntry::interactive` +
   new `evaluated_metadata_tokens`, real `output.py:833` +
   `PkgAttrDisplay.__str__`) **shipped 2026-08-27** too — `I` before the
   code letter for a merge-bound entry whose USE-conditional-evaluated
   `PROPERTIES` contains `interactive`. The trailing `_PackageCounters`
   totals line (`Total: N packages (…)` + `Conflict: N blocks`,
   `package_counters_summary`, real `output_helpers.py`, `-v`-gated)
   **shipped 2026-08-27** too — the package-count half only. The `f`/`F`
   fetch-restrict column (`GraphEntry::fetch_restrict` /
   `fetch_restrict_satisfied` + new `fetch_restrict_files_all_present`,
   `portage-repo` gained a `portage-fetch` dep; real `output.py:633` +
   `getfetchsizes(only_restricted=True)`) **shipped 2026-08-27** —
   `resolve_pretend_graph` gained a `distdir` param. `, Size of
   downloads: N KiB` + the `Fetch Restriction: N package[s]` line
   (`GraphEntry::download_files` + new `fetch_bytes_to_download` +
   `localized_size`, real `_calc_size`/`counters.totalsize`) **shipped
   2026-08-27** too, completing `_PackageCounters.__str__`. The real
   `PkgAttrDisplay` fixed-width bracket field (`[I][N/r][S/R][f/F/g][U][D]`
   + a 7th mask column at `-v`) and the `[old-ver]` column replacing the
   `(upgrade from X)` / `(reinstall for …)` prose **shipped 2026-08-29**
   (`attr_display_field` / `_attr_display_field`, real
   `PkgAttrDisplay.__str__` + `_set_no_columns` + `convert_myoldbest`;
   `reinstall_reason` deleted — real `-pv` shows no inline reinstall
   reason) — increment 1 of the colour buildout (item 14). The new-slot
   other-slot version list and verbosity-3 `:slot`/`::repo` decoration of
   the bracket cpv + every `[old-ver]` (real `_append_slot` /
   `_append_repository` / `convert_myoldbest`) **shipped 2026-08-29**
   (`GraphEntry::sub_slot`/`repo_name`/`oldbest`, `InstalledRef`,
   `decorate_version`; fixture vdb entries gained `repository` files).
   ANSI colour shipped across increments 2-4. **Correction 2026-08-30**
   (real-tree finding): the 7th mask column and the blocker line's
   `empty_space_in_brackets()` were shipped `-v`-gated, but real
   `include_mask_str()` = `verbosity > 1` and real default `emerge -p`
   verbosity is **2** — so both are present at plain `-p`, absent only
   under `--quiet` (verbosity 1, not modelled). `attr_display_field` /
   `_attr_display_field` / `format_blocker_lines` now render the column
   unconditionally; ~240 pinned assertions widened. **Merge order fixed
   2026-08-30** (same real-tree finding, "Model A" confirmed with the
   user): the pilot's BFS built `entries` parent-first, but real
   portage's `mylist` is a topological merge schedule (deps first).
   `resolve_pretend_graph` now re-sorts `entries` into dependency-first
   order as its last step (`topological_merge_order` /
   `_topological_merge_order`, a stable topological sort off the
   `required_by` edges; cycles kept in discovery order). `entries` is
   canonically merge-ordered for the flat list, `--json` (which also
   stamps an explicit `"merge_order"` int per entry), and `emerge
   --buildpkgonly`; `--tree` re-derives structure from `required_by`
   unaffected. ~240 more pinned multi-entry assertions reordered.
   **`USE="…"` at plain `-p` — done 2026-08-30** (real-tree finding):
   real `print_use_string = verbosity != 1` (not `-v`-gated), default
   `emerge -p` verbosity is 2; `-v` changes `all_flags` = *which* flags
   render. Increment 1: `use_suffix` / `_use_suffix` render a **`New`**
   entry's USE line at plain `-p` (identical to `-pv` bar `::repo` +
   counters). Increment 2: `build_use_expand_display` /
   `_build_use_expand_display` grew an `all_flags: bool` param,
   `render_flag` returns `Option` and omits an *unchanged* flag (and the
   `(-flag%)` removed list) when off — so a `Reinstall`/`Upgrade`/
   `Downgrade` shows only its changed flags at `-p`
   (`GraphEntry::use_expand_display` for `-pv`, `use_expand_display_p`
   for `-p`; Python re-renders at display time). ~30 pinned `-p`
   assertions updated total. **Increment 3 — `reinst_flags` — done
   2026-08-31**: `build_use_expand_display` gained a `reinst_flags` set
   (real `reinst_flags_map`, the `Reinstall::changed_flags` /
   `_reinstall_for_flags` trigger set); the three `all_flags`-gated
   `return None` branches in `render_flag` now also pass for a trigger
   flag. Only visible effect at `-p`: a flag the new ebuild dropped from
   IUSE that still triggered a `--newuse`/`--changed-use` reinstall shows
   in the `(-flag%)` removed list (new `dev-libs/reinstdropiusepkg`
   fixture). `_create_use_string` is now fully modelled bar ANSI colour.
   The `g` (remote binary)
   bracket column **shipped 2026-08-29** with the `--pretend` half of
   `--getbinpkg` (item 6 / item 19). The `-pv` output arc is complete
   bar `--autounmask` message colour (its own future slice). The blocker
   line's own real `output.py::_blockers` layout + colour **shipped
   2026-08-29** (item 14).

3. ~~**IUSE-aware `_*` wildcard expansion**~~ **shipped 2026-08-27**
   (`portage_repo::effective_use_flags`'s own `_*` block): a `k_*` flag
   still in the USE set after `package.use` enables every `k_<x>` in the
   candidate's own `IUSE` not masked, then the `_*` pseudo-flags are
   stripped. Not guarded on `k` being a real `USE_EXPAND` var name (a
   documented simplification). New `dev-libs/wildexpand*` fixtures.

4. ~~**`--changed-deps` structured (non-flat) `||`-tree comparison.**~~
   **Shipped 2026-08-27** in two slices: per-key comparison + `strip_
   slots` (`:=` normalization) first, then the full structured
   comparison — new `portage_use_reduce::use_reduce_structured` ports
   real `use_reduce`'s own `flat=False`/`opconvert=False` bracket-
   optimization pass (verified byte-for-byte against real
   `portage.dep.use_reduce` over ~4000 randomized dep strings);
   `deps_changed` compares the canonical per-key token streams, and the
   Python mirror calls real `use_reduce` + `strip_slots` +
   `strip_libc_deps` directly. Faithful to real portage's Python-list
   `!=` (order-significant everywhere, redundant brackets collapsed),
   confirmed with the user. No residual.

### B. `--root-deps` recursion follow-up

5. ~~**Walk the running-root build entry's own further dependencies.**~~
   **Shipped 2026-08-27** (`resolve_root_deps_build_entries`): a
   running-root build entry's own `DEPEND` + `BDEPEND` + `RDEPEND` +
   `IDEPEND` are walked against the running root recursively,
   cycle-guarded by the existing `root_deps_build_seen` set; an
   unbuildable, not-installed build dep is now surfaced as its own
   `NoVisibleCandidate` entry rather than swallowed. A *top-level*
   package's own `IDEPEND` now routes to the running root too, **shipped
   2026-08-27** (`root_deps_satisfied_atoms` gained a `dep_keys`
   parameter; both ordinary dep-walk sites pass `["DEPEND", "BDEPEND",
   "IDEPEND"]`). **Residual:** in the pilot a top-level `IDEPEND` reaches
   the running root only under `--root-deps` (real portage does it
   unconditionally -- a consequence of this pilot's opt-in
   `root_deps_running_root` plumbing, not a per-dependency `root`);
   `PDEPEND` of a running-root entry (real portage keeps it a
   target-`ROOT` concern -- likely a permanent non-gap); and the full
   multi-root graph architecture (a `root` carried per dependency edge)
   this pilot still approximates edge by edge.

### C. Binary packages / fetch

6. **Remote binpkg fetching** — the `--pretend` half of `--getbinpkg`/
   `--getbinpkgonly` **shipped 2026-08-29**: `binrepos.conf` +
   `PORTAGE_BINHOST` parsing (`portage-profile` `parse_binrepos`,
   `BinRepo`), remote binhost candidates read from each binhost's cached
   `Packages` index (`portage-repo` `list_remote_binary_candidates`),
   the `g` bracket column (`GraphEntry::remote_binary`), and the download
   `SIZE` feeding `Size of downloads:` / the `-pv` line suffix.
   The **actual remote download + merge shipped 2026-08-31**: `emerge
   --getbinpkgonly <atom>` (non-`--pretend`) refreshes each `http(s)`
   binhost's live `Packages` into the edb cache (real
   `bintree._populate_remote`, via `wget`), resolves binary-only,
   downloads each remote binpkg into `$PKGDIR`, size-checks it against
   the index, and merges it — new `binpkg::extract_binpkg` (image +
   build-info from xpak/gpkg), new `ebuild_merge::merge_binpkg` (reuses
   `merge_tree` + a refactored `write_vdb_entry_from_dir` + `env_update`),
   new `emerge_getbinpkg.rs`. Rust-unit-tested end to end over loopback
   HTTP. **Replacing an installed version shipped 2026-08-31**:
   `merge_binpkg` does the real merge-then-unmerge (new binpkg + vdb
   entry written first, then every *same-slot* installed version's files
   unmerged with the new `PF` folded into `others_in_slot` so a path the
   new version owns survives, then the old vdb dir deleted, then
   `env_update`); `run_unmerge` was split into `pub(crate)`
   `unmerge_pkgfiles` (env-free file-removal core) + `delete_vdb_dir` so
   the replace path reuses them phase-free; `run_getbinpkgonly` now
   accepts `Upgrade`/`Downgrade`/`Reinstall` outcomes too.
   **All four install/remove `pkg_*` phase hooks for a binpkg merge
   shipped 2026-08-31**: `extract_binpkg` keeps `environment.bz2` +
   `<pf>.ebuild` verbatim; `ebuild_phases::run_phase_from_saved_env` =
   `run_single_phase` + real `BinpkgEnvExtractor` (`bunzip2
   environment.bz2 > ${T}/environment` + `${T}/environment.raw` marker,
   `EMERGE_FROM=binary`) so `bin/ebuild.sh` sources the saved env instead
   of re-sourcing (and re-`inherit`-ing) the ebuild -- for `pkg_setup`
   too. `merge_binpkg` peeks metadata first (image → real
   `${PORTAGE_BUILDDIR}/image`), runs `setup`→`preinst` before the copy,
   then for every replaced same-slot version `prerm`→remove files→`postrm`
   from *that* version's own vdb `environment.bz2`, then `postinst`, all
   gated on `DEFINED_PHASES`. New committed fixtures
   `dev-libs/binpkgphasepkg-1.0.tbz2`, `dev-libs/binpkgrmpkg-{1.0,2.0}.tbz2`.
   **`--getbinpkg` (mixed ebuild + binary) merge shipped 2026-08-31**:
   `run_getbinpkgonly` → `run_merge_plan`, dispatching per resolved entry
   on `entry.source` — `Binary` → `merge_one_binary_entry`
   (`merge_binpkg`), else → `emerge_build::merge_one_source_entry`
   (`run_merge`). "Prefer binary, fall back to source" is the resolver's
   job (unchanged); `--getbinpkgonly` just never yields a source entry.
   **`merge_binpkg` collision-protect / blocker exclusion / preserve-libs
   parity with `merge_after_install` shipped 2026-08-31**: the same
   `find_collisions` pre-copy check + `collision-protect`/`protect-owned`
   abort + `unregister_preserved_libs` now run for a binpkg;
   `blockers_from_flat_deps` (factored out of `blocked_installed_packages`)
   supplies the `mypkglist` blocker term from the binpkg's own
   already-USE-reduced `*DEPEND` build-info files (no repo/config resolve).
   **Still open:** live `layout.conf` negotiation, `Packages.gz`, resume
   support, and real digest (`SHA*`) verification. A real debug trace is vendored at
   `PORTING/helpers/emerge_-1v_--debug_--getbinpkgonly__sys-fs--fuse.log`.

7. **`gpkg` binary package format** (`bin/gpkg-helper.py`,
   `lib/portage/gpkg.py`). The **`$PKGDIR` directory-scan fallback**
   buildout (real `bintree._populate_local` — open each binpkg file and
   rebuild the pool when there's no trusted index; a `gpkg` listed *in*
   an index already resolves for `--pretend` today, the index being
   format-agnostic) is **DONE, 3 increments, 2026-08-29/30**:
   `portuale/src/binpkg.rs::read_gpkg_metadata` (real
   `gpkg.get_metadata()` — outer tar → classify `metadata.tar[.<comp>]`
   → decompress via the real `_compressors` argv → inner tar; `tar` +
   the seven decompressors), `::read_xpak_metadata` (real
   `xpak.tbz2.scan` — the self-describing `XPAKPACK…STOP` trailer, pure
   Rust, no subprocess), and `::scan_pkgdir` wired into `pretend.rs`
   (`<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}` → `Config::scanned_binpkgs`,
   only when `Packages` is absent, never written back). Landed a
   `portage_repo::BinaryIndex` refactor (`from_pkgdir`/`from_entries`
   through every binary-candidate fn). Both sides; contract-tested.
   ~~`ebuild … package` *emitting* gpkg~~ **shipped 2026-08-30**:
   `PackageOptions::binpkg_format` (`BINPKG_FORMAT`, env-var-sourced at
   the CLI boundary; `"xpak"` or `"gpkg"`) routes real, unmodified
   `bin/misc-functions.sh __dyn_package` to its own real, unmodified
   `bin/gpkg-helper.py compress` branch — a genuine `<cat>/<pf>.gpkg.tar`
   this pilot's own `read_gpkg_metadata` round-trips (`emerge
   --buildpkgonly` picks it up too). `Packages` gets a `PATH` field for
   the gpkg entry. **Still open:** `Manifest`/`.sig` *verification* and
   gpkg *signing* (`FEATURES=binpkg-signing` — deliberately cut, this
   pilot has no crypto), bare `.xpak` multi-instance files, real
   portage's mtime-staleness index revalidation, `BUILD_ID` in the
   basename.
   ~~**Also found (increment 2):** the pilot's own `build-info`
   generation omits every dependency-string metadata file~~ **shipped
   2026-08-30**: `ebuild_phases::write_post_install_metadata` (real
   `doebuild.py::_post_src_install_write_metadata`) writes
   `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/`IDEPEND`/`LICENSE`/
   `PROPERTIES`/`RESTRICT`/`IUSE` into `build-info` (`use_reduce`'d
   against the empty phase USE), and `write_vdb_entry` now copies the
   *whole* `build-info` dir into the vdb (real `treewalk()`).
   **`:=` slot-operator binding shipped 2026-08-31**: `bind_slot_operator`
   (real `_slot_operator._eval_deps`'s per-atom step) rewrites each
   `*DEPEND` `:=` atom to `:<slot>/<sub-slot>=` from the highest
   installed match in `<root>/var/db/pkg`, bare if unresolvable. v1
   simplification: every `*DEPEND` key bound against the one target-`ROOT`
   vdb (real splits RDEPEND/PDEPEND vs DEPEND/BDEPEND); no `|| ( A:= B:= )`
   handling. **Slot-operator REBUILD edges shipped 2026-08-31 (v1)**:
   `resolve_pretend_graph`'s post-pass `slot_operator_rebuild_entries`
   (real `_slot_operator_trigger_reinstalls` / the
   `@__auto_slot_operator_replace_installed__` set) — an installed
   package whose built `cat/pkg:S/SS=` dep no longer matches how the run
   leaves `cat/pkg` in that slot becomes a
   `Reinstall { slot_operator_rebuild: true }` (`[ebuild R]`, `--json`
   `slot_operator_rebuild` bool), ordered after the dep. New sixth
   `PretendOutcome::Reinstall` trigger field. **Increment 2 shipped
   2026-08-31**: `_show_abi_rebuild_info`'s "The following packages are
   causing rebuilds:\n\n  <provider> causes rebuilds for:\n    <consumer>"
   block (`GraphResult::abi_rebuilds` pairs; `--verbose-slot-rebuilds[=y|n]`
   wired, default on, NOT `--verbose`-gated; `--json` `abi_rebuilds`
   array). Also fixed `unresolved_runtime_deps`' sub-slot-less candidate
   strings (a kept `foo:2/3=` dep falsely read unsatisfied). v1 cuts:
   single-pass (no backtracking), consumer's own `:=` not re-bound, no
   `--changed-slot`/`--ignore-built-slot-operator-deps` interaction. New
   `dev-libs/slotbind{target,consumer,fresh}` fixtures. Cut still:
   `IUSE_EFFECTIVE`. `FEATURES=verify-sig`
   (GPG) lives here too — it is a `gpkg`/repo-sync concept, **not**
   `SRC_URI` fetch (the earlier backlog mis-scoped it).

8. **`BUILD_ID` / `splitdebug` / `packdebug` / RPM**, and PKGDIR-index
   locking. All named as cuts in `ebuild_package.rs`. (`gpkg` on the
   write side shipped 2026-08-30 — see item 7.)
   **`FEATURES=buildpkg` / `emerge --buildpkg`/`-b` shipped 2026-08-31**:
   a binpkg of each source entry is written into `$PKGDIR` before the vdb
   merge (real `_emerge/EbuildBinpkg`). `ebuild_package::
   package_after_install` (split from `run_package`) + a new
   `run_merge`/`merge_one_source_entry`/`run_source_merge`/`run_merge_plan`
   `buildpkg: Option<&PackageOptions>` param; `pretend.rs` gates it on
   `--buildpkg[=y|n]`/`-b` OR `FEATURES=buildpkg` (`=n` wins). `--buildpkg`
   is a `--pretend` no-op on the Python side. **`--buildpkg-exclude
   <atoms>` shipped 2026-08-31**: `emerge_build::entry_matches_any`
   (real `InternalPackageSet.findAtomForPackage`); `run_source_merge` /
   `run_merge_plan` filter `buildpkg` to `None` per matching entry;
   space-separated + repeatable; missing value → exit 2; `--pretend`
   no-op. **Still open:** `FEATURES=buildpkg-live` /
   `binpkg-multi-instance`, real `EbuildBinpkg` failure semantics under
   `--keep-going`.

9. **Fetch: resume support** (`RESUMECOMMAND`'s retry-with-`-c`), **live
   per-mirror `layout.conf` negotiation**, real candidate ordering/
   shuffling, `RESTRICT=primaryuri` (the SRC_URI-vs-mirror interleave --
   doesn't port cleanly because this pilot's candidate ordering already
   deliberately deviates from real). ~~`RESTRICT=mirror`~~ **shipped
   2026-08-29**: `FetchOptions::restrict_mirror` (from the md5-cache
   `RESTRICT` field via `restrict_mirror_from_restrict`,
   USE-conditional-evaluated) gates the `gentoo_mirror_fallback` step --
   real `file_restrict_mirror`, `fetch.py:1117-1127`. ~~`mirror+`/`fetch+`
   SRC_URI prefixes (`override_mirror`) still not parsed~~ **shipped
   2026-08-30**: `portage_fetch::flatten_src_uri` strips the prefix and
   records `SrcUriEntry::override_mirror`/`override_fetch` (`mirror+`
   sets both, real `fetch.py:1103-1106`); `fetch.rs` checks
   `entry.override_mirror` per-entry so a `mirror+` URI re-permits the
   public `GENTOO_MIRRORS` fallback even under `RESTRICT=mirror`.
   ~~`RESTRICT=fetch`~~ **shipped 2026-08-30**:
   `FetchOptions::restrict_fetch` (from `RESTRICT` via
   `restrict_fetch_from_restrict`) bars a plain (non-`mirror://`)
   `SRC_URI` URI from the candidate list (real `fetch.py:1167`) and the
   public `GENTOO_MIRRORS` fallback -- so a fetch-restricted package
   fetches only from an already-verified `DISTDIR` copy /
   `custommirrors` / a `mirror://`-named mirror; `override_fetch`
   (`fetch+`/`mirror+`) re-permits the URI. **v1 cut:** running the
   ebuild's own `pkg_nofetch` phase for a missing file (real
   `spawn_nofetch`) -- fails with a generic pointer instead. New
   `dev-libs/fetchrestrictpkg` fixture. Remaining
   items named as cuts in `fetch.rs:28-48`.

### D. Config-resolution `USE_ORDER` depth

10. **`package.use` full per-level `USE_ORDER`.** Real repo-level
    `package.use` belongs in `configdict["repo"]` and profile-level in
    `configdict["defaults"]` (merged per-level with that level's own
    `make.defaults` USE); this pilot flattens all three sources into one
    incremental list. Also missing: the `env`, `pkginternal`, `features`,
    and `env.d` `USE_ORDER` layers entirely (`portage-profile` module
    doc, "Only the `defaults` and `conf` layers … are implemented").
    A genuinely bigger undertaking — the flat `Config` model has no
    per-layer structure at all.

### E. brush / shell backend

11. **brush strategy #2** — rewrite this repo's own `bin/*.sh` to avoid
    brush-hostile constructs. Low-risk, immediately effective for this
    tree, doesn't preempt real-world ebuilds.

12. ~~**brush strategy #3** — a fork-tracking doc for `vivo75/brush`.~~
    **Shipped 2026-08-27**: `PORTING/BRUSH_FORK.md` records the pinned
    rev's ancestry, which fixes are upstream-merged
    ([#1274](https://github.com/reubeno/brush/pull/1274), `18851e7`,
    2026-08-20) vs fork-only
    ([#1276](https://github.com/reubeno/brush/pull/1276), the pipeline
    deadlock fix — open, no review yet), and a re-pin checklist for when
    #1276 merges. Still to do: the actual bump to upstream (blocked on
    #1276) and any periodic rebase in the meantime.

### F. preserve-libs

13. **The one live-`scanelf` branch inside real `LinkageMapELF.rebuild()`**
    (`LinkageMapELF.py:233-324`) — orphaned preserved libs with no
    `NEEDED.ELF.2` entry. Everything else in `rebuild()`/`findConsumers()`/
    `_find_libs_to_preserve()` is ported (`needed_elf.rs`). Deliberately
    excluded, **confirmed with the user each time it comes up**: it is the
    one real spot a raw ELF-header read (not `scanelf` output) would
    matter. **Update 2026-08-29**: the `needed_elf` module is no longer
    dead code -- `NeededEntry` + `rebuild()` + `findConsumers()` now have
    a live caller via `--depclean-lib-check` (§0). `_find_libs_to_preserve`
    and the merge/unmerge wiring are still unused pending a preserve-libs
    control-flow slice.

### G. `emerge -pv` real `output.py` layout + ANSI colour

14. **The full real `resolver/output.py` rendering.** Scoped 2026-08-29
    (user chose maximum fidelity + scope via `AskUserQuestion`), landed as
    a multi-increment buildout:
    - **Increment 1 — real bracket layout, no colour** — **shipped
      2026-08-29**: `attr_display_field` (real `PkgAttrDisplay.__str__`),
      `[old-ver]` column replacing the `(upgrade from X)` / `(reinstall
      for …)` prose, `reinstall_reason` deleted. See item 2 above.
    - **Increment 2 — colour primitive + gating + bracket-line colours**
      — **shipped 2026-08-29**: new `portuale/src/color.rs` (+ the Python
      mirror) -- the real `\x1b[` escape table, `colorize()`, the
      `_styles` entries reached, `nc_len()`, and `resolve_havecolor`
      (real `actions.py:2816-2828` + `util.no_color`: `--color y|n`
      wins, else on unless `NO_COLOR`/`NOCOLOR` or non-tty/`TERM=dumb`).
      No `color.map`/`PORTAGE_COLORMAP`. The bracket line is coloured:
      `Display.pkgprint` palette (`PKG_MERGE_WORLD`/`_SYSTEM`/plain +
      binary variants; `check_system_world` narrowed to favorite-or-
      world-file / `@system`-atom), the per-letter `PkgAttrDisplay`
      colours, `blue("[old-ver]")`, `darkgreen("to <root>")`, and
      `nc_len`-aware `--columns` padding.
    - **Increment 3 — USE-flag colours** — **shipped 2026-08-29**: real
      `_create_use_string`'s per-flag colour (`red` plain-enabled, `blue`
      plain-disabled `-flag`, `yellow` for `%`/`%*` newly-in-IUSE,
      `green` for a lone `*` polarity flip) applied as a render-time
      token-shape pass (`colorize_use_token` / `_colorize_use_token`) --
      only the `flag`/`-flag` core, markers and `( )` wrap stay plain.
      One documented, fixture-unreachable imperfection (forced disabled
      flag newly in IUSE on an Upgrade -> `blue` here, `yellow` in real).
    - **Increment 4 — counters line + `-pc`/`-pC`/`-pP` cleanup output +
      `--columns`/`--tree`** — **shipped 2026-08-29**: `_PackageCounters`'s
      `interactive` (WARN) + fetch `bad(...)`; real `_unmerge_display` /
      `action_depclean` colour for the standalone cleanup actions
      (selected red / protected-omitted green / `darkgreen` header /
      `BAD`+`WARN` system-profile warning / `WARN`+`GOOD` legend / the
      `-pc` advisory's `WARN " * "` + green backtick commands).
      `--columns`/`--tree` were already coloured via `print_entry_line`
      in increment 2. `show_parents` left plain (no colour in real).
    - **Increment 5 — blocker line real `_blockers` layout + colour** —
      **shipped 2026-08-29**: real `ResolverOutput._blockers`
      (`output.py:75-123`) -- the `[blocks B     ]` fixed-width bracket
      (+ mask-column space at `-v`), the `!`-stripped (`dep_expand`'d)
      atom, `("<atom>" is {hard,soft} blocking <parent cpv>)`, all
      `colorize("PKG_BLOCKER", …)` = red under `--color=y`. Blocker
      lines are now collected and printed as one group *after* every
      package line (real `Display.print_blockers`), not interleaved. New
      fixture `dev-libs/blockerorderpkg`. The teal `b` /
      `PKG_BLOCKER_SATISFIED` branch and real's `(is <desc> <parents>)`
      alternative are both unreachable in this pilot (documented).
      **Update 2026-08-30:** the real `--autounmask` *block* (not just its
      colour) shipped for the **keyword** kind -- `emerge --pretend
      --autounmask <kwmasked>` now *resolves* the graph (implicit
      `=cpv ~arch`, `[ebuild N ~]` in the merge list) and prints real
      `depgraph.py::_display_autounmask`'s `The following keyword changes
      are necessary to proceed:` block (`colorize("BAD")` header +
      `colorize("INFORM")` change line + `#required by` dep chain), exit
      0. `resolve_pretend` gained an `autounmask_keywords` param;
      `GraphResult::autounmask_keyword_changes`; `--json` array. 6
      contract tests updated. **Increment 2 -- the USE kind -- shipped
      2026-08-30**: `--autounmask-use` (on by default) now *resolves* a
      plain USE-dep mismatch via an implicit `package.use` flip
      (`resolve_pretend` `autounmask_use` param + `suggested_use_flip`),
      applies the flip to the entry's effective USE (so `-pv`'s
      `USE="…"` line reflects it), and prints `The following USE changes
      are necessary to proceed:` with the `>=<cpv>` atom form
      (`check_if_latest`, bug #536392). `GraphResult::autounmask_use_
      changes`; `--json` array; `AutounmaskChange.atom` carries the op
      prefix. `--autounmask-use=n` restores strict matching. ~10 contract
      tests updated. **Increment 3 -- the `opt=` parent flip -- shipped
      2026-08-30**: when a dep's `opt?`/`opt=` use-dep is unsatisfiable
      AND the child's flag is `use.mask`'d, `resolve_pretend_graph` flips
      the *requesting parent's* flag, re-resolves the freed dep, records
      `>=<parent-cpv> -flag` in the same USE block (new
      `dev-libs/parentflip{childpkg,eqpkg}` fixtures; cuts: re-resolves
      only the freed dep, one-level parent dep chain, non-`New` parent
      re-rendered as `New`). **`--autounmask-license` still open.**
    The two increment-1 follow-ups -- a new-slot install's other-slot
    version list, and verbosity-3 `:slot`/`::repo` decoration on the cpv
    + every `[old-ver]` -- **shipped 2026-08-29** (see item A.2).

---

## Part 3 — explicit non-goals / architecture boundaries (`PROMPT.md`)

Not oversights — standing decisions, listed for completeness.

- **`--autounmask-write`** (and any file-*writing* autounmask mode).
  Conflicts with the pilot's "never writes config" invariant. The
  read-only "suggest changes" half is shipped (#17).
- **Virtuals as dedicated code / backtracking.** Virtuals are ordinary
  packages with an any-of `RDEPEND`, already handled; a real backtracking
  resolver is out of scope.
- **PyO3 / in-process FFI embedding.** Would foreclose the
  two-sibling-implementations end state (`PROMPT.md` "Open / deliberately
  undecided").
- **EAPI 0/1/2/3/4/6.** Dead in this repo — every profile is EAPI 5+, and
  the `portage-*` crates go further with no EAPI parametrization at all
  within the 5+ floor.
- **`bsd_chflags`.** `lib/portage/__init__.py:311` sets it to `None`
  unconditionally on non-BSD; the pilot is Linux-only/musl-static.
- **RPM binary packages, repo syncing (`emerge --sync`), news items.**
  Not in scope.
