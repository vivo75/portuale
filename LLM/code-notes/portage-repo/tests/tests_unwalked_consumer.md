# crates/portage-repo/src/tests/tests_unwalked_consumer.rs
Complete mode and the installed consumers the walk never recursed into (#603).

## module
- Oracle: host Portage 3.0.83 with the same argv on a scratch copy of the fixtures, captures in `LLM/evidence/2026-11-06-sweep/603/` (`plain`, `matching`).
- `resolve_with_world` feeds the world file as complete-graph seeds and turns `selective` off, as the CLI does for a plain `emerge -p <atom>`; with `selective` on the installed `w603lib-3` would stay `AlreadyInstalled`.
- `unwalked_consumer_atom_matching_nothing_installed_does_not_pin` fails on the pre-#603 tree (checked by stashing the `rev_dep.rs` change).
