# crates/portage-repo/src/tests/tests_571.rs
Task 12 (#571): the category allow-list at every consumer.

## fn all_cp_lists_only_listed_categories
- A miniature main+overlay pair (real `profiles/categories` files): `all_cp` yields the listed and the overlay-listed cps, never the unlisted dir. Pins `portdbapi.cp_all(categories=settings.categories)`.

## fn gate_returns_empty_for_unlisted_categories
- `CategoryGate::repo_candidates` on the miniature pair: listed yields candidates; unlisted-with-ebuilds yields nothing and marks the once-per-cp `!!!` record (a second lookup stays empty without re-marking); unlisted-without-ebuilds yields nothing silently. The `warned` set stands in for the `!!!` line itself (stderr is not captured in unit tests).

## fn resolve_honours_the_allow_list
- End to end over the shared fixtures: the unlisted direct target fails resolve with the plain miss, the overlay-listed cp merges `New 1.0`, and the dependency on the unlisted cp resolves `Ok` with an `Aborted` outcome and a `NoVisibleCandidate` entry for it (the required-by chain names `p571-need-1.0::p571main`).
