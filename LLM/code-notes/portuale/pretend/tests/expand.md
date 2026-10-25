# crates/portuale/src/pretend/tests/expand.rs
Short-name expansion against the allow-list.

## fn short_names_expand_only_within_listed_categories
- `expand_targets` over the `repo-p571`/`overlay-p571` fixtures: the overlay-listed `p571-shared` and the listed `p571-listed` qualify, the unlisted `p571-hidden` misses with no `!!!`, and a qualified unlisted atom passes expansion untouched (the allow-list bites at resolution). Mirrors the `probe-p571.sh` short-name cells.
