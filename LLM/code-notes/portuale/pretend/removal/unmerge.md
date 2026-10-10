# crates/portuale/src/pretend/removal/unmerge.rs
`--unmerge`: listing and removing installed packages.

## fn still_listed_parents
- Ports Portage's `unmerge.py:355-447` "still listed in the following package sets" check. The sets are the user-editable `installed_sets` (`EditablePackageSet`s reached via `world_sets`). Minus rule: Portage's `unmerge.py:421-441` `higher_slot` (`pkg.slot_atom != inst_pkg.slot_atom` after the descending-order `pkg >= inst_pkg` break -- removing this version leaves that set satisfied). Shared by `run_unmerge_pretend` and `run_prune_pretend`, matching Portage's own single `_unmerge_display` handling every `unmerge_action`.

## struct RunUnmergePretendArgs
- `action`: Portage's `unmerge_action` -- threaded into the messages via Portage's `f"... {unmerge_action}"`. `--rage-clean` only differs by skipping `CLEAN_DELAY` + prerm/postrm.
- `repos`: threaded into the post-removal GLEP 42 count notice (backlog #196 fix round 1: Portage's `post_emerge` after uninstall actions, `actions.py:4164-4175`).
- `preserve_order`: Portage's `_unmerge_display` `ordered` flag (`unmerge.py:459`). Only `--depclean`'s own topologically-sorted cleanlist sets this (`run_depclean_pretend`).
- `pretend`: `false` for a Portage `emerge -C <atom>` (no `--pretend`).
- `ask`: `--ask`/`-a` prompt `Would you like to unmerge these packages?` (Portage's `unmerge.py:621`).
- `shell`: `--shell bash|brush`, including the `FEATURES=unmerge-backup` `quickpkg`.
- `debug`: `--debug`/`-d`, including the `unmerge-backup` `quickpkg`.
- `quiet` / `noinfo`: Portage's `--quiet` (`noiselimit < 0`, `actions.py:3907-3908`); `noinfo` is the resolved-config `FEATURES` read (`config_features_list`).

## fn run_unmerge_pretend
- Set expansion: Portage's `root_config.sets[s].getAtoms()` / `_iter_atoms_for_pkg`. `@world`/`@selected`/`@system`/`@installed` are the built-in sets; anything else `@name` is a custom set file (recursively expanded, cycle-guarded). Portage's `root_config.setconfig.active` exclusion.
- Header: Portage's `_unmerge_display` (`unmerge.py:195`) prints it only under `--pretend`/`--ask`, before the per-atom matching loop.
- Accumulators: Portage's own `pkgmap` entry with its `selected`/`protected`/`omitted` sets; Portage's `unordered` dedup.
- The one package `unmerge` always refuses: Portage's `PORTAGE_PACKAGE_ATOM`.
- Bare name: Portage's `vartree.dep_match` "null category" lookup; ambiguous is Portage's `AmbiguousPackageName`.
- Omitted: Portage's `vartree.dep_match(cp)` minus selected/protected.
- System warning: Portage's `_unmerge_display` `if not (protected or omitted) and cp in syslist`, to stderr (Portage's `writemsg_level(..., level=logging.WARNING)`).
- `syslist = root_config.sets["system"].getAtoms()`.
- Removal: Portage's `unmerge()` (`unmerge.py:637-699`): once `_unmerge_display` returns EX_OK and we're not in `--pretend`, the packages are actually removed. The removal loop walks `pkgmap` in exactly the preview order.
- Still-listed suppression detail: Portage's `unmerge.py:421-441` suppresses the flag when an installed *higher-versioned* instance of the same cp *in a different slot* also matches the set atom (`pkg.slot_atom != inst_pkg.slot_atom` after the `pkg >= inst_pkg` break).
- Portage's `root_config.setconfig.active` -- the sets the user passed as `-C` targets. Excluded from the "still listed in package sets" check (their members are being removed from the set anyway).
- The "still listed in the following package sets" warning: a `selected` package that a user-editable set (reached via `world_sets`) still lists would be re-pulled on the next `@world` update, so it's flagged. Portage's `unmerge.py:421-441` suppresses the flag for a set when an installed *higher-versioned* instance of the same cp *in a different slot* also matches the set atom (`pkg.slot_atom != inst_pkg.slot_atom` after the `pkg >= inst_pkg` break -> "a newer version, other slot") -- removing this version leaves the set satisfied by that one.

## fn feature_enabled
- Ports Portage's `"unmerge-backup" in self.settings.features` -- not a `make.globals` default token, so absent unless `FEATURES` names it. The two `execute_unmerge` reads (`unmerge-backup`, `split-elog`) predate its resolved `config` param (backlog #196 fix round 1) and keep the old fallback. Every build/merge path reads the resolved list instead: `config_features_list`/`config_features_string`, `MergeOptions::set_resolved_features`, and `ebuild_phases::features_string`/`emerge_build::resolved_features`.
- This is the **raw process-env fallback**, for the CLI boundaries with no resolved config (`ebuild <file>`) and for the two `execute_unmerge` reads (`unmerge-backup`, `split-elog`) that predate its resolved `config` param (backlog #196 fix round 1) and keep the old fallback. Every build/merge path reads the resolved list instead: `config_features_list`/`config_features_string` here, `MergeOptions::set_resolved_features`, and `ebuild_phases::features_string`/`emerge_build::resolved_features`.

## struct ExecuteUnmergeArgs
- `quiet` / `noinfo`: Portage's `--quiet` (`noiselimit < 0`); `noinfo` is Portage's `"noinfo" not in settings.features` (`post_emerge.py:127`).
- `repos` / `config`: Backlog #196 fix round 1: Portage calls `post_emerge` after the uninstall actions (clean/depclean/prune/unmerge/rage-clean -- `actions.py:4164-4175`, except deselect/buildpkgonly/fetchonly/pretend), whose tail is the GLEP 42 count notice.
- `repos` + `config` feed the GLEP 42 count notice; the vdb-changed gate is structural (every caller only reaches here with a non-empty `removal_list` and `pretend == false`).

## fn execute_unmerge
- `FEATURES=unmerge-orphans` keeps what the resolved `CONFIG_PROTECT` protects (Portage's `dblink.isprotected`); #333.
- `dblink._pre_unmerge_backup` quickpkg; `BINPKG_FORMAT` follows the resolved chain (backlog #173: calling env over `make.conf`/profile/`make.globals`, Portage's `config` precedence) -- resolved here, since this path carries no graph entry. `resolved_features` stays `None` on purpose (only the format joins the chain).
- Standalone `emerge -C`: no replacing package, so neither feed (backlog #224). The entry is deleted at once, as before (S4.1 defers only the replace loop's deletes).
- Failure notice: backlog #196 fix round 1 -- Portage's `post_emerge` prints the notice regardless of retval, gated on the vdb having changed (`:112-117` early return). Same arm for the deselect-failure and info-regen-failure paths.
- Elog: Portage's `dblink.unmerge()` -> `self._elog_process(phasefilter=("prerm", "postrm"))`. `${T}` for a removal is `unmerge_one_installed`'s own `run_phase_from_saved_env` builddir (`<PORTAGE_TMPDIR>/portage/<cat>/<pf>/temp`), which only ever holds prerm/postrm logs here.
- Info/preserved/news tail: Portage's `post_emerge()` after `emerge -C` / `--depclean` / `--prune`; the GLEP 42 tail is `post_emerge.py:155` after the uninstall actions (`actions.py:4164-4175` -- clean/depclean/prune/unmerge/rage-clean; NOT deselect -- `run_deselect` never reaches here -- and never under `--pretend`). The vdb-changed gate is structural (`:112-117` early return).
- `FEATURES=unmerge-backup` -> a `quickpkg` of each package before it's removed (`dblink._pre_unmerge_backup`). Its `BINPKG_FORMAT` follows the same resolved chain as every other `PackageOptions` construction site (backlog #173: calling env over `make.conf`/profile/`make.globals`, Portage's `config` precedence) -- resolved here, since this path carries no graph entry. A failed resolution keeps the old env-only fallback inside `package_options_from_env`'s `None` arm. `resolved_features` stays `None` on purpose: the `FEATURES` tokens keep their raw process-env fallback (only the format joins the chain).
- Elog: `dblink.unmerge()` -> `self._elog_process(phasefilter=("prerm", "postrm"))`: hand each removed package's `pkg_prerm`/`pkg_postrm` `elog`/`ewarn`/`eerror` output to the `echo`/`save`/`save_summary` modules.

## fn deselect_from_world
- Ports Portage's `WorldSelectedPackagesSet.cleanPackage` (`_sets/files.py:336`), called by Portage's `unmerge()` right after each package is removed. The file rewrite matches Portage and `update_world_file` (Portage's `.write`). A world file that ends up empty is written empty (Portage's `replace([])`).

## fn print_unmerge_row
- Ports Portage's `_unmerge_display` per-package rows: `(mytype + ": ").rjust(14)`; each `selected` version is `colorize("UNMERGE_WARN", v + " ")` (red), each `protected`/`omitted` version `colorize("GOOD", v + " ")` (green) -- the label and `none` stay plain. Reproduced faithfully, trailing spaces and all.
- One `    selected: 1.0 ` / `   protected: none ` / `     omitted: ...` row of `_unmerge_display`'s per-package block: the label right-justified into 14 columns (`(mytype + ": ").rjust(14)`), then each version followed by a trailing space, or the literal `none ` when empty -- reproduced faithfully, trailing spaces and all. The label and `none` stay plain.

## fn installed_cp_versions
- Ports Portage's `vartree.dbapi.cpv_all()` split out. The backend's listing (`files`: the same walk, each category and entry directory tested with `is_dir`; `-MERGING-<pf>` entries skipped). #116: through the vdb seam; the main slot only (an absent `SLOT` is `""`, like the old `unwrap_or_default`).

## fn resolve_cleanup_args
- Ports Portage's `action_depclean` argument handling (`actions.py:848-863`): ambiguous bare name -> `!!! ... ambiguous` + `Err(1)`; one matching nothing prints to stderr; `null/` prefix stripped for display.
- Shared by `--depclean` and `--prune`: resolve each bare name against the vdb null-category scan (ambiguous -> `!!! ... ambiguous` + `Err(1)`), then check every resulting atom -- one matching nothing prints `--- Couldn't find 'X' to <action>.` (stderr), and if *none* match, print `>>> No packages selected for removal by <action>` and return `Err(1)`. An empty `targets` is `Ok(vec![])` (the full, no-args form).
