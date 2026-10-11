# crates/portuale/src/ebuild_phases/tests/sandbox.rs
Sandbox/isolation unit tests for phase execution. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn sandbox_wrapped_command_configures_the_real_addrconfig_workaround_addresses
- Ports Portage's `_configure_loopback_interface` (lib/portage/process.py:838-871), the workaround for bug #690758: `ip link set lo up` plus a non-loopback-looking address per family, so glibc's `getaddrinfo()` with `AI_ADDRCONFIG` set doesn't return spuriously empty results for a build that only ever talks to `localhost`.

## fn network_sandbox_exempt_matches_portage_doebuild_spawns_own_formula
- Expected value from Portage's `doebuild` spawns formula: `PROPERTIES=live` only exempts the unpack phase; `PROPERTIES=test_network` only exempts the test phase; `RESTRICT=network-sandbox` exempts every phase.
- USE-conditional groups are already dropped by the time these strings arrive (`restrict_and_properties`'s job), so a raw conditional token string is simply never matched.

## fn feature_gates_use_the_resolved_features_string
- From #37 S3: the phase-execution gates read the resolved `FEATURES` threaded on `extra_env` (`features_string`), never the raw process env -- a `make.conf` `-sandbox` disables the sandbox even when the calling env exports `sandbox`. The helpers take the features string as a parameter precisely so this is directly testable.
- The `extra_env` pair wins over the process env (here: whatever the test runner exported); an absent pair falls back to it.
