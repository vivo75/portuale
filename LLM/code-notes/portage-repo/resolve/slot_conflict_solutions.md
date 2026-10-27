# crates/portage-repo/src/resolve/slot_conflict_solutions.rs
The slot-conflict handler's search for USE changes that would solve a slot conflict (plan 02.589 R4, backlog #597).

## fn solve_slot_conflicts
- Port of `slot_conflict_handler.__init__` (`lib/_emerge/resolver/slot_collision.py:81-182`, Portage 3.0.83): configurations from `_configuration_generator` (SC:1191-1237; installed packages last in each conflict, last conflict varies fastest), `_check_configuration` (SC:786-982), `_solution_candidate_generator` (SC:1240-1301), `_check_solution` (SC:1012-1188), `_force_flag_for_package` (SC:984-1010), `_get_change` / `_add_change` / `_is_subset` (SC:187-240).
- The all-ebuild configuration is checked first and ends the search when it has a solution. With more than four slot conflicts (`len(conflict_pkgs) > 4`, one entry per slot conflict, not per package) only that first configuration is ever checked (SC:170-179).
- At most 1024 flag settings are tried per configuration (SC:79, SC:954-975).
- Parent order inside a conflict is first-seen order. Portage iterates a Python set, so flag order inside a change can differ between Portage runs; the captured cases have one flag per package.
- Quirk Q21 (`LLM/portage-quirks.md`): `_force_flag_for_package` records "disabled" as the empty string on a flag the package already has off, and `_check_solution` then rejects the candidate (SC:1115-1122). Copied in `force_flag`; test `turning_off_a_flag_that_is_already_off_invalidates_the_candidate`.
- Quirk Q22: the explanation header counts `solutions`, the body prints the minimised `changes`. The counts are kept apart in `SlotConflictSolutions`.
- Not copied: the empty-USE trap of `findAtomForPackage` (`lib/portage/_sets/base.py:122-124` with `Package.py:746-754`): a tree package whose new USE is empty is recomputed from the configuration in Portage, so its candidate fails the match check. It only shows on EAPI-8 playground ebuilds without ELIBC flags; the fixtures keep `elibc_glibc` in USE. See `LLM/evidence/2026-10-26-575-residues/r4/captures/pg-k-eapi8-debug.txt`.
- `changes == [{}]` (every solution already the current USE, `_get_change` SC:226-239) is a non-empty list in Portage and stops the search with an empty body. The port keeps that shape (an empty change list inside `changes`). No contract case reaches it.

## fn violated_conditionals
- `_use_dep.violated_conditionals` (`lib/portage/dep/__init__.py:1342-1466`) over portage-dep's `UseDep` list. `required` is the whole atom's flag list, not only the violated flags, as in Portage. `None` stands for Portage's atom with no `.use` (all tokens satisfied).

## fn add_change
- `_add_change`: an existing change that is a subset of the new one makes the new one redundant; existing supersets are dropped (SC:205-224).
