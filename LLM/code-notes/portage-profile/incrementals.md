# crates/portage-profile/src/incrementals.rs
Incremental-variable token semantics (USE, FEATURES, and the tracked set behind `emerge --info`).

## const TRACKED_INCREMENTALS
- The rest of Portage's `const.INCREMENTALS` tuple is either USE-related (handled separately) or `environ_filter`ed. `PROFILE_ONLY_VARIABLES` is exported folded and sorted (`const.py:132`): the profile's `"${PROFILE_ONLY_VARIABLES} ARCH …"` stacking must not survive as a literal concatenation.

## fn note_higher
- Records that a layer above env.d assigned `key` (see `Config::envd_higher_keys`). Every key, not only the ones the first env.d read held: a `profile.env` rewritten mid-run can introduce a key `make.conf` or the profile already sets, and that higher value must still win after `reload_env_d`.
- `value` in `note_incremental` is the already-`${VAR}`-substituted string (matching Portage, whose per-db value is what `getconfig` expanded it to).

## struct IncrementalSnapshot
- Config files are *sourced*, so an incremental variable's own layer for one file is its **final** assignment's value, not every assignment stacked. Confirmed against Portage 3.0.82.2: a second `FEATURES=` line later in `make.conf` replaces the first (bash semantics; a second assignment is not an incremental addition), and `emerge --info` drops the first line's tokens entirely. Portuale records one source per assignment, so each file's processing is bracketed by `IncrementalSnapshot::take` / `restore`, which collapses that file's assignments to the last one.

## fn apply_incremental
- A leading `+` is invalid per PMS but Portage's config.py tolerates it by stripping it, which this mirrors. Public so `portage-repo` can reuse it to apply `package.use` tokens on top of a per-package clone of the base USE set.
- `apply_incremental_iter` skips the `Vec::join(" ")` + re-`split_whitespace()` round-trip that `effective_use_flags` / `specificity_ordered_flags` would otherwise do for every matching `package.use` entry per candidate (a hot path on a deep `emerge -pu`).

## fn apply_use_incremental
- Portage's `config.py:2888-2945` USE-loop order is wildcard removal first, then `discard(token[1..])`; for `-foo_*` that second step is a no-op on a flag literally named `foo_*` (which is why `apply_incremental`'s exact-removal arm could never express this).
- The wildcard arm lives **inside Portage's USE loop**, not in the generic incremental loop: non-USE incrementals (`FEATURES`, `CONFIG_PROTECT`, `ACCEPT_KEYWORDS`, the name lists, …) keep using `apply_incremental` and have no `_*` semantics. The positive `foo_*` form is deliberately out of scope here too: it expands against a package's `IUSE`, which only the IUSE-aware `effective_use_flags` path has.

## fn apply_use_incremental_token
- `-<prefix>_*`: Portage `x[-2:] == "_*"`, `prefix = x[1:-1]` (the trailing underscore is part of the prefix).
