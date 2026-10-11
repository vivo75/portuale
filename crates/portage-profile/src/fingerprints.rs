//! Fingerprints of the config parts that resolver caches depend on.

use crate::*;

/// Content fingerprint of every USE-context `Config` field the
/// `portage-repo` memo keys read. `autounmask_use` is excluded: the
/// `'backtrack` loop mutates it, and callers hash it against this base.
///
/// Computed once by `resolve_config` into [`Config::use_context_base`];
/// `None` on a hand-built `Config` means "fall back to the full hash".
pub fn use_context_base_fingerprint(config: &Config) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();

    // Order-independent digest of a `HashSet<String>` (iteration order is
    // not stable): count + xor-fold of per-element hashes.
    let hash_set = |s: &HashSet<String>, h: &mut std::collections::hash_map::DefaultHasher| {
        s.len().hash(h);
        let mut acc: u64 = 0;
        for v in s {
            let mut e = std::collections::hash_map::DefaultHasher::new();
            v.hash(&mut e);
            acc ^= e.finish();
        }
        acc.hash(h);
    };

    hash_set(&config.accept_keywords, &mut h);
    hash_set(&config.use_force, &mut h);
    hash_set(&config.use_mask, &mut h);
    hash_set(&config.use_stable_force, &mut h);
    hash_set(&config.use_stable_mask, &mut h);

    config.use_tokens.hash(&mut h);
    config.conf_use_tokens.hash(&mut h);
    config.env_use_tokens.hash(&mut h);
    config.envd_use_tokens.hash(&mut h);
    config.features_use.hash(&mut h);

    config.package_use.hash(&mut h);
    config.package_use_user.hash(&mut h);
    config.package_use_repo.hash(&mut h);
    config.package_env_use.hash(&mut h);
    config.package_use_force.hash(&mut h);
    config.package_use_mask.hash(&mut h);
    config.package_use_stable_force.hash(&mut h);
    config.package_use_stable_mask.hash(&mut h);
    // The per-level view drives `effective_use_flags`; two configs could
    // share a flat set yet differ in the interleaving, so hash it in full.
    config.use_mask_force_levels.len().hash(&mut h);
    for lvl in &config.use_mask_force_levels {
        lvl.use_mask.hash(&mut h);
        lvl.use_force.hash(&mut h);
        lvl.use_stable_mask.hash(&mut h);
        lvl.use_stable_force.hash(&mut h);
        lvl.package_use_mask.hash(&mut h);
        lvl.package_use_force.hash(&mut h);
        lvl.package_use_stable_mask.hash(&mut h);
        lvl.package_use_stable_force.hash(&mut h);
    }
    config.package_accept_keywords.hash(&mut h);
    config.repo_make_defaults_use.hash(&mut h);

    // `profile_use_layers`: every layer in full, each inner list in full.
    config.profile_use_layers.len().hash(&mut h);
    for l in &config.profile_use_layers {
        l.make_defaults_use.hash(&mut h);
        l.package_use.hash(&mut h);
    }

    h.finish()
}

/// Content fingerprint of every `is_visible`-relevant `Config` field the
/// USE-context base does not cover. Computed once by `resolve_config`
/// into [`Config::is_visible_base`]; same `None`-means-fallback contract.
pub fn is_visible_base_fingerprint(config: &Config) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    use_context_base_fingerprint(config).hash(&mut h);
    config.package_mask.hash(&mut h);
    config.package_unmask.hash(&mut h);
    config.accept_license.hash(&mut h);
    config.package_license.hash(&mut h);
    // `license_groups` is a `HashMap` (iteration order not stable):
    // hash its keys sorted, values in their own order.
    {
        let mut keys: Vec<&String> = config.license_groups.keys().collect();
        keys.sort();
        keys.len().hash(&mut h);
        for k in keys {
            k.hash(&mut h);
            config.license_groups[k].hash(&mut h);
        }
    }
    config.package_accept_keywords.hash(&mut h);
    config.accept_properties.hash(&mut h);
    config.package_properties.hash(&mut h);
    config.accept_restrict.hash(&mut h);
    config.package_accept_restrict.hash(&mut h);
    h.finish()
}
