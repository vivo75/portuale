# crates/portage-repo/src/tests/tests_571.rs
Task 12 (#571): the category allow-list at every consumer.

## fn all_cp_lists_only_listed_categories
- A miniature main+overlay pair (real `profiles/categories` files): `all_cp` yields the listed and the overlay-listed cps, never the unlisted dir. Pins `portdbapi.cp_all(categories=settings.categories)`.

## fn gate_returns_empty_for_unlisted_categories
- `CategoryGate::repo_candidates` on the miniature pair: listed yields candidates; unlisted-with-ebuilds yields nothing and marks the once-per-cp `!!!` record (a second lookup stays empty without re-marking); unlisted-without-ebuilds yields nothing silently. The `warned` set stands in for the `!!!` line itself (stderr is not captured in unit tests).

## fn unlisted_category_line_renders_config_root_with_one_trailing_slash
- The exact `!!!` text for `/`, `/tmp/cfg` and `/tmp/cfg/`: one trailing slash in every rendering (`/etc/portage/categories` for `/`). Portage: `LocationsManager.py:79-83`, `porttree.py:1216-1225`.

## fn two_walks_sharing_categories_warn_once
- Two gates over one `Categories`: the first unlisted lookup marks the record, the second finds it marked — one `!!!` across two walks. Portage: the frozen `xcache["cp-list"]` (`porttree.py:1138-1151`).

## fn cloned_request_shares_the_warning_record_across_walks
- Resolver-level half of the above: a second `ResolveCtx` built from a request carrying the first walk's `Rc<Categories>` shares the record, so both unlisted lookups leave one mark.

## fn resolve_honours_the_allow_list
- End to end over the shared fixtures: the unlisted direct target fails resolve with the plain miss, the overlay-listed cp merges `New 1.0`, and the dependency on the unlisted cp resolves `Ok` with an `Aborted` outcome and a `NoVisibleCandidate` entry for it (the required-by chain names `p571-need-1.0::p571main`).
