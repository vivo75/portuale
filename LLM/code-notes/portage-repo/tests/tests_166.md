# crates/portage-repo/src/tests/tests_166.rs
Direct predicate-result legs for rebuild/circular/instance-use residue.

## module
- Backlog #166 (circular/instance-use solution residue). No end-to-end contract reproduction: every in-body operator flip changes the asserted outcome. Self-contained (own scratch repo/vdb/entry helpers) so the other Track U branches' blocks rebase mechanically.
- Direct predicate-result legs in the #161/#162 scratch style: each test observes the predicate's own return value. Do not edit other items' `mod tests_16x` blocks.

## section S2 (`filter_usepkg_exclude_include`)
- Portage `create_depgraph_params.py` `--usepkg-exclude`/`--usepkg-include` handling, applied to the binary pool in `depgraph.py`'s own break-out-of-the-loop rejection (see the call site's doc comment).

## section S3 (`rebuilt_binary_changed`)
- Portage `depgraph.py`'s two independent `rebuilt_binary` triggers folded into one: the `--rebuilt-binaries` BUILD_TIME comparison and the no-ebuild-visible rejection of an installed built instance (see the function's own doc comment).

## section S4 (`direct_solve_arg_mode`)
- Portage `_want_installed_pkg` (depgraph.py:7280-7302), folded to what the direct solve needs: the installed instance is "argued" exactly when the user asked for that cp without `--exclude` and outside selective mode.

## section S5 (`direct_solve_instance_use`)
- Portage `pkg.with_use(self._pkg_use_enabled(pkg))`: a merge instance with an entry reads its resolved display flags, a merge instance without one reads the tree candidate effectively, an installed instance reads its vdb record (see the function's own doc comment).

## section S6 (`synthesize_surviving_conflict_entries`)
- Files each surviving conflict instance beside its merge-bound siblings, so the final merge list prints conflicting instances adjacently (see the function's own doc comment).

## section S7 (`circular_dep_solutions`)
- Portage `_find_suggestions` (`3rdparty/portage/lib/_emerge/resolver/circular_dependency.py:114`): per-cycle-position USE assignments whose reduced dep drops the pull atom while REQUIRED_USE still holds, minimised and checked against grandparent use-deps. Cycles are handed in directly as cpv strings over scratch repos -- no graph walk needed.

## fn strip_revision_166_strips_numeric_revisions_only
- Kills the whole-body `""`/`"xyzzy"` rows, both guard replacements, the `&&` -> `||` widening (empty rev strips) and the `!` deletion (numeric rev kept).
- Non-numeric tail: the guard stays shut (guard-`true` would strip to "1.0").
- Empty rev: `!rev.is_empty()` is false while `all(digits)` is vacuously true, so `&&` -> `||` would strip to "1.0".

## fn filter_usepkg_166_exclude_drops_only_the_match
- Kills the either-empty gate widening (`&&` -> `||` keeps everything), the `||` -> `&&` narrowing in `is_included` (drops everything) and the `!` deletion (keeps the excluded row).

## fn filter_usepkg_166_include_drops_a_non_match
- Kills the `!excluded && included` -> `||` widening (keeps the row through `!excluded` alone). A matching include keeps the row: no mutant target; documents the `is_empty() ||` arm.

## fn rebuilt_binary_166_timestamped_comparison
- Kills the `&&` -> `||` widening (older binary reports through the floor arm), the three `>` rewrites and the `>=` -> `<` flip. Older binary: `false && true` is false, `false || true` is true. Equal stamps: `>` is false, `>=` is true.
- Pins both `let-else return false` arms; no mutant targets.

## fn rebuilt_binary_166_untimestamped_comparison
- Kills the `!=` -> `==` flip in both directions (newer reports, equal does not). No mutant target on the missing-record tolerance: the second `let-else` fires with no installed BUILD_TIME at all.

## fn direct_solve_arg_mode_166_argues_the_requested_installed
- Kills the whole-body `-> false` row.

## fn direct_solve_arg_mode_166_rejects_every_non_arg_shape
- Kills the `!any(installed)` deletion and the `||` -> `&&` narrowing (both proceed to `true`); kills the `!excluded.any` deletion (which reports `true`).

## fn write_pkg_166
- The md5-cache validation guard fails otherwise; the `DEPEND`/`REQUIRED_USE` lines are for the circular legs below.
- One scratch ebuild plus its md5-cache entry, in the #162 `write_pkg_162` shape extended with `DEPEND`/`REQUIRED_USE` lines.

## fn direct_solve_instance_use_166_installed_reads_vdb_over_tree_iuse
- Kills the whole-body rows and the `==` -> `!=` version-find flip (which falls back to `enabled.clone()`).

## fn synthesize_166_files_the_instance_beside_its_siblings
- Kills both `+` -> `-`/`*` index shifts, both `&&` -> `||` widenings (which file behind the trailing decoy instead) and both `==` -> `!=` flips (which file behind the other-category same-package row, or at the end).

## fn synthesize_166_new_slot_tracks_the_installed_set
- Kills the `!is_empty()` deletion in both directions (installed reads `true`, fresh root reads `false`).

## fn circular_166_two_flag_or_requirement_suggests_enabling_y
- Kills the cur-construction `<<` -> `>>` (bit 1 never sets, no solution), the sol-diff `<<` -> `>>` plus both `&&` flips and both `!` deletions around the diff (all emit `[{}]` or a bloated pair), the REQUIRED_USE `!empty` deletion and the `!ok` deletion (both admit the violating empty assignment), and the `>` -> `<` pair on the MAX_AFFECTING_USE gates (retain/drop to nothing).
- Two entangled flags under `|| ( x y )` with both off: only "enable y" removes the atom while REQUIRED_USE holds. The sol-diff `<<` -> `>>` plus both `&&` flips and both `!` deletions around the diff all emit `[{}}` or a bloated pair.

## fn circular_166_ten_flags_yield_nine_minimal_pairs
- Kills the REQUIRED_USE `!untouchable` deletion (collapses to nothing), the `<=` -> `>` expansion flip (stays unexpanded, nothing holds), and the `>` -> `==`/`>=` pair on both MAX_AFFECTING_USE gates (retain-to-one / give-up, both empty).

## fn circular_166_plain_gated_edge_suggests_disabling_x
- Kills the `&&` -> `||` widening on the REQUIRED_USE gate (skips the only solution).

## fn circular_166_eleven_flags_retain_to_ten_and_proceed
- Every retained flag gates its own occurrence of the pull atom. Kills the second gate's `==` -> continue and `>=` -> continue (both give up at exactly ten where Portage proceeds).

## fn circular_166_eleven_flags_retain_to_nine_and_proceed
- Kills the second gate's `<` -> continue (which gives up below ten where Portage proceeds).

## fn circular_166_three_ring_reports_each_parent
- Kills the parent-index `-` -> `+` rotation (identical on two-rings, wrong parent on three).

## fn direct_solve_instance_use_166_merge_reads_its_own_entry
- Kills the whole-body rows, the category/package `==` -> `!=` flips (which match the first decoy instead) and the first `&&` -> `||` widening (which matches the same-category decoy).

## fn cand_166
- Hand-built binary candidate in the #162 `cand_162` shape (same fields; `candidate_str` renders `cat/pkg-ver:slot/sub::repo`).

## fn cfg_166
- Minimal visibility config in the #161 `test_config` shape (amd64-only, no overrides).

## fn arg_input_166
- Eight-arg input builder (same shape as #161's `ds_input_161`).
