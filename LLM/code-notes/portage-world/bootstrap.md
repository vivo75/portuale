# crates/portage-world/src/bootstrap.rs
The charter 2a bootstrap of the fast plan: choice and row placement.

## fn select (#760, step 0 of plan 02.759-764 S2)
- Moved out of `portuale`'s `plan_world.rs` (`select_bootstrap`/`apply_bootstrap`) so the binary keeps presentation only. `analyse` stores the result in `World::bootstrap`.
- The flag eligibility is the resolver's own: `portage_repo::bootstrap_flag_eligible` (flag on, not forced by the profile, removal switches on no build-time `!flag?` arm, first pass also keeps `REQUIRED_USE`), driven by `portage_use_reduce::bootstrap_reduction_strict_first`, the same two-pass call the resolver makes. Before #759 the fast plan passed `|_, _| true` and could pick another flag than the resolver; `the_fast_plan_skips_a_flag_whose_removal_adds_a_build_dependency` pins the agreement.
- A boot slot that is not in `World::planned_use` (unchanged, no declared USE to reduce from) is ineligible; it used to be refused after the choice, which ended the whole bootstrap.

## fn World::build_plan
- Row placement: the boot row takes the earliest of the boot slot's own position and its dependents', the definitive row follows the last dependent. A dependent is a row that reaches the boot slot over `World::edges` minus the boot slot's flag-guarded edges (`portage_use_reduce::reaches`, shared with `boot_closure_reaches_member`). The old code dropped every flag-guarded edge of any slot, not only the boot's; with guards only for cycle edges that reading is gone.
