# crates/portuale/src/remote/mod.rs
Remote binary-package merge over SSH (`mrg --remote-*`): option validation and shared context.

## module
- Slice 1 covers this module's surface: option validation, the multiplexed `ssh` transport, and the read-only preflight; no payload streaming yet.
- Binary-format scope rationale: `mrg` does not target full binary-format compatibility (`docs/remote-merge.md` §1 / §14).

## enum ConfigPlacement
- The vdb/edb placements of the plan table land in slice 6 with the vdb shadow; only etc-portage is placed here.

## static REMOTE_EXEC
- Same process-global-options precedent as portage-repo's own `USEOLDPKG_ATOMS`/`BINPKG_CHANGED_DEPS_OVERRIDE` statics (plain `RwLock`, `unwrap()` like theirs).

## struct RemoteContext (fields)
- `vdb`: the stateless-client shape lands in slice 6.
- `portuale_binary`: backlog #326 D5/Q3.
- `portuale_dir`: backlog #326 Q1.
- `bin_plan`: backlog #326 S8.1 gates; local transport without the override keeps `current_exe()` and installs nothing (#326 D5).
- `config_protect`: the resolve path derives it from the placed config unless explicitly flagged (slice 6).

## enum ClientBinPlan
- Backlog #326 S8.1 gates. `Local` is the #326 D5 shape: no preflight checks, no install.

## fn provided
- The `REMOTE_OPTION_IDS` presence check must see both `Value` string options and `Flag` bools (which `get_one::<String>` would panic on).

## fn check_remote (edb)
- Informational (plan §7/§10): the client has no binhosts. Records intent for a future slice that threads it through.
