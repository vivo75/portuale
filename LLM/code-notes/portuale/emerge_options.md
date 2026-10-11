# crates/portuale/src/emerge_options.rs
Recognition tables for Portage's full `emerge` CLI option surface; every implemented flag is excluded and handled by the caller.

## module
- Purpose: using any Portage flag portuale doesn't implement yet produces a clear "recognized, but not implemented" message, distinct from a genuinely unknown/misspelled flag. The implemented set is what `emerge --help` documents (`pretend/help.txt`). The contract suite pins the text each table produces.
- Short-flag bundling (`-pv`) is supported: it is Portage `argparse`-native behaviour for plain boolean short options, not (as an earlier version of the code comment claimed) something `insert_optional_args` alone provides; verified empirically against Portage's `argparse` before relying on it. `insert_optional_args` is separate: it lets a handful of specific short options (`-v` among them) take an optional value, which argparse's own bundling can't express -- see pretend.rs for the `-v`-specific port. Algorithm documented in pretend.rs.
- `Category::Value` marks which options take an argument (Portage's `argument_options` dict), for accurate enumeration, but the caller (`pretend.rs`) never parses or skips that argument: every lookup here reports and exits immediately, so nothing after a recognized-but-unimplemented option is looked at again in that invocation.

## flag --help
- Implemented, excluded from `ACTIONS` like `--pretend`/`-p` and `--verbose`/`-v` are from theirs. Portuale-specific help text (not a port of Portage's `_emerge/help.py`); checked before anything else, unconditionally, regardless of position in argv. See pretend.rs.

## flag --newuse, --changed-use
- Both implemented (`--changed-use`/`-U` is Portage's narrower alternative). Flag parsing in pretend.rs; reinstall detection in portage-repo's `reinstall_flags_for_use_change`, including how the two flags combine when both are given.

## flag --nodeps
- Implemented (`--nodeps`/`-O`): disables the dependency walk entirely (portage-repo's `resolve_pretend_graph`).

## flag --onlydeps
- Implemented (`--onlydeps`/`-o`, Portage's complement to `--nodeps`; man/emerge.1: "Only merge (or pretend to merge) the dependencies of the packages specified, not the packages themselves"). Only pretend.rs's print loop changes: `resolve_pretend_graph` needs no changes at all, since dependency recursion already happens identically regardless of which top-level entries end up printed.

## flag --update
- Implemented (`--update`/`-u`): ports the `avoid_update`/`dont_miss_updates` behaviour (portage-repo's `resolve_pretend`): without it, an already-installed version that still satisfies the requested atom is kept as-is, never upgraded to a newer visible version just because one exists.

## flag --deep
- Implemented (`--deep`/`-D`): Portage's `argument_options` with an optional value (`--deep` alone means unlimited depth; `--deep=N`/`--deep N` bounds it), not a plain boolean, so pretend.rs's parsing follows the same `insert_optional_args` pattern established for `--verbose`/`-v`. Ports `Deep`, the depth-cutoff semantics (portage-repo's `resolve_pretend_graph`).

## flag --exclude
- Implemented (`--exclude`/`-X`): unlike every other value option here, its value is required, not optional, and Portage's `main.py` declares it `"action": "append"` (repeatable, each occurrence's own value itself a space-separated atom list -- see pretend.rs's parsing). Deliberately NOT bundle-compatible (a bundled `-X` gets its own "requires an argument, can't be bundled" message), since there is no sensible default value the way a bundled `-v`/`-D` has. Ports the `excluded_pkgs`/`WildcardPackageSet` behaviour (portage-repo's `resolve_pretend`), with a documented scope cut relative to Portage depgraph.py's ~18 call sites.

## flag --deselect
- Implemented (`--deselect`/`-W`): like `--verbose`/`-v`, Portage's `argument_options` with a `y_or_n`-choices optional value, ported with the same "peek the next token, consume only if exactly y/n" pattern. Unlike every other implemented flag here, a bare `--deselect`/`-W` turns the whole invocation into a different, standalone action (Portage `main.py`'s "if myaction is None and myoptions.deselect is True: myaction = 'deselect'") rather than modifying the ordinary `--pretend` resolution -- see pretend.rs's `run_deselect` for the ported `action_deselect` behaviour, and its documented `Atom.intersects()` scope cut.

## flag --with-bdeps, --with-bdeps-auto
- `--with-bdeps` implemented: Portage's `argument_options` with `"choices": ("y", "n")`, a REQUIRED closed-choice value (unlike `--exclude`'s arbitrary text or `--deep`/`--verbose`'s optional peek): a missing value, or one that is neither `y` nor `n`, is an immediate usage error either way (the latter is Portage `argparse`'s own choices validation), so it has no short alias and is deliberately NOT bundle-compatible either -- Portage's `main.py` declares no `shortopt` for it at all, so there is no bundling concept to begin with. Ports the `depgraph.py`/`create_depgraph_params.py` `bdeps` semantics (portage-repo's `resolve_pretend_graph`): `n` skips DEPEND/BDEPEND for an already-installed package's own dependency walk under `--deep`; `y`/the Portage default `auto` both keep them, collapsed into one bool since `depgraph.py` itself never distinguishes the two.
- `--with-bdeps-auto` (the only other Portage lever on the same `bdeps` value) implemented too, with the identical REQUIRED closed-choice shape. Precedence in pretend.rs's CLI parsing: `--with-bdeps-auto=n` only changes the default `with_bdeps` value (from Portage's own "auto" to "n") -- an explicit `--with-bdeps` always wins regardless, matching Portage `create_depgraph_params.py`'s `bdeps = myopts.get("--with-bdeps"); if bdeps is not None: ... elif ... myopts.get("--with-bdeps-auto") != "n" ...: myparams["bdeps"] = "auto"` -- the `--usepkg`-gated half of that same condition is always true here, since portuale's CLI has no `--usepkg` at all.

## flag --changed-deps, --changed-deps-report
- `--changed-deps` implemented: Portage's `default_arg_opts` with a `y_or_n` optional value, the identical shape `--verbose`/`-v` and `--deselect`/`-W` have (no short alias -- Portage's `main.py` declares none). Ports `depgraph.py::_changed_deps` (portage-repo's `PretendOutcome::Reinstall` and `deps_changed`): reinstalls an already-installed package whose vdb-recorded dependency strings differ from the repo's current ebuild, with a documented flat-comparison scope cut (portuale has no structured, non-flat `use_reduce` anywhere, so a dependency moved between two dep-string keys with the same net atom set, or a pure `||`-restructuring, isn't detected as "changed" the way Portage's own structured comparison would).
- `--changed-deps-report` implemented too, with the identical `y_or_n` optional-value shape (no short alias either). Ports `depgraph.py::_changed_deps_report` (portage-repo's `resolve_pretend_graph`): a report-only WARN, never a resolution change, reusing `deps_changed` unmodified for the comparison itself. Portage: "This is completely silent... if --changed-deps or --dynamic-deps is enabled" -- ported as simply never computing anything at all when `changed_deps` is true, since Portage's own `_changed_deps_pkgs` collection is discarded unread in that case anyway (a documented, behaviour-preserving simplification, not a guess). `--dynamic-deps` itself stays unimplemented/unrecognized in portuale (Portage's own now-defunct alternate resolver strategy), so only the `--changed-deps` half of that silencing condition is reachable here at all.

## flag --changed-slot
- Implemented: Portage's `default_arg_opts` with a `y_or_n` optional value, the identical shape `--changed-deps` has (no short alias). Ports `depgraph.py::_changed_slot` (portage-repo's `PretendOutcome::Reinstall` and `slot_changed`): reinstalls an already-installed package whose vdb-recorded `SLOT` differs from the repo's current ebuild, with a documented scope cut: Portage's own consumers of `_changed_slot` live deep inside binary-package/slot-operator-rebuild scheduling portuale has none of, so this is ported as simply another independent `Reinstall` trigger instead of replicating that considerably messier control flow.

## flag --with-test-deps
- Implemented: the identical `y_or_n` optional-value shape `--changed-slot` has (no short alias). Ports the `depgraph.py::_add_pkg` gating (portage-repo's `resolve_pretend_graph`): top-level atoms only, "test" a valid, not-already-enabled, not-use-masked IUSE flag; built on the `use_reduce(..., subset={"test"})` extraction (portage-use-reduce's `use_reduce_flat_subset`).

## flag --noreplace, --selective
- Both implemented; found and grounded by comparing portuale's own output against the installed system `emerge` on a genuine package (`sys-apps/portage`) and tracing Portage's own decision live: a bare `emerge <atom>` with no other flags does NOT keep an already-installed, already-satisfying package as-is in Portage -- it is reinstalled anyway, unless Portage's `myparams["selective"]` is set, which `--update`/`--newuse`/`--changed-use`/`--changed-deps`/`--changed-slot`/`--noreplace`/`--selective`/`--newrepo` all independently do (`create_depgraph_params.py`). `--noreplace` is a plain boolean, hence bundle-compatible (`-pn`, alongside the other bundle-compatible booleans); `--selective` has the identical meaning but a Portage `y_or_n` optional value instead (the same shape `--changed-deps` has, no short alias of its own -- "n" explicitly cancels `selective` even if another flag already set it, Portage `create_depgraph_params.py`'s own unconditional `if myopts.get("--selective") == "n": pop`). See portage-repo's `resolve_pretend` for the full grounding, including why portuale's own `selective` computation needs no separate `--reinstall` flag (`--changed-use` already covers its whole contribution) and the documented, narrower scope cut around Portage's `--newrepo` (needs a vdb `REPOSITORY` reader portuale doesn't have).

## flag --binpkg-changed-deps, --use-ebuild-visibility
- Both implemented (`--binpkg-changed-deps[=y|n]`, `--use-ebuild-visibility[=y|n]`): both Portage `true_y_or_n` + `default_arg_opts` (bare -> `y`), the same shape the `--rebuild-if-*` block parses. Each is a resolver-time override of an automatic default: `--binpkg-changed-deps=n` turns off the changed-`*DEPEND` binary-rejection otherwise auto-on for any non-`--usepkgonly` run (and `=y` forces it on even under `--usepkgonly`); `--use-ebuild-visibility` enforces the `_equiv_ebuild_visible` check on a built candidate even under `--usepkgonly` / a `--useoldpkg-atoms` match (Portage `depgraph.py:8027`'s `not use_ebuild_visibility and (usepkgonly or useoldpkg)` guard). Both applied via process-globals (`portage_repo::set_binpkg_changed_deps_override` / `set_use_ebuild_visibility`), the env-free pattern `--useoldpkg-atoms` / `--package-moves` use.

## flag --ask
- Implemented (`--ask`/`-a`, Portage `true_y_or_n`): the interactive `Would you like to ...?` prompt before a merge/removal (pretend.rs's parse loop + `ask_confirm`).

## flag --autounmask-continue, --autounmask-backtrack, --autounmask-only
- `--autounmask-continue` (Portage `true_y_or_n`) and `--autounmask-backtrack` (Portage `y|n`) are recognized but inert in pretend-only portuale (Portage gates write-and-continue on `"--pretend" not in myopts`, and portuale has no backtracking resolver), except the `actions.py:3772` `--autounmask-continue has been disabled by --autounmask=n` warning. See pretend.rs.
- `--autounmask-only` (Portage `true_y_or_n`, `actions.py:456`) implemented: resolve, show the `display_problems()` equivalent, exit 0 (pretend.rs's `autounmask_only` / `show_merge_list`); under `--pretend` the merge list is re-shown ahead of the change blocks too (backlog #271, Portage `_display_autounmask` -> `_show_merge_list`).

## flag --complete-graph
- `--complete-graph` / `--complete-graph-if-new-use` / `--complete-graph-if-new-ver` implemented: Portage `create_depgraph_params.py:169-175` + `depgraph.py::_complete_graph`. In pretend-only portuale `complete` mode collapses to a forced deep walk (see `portage_repo::resolve_pretend_graph`'s `complete` param and `complete_graph_auto_enable`); `pretend.rs` parses all three and runs the two-pass auto-enable.

## flag --dynamic-deps
- Implemented: ON by default (portuale's long-standing `--deep` behaviour); `=n` walks an installed package's vdb dep snapshot instead (see `enqueue_dependencies`).

## flag --fuzzy-search, --regex-search-auto, --search-similarity, --misspell-suggestions
- `--fuzzy-search` / `--regex-search-auto` / `--search-similarity` implemented: `emerge --search`'s Portage `difflib` fuzzy matching + regex-auto detection (pretend.rs's `run_search` + `portuale/src/difflib.rs`).
- `--misspell-suggestions` implemented: `difflib` close-name suggestions when a top-level `cat/pkg` doesn't exist (pretend.rs's `misspell_suggestion_block`).

## flag --ignore-world
- Implemented: Portage `create_depgraph_params.py:128` (`is True`, so bare / `=y` / `=True` only) + `depgraph.py:357-360` (empties `_required_set_names`, dropping `@world` -- which nests `@selected`/`@system`/`@profile` -- from the complete-graph seeds) + `:10107` (skips the `@selected` uninstall guard). Portuale empties `complete_seed_atoms` (which also empties `blocker_retry_seed_atoms` via the existing clone); explicit args, `--deep`/`--update` and the removal actions (Portage returns early for those) are untouched. See pretend.rs's parse loop + `run_resolve`'s seed block. No short alias (Portage's `main.py` declares none), and the `=n` spelling must not match either (lookup strips the value before the table search).

## flag --reinstall-atoms
- Implemented (`--reinstall-atoms ATOMS`): a matching already-installed package is forced to re-merge (pretend.rs's parse loop + `resolve_pretend_graph`'s `reinstall_atoms`).

## flag --onlydeps-with-rdeps, --onlydeps-with-ideps
- Both implemented: Portage `main.py` `true_y_or_n` (`"True"`/`"y"`/`"n"`, bare inserts `"True"`), `depgraph.py:4186-4193` (blank an `--onlydeps` root's `RDEPEND`+`PDEPEND` on `=n`, its `IDEPEND` too unless ideps is `y`/`True`). See pretend.rs's parse loop + `ResolveRequest`'s `onlydeps*` fields.

## flag --rebuild-exclude, --rebuild-ignore, --rebuild-if
- `--rebuild-exclude` / `--rebuild-ignore` implemented: filters for the `--rebuild-if-*` scan (pretend.rs + portage-repo's `rebuild_if_entries`).
- `--rebuild-if-new-slot` / `-new-rev` / `-new-ver` / `-unbuilt` implemented: rebuild an installed package whose build-time dep is being merged this run (pretend.rs + portage-repo's `rebuild_if_entries`; `-new-slot` is the pre-existing `:=` slot-operator auto-rebuild).

## flag --package-moves
- Implemented: `=n` disables `profiles/updates/` move application (`portage_repo::set_package_moves_enabled`).

## flag --quickpkg-direct
- `--quickpkg-direct` / `--quickpkg-direct-root` implemented: when `--usepkg` + `--quickpkg-direct=y` + target `ROOT` != source root, the source root's installed packages join the binary pool (see `portage_repo::set_quickpkg_direct_root`).

## flag --quiet, --quiet-build
- `--quiet`/`-q` (Portage `true_y_or_n`, verbosity level 1) and `--quiet-build[=y|n]` (redirect build output to `${T}/build.log`) both implemented; the caller parses them directly like `--verbose`/`-v`. See pretend.rs.

## flag --read-news
- Implemented: with `--ask`, offer `eselect news read` while calculating dependencies (see pretend.rs, backlog #231).

## flag --useoldpkg-atoms
- Implemented (`--useoldpkg-atoms ATOMS`): for a matching package, prefer an existing binary package over a newer unbuilt ebuild (see `portage_repo::set_useoldpkg_atoms`; only bites under `--usepkg`/`--getbinpkg`).

## flag --oneshot
- Implemented (`--oneshot`/`-1`): suppresses the world-file write on a merge, and the world colour at `--pretend`.

## tests
- Each `does_not_recognize_*` test pins that an implemented flag is excluded from the recognition tables (handled directly by the caller). The `--ignore-world` case (implemented: Portage `create_depgraph_params.py:128-129` + `depgraph.py:357-360`) additionally pins that the `=n` spelling doesn't match (lookup strips the value first) and that it has no short alias.
