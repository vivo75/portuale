# crates/portage-util/tests/shuffle_seed_freeze.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): #113: `shuffle_seed` reads `PORTUALE_SHUFFLE_DIRS` once and freezes it for the life of the process.  This file is its own test binary, so it owns the variable from the first instruction -- the one state the in-process unit tests cannot reach once the global freeze exists (any earlier directory read pins the value, and cargo runs a binary's tests in parallel). It also keeps the seeded half of the old `read_dir_entries` unit test: with the variable set, the shuffle still executes and reproduces per (seed, directory).
