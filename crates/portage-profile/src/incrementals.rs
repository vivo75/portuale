//! Incremental variables (USE, FEATURES, …) and how their tokens stack.

use crate::*;

/// The `const.INCREMENTALS` variables [`Config::incremental_sources`]
/// tracks for `emerge --info` and the build-phase env (the rest of real's
/// tuple is either USE-related -- handled separately -- or
/// `environ_filter`ed). `PROFILE_ONLY_VARIABLES` is exported folded and
/// sorted (`const.py:132`): the profile's `"${PROFILE_ONLY_VARIABLES}
/// ARCH …"` stacking must not survive as a literal concatenation.
pub(crate) const TRACKED_INCREMENTALS: [&str; 5] = [
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "ENV_UNSET",
    "FEATURES",
    "PROFILE_ONLY_VARIABLES",
];

/// Records one `key=value` assignment against [`Config::incremental_sources`]
/// when `key` is a [`TRACKED_INCREMENTALS`] variable. `value` is the
/// already-`${VAR}`-substituted string (matching real, whose per-db
/// value is what `getconfig` expanded it to).
/// Records that a layer above env.d assigned `key` (see
/// [`Config::envd_higher_keys`]). Every key, not only the ones the first
/// env.d read held: a `profile.env` rewritten mid-run can introduce a key
/// `make.conf` or the profile already sets, and that higher value must
/// still win after `reload_env_d`.
pub(crate) fn note_higher(config: &mut Config, key: &str) {
    config.envd_higher_keys.insert(key.to_string());
}

pub(crate) fn note_incremental(config: &mut Config, key: &str, value: &str) {
    if TRACKED_INCREMENTALS.contains(&key) {
        config
            .incremental_sources
            .entry(key.to_string())
            .or_default()
            .push(value.split_whitespace().map(String::from).collect());
    }
}

/// Real config files are *sourced*, so an incremental variable's own
/// layer for one file is its **final** assignment's value -- not every
/// assignment stacked. Confirmed against real 3.0.82.2: a second
/// `FEATURES=` line later in `make.conf` replaces the first (bash
/// semantics; a second assignment is not an incremental addition), and
/// `emerge --info` drops the first line's tokens entirely. Portuale
/// records one source per assignment, so each file's processing is
/// bracketed by [`IncrementalSnapshot::take`] / `restore`, which
/// collapses that file's assignments to the last one.
pub(crate) struct IncrementalSnapshot(pub(crate) Vec<(String, usize)>);

impl IncrementalSnapshot {
    pub(crate) fn take(config: &Config) -> Self {
        Self(
            TRACKED_INCREMENTALS
                .iter()
                .map(|key| {
                    (
                        (*key).to_string(),
                        config
                            .incremental_sources
                            .get(*key)
                            .map(Vec::len)
                            .unwrap_or(0),
                    )
                })
                .collect(),
        )
    }

    /// Collapse everything appended since `take` to the file's final
    /// assignment (no-op when the file assigned nothing).
    pub(crate) fn restore(self, config: &mut Config) {
        for (key, depth) in self.0 {
            let Some(sources) = config.incremental_sources.get_mut(&key) else {
                continue;
            };
            if sources.len() > depth {
                let last = sources.last().cloned().unwrap_or_default();
                sources.truncate(depth);
                sources.push(last);
            }
        }
    }
}

/// Applies real incremental-variable token semantics: `-*` clears
/// everything accumulated so far, `-flag` removes, `flag`/`+flag` adds
/// (a leading `+` is invalid per PMS but real config.py tolerates it by
/// stripping it, which this mirrors). Public so `portage-repo` can reuse
/// it to apply `package.use` tokens on top of a per-package clone of the
/// base USE set -- see the module doc comment.
pub fn apply_incremental(tokens: &str, set: &mut HashSet<String>) {
    for tok in tokens.split_whitespace() {
        apply_incremental_token(tok, set);
    }
}

/// Like [`apply_incremental`], but over an already-split token slice --
/// skips the `Vec::join(" ")` + re-`split_whitespace()` round-trip that
/// `effective_use_flags` / `specificity_ordered_flags` would otherwise do
/// for every matching `package.use` entry per candidate (a hot path on a
/// deep `emerge -pu`).
pub fn apply_incremental_iter<S: AsRef<str>>(tokens: &[S], set: &mut HashSet<String>) {
    for tok in tokens {
        for t in tok.as_ref().split_whitespace() {
            apply_incremental_token(t, set);
        }
    }
}

pub(crate) fn apply_incremental_token(tok: &str, set: &mut HashSet<String>) {
    if tok == "-*" {
        set.clear();
    } else if let Some(rest) = tok.strip_prefix('-') {
        set.remove(rest);
    } else if let Some(rest) = tok.strip_prefix('+') {
        if !rest.is_empty() {
            set.insert(rest.to_string());
        }
    } else {
        set.insert(tok.to_string());
    }
}

/// Real `config.py:2888-2945`'s **USE loop** token semantics: exactly
/// [`apply_incremental_token`], plus the negative prefix wildcard
/// `-<prefix>_*`, which removes every flag accumulated so far whose name
/// starts with `<prefix>_`. Real's per-token order is wildcard removal
/// first, then `discard(token[1..])` -- for `-foo_*` that second step is
/// a no-op on a flag literally named `foo_*` (which is why
/// `apply_incremental`'s exact-removal arm could never express this).
///
/// The wildcard arm lives **inside real's USE loop**, not in the generic
/// incremental loop: non-USE incrementals (`FEATURES`, `CONFIG_PROTECT`,
/// `ACCEPT_KEYWORDS`, the name lists, …) keep using
/// [`apply_incremental`] and have no `_*` semantics. The positive
/// `foo_*` form is deliberately out of scope here too: it expands
/// against a package's `IUSE`, which only the IUSE-aware
/// `effective_use_flags` path has.
pub fn apply_use_incremental(tokens: &str, set: &mut HashSet<String>) {
    for tok in tokens.split_whitespace() {
        apply_use_incremental_token(tok, set);
    }
}

/// Like [`apply_use_incremental`], but over an already-split token slice
/// (the same `Vec::join(" ")` round-trip avoidance as
/// [`apply_incremental_iter`]).
pub fn apply_use_incremental_iter<S: AsRef<str>>(tokens: &[S], set: &mut HashSet<String>) {
    for tok in tokens {
        for t in tok.as_ref().split_whitespace() {
            apply_use_incremental_token(t, set);
        }
    }
}

pub(crate) fn apply_use_incremental_token(tok: &str, set: &mut HashSet<String>) {
    // `-<prefix>_*`: real `x[-2:] == "_*"`, `prefix = x[1:-1]` (the
    // trailing underscore is part of the prefix).
    if let Some(rest) = tok.strip_prefix('-')
        && let Some(stem) = rest.strip_suffix("_*")
    {
        let prefix = format!("{stem}_");
        set.retain(|flag| !flag.starts_with(&prefix));
        set.remove(rest);
        return;
    }
    apply_incremental_token(tok, set);
}
