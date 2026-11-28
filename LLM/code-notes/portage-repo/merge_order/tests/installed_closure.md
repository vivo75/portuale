# crates/portage-repo/src/merge_order/tests/installed_closure.rs
Context moved out of the code comments.

## fn usable_installed_by_cp_drops_a_package_whose_slot_operator_binding_the_plan_breaks
- Backlog numbers removed from the comment (original text): #680: the installed XS module is bound to perl:0/1.0; the plan merges perl at sub-slot 2.0, so the installed module cannot stand in for its rebuild. The unbound tool stays usable.
