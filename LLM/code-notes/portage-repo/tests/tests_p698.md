# crates/portage-repo/src/tests/tests_p698.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): #698: an upgrade is withheld only when an installed pin holder has no route to the new version; otherwise the holders join the plan, in every solver.  Expected behaviour comes from the charter (property 1: every kept package keeps its dependencies; the system stays as up to date as it can), not from Portage, which gives up and keeps the old version (`LLM/portage-quirks.md`).

## fn p698_root
- Backlog numbers removed from the comment (original text): The shared fixture vdb plus the installed #698 holders, all recording the pins the provider's old sub-slot left in them.
- Backlog numbers removed from the comment (original text): #733: `p733top` has `PDEPEND=p733dep`, and `p733dep` needs `p733top` back.

## fn p713_co_upgraded_holders_name_the_provider_as_parent
- Backlog numbers removed from the comment (original text): #713, charter property 5: a holder that joins the plan only because the provider moves names that provider as its parent, so the plan says why it is there.

## fn p718_pubgrub_holders_name_the_provider_as_parent
- Backlog numbers removed from the comment (original text): #718 (a): the pubgrub bridge names the provider as the parent of its holders too.

## fn p713_slot_operator_rebuilds_name_the_provider_as_parent
- Backlog numbers removed from the comment (original text): #713, charter property 5: a consumer rebuilt only because the provider's sub-slot changed names that provider as its parent (the walk queues such rebuilds as parentless roots).

## fn a_holder_without_a_route_still_withholds_the_upgrade
- Backlog numbers removed from the comment (original text): The bridge prints no skipped-update block (#431 S1 cut).

## fn p727_resolve
- Backlog numbers removed from the comment (original text): The #698 holders, the not-installed `p727top` (its `||` group lists the new provider first) and a request for it, with the given `--update` setting.

## fn p727_a_plain_request_leaves_the_provider_where_it_is
- Backlog numbers removed from the comment (original text): #727, owner decision 2026-11-12 (smallest plan): a request that names neither the provider nor `--update` leaves the provider where it is, in every solver; the co-upgrade applies when the provider is requested or the request updates deeply.

## fn p730_a_kept_provider_picks_the_virtual_that_accepts_it
- Backlog numbers removed from the comment (original text): #730, charter property 1: `p727virt-2` accepts only the new provider through an `||` arm whose package has no ebuild (like `virtual/perl-Storable`); with the provider kept at 5.42 every solver plans `-1`, the virtual that accepts it.

## fn p733_a_kept_installed_package_carries_no_edges_without_deep
- Backlog numbers removed from the comment (original text): #733: a kept installed package is a leaf without `--deep`, so its entry carries no dependency edges; the `--tree` would otherwise nest the requested `p733top` under the hidden `p733dep` that needs it back.

## fn p732_every_merge_bound_pubgrub_entry_carries_its_dep_input
- Backlog numbers removed from the comment (original text): #716/#732: every merge-bound pubgrub entry, the slot-operator rebuilds of the holders included, carries the dependency data `--json` reports.

## fn p728_a_rebuilt_package_moves_to_its_best_visible_version
- Backlog numbers removed from the comment (original text): #728, charter property 3: a package rebuilt for the provider's new sub-slot moves to its best visible version, in every solver. `p728virt` pins `p728core` with `~`, so both move together and the plan stays consistent.

## fn p715_root
- Backlog numbers removed from the comment (original text): The shared fixture vdb plus the installed #715 provider (sub-slot `0/1`) and two holders built against it (`p715prov:0/1=`).

## fn p715_pubgrub_rebuild_rows_follow_the_provider
- Backlog numbers removed from the comment (original text): #715 (charter property 2): holders rebuilt for a new provider sub-slot come after the provider, even when the provider waits for a dependency of its own.

## fn expected_no_route
- Backlog numbers removed from the comment (original text): The note #703 records: the provider slot, both versions and each route-less holder with the recorded atom that pins the old version. `p698rebuild` has a route, so it is not listed.

## fn p703_route_less_holder_is_recorded
- Backlog numbers removed from the comment (original text): #703, charter property 5 (messages help): the withheld upgrade names the one holder that cannot follow it (default solver).

## fn p774_a_masked_only_version_is_not_a_route
- Backlog numbers removed from the comment (original text): #774, charter properties 1 and 5: the only version of a holder that accepts the new provider (`p774mask-2`) is keyword-masked, so the holder has no route the user can install. The provider stays where it is and the holder is named, in every solver. Source: charter, not Portage.
