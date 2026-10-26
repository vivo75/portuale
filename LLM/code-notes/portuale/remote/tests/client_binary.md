# crates/portuale/src/remote/tests/client_binary.rs
Client-binary gate unit tests and install/reuse end-to-end tests.

## tests: bin_search_dirs_prefers_the_override_pair
- D5 order: per directory, `portuale` then `portuale-<hash>`.

## tests: bin_gates_use_the_first_match_in_d5_order
- Both a plain and a hashed candidate match: the first in D5 order wins.

## tests: bin_gates_fail_without_a_writable_dir
- The no-override message names the option and the root-login alternative (Q1).

## tests: s8_install_then_reuse_over_local_transport
- #326 S8 e2e.

## tests: s8_matching_plain_portuale_is_reused
- #326 S8 e2e (Q5/D5).

## tests: s8_mismatched_plain_portuale_is_left_alone
- #326 S8 e2e (Q5/D5).

## tests: s8_corrupted_hashed_binary_is_replaced
- #326 S8 e2e (D5).

## tests: s8_pretend_installs_nothing
- #326 S8 e2e (S8.2). Pretend stays local (`mrg` routes it to `pretend::run` with the remote options dropped). Setup mirrors `pretend.rs`'s local-transport remote test: a `file://` binhost plus the fixture client config.

## fn s8_pretend_installs_nothing (#511)
- Unsigned test binhost: opt out of the verifying default, so the `.tbz2` entry is served.
