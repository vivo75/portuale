# crates/portage-repo/src/tests/tests_p572.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): #572 (from #567's S0): an installed new-style virtual's `||` must be re-resolved in the consumer's graph, so a co-scheduled upgrade that replaces the arm it bound to makes the walk fall through to the next arm.  Expected values are Portage 3.0.83's own resolver on the same shape (`portage/tests/resolver/ResolverPlayground.py`; captures and the mechanism write-up in `LLM/evidence/2026-10-28-572/`): `p572enc` is merged as a child of the kept `[nomerge] virtual/p572virt-2.0`, because the scheduled `p572prov-5.44` replaces the installed `5.42` in slot 0 and `dep_zapdeps`'s downgrade guard (bug #531656) drops the `=p572prov-5.42*` arm.

## fn nodeps_merges_the_arguments_but_not_the_fall_through_arm
- Backlog numbers removed from the comment (original text): Backlog #572 review focus: `--nodeps` follows no dependencies, so the fall-through arm must not merge even though the upgrade is co-scheduled. Oracle: hermetic playground `probe572_nodeps_onlydeps.py` (`nodeps MERGELIST ['dev-lang/perl-5.44.0', 'dev-perl/Encode-Locale-1.5']`).

## fn onlydeps_does_not_merge_the_fall_through_arm
- Backlog numbers removed from the comment (original text): Backlog #572 review focus: `--onlydeps` merges build-time deps only, so the runtime fall-through arm must not merge. Oracle: hermetic playground `probe572_nodeps_onlydeps.py` (`onlydeps MERGELIST []`).

## fn or_group_of_an_installed_virtual_falls_through_when_an_upgrade_replaces_its_arm
- Backlog numbers removed from the comment (original text): The `#567` p02 shape: no `--update`, no `--deep`. The perl-shape upgrade is an argument; the virtual is installed and kept; its `||` must fall through.  Green since #572 Task 2 (was RED on `main`: the `||` never reached the graph). (`LLM/evidence/2026-10-28-572/REPORT.md`).

## fn installed_virtual_without_tree_candidate_stays_silent
- Backlog numbers removed from the comment (original text): Tripwire (Task 2 Step 1): the installed `virtual/libc-1.0` shape stays silent — no `U`, no `N fakeglibc` row. Same oracle as `tests::graph::update_without_deep_keeps_an_installed_virtual_provider_silent`; the #572 expansion must never walk a virtual with no tree ebuild.

## fn q19_sibling_virtual_skip_rows_stay
- Backlog numbers removed from the comment (original text): Tripwire (Task 2 Step 1): the Q19 sibling-virtual skip rows stay as `tests_consumer_or_view` pins them. Same oracle (`q19-merge-portage.txt`); the #572 expansion must not erase the pins by queueing the chosen arm as a hard dep.

## fn or_group_of_a_nested_installed_virtual_falls_through_to_the_next_arm
- Backlog numbers removed from the comment (original text): The `#567` p03 shape (nested virtual): Try-Tiny's `||` names the installed virtual first, but the virtual's own `||` is doubly dead once the co-scheduled upgrade replaces its match, so the choice must fall through to `dev-libs/p572sub`.  Portage playground oracle (`LLM/evidence/2026-10-28-572/probe572_p03.py`): mergelist `['dev-perl/Sub-Name-0.280.0-r1', 'dev-lang/perl-5.44.0', 'dev-perl/Try-Tiny-0.320.0']` — the fall-through leaf plus the perl upgrade plus the requested leaf, the virtual kept (nomerge).  Green since #643 Task 2 (was RED on `main`: the `||` never reached the graph). (`LLM/evidence/2026-11-02-643/REPORT.md`).

## fn no_upgrade_nested_virtual_keeps_the_consumer_bound
- Backlog numbers removed from the comment (original text): The `#646` no-upgrade tie: the same nested-virtual shape as p03 above, but requested WITHOUT the co-scheduled upgrade — both legs of the virtual's `||` stay installed-satisfied, the consumer keeps binding the installed virtual, and nothing new merges.  Portage playground oracle (`LLM/evidence/2026-11-03-646/probe646_noupgrade.py`: p03 ebuilds/installed/world blocks verbatim, bare-consumer request `["dev-perl/Try-Tiny"]`, no options): `SUCCESS True`, `MERGELIST ['dev-perl/Try-Tiny-0.320.0']` — the one uninstalled requested package; the virtual is kept (nomerge) and `Sub-Name` is absent. The Rust request (`dev-libs/p572try` alone) is the same shape as the oracle request.

## fn deep_update_still_lists_the_fall_through_arm
- Backlog numbers removed from the comment (original text): The same graph under `-uD @world`: already green before #572, and it must stay green — the deep walk reaches the virtual by a different route. Asserts the fall-through arm merges exactly once (review focus: no duplicated child).
