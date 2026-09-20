# 06 — Support subsystems and complete function index

## 6.1 Binary-package formats (`binpkg.rs`, 4164 lines)

- **xpak** (`.tbz2`): `read_xpak_metadata` (`:856`) walks the trailer
  index (`read_xpak_segment` `:882`, `parse_xpak_members` `:933`,
  `be32` `:969`, raw member `:870`); `extract_xpak_image` (`:1470`).
- **gpkg** (`.gpkg.tar`): outer tar scan (`read_outer_members` `:202`,
  `OuterMember` `:187`, `trusted_outer_member_name` `:174`,
  streaming read `:279`); inner compressed members
  (`collect_inner_members` `:433`, `InnerMember` `:384`,
  `InnerEntry` `:406`, `InnerMetadata` `:419`,
  `resolve_inner_member` `:544`, decompress `:606`,
  `read_inner_metadata` `:653`); path normalisation (`normpath` `:356`),
  entry typing (`classify_inner_member` `:122`,
  `known_inner_entry_type` `:331`); metadata extract (`:1335`),
  image safety scan (`scan_image_tar` `:1392`), single-member extract
  (`:1522`); manifest verification (`verify_gpkg_manifest` `:1015`,
  compression table `:105`); `extract_binpkg` (`:1275`) dispatches on
  format.
- **Local index**: `populate_local_pkgdir` (`:1640`) synthesises
  `Packages`-style entries per file (`scan_binpkg_file` `:1737`);
  `file_mtime` (`:1829`); tar runner (`run_tar` `:1834`).
- **GPG**: `GpgVerify` (`:1874`, defaults `:1889-1892`,
  `from_env` `:1920`, `from_features` `:1929`, `from_binrepo` `:1954`);
  policy precedence (`gpg_policy_for_features` `:1983`); argv
  templating (`:2002`); subprocess verify (`run_gpg_verify` `:2018`);
  detached (`:2086`) and clearsigned (`:2115`) verification.
- Errors: `BinpkgError` (`:60`, `is_invalid`/`is_fatal`/`message`
  `:69-79`); scratch dir (`ScratchDir` `:142-163`).

## 6.2 Elog and mail (`elog.rs`, 1873 lines)

Message files under `${T}/elog` are collected (`collect` `:204`,
`collect_all` `:151`, `collect_all_phases` `:184`,
`ElogMessage` `:77`, `ElogPackage` `:88`), filtered by class
(`:192`, `module_classes`), combined (`:219`), and dispatched:

- `save_process` (`:283`) / `save_summary_process` (`:308`) append to
  `PORTAGE_LOGDIR/elog`;
- `syslog_process` (`:361`, priority map `:334`, line shape `:347`);
- `custom_process` (`:410`) runs the configured command with
  logfile/package substitution;
- `save_modules_process` (`:1133`) fans out to mail (`mail` action with
  `MAILURI`/`MAILFROM`/subject, sendmail-or-SMTP, RFC2822 + base64);
- `process_batch` (`:1223`) is the whole-run entry;
- `echo_summary` (`:1286`) renders the terminal block.
- Gating: `module_enabled` (`:108`), `echo_enabled` (`:115`),
  elog-system tokens (`:96`), `logdir` (`:238`).

## 6.3 Output colour (`color.rs`, 454 lines)

`Colorizer` (`:330`, `new` `:335`, `c` `:343`, `pkgprint` `:374`):
palette with system-over-world precedence; `phase_colormap_export`
(`:231`) for phase env; `resolve_havecolor` (`:302`, explicit
`--color` wins over `NO_COLOR`/`NOCOLOR`/isatty); `nc_len` (`:275`)
strips ANSI for width math.

## 6.4 `env-update` (`env_update.rs`, 539 lines)

`run_env_update` (`:239`): reads `/etc/env.d`, writes `ld.so.conf`,
`profile.env`, `csh.env`, `08portage.*`-style generated files;
`info_dirs_inodes` (`:125`) resolves INFOPATH/INFODIR entries;
`candidate_lib_dirs` (`:165`, excludes `libexec`); runs a real
root-scoped `ldconfig` when present+executable (`:206`);
`parse_env_d_line` (`:78`), `is_env_d_filename` (`:96`).

## 6.5 Install mask (`install_mask.rs`, 296 lines)

`resolve` (`:28`) folds `INSTALL_MASK` + `no*` FEATURES in real order;
`InstallMask` (`:49`, patterns `:40`, anchored/unanchored/exclusion
semantics); `install_mask_dir` (`:115`) deletes matched paths and
records the mask; `fnmatch` (`:164`,→regex `:178`).

## 6.6 Merge-engine dispatch (`merge_engines.rs`, 360 lines)

`merge_unit_for_entry` (`:37`) maps a `GraphEntry` to a `MergeUnit`
(source vs binary); `SourceEngine` (`:115`) and `BinaryEngine`
(`:175`) implement `MergeEngine::merge_entry` (`:134`/`:191`) via
`execute` (`:154`/`:204`) — the seam `run_merge_loop` drives.

## 6.7 ELF / soname tracking (`needed_elf.rs`, 1939 lines)

`generate_soname_deps` (`:333`) scans ELF objects
(`compute_multilib_category` `:150` from ELF headers) into
`NEEDED.ELF.2` lines (`NeededEntry` `:47`, parse `:75`, file `:110`,
render `:126`); `read_all_needed_entries` (`:491`); `rebuild` (`:694`)
builds the `LinkageMap`/`SonameMap` (`:654/661`, `ObjKey` `:615`,
`ObjProperties` `:635`, `obj_key` `:620`, path helpers `:803-825`);
`find_consumers` (`:962`) via `getlibpaths` (`:859`, ld.so.conf +
defaults + `LD_LIBRARY_PATH`); `find_libs_to_preserve` (`:1087`);
`soname_consumers` (`:1196`); `find_unneeded_preserved` (`:1225`);
RPATH/`$ORIGIN` expansion (`:563-615`); exclude patterns (`:299`).

## 6.8 Resume DB (`mtimedb.rs`, 644 lines)

`mtimedb_path` (`:99`); `write_resume_list` (`:313`), `read_resume_list`
(`:356`, → `ResumeList` `:86` = display list + `ResumeCpv` `:73` +
`ResumeOpts` `:80`), `rotate_resume_to_backup` (`:340`, multi-item
only), `clear_resume_list` (`:382`, keeps backup);
`ResumeEntryKind` (`:57`).

## 6.9 Locks, privileges, preserved-libs notice

- `portage_lock.rs` (`55 lines`): `PortageLockfile` (`:21`) — sibling
  lockfiles for distfiles/repos.
- `privileges.rs` (`133 lines`): `is_privileged` (`:26`, root-owned or
  unowned-but-writable check), `deny_superuser` (`:68`).
- `preserved_libs.rs` (`147 lines`): `show_preserved_libs_notice`
  (`:26`) + display (`:47`) after merges/unmerges.

## 6.10 Remote mode (`remote.rs` + `remote_bundle.rs`)

- `remote.rs` (3441 lines): SSH transports (`RemoteTransport` `:64`,
  `StrictHostKeyChecking` `:33`, `ConfigPlacement` `:87`);
  `check_remote` (`:230`) parses remote flags into `RemoteContext`
  (`:131`, handoff `:119-124`); `sh_quote` (`:339`); control dir
  (`:363-374`); `ssh_argv` (`:404`); transport-vs-remote error
  classification (`:444`); raw command + dir pull + file send
  (`:535-621`); preflight gates incl. clock skew (`:685-775`);
  known-hosts/fingerprint (`:795-807`); `run_remote_cli` (`:891`),
  `run_remote_resolve` (`:941`), `run_remote` (`:1265`),
  merge/phase drivers (`run_merge_stage` `:2544`).
- `remote_bundle.rs` (599 lines): `BundleManifest` (`:28`,
  render `:45`, parse `:58`); `split_pf` (`:129`), `split_pvr`
  (`:145`), `read_eapi` (`:160`); `FileMeta` (`:185`), render
  (`:202`), collect (`:228`); `select_phases` (`:295`);
  `build_bundle` (`:315`) stages image + build-info + environment +
  manifest.

## 6.11 `mrg` applet (`mrg.rs`, 1833 lines)

`command` (`:1125`, clap surface: `Opt` `:127`, `Kind` `:111`,
`build_arg` `:1093`, validators `:1301-1315`); `action_selected`
(`:1148`); `emerge_handles` (`:1160`, which longs forward);
`to_emerge_argv` (`:1231`, incl. `join_optional_values` `:1332`);
`run` (`:1385`) → forwards to the emerge codepath. Remote flags parse
but never forward (`:1641-1732`).

## 6.12 Misc

- `difflib.rs` (208 lines): `ratio` (`:24`), `get_close_matches`
  (`:50`) — CPython-compatible fuzzy matching for search/misspell.
- `error.rs` (89 lines): `Error` (`:11`) + `From` impls.
- `regen.rs` (850 lines): `run` (`:175`), `RegenWorkItem` (`:128`),
  `RegenFailure` (`:142`), retry helpers (`:157-167`), parallel
  dispatcher (`run_parallel` `:409`, `next_dispatchable_in` `:481`,
  `run_work_item` `:377`), `regen_one` (`:526`),
  `prune_stale_entries` (`:498`), `render_entry` (`:586`),
  `write_entry` (`:636`), `eclasses_field` (`:672`).
- `ebuild.rs` (546 lines): `run` (`:122`) — full single-ebuild
  command runner over `ebuild_options` tables.

## 6.13 Complete function index (non-test functions)

Counts via signature scan; line = definition start.

**`main.rs`** (167 lines): `Applet::from_name` :55 · `basename` :65 ·
`print_applets` :73 · `run_emerge` :99 · `run_ebuild` :113 ·
`run_mrg` :117 · `run` :121 · `main` :129.

**`emerge_options.rs`** (632): `find` :406 · `lookup` :416 · tables
`BOOLEAN_OPTIONS` :240 · `VALUE_OPTIONS` :268 · `ACTIONS` :387 ·
`Category` :220 · `Lookup` :227.

**`ebuild_options.rs`** (154): `lookup_option` :102 ·
`is_valid_command` :114 · `OPTIONS` :53 · `COMMANDS` :67.

**`pretend.rs`** (14469): display — `columnwidth_from_env` :262 ·
`columns_line` :294 · `attr_display_field` :366 ·
`use_flag_sort_key` :440 · `colorize_use_token` :457 ·
`decorate_version` :488 · `use_suffix` :525 ·
`resolve_root_deps_running_root` :594 · `root_suffix` :622 ·
`localized_size` :638 · `package_counters_summary` :657 ·
`merge_bound_version` :827 · `entry_display_version` :843 ·
`kept_alt_for_display` :895 · `blocker_row_disposition` :917 ·
`replacement_entry_index` :999 · `replacement_wait_index` :1011 ·
`format_blocker_row` :1110 · `trailing_blocker_lines` :1152 ·
`collect_inline_blocker_lines` :1197 · `count_blocker_rows` :1245 ·
`print_entry_line` :1298 · `print_tree` :1904 (+ tree-walk helpers
:2067-2108) · JSON :2304-2768 (`json_escape` :2304 · `json_string`
:2320 · `entry_to_json` :2355 · `slot_conflict_to_json` :2581 ·
`changed_deps_report_entry_to_json` :2621 ·
`autounmask_change_to_json` :2634 · `abort_outcome_to_json` :2649 ·
`print_circular_block` :2684 · `print_json` :2768) · help
(`report_option` :2852 · `wants_help` :2879 · `print_help` :2892) ·
world/sets (`read_world_atoms` :3034 · `expand_selected` :3058 ·
`installed_set_atoms` :3073 · `preserved_rebuild_atoms` :3104 ·
`update_world_file` :3166 · `update_world_sets_file` :3271 ·
`read_world_sets` :3324 · `resolve_custom_set` :3369 ·
`collect_installed_sets` :3404 · `run_deselect` :3523) · atoms
(`dep_expand_token` :3784 · `qualify_bare_name` :3833 ·
`resolve_vdb_path_arg` :3862 · `still_listed_parents` :3914) ·
unmerge (`run_unmerge_pretend` :3950 · `execute_unmerge` :5054 ·
`deselect_from_world` :5139 · `print_unmerge_row` :5184 ·
`installed_cp_versions` :5206 · `split_pf` :5244 ·
`resolve_cleanup_args` :5263) · prune/depclean (`run_prune_pretend`
:5337 · `run_prune_nodeps_pretend` :5436 · `run_clean_pretend` :5468 ·
`run_prune_nodeps_or_clean` :5492 · `lib_consumer_scan` :5662 ·
`apply_depclean_lib_check` :5738 · `depclean_unresolved_halt` :5790 ·
`run_depclean_pretend` :5857) · query (`run_list_sets` :6099 ·
`defined_set_names` :6111 · `run_search` :6183 ·
`search_best_candidate` :6365 · `search_candidate_visible` :6381 ·
`render_ambiguous_search_output` :6403 · news :6635-6812
(`run_check_news` :6635 · `write_news_state_if_changed` :6711 ·
`news_item_format` :6746 · `news_item_valid` :6760 ·
`news_item_relevant` :6812) · info :6889-7789
(`build_config_env` :6927 · `use_expand_display_value` :7012 ·
`resolved_global_use` :7049 · `tool_first_line` :7061 ·
`highest_installed_pvr` :7074 · `print_info_header` :7088 ·
`info_profile_version` :7271 · `run_info` :7311) ·
`run_config_action` :7789 · conflicts (`misspell_suggestion_block`
:7943 · `slot_conflict_atom_string` :8022 ·
`slot_conflict_reasons` :8068 · `slot_conflict_caret_idx` :8140 ·
`colorize_marked_spans` :8257 · `slot_conflict_need_rebuild` :8288 ·
`render_pkg_use_display` :8342 · `skip_conflict_caret_line` :8362) ·
`run` :8389 · parse/ask infra (`package_options_from_env` :4325 ·
`shell_split` :4391 · `args_with_emerge_defaults` :4456 ·
`apply_portage_scheduling_policy` :4509 ·
`apply_scheduling_policy` :4565 · `stdin_is_tty` :4628 ·
`ask_confirm` :4654 · `ask_enter_invalid` :4694 ·
`classify_yes_no` :4710 · `ask_select` :4734 ·
`clean_delay_countdown` :4778 · `entries_not_merged` :4796 ·
`run_resume` :4838 · `feature_enabled` :5048).

**`emerge_build.rs`** (3196): `entry_version` :58 ·
`locate_candidate` :79 · `ebuild_path` :95 · `run_buildpkgonly` :121 ·
`run_source_merge` :270 · `entry_matches_any` :353 ·
`entry_is_live` :379 · `entry_buildpkg_wanted` :409 ·
`run_merge_loop` :440 · `merge_one_source_entry` :522 ·
`build_use_env` :606 · `entry_package_env_vars` :625 ·
`entry_identity_env` :654 · `run_wide_phase_env` :692 ·
`entry_metadata_env` :717 · `entry_phase_env_tail` :748 ·
`entry_build_env` :789 · `scheduler_cp_version` :809 ·
`scheduler_needs_build` :830 · `resume_entry` :852 ·
`resolved_features` :900 · `build_log_path` :908 ·
`ensure_portage_logdir_symlink` :983 · `tail_of` :1047 ·
`build_one_source_entry` :1079 · `merge_one_built_entry` :1201 ·
`scheduler_skip_dependents` :1259 · `system_loadavg_1min` :1294 ·
`run_build_scheduler` :1309.

**`emerge_getbinpkg.rs`** (1719): `refresh_binhost_indexes` :63 ·
`run_merge_plan` :124 · `merge_one_binary_entry` :192 ·
`resolve_local_binpkg` :326 · `download_and_verify` :376.

**`ebuild.rs`** (546): `wants_help` :80 · `print_help` :87 ·
`run` :122.

**`ebuild_phases.rs`** (6424): `parse_eapi` :218 ·
`split_package` :276 · `phase_prerequisites` :316 ·
`is_real_phase_command` :337 ·
`is_real_standalone_phase_command` :370 · `compute_environment` :398
(+ `Environment` accessors :511-531) · `repo_root` :652 ·
`portage_checkout` :670 · `bin_dir` :698 · `repo_root_for` :779 ·
`restrict_*` :810-828 · `phase_standalone_base_env` :926 ·
`restrict_and_properties` :1030 · `match_package_env_vars` :1060 ·
`depend_use_set` :1091 · `resolved_fetch_config` :1138 ·
`resolved_ro_distdirs` :1174 · `bind_slot_operator` :1366 ·
`build_phase_use` :1420 · `write_post_install_metadata` :1428 ·
`write_post_install_soname_deps` :1583 · `inject_libc_dep` :1674 ·
`dir_size_bytes` :1712 · `environ_whitelisted` :1964 ·
`phase_isolation` :2201 · `sandbox_wrapped_command` :2267 ·
`eclass_locations_value` :2360 · `phase_env_vars` :2386 ·
`phase_setup_script` :2658 · `shell_single_quote` :2706 ·
`real_bash_path` :2800 · `run_one_phase_bash` :2941 ·
scheduler registry :3058-3109 · `run_depend_phase` :3152 ·
`depend_phase_metadata` :3277 · `run_misc_functions_bash` :3527 ·
`run_misc_function` :3634 · `run_commands` :3839 ·
`run_commands_logged` :3871 · `brush_phase_params` :4055.

**`ebuild_merge.rs`** (6973): `is_real_merge_command` :212 ·
`is_real_qmerge_command` :220 · `MergeOptions::from_env` :427 ·
`set_resolved_features` :492 · `is_protected` :510 ·
`new_protect_filename` :561 · `new_backup_path` :614 ·
`protect_decision` :694 · cfgfiledict :766-787 · plib registry
:806-1006 · `preserve_libs_on_unmerge` :1215 ·
`preserved_lib_paths` :1281 · `find_unused_preserved_libs` :1310 ·
`prune_unused_preserved_libs` :1384 · `parse_slot` :1462 ·
`repository_name_for` :1485 · md5 :1498-1505 ·
`format_contents_line` :1511 · `mtime_secs` :1533 ·
`merge_tree` :1552 · `create_special_node` :1892 ·
`lchown_or_chown` :1962 · `needs_move` :2001 · `files_equal` :2019 ·
`next_counter` :2067 · `write_vdb_entry` :2115 ·
`apply_install_mask` :2144 · `write_consolidated_metadata_file`
:2204 · `write_vdb_entry_from_dir` :2248 · `read_installed_slot`
:2315 · `installed_instance_pf` :2341 · `owns_path` :2377 ·
`owns_path_pf` :2399 · `owned_node_type_pf` :2424 ·
`owned_node_value_pf` :2455 · `blocked_installed_packages` :2522 ·
`blockers_from_flat_deps` :2609 · `find_collisions` :2696 ·
`find_owners` :2814 · `collision_message` :2867 ·
`feature_enabled` :2917 · `process_merge_elog` :2941 ·
`run_merge` :2968 · `run_qmerge` :3070 · `source_distfiles` :3096 ·
`merge_after_install` :3131 · `unmerge_replaced_same_slot` :3387 ·
`unmerge_one_installed` :3505 · `run_vdb_saved_env_phase` :3600 ·
`merge_binpkg` :3684.

**`ebuild_unmerge.rs`** (1669): `is_real_unmerge_command` :161 ·
`parse_contents` :178 · `remove_contents` :235 ·
`cleanup_info_dir` :397 · `remove_dirs` :443 · `unmerge_pkgfiles`
:577 · `delete_vdb_dir` :690 · `run_unmerge` :702.

**`ebuild_package.rs`** (1968): `is_real_package_command` :101 ·
`compress_template` :231 · `find_binary` :246 ·
`resolve_compression_command` :281 · `makeopts_to_job_count` :299 ·
`phase_compression_command` :342 ·
`resolve_compression_command_jobs` :355 · `now_unix_time` :374 ·
`packages_index_header` :397 · `format_packages_entry` :401 ·
`write_packages_index_entry` :430 · `require_signing_config` :500 ·
`run_package` :528 · `package_after_install` :569 ·
`binpkg_extension` :793 · `multi_instance_binpkg_suffix` :805 ·
`allocate_binpkg_build_id` :822 · `binpkg_checksums` :866 ·
`invoke_dyn_package` :903 · `quickpkg_from_vdb` :1050 ·
`copy_dir_recursive` :1269.

**`fetch.rs`** (2715): `gentoo_mirrors_from_env` :105 ·
`wget_fetch` :257 · `portage_ssh_opts_from_config` :265 ·
`fetch_commands_from_config` :280 · `assemble_candidates` :338 ·
`primary_uris` :397 · `checksum_failure_max_tries` :424 ·
`group_by_filename` :455 · `mirror_url` :495 · `fsmirrors` :563 ·
`copy_from_fsmirrors` :587 · `distdir_writable` :612 ·
`has_enough_space` :624 · `fetch_src_uri` :655.

**`binpkg.rs`** (4164): `read_gpkg_metadata` :776 ·
`read_xpak_metadata` :856 · `read_xpak_member_raw` :870 ·
`read_xpak_segment` :882 · `parse_xpak_members` :933 · `be32` :969 ·
`verify_gpkg_manifest` :1015 · `extract_binpkg` :1275 ·
`extract_gpkg_metadata` :1335 · `scan_image_tar` :1392 ·
`extract_xpak_image` :1470 · `extract_gpkg_member` :1522 ·
`populate_local_pkgdir` :1640 · `scan_binpkg_file` :1737 ·
`GpgVerify::{from_env,from_features,from_binrepo}` :1920-1954 ·
`gpg_policy_for_features` :1983 · `gpg_verify_argv` :2002 ·
`run_gpg_verify` :2018 · `verify_detached_signature` :2086 ·
`verify_clearsigned_manifest` :2115 (+ private helpers
:105-653 as listed in §6.1).

**`elog.rs`** (1873): `module_enabled` :108 · `echo_enabled` :115 ·
`collect_all` :151 · `collect_all_phases` :184 · `collect` :204 ·
`logdir` :238 · `save_process` :283 · `save_summary_process` :308 ·
`syslog_process` :361 · `custom_process` :410 ·
`save_modules_process` :1133 · `process_batch` :1223 ·
`echo_summary` :1286.

**`color.rs`** (454): `phase_colormap_export` :231 · `nc_len` :275 ·
`resolve_havecolor` :302 · `Colorizer::{new,c,pkgprint}` :335-374.

**`env_update.rs`** (539): `info_dirs_inodes` :125 ·
`run_env_update` :239 (+ private :78-206).

**`install_mask.rs`** (296): `resolve` :28 · `InstallMask` methods
:53-113 · `install_mask_dir` :115 · `fnmatch` :164.

**`merge_engines.rs`** (360): `merge_unit_for_entry` :37 ·
`SourceEngine::merge_entry` :134 · `BinaryEngine::merge_entry` :191.

**`needed_elf.rs`** (1939): `compute_multilib_category` :150 ·
`generate_soname_deps` :333 · `read_all_needed_entries` :491 ·
`obj_key` :620 · `rebuild` :694 · `getlibpaths` :859 ·
`find_consumers` :962 · `find_libs_to_preserve` :1087 ·
`soname_consumers` :1196 · `find_unneeded_preserved` :1225
(+ `NeededEntry::{parse,parse_file,to_needed_line}` :75-126).

**`mtimedb.rs`** (644): `mtimedb_path` :99 ·
`write_resume_list` :313 · `rotate_resume_to_backup` :340 ·
`read_resume_list` :356 · `clear_resume_list` :382.

**`remote.rs`** (3441): `set_remote_exec` :119 ·
`take_remote_exec` :124 · `check_remote` :230 · `sh_quote` :339 ·
`run_remote_cli` :891 · `run_remote_resolve` :941 ·
`run_remote` :1041/1265 (+ private :339-807).

**`remote_bundle.rs`** (599): `split_pf` :129 · `split_pvr` :145 ·
`read_eapi` :160 · `render_filemeta` :202 · `collect_filemeta` :228 ·
`select_phases` :295 · `build_bundle` :315
(+ `BundleManifest::{render,parse}` :45-58).

**`mrg.rs`** (1833): `command` :1125 · `action_selected` :1148 ·
`emerge_handles` :1160 · `to_emerge_argv` :1231 · `run` :1385.

**`regen.rs`** (850): `run` :175 · `run_work_item` :377 ·
`run_parallel` :409 · `next_dispatchable_in` :481 ·
`prune_stale_entries` :498 · `regen_one` :526 · `render_entry` :586 ·
`write_entry` :636 · `eclasses_field` :672.

**`difflib.rs`** (208): `ratio` :24 · `get_close_matches` :50.
**`error.rs`** (89): `Error::new` :17 + `From` impls :43-64.
**`preserved_libs.rs`** (147): `show_preserved_libs_notice` :26.
**`privileges.rs`** (133): `is_privileged` :26 ·
`deny_superuser` :68.
**`portage_lock.rs`** (55): `PortageLockfile` :21.

> Note: `#[cfg(test)]` helper/test functions (several hundred) are
> intentionally omitted — they verify behaviour rather than define it.
> Supporting crates (`portage-repo`, `portage-profile`,
> `portage-fetch`, `portage-dep`, `portage-versions`,
> `portage-use-reduce`, `portage-required-use`, `portage-util`,
> `mrg-director`) are specified through the behaviour they provide in
> §4.1–§4.4; their own public entry points are named inline there.
