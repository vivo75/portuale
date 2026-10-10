# crates/portage-vdb/tests/no_hand_built_vdb_paths.rs
Lint test: production code must not build installed-database paths by hand.

## fn strip_comments_and_strings
- Strips the same rough way `LLM/evidence/305-s0-inventory.py` does.
