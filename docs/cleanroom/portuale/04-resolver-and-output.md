# 04 — Resolver, merge order, and output rendering

## 4.1 Config resolution (`portage-profile::resolve_config`, `lib.rs:2490`)

Inputs: config root, main-repo location, overlay `(name, location)` list,
repo aliases, main-repo name, per-repo masters map, ROOT.

Layering (lowest → highest precedence):

1. Profile chain: `parent` files (including cross-repo
   `reponame:path` via aliases), then cascading `package.use`,
   `package.mask`/`package.unmask` (repo-scoped `::name` entries use
   each overlay's own name), `package.keywords`, `use.mask`,
   `use.force`, `make.defaults`.
2. `make.conf` / `make.globals`: `USE`, `FEATURES`,
   `PORTAGE_FEATURES`, `USE_EXPAND` values, `INSTALL_MASK`,
   `EMERGE_DEFAULT_OPTS`, `BINPKG_FORMAT`, `PORTAGE_BINHOST`, fetch
   variables, `PORTAGE_LOGDIR`, `PORTAGE_TMPDIR`, compression vars.
3. Command-line / environment overrides applied by the caller.

Outputs: the `Config` struct (`lib.rs:389`) — resolved USE, FEATURES,
per-package USE (`ProfileUseLayer` `:364`), mask/force levels
(`UseMaskForceLevel` `:377`), `system_packages`, binrepos
(`BinRepo` `:1135`), incremental-application helpers
(`apply_incremental` `:1397`, `apply_use_incremental` `:1445`).

Phase-environment derivation (`phase_environ.rs`): implicit USE,
resolved FEATURES, multilib vars, `SLOT`/`PORTAGE_REPO_*`,
`SOURCE_DATE_EPOCH` — consumed by `emerge_build` (`entry_build_env`)
and `ebuild_phases` (`phase_env_vars`).

## 4.2 Repository discovery and metadata

- `find_repos` (`portage-repo/src/lib.rs:1111`) parses `repos.conf` →
  `RepoConfig` (`:886`): name, location, priority (`repo_priority`),
  `is_main`, `masters`, `aliases`, `cache-formats`.
- `pregen_md5_cache_enabled` (`:1724`) /
  `regen_writes_md5_cache` (`:1751`): `layout.conf` `cache-formats`
  rule (lowercase + split; empty = auto-detect `md5-dict` then `pms`);
  pregen cache used only when the first known format is `md5-dict`
  and `FEATURES=metadata-transfer` is absent.
- `repo_aux_metadata` (`:1508`): read path — pregen md5-cache →
  depcachedir → `depend`-phase fallback. A present md5-cache entry is
  validated (`_md5_`, EAPI, `_eclasses_` pairs/triples) and rejected
  entries fall through; provider failures never return stale data.
  `register_aux_metadata_provider` (`:1475`) plugs the ebuild phase
  runner in (wired by `main.rs:139`).
- `has_usable_md5_cache` (`:1837`).
- `profiles/updates/` moves: `UpdateCmd` (`:248`), `parse_updates_content`
  (`:295`), `apply_updates_to_atom/dep_string/cp/slot` (`:661-806`),
  gated by `set_package_moves_enabled` (`:382`).

## 4.3 Candidate selection

- `list_candidates` (`:2035`): all ebuild versions of `cat/pkg` across
  repos with parsed metadata + visibility (`is_visible` `:4387`,
  `forced_or_masked_flags` `:3648`, `effective_use_flags` `:3178`).
- Binary pool: `list_binary_candidates` (`:2396`, local `PKGDIR` via
  `read_packages_index` `:2147` / `BinaryIndex` `:2187`),
  `list_remote_binary_candidates` (`:2562`),
  `find_remote_binpkg` (`:2609`); `--quickpkg-direct` adds the source
  root's installed packages (`set_quickpkg_direct_root` `:2313`).
- Binary eligibility filters: `--useoldpkg-atoms`
  (`set_useoldpkg_atoms` `:425`), `--usepkg-exclude/include`,
  `--buildpkg-exclude`, changed-`*DEPEND` rejection
  (`set_binpkg_changed_deps_override` `:453`), ebuild-visibility check
  (`set_use_ebuild_visibility` `:477`).
- Installed side: `installed_candidates` (`:5595`),
  `installed_versions` (`:5802`), `installed_pkg_repo` (`:5629`),
  `installed_refs` (`:5641`), `installed_pkg_iuse_and_use` (`:6082`).

## 4.4 Dependency resolution and merge order

The resolver (`portage-repo`, `merge_order.rs`, `solver_bridge.rs`)
takes expanded targets + flags and returns:

- `entries: Vec<GraphEntry>` in dependency-first merge order
  (`topological_merge_order`; `--implicit-system-deps=n` skips the
  @system-first/reference-count bias, leaving discovery order).
- `PretendOutcome` per entry (`:5970`): `New{version}`,
  `Upgrade{from,to}`, `Downgrade{from,to}`, `Reinstall{version,…}`,
  `AlreadyInstalled{…}`, `NoVisibleCandidate`, `Uninstall{…}`.
- `BlockerConflict` list, `SlotConflict` list, autounmask changes
  (`AutounmaskChange`), changed-deps report entries,
  `buildpkgonly_deps_unsatisfied` gate flag.

Key pieces:

- `dep_edges_from_metadata` (`merge_order.rs:250`), `DepEdge` (`:88`),
  `DepPriority` (`:65`); `dep_edge_satisfied_by_installed` (`:998`).
- `resolved_dep_targets` (`:1785`), `kept_alt_branches` (`:1757`),
  `tree_solved_replacements` (`:2725`), `cycle_report` (`:2549`) /
  `print_circular_block` (`pretend.rs:2684`).
- Solver bridges: `PubGrubResolver` (`solver_bridge.rs:810`),
  `ResolvoResolver` (`:1019`) — interchangeable back ends behind the
  same outcome types.
- Rebuild scan (`rebuild_if_entries` in portage-repo): `:=`
  slot-operator rebuilds, `--rebuild-if-{unbuilt,new-rev,new-ver}`,
  `--rebuild-exclude/ignore` filters; the undo path
  (`_eliminate_rebuilds`) and slot-move probe.
- Misspell suggestions (`misspell_suggestion_block` `:7943`) via
  `difflib::get_close_matches` when a top-level `cat/pkg` is unknown.
- Unparsed dependency tokens are counted
  (`note_unparsed_dep_token` `:524`, `unparsed_dep_tokens` `:532`);
  the contract requires zero.

## 4.5 Output rendering (`pretend.rs`)

### Flat list (default)

`print_entry_line` (`:1298`, ~600 lines) renders one row per entry:

- `[ebuild   R   ]`, `[binary     ]`, `[ebuild  N   ]`… operation tag
  with slot-change/rebuild markers.
- `category/package-version` with `decorate_version` (`:488`) colour.
- USE rendering: `use_suffix` (`:525`) — short vs verbose form by
  verbosity; `use_flag_sort_key` (`:440`); per-flag colour
  (`colorize_use_token` `:457`); `attr_display_field` (`:366`) mask
  column unless `--quiet`; `render_pkg_use_display` (`:8342`).
- `root_suffix` (`:622`) for non-default roots
  (`resolve_root_deps_running_root` `:594`).
- Size: `localized_size` (`:638`).
- Counters header/footer: `package_counters_summary` (`:657`),
  `merge_bound_version` (`:827`), `entry_display_version` (`:843`).
- Column layout honours `COLUMNS` (`columnwidth_from_env` `:262`,
  `columns_line` `:294`).

### Blockers

`blocker_row_disposition` (`:917`, `BlockerRowDisposition` `:856`):
satisfied-by-replacement rows are hidden or shown inline
(`collect_inline_blocker_lines` `:1197`,
`replacement_entry_index` `:999`, `replacement_wait_index` `:1011`,
`kept_alt_for_display` `:895`); unresolved blockers print full rows
(`format_blocker_row` `:1110`) plus trailing advisory lines
(`trailing_blocker_lines` `:1152`); counts via `count_blocker_rows`
(`:1245`).

### Tree / columns

`print_tree` (`:1904`): indented dependency tree with cycle-safe walk
(`children_of`/`parents_of`/`unordered_walk` `:2067-2108`);
`--unordered-display` keeps discovery order; `--alphabetical` sorts
siblings. Mutually exclusive with `--columns` (parse-time error).

### JSON (`--json`)

`print_json` (`:2768`): `entry_to_json` (`:2355`),
`slot_conflict_to_json` (`:2581`),
`changed_deps_report_entry_to_json` (`:2621`),
`autounmask_change_to_json` (`:2634`),
`abort_outcome_to_json` (`:2649`); string escaping
(`json_escape` `:2304`, `json_string` `:2320`).

### Slot conflicts and errors

`slot_conflict_atom_string` (`:8022`), `slot_conflict_reasons`
(`:8068`), `slot_conflict_caret_idx` (`:8140`),
`colorize_marked_spans` (`:8257`), `slot_conflict_need_rebuild`
(`:8288`), `skip_conflict_caret_line` (`:8362`): caret-annotated atom
spans explaining each conflict and whether a rebuild resolves it.

### World/set helpers behind the display

`read_world_atoms` (`:3034`), `expand_selected` (`:3058`),
`installed_set_atoms` (`:3073`), `preserved_rebuild_atoms` (`:3104`),
`update_world_file` (`:3166`), `update_world_sets_file` (`:3271`),
`read_world_sets` (`:3324`), `resolve_custom_set` (`:3369`),
`collect_installed_sets` (`:3404`).
