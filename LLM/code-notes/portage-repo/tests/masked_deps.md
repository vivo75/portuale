# crates/portage-repo/src/tests/masked_deps.rs
Masked-candidate disclosure, invalid USE-conditional masking, and bug-515584 USE-probe tests.

## fn masked_candidates_for_atom_reports_masked_only_matches
- Portage's `_show_unsatisfied_dep` version match: the masked list fires when every version-matching ebuild is masked, and stays silent when something is visible or nothing matches at all.
- A visible candidate silences the report (USE-mismatch-only and friends take Portage's other paths).
- Nothing matching by version at all: Portage's plain "no ebuilds" case, also silent here.

## fn invalid_use_conditional_mask_matches_portage_on_all_three_fixture_cases
- Backlog #153: Portage 3.0.82.2 masks all three `invalid` (oracles captured against a `cp -a` copy of this same tree) -- `fucyclec` (atom `[x?]` in DEPEND), `usedeppkg` (atom `[baz(+)?]` in RDEPEND -- a defaulted conditional is still validated), `somercurial` (`soflag? ( … )` group in RDEPEND).
- Negative control: `gpcyclea` conditions on `x` with `IUSE="+x"`.

## fn first_invalid_conditional_covers_forms_nesting_and_order
- Encounter order: an atom before a group reports the atom (Portage validates each token as scanned, groups at their `)` close).
- Nested groups validate innermost first.

## fn atom_all_use_unmasked_is_true_for_the_unsatuseor_alternative
- `dev-libs/unsatuseor`'s `||` group's first alternative is `dev-libs/unsatusealt[unsatuseorflag]`: unsatusealt is visible but the flag is off by default (IUSE declares it, nothing enables it), and `unsatuseorflag` is in NO use.mask / use.force profile file -- so Portage's bug-515584 probe (`all_use_unmasked`) must keep the choice in the selectable `unsat_use_*` bins, not demote it to `other`. The second alternative (dev-libs/doesnotexist-unsatuseor) has no visible ebuild at all.

## fn atom_all_use_unmasked_is_false_when_the_violated_flag_is_in_use_mask
- The `other`-demotion side of bug-515584: a `[use]` dep on a flag that sits in the candidate's own use.mask is NOT "fixable by a USE change", so the whole `||` choice drops out of every selectable bin. `dev-libs/slotusetarget` has `=...-2.0` mask `x` in the fixture profile (fixtures/repo/profiles/package.use.mask) -- see the slotconflict fixture tests for the matching side.
- The 1.0 version keeps its +x default (unmasked) -- only the masked 2.0 probes false.

## fn highest_available_candidate_ignoring_use_picks_the_use_unsatisfiable_ebuild
- Portage `mydbapi_match_pkgs(atom.without_use)[-1]`: asking for `dev-libs/unsatusealt[unsatuseorflag]` must still find unspecified-use candidate `unsatusealt-1.0` (the `[...]` block stripped) -- the seed for both the mask/force probe and the violated-flag list. The hard dep would find NO candidate (`doesnotexist-unsatuseor`).

## fn highest_available_candidate_ignoring_use_honours_extra_constraints
- F5: Portage's `mydbapi` in `dep_zapdeps` is `_dep_check_composite_db`, which honours the `'backtrack` loop's accumulated `runtime_pkg_mask` -- so once backtracking excludes the highest version of a `cat/pkg`, the bug-515584 probe must be seeded from the highest *unmasked* version instead. `dev-libs/slotusetarget` has two visible versions (1.0, 2.0); a negative constraint excluding 2.0 must fall back to 1.0.
