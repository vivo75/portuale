# crates/portage-repo/src/tests/pretend.rs
`resolve_pretend` single-selection pins: usepkgonly, USE-flag reinstalls, sub-slot matching.

## fn usepkgonly_dependency_satisfied_by_an_installed_version_with_no_binpkg
- L1-a: under `--usepkgonly` the ebuild candidate pool is emptied wholesale. Portage's `--usepkgonly` restricts the merge pool, not installed-satisfaction, so the dependency still resolves `AlreadyInstalled`.
- `NoVisibleCandidate`, in a Portage `-K` merge, lands on the resume list ("no visible ebuild to merge").
- A top-level `--usepkgonly` atom still reports the missing binary: Portage requires something mergeable for a named package. Without `--usepkg` the ordinary installed-vs-ebuild path handles it unchanged.

## fn installed_dep_missing_a_use_flag_reinstalls_not_already_installed
- L0 finding L: Portage reinstalls `foo` with `bar` (and autounmasks it). Gated on `autounmask_use`.
- With `--autounmask-use=n` the `matched` filter drops the candidate entirely instead (no flip allowed).

## fn emptytree_reinstalls_an_installed_dependency_that_would_otherwise_be_hidden
- Without `--deep`/`--emptytree` deeppkg2 is `AlreadyInstalled` (and not shown). Under `--emptytree` the dep path runs `dependency_avoid_update_candidate` -> `already_installed_or_reinstall(empty=true)`.

## fn resolve_pretend_matches_a_sub_slot_restricted_atom_against_the_real_sub_slot
- Unlike a slot-operator atom (`:=`/`pkg:slot=`), which needs no candidate-side sub-slot data to match, the plain PMS 8.3.3 `cat/pkg:slot/sub-slot` restriction form does -- see `Candidate::sub_slot` for the bug this closes.

## fn slot_operator_binary_masked_version_reads_only_marked_negatives
- Backlog #212 (v2 `#24c`): a plain `!=cpv` is Portage's slot-conflict / missing-dependency negatives; only `!=cpv[binary]` reads as a `slot_operator_mask_built` record. `is_binary_only_mask` answers the same question for the string-constraint readers.

## fn stale_built_slot_operator_dep_with_no_ebuild_reports_at_this_level
- Backlog #233: the live shape is an installed world member's stale recorded built slot-operator atom `dev-libs/reinstslottarget:0/1=` (installed 1.0 at `SLOT="0/1"`, only the `0/2` ebuild left in the tree). The `@world` walk aborted `NoVisibleCandidate` on it (rc 1) before the #210 reverse-dependency scan could withhold the reinstall, while Portage 3.0.82.2 merges the unrelated world updates and withholds silently (rc 0; S0 probe `probe.log`, offline comparator simulation clean).
- Portage `_wrapped_select_pkg_highest_available_imp` (depgraph.py:7799-7840) iterates the installed db for every resolve (no `--usepkg` gate; the old gate here was the L1-a narrowing).
- Flags mirror that cell (`--changed-slot --update --deep --newuse`, no `--usepkg`). Portage sets selective with `--update`. A top-level atom with the same shape still reports: Portage needs something mergeable for a package named on the command line.
- The runtime-keyed installed hearing lives one layer up (the queue loop's #233 rewrite); the build-time-keyed pins depend on this level reporting.
