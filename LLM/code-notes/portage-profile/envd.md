# crates/portage-profile/src/envd.rs
Reading `<eroot>/etc/profile.env` (env.d): the scalar layer, the tracked incremental layers, and the `USE=` tokens.

## struct EnvdParse
- Shared by the first load in `resolve_config` and by `Config::reload_env_d`, so the two can never drift apart in what "reading env.d" means.
- Parsing mirrors the first load's long-standing semantics (Portage's `_get_env_d`'s `getconfig(..., expand=False)` plus portuale's eager within-file `${VAR}` chaining): each line's optional leading `export ` is stripped, the value is `shlex_token`-processed by `parse_kv_line` and `${VAR}`-substituted against the earlier lines of this same file (plus the process-env fallback `substitute` already has), seeding every later line (Portage's `expand_map = env_d.copy()` seeding). A sourced `profile.env` re-assigning a key keeps the last assignment (bash semantics), as does each tracked incremental's recorded layer. The caller applies `_env_d_blacklist`.

## fn read_envd_use_tokens
- Portage's `configdict["env.d"]["USE"]`: Portage's `_get_env_d`'s `getconfig(..., expand=False)` (an optional leading `export ` keyword, then `KEY=value` with quote removal, no `${VAR}` expansion). Returned in file order for `effective_use_flags` to fold in via `apply_incremental`.
