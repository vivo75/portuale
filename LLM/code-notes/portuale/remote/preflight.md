# crates/portuale/src/remote/preflight.rs
Read-only preflight gates checked before a remote merge, plus the bundle unpack driver.

## fn preflight_script
- `bin` is the `(dirs, candidates)` pair from the client-binary search (slice 8.1, backlog #326 D5): per candidate the full digest or `missing`, plus `uname -m` and each directory's existence, writability and octal mode. `None` skips the binary section (local transport without the override installs nothing, so there is nothing to report).

## fn preflight_gates
- A missing vdb only warns: the slice-1 run owns no payload, and the placement matrix plus the stateless degrade land in slice 5 (see docs/remote-merge.md §7).

## fn run_preflight_inner
- The S8.1 gates decide on the binary facts (`None` when the run skips those gates by design).

## fn unpack_script
- The `build-info/CONTENTS` gate is deliberately absent here (fixture binpkgs lack it; Portage-built ones carry it): slice 4's merge owns that check, where a missing `CONTENTS` is genuinely unmergeable.
