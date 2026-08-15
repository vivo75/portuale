//! Incremental variables (USE, FEATURES, …) and how their tokens stack.

use crate::*;

/// The `const.INCREMENTALS` variables [`Config::incremental_sources`]
/// tracks for `emerge --info` and the build-phase env.
// Portage: const.py, INCREMENTALS
pub(crate) const TRACKED_INCREMENTALS: [&str; 5] = [
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "ENV_UNSET",
    "FEATURES",
    "PROFILE_ONLY_VARIABLES",
];

/// Record that a layer above env.d assigned `key` (see
/// [`Config::envd_higher_keys`]).
pub(crate) fn note_higher(config: &mut Config, key: &str) {
    config.envd_higher_keys.insert(key.to_string());
}

/// Record one `key=value` assignment's tokens when `key` is tracked.
pub(crate) fn note_incremental(config: &mut Config, key: &str, value: &str) {
    if TRACKED_INCREMENTALS.contains(&key) {
        config
            .incremental_sources
            .entry(key.to_string())
            .or_default()
            .push(value.split_whitespace().map(String::from).collect());
    }
}

/// Brackets one file's processing so its assignments collapse to the
/// file's final one (sourced files re-assign; the last wins).
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

/// Fold stacked token layers with Portage's incremental semantics
/// (`-*` clears, `-tok` discards, bare/`+`-prefixed adds), then sort.
/// Shared by [`Config::resolved_incremental`] and the `Packages`-index
/// header's child re-fold.
// Portage: config.py, regenerate
pub fn fold_token_layers(layers: &[&[String]]) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for tokens in layers {
        for tok in *tokens {
            if tok == "-*" {
                set.clear();
            } else if let Some(rest) = tok.strip_prefix('-') {
                set.remove(rest);
            } else {
                set.insert(tok.strip_prefix('+').unwrap_or(tok).to_string());
            }
        }
    }
    set.into_iter().collect()
}

/// Incremental-variable token semantics: `-*` clears, `-flag` removes,
/// `flag`/`+flag` adds. Shared by config files and per-package
/// `package.use` application.
// Portage: config.py, regenerate
pub fn apply_incremental(tokens: &str, set: &mut HashSet<String>) {
    for tok in tokens.split_whitespace() {
        apply_incremental_token(tok, set);
    }
}

/// Like [`apply_incremental`], but over an already-split token slice
/// (avoids a join + re-split round-trip on the hot path).
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

/// The USE-loop token semantics: like [`apply_incremental_token`], plus
/// the `-<prefix>_*` negative wildcard removing every flag starting with
/// `<prefix>_`. Only USE has this; other incrementals do not.
// Portage: config.py, USE loop
pub fn apply_use_incremental(tokens: &str, set: &mut HashSet<String>) {
    for tok in tokens.split_whitespace() {
        apply_use_incremental_token(tok, set);
    }
}

pub(crate) fn apply_use_incremental_token(tok: &str, set: &mut HashSet<String>) {
    // `-<prefix>_*`: Portage checks `x[-2:] == "_*"` with the trailing
    // underscore part of the prefix.
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
