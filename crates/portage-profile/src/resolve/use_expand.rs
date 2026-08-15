//! USE_EXPAND, USE_EXPAND_UNPREFIXED and IUSE_EFFECTIVE from the folded scalars.

use crate::resolve::*;

/// USE_EXPAND, USE_EXPAND_UNPREFIXED and IUSE_EFFECTIVE from the folded scalars.
pub(crate) fn expand_use_vars(
    config: &mut Config,
    scalars: &mut HashMap<String, String>,
    profile_scalars: &HashMap<String, String>,
    level_scalar_deltas: &[HashMap<String, String>],
) {
    // USE_EXPAND (PMS 7.3.4): each named variable's last-wins value expands
    // into lowercase-prefixed pseudo-USE-flags with USE token semantics.
    // UNPREFIXED folds with no prefix; IMPLICIT/IUSE_IMPLICIT/VALUES feed
    // IUSE_EFFECTIVE below (IMPLICIT drives `is_valid_flag`; HIDDEN is
    // display-only). `_*` expansion lives in `effective_use_flags`;
    // `package.use` shorthand in `parse_package_use_lines`.
    // Env overrides fold into `scalars` here: names are known only now.
    // Portage: config.py regenerate()
    let env_expand_names: Vec<String> = config
        .use_expand
        .iter()
        .chain(config.use_expand_unprefixed.iter())
        .cloned()
        .collect();
    for var in env_expand_names {
        if let Some(value) = config_env_var(&var) {
            note_higher(&mut *config, &var);
            scalars.insert(var, value);
        }
    }

    // Per-level USE_EXPAND fold (Portage config.py regenerate): each level's
    // value becomes USE flags *prepended* before that level's USE=, so a
    // sub-profile or that level's package.use can still cancel them.
    {
        let expand_names: Vec<String> = {
            let mut v: Vec<String> = config.use_expand.iter().cloned().collect();
            v.sort();
            v
        };
        let unprefixed_names: Vec<String> = {
            let mut v: Vec<String> = config.use_expand_unprefixed.iter().cloned().collect();
            v.sort();
            v
        };
        let prefix_tok = |var: &str, tok: &str| -> String {
            let prefix = var.to_lowercase();
            if let Some(rest) = tok.strip_prefix('-') {
                format!("-{prefix}_{rest}")
            } else if let Some(rest) = tok.strip_prefix('+') {
                format!("{prefix}_{rest}")
            } else {
                format!("{prefix}_{tok}")
            }
        };
        // Each level's value stacks incrementally through USE_ORDER (a later
        // level never wipes an earlier one); only a make.conf/env override
        // of the var suppresses the per-level fold (Portage 2961-2978).
        let conf_overrode = |var: &str| profile_scalars.get(var) != scalars.get(var);
        for (i, delta) in level_scalar_deltas.iter().enumerate() {
            let mut expand_use: Vec<String> = Vec::new();
            for var in &unprefixed_names {
                if conf_overrode(var) {
                    continue;
                }
                if let Some(v) = delta.get(var) {
                    expand_use.extend(v.split_whitespace().map(String::from));
                }
            }
            for var in &expand_names {
                if conf_overrode(var) {
                    continue;
                }
                if let Some(v) = delta.get(var) {
                    expand_use.extend(v.split_whitespace().map(|t| prefix_tok(var, t)));
                }
            }
            if expand_use.is_empty() {
                continue;
            }
            let joined = expand_use.join(" ");
            apply_use_incremental(&joined, &mut config.use_flags);
            config.profile_use_layers[i]
                .make_defaults_use
                .insert(0, joined);
        }
    }

    let use_expand_vars: Vec<String> = config.use_expand.iter().cloned().collect();
    for var in use_expand_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        // Already folded per-level above (Portage skips the `defaults`
        // re-fold, 2964).
        if profile_scalars.get(&var) == scalars.get(&var) {
            continue;
        }
        let prefix = var.to_lowercase();
        let prefixed: String = value
            .split_whitespace()
            .map(|tok| {
                // "-flag" keeps its "-" outside the prefix; "+" is stripped,
                // matching ordinary USE token handling.
                if let Some(rest) = tok.strip_prefix('-') {
                    format!("-{prefix}_{rest}")
                } else if let Some(rest) = tok.strip_prefix('+') {
                    format!("{prefix}_{rest}")
                } else {
                    format!("{prefix}_{tok}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        apply_use_incremental(&prefixed, &mut config.use_flags);
        config.conf_use_tokens.push(prefixed);
    }

    // USE_EXPAND_UNPREFIXED: same mechanism with no prefix -- e.g. ARCH="amd64"
    // contributes the bare `amd64` flag. Portage: config.py
    let use_expand_unprefixed_vars: Vec<String> =
        config.use_expand_unprefixed.iter().cloned().collect();
    for var in use_expand_unprefixed_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        // Profile-only values folded per-level above (Portage 2863-2866).
        if profile_scalars.get(&var) == scalars.get(&var) {
            continue;
        }
        apply_use_incremental(value, &mut config.use_flags);
        config.conf_use_tokens.push(value.clone());
    }

    // EAPI 5+ IUSE_EFFECTIVE (Portage `config.py::_calc_iuse_effective`),
    // now that every input is stacked. Plain scalar reads of
    // `USE_EXPAND_VALUES_<v>` (outside even Portage's INCREMENTALS list).
    // See `Config::iuse_effective` for what this grants.
    config.iuse_effective = config.iuse_implicit.clone();
    for var in &config.use_expand_unprefixed {
        if !config.use_expand_implicit.contains(var) {
            continue;
        }
        if let Some(values) = scalars.get(&format!("USE_EXPAND_VALUES_{var}")) {
            config
                .iuse_effective
                .extend(values.split_whitespace().map(String::from));
        }
    }
    for var in &config.use_expand_implicit {
        if !config.use_expand.contains(var) {
            continue;
        }
        let prefix = var.to_lowercase();
        if let Some(values) = scalars.get(&format!("USE_EXPAND_VALUES_{var}")) {
            config
                .iuse_effective
                .extend(values.split_whitespace().map(|v| format!("{prefix}_{v}")));
        }
    }
}
