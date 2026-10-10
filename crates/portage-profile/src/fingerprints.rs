//! Fingerprints of the config parts that resolver caches depend on.

use crate::*;

/// Computes the real USE/ACCEPT_KEYWORDS/visibility `Config` for
/// `config_root`: the profile chain rooted at
/// `<config_root>/etc/portage/make.profile` (if it exists -- a missing
/// profile is not an error, it just contributes nothing), then
/// `<config_root>/etc/portage/make.conf` (if it exists) as the final,
/// highest-priority USE/ACCEPT_KEYWORDS layer, then
/// `package.mask`/`.unmask`/`.accept_keywords`.
///
/// `main_repo_location` (the main repo's own tree root, e.g. what
/// `portage_repo::find_repos` marks `is_main` -- see that crate) is
/// needed for `package.mask`/`.unmask`'s repo-level source,
/// `<main_repo_location>/profiles/package.mask` -- real portage's most
/// common real-world masking source (security/arch masks, etc.), stacked
/// together with every profile level's own `package.mask`/`.unmask` (in
/// chain order) and the user-level `/etc/portage` files, exactly
/// matching real `MaskManager.py`'s three-source stack (see
/// `stack_mask_lines`'s doc comment for the `-atom`-removal semantics).
///
/// `overlay_repos` (each overlay's own `(name, location)`, e.g. every
/// non-main entry from `portage_repo::find_repos` -- this crate can't
/// depend on `portage-repo` itself, hence the plain pair instead of
/// reusing `RepoConfig`) supplies each overlay's own repo-level
/// `package.mask`/`.unmask` too, real `MaskManager.py`'s own
/// `repositories.repos_with_profiles()` loop -- confirmed by reading it
/// directly: it iterates *every* configured repo unconditionally, not
/// just the main one. Each overlay's own lines are scoped with a
/// `::reponame` suffix first (`scope_repo_mask_lines`, real
/// `append_repo`'s own "atoms without an explicit repo part get one;
/// atoms that already have one are left alone" rule, applied to a
/// `-atom` removal line's own atom portion too, preserving the leading
/// `-`) before being folded into the same stack -- otherwise an
/// overlay's own mask entry would silently also mask a same-named
/// package in the main repo or another overlay, which real portage's
/// own scoping specifically prevents. Deliberately asymmetric,
/// confirmed while implementing this: the main repo's own entries above
/// stay unscoped, matching portuale's own pre-existing (unchanged)
/// behavior -- real portage scopes *every* repo's own repo-level
/// entries this same way, including the main repo's, so a `package.mask`
/// entry from main only masking main's own packages (not an
/// identically-named overlay package) is a separate, distinct
/// correctness question this slice doesn't also take on. Real
/// `masters` (each repo's own `package.mask` -- and *only*
/// `package.mask`, real `MaskManager.py`'s own `package.unmask` loop
/// never consults masters at all -- stacks with its declared masters'
/// own lines before repo-scoping) is now modeled to the extent every
/// fixture in portuale ever needs: a repo with no explicit
/// `masters =` (portuale doesn't parse that `repos.conf` key at all
/// yet) implicitly masters the main repo alone, real `config.py`'s own
/// `repo.masters = (self.mainRepo(),)` default -- every overlay here
/// gets exactly that, the main repo gets `()` (itself, since it can
/// never be its own master). An explicit `masters =` override, or a
/// multi-master chain, stays unimplemented (would need a `masters` key
/// threaded through `portage-repo::find_repos` first). `license_groups`
/// *is* read from every repo's own `profiles/` directory
/// (`<repo>/profiles/license_groups`, main repo then each overlay) --
/// real `LicenseManager._read_license_groups` over
/// `LocationsManager.profile_locations` (`LocationsManager.py:432`),
/// which is exactly `[main_repo/profiles] + [overlay/profiles …]`, never
/// the per-profile-chain levels (real gentoo never puts `license_groups`
/// in a `profiles/<foo>/` profile dir). An overlay's own `profiles/`
/// *profile directory* joining the active chain is still only reached
/// via a chain `parent` file's `reponame:path` syntax
/// (`expand_parent_colon`).
///
/// `main_repo_name` (the main repo's own name from `repos.conf`, e.g.
/// `portage_repo::find_repos`'s main entry) plus `overlay_repos` above
/// together give `resolve_profile_chain` every configured repo's own
/// `(name, location)`, needed to resolve a `parent` file's real
/// cross-repo syntax (`expand_parent_colon`, grounded against
/// `LocationsManager._expand_parent_colon`): a bare `:some/path` means
/// "this same repo" (whichever repo the *current* profile node's own
/// directory belongs to -- `repo_containing`), `reponame:some/path`
/// means a different, named repo. Both expand to
/// `<repo_location>/profiles/some/path`. A `reponame:path` whose
/// `reponame` is a repo *alias* (`repos.conf`/`layout.conf` `aliases =`,
/// `portage_repo::RepoConfig::aliases`) resolves too -- real
/// `LocationsManager._expand_parent_colon` looks the token up in
/// `repositories.get_location_for_name`, which is keyed on aliases as
/// well as canonical names -- so `repo_aliases` carries every
/// `(alias, location)` pair. (An atom's own `::alias` is a *different*
/// thing and is deliberately NOT resolved -- real `match_from_list` does
/// a straight `pkg.repo == atom.repo` name comparison with no alias
/// step, and portuale matches that.)
///
/// `eroot` is real `EROOT` (the target root -- portuale's own `ROOT`,
/// passed as `root` at every call site): `env.d`
/// (`<eroot>/etc/profile.env`, real `_get_env_d`) is read relative to
/// it, not to `config_root` (real `PORTAGE_CONFIGROOT`). The two
/// coincide in every typical config and in every fixture test; they
/// differ only on a split `--config-root`/`--root` setup. The rest of
/// the resolution (profile chain, `make.conf`, `package.*`) stays
/// `config_root`-relative, exactly like real. Deliberate cut inside
/// the cut: real `_get_env_d` merges *two* files (broot's and eroot's,
/// routing `PATH`/`ROOTPATH`-class vars to broot and everything else
/// to eroot); portuale reads eroot's file only -- it has no distinct
/// `BROOT` (builds always run on the host), and reading the host's
/// own `/etc/profile.env` would leak host state into
/// fixture-deterministic resolution.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
/// An exact content fingerprint of every USE-context `Config` field the
/// `portage-repo` memo keys read (see `use_context_fingerprint` there).
/// `autounmask_use` is deliberately **excluded**: it is the one field the
/// `'backtrack` loop mutates, and the caller hashes it separately against
/// this frozen base (`#105`).
///
/// `resolve_config` computes this once and stores it in
/// [`Config::use_context_base`]; callers use that instead of recomputing.
/// Every ordered field is hashed in full; the `HashSet` fields use the
/// order-independent xor-fold below (their iteration order is not stable),
/// which is already exact. Computed once per process, so a full hash
/// costs microseconds (backlog #121).
pub fn use_context_base_fingerprint(config: &Config) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();

    // Order-independent digest of a `HashSet<String>` (its iteration order
    // is not stable): count + xor-fold of per-element hashes.
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
    // Per-level view drives `effective_use_flags`/`forced_or_masked_flags`;
    // two configs could share a flat `use_mask` set yet differ in the
    // interleaving, so hash the ordered structure in full.
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

    // `profile_use_layers` -- every layer in full, each inner list in full.
    config.profile_use_layers.len().hash(&mut h);
    for l in &config.profile_use_layers {
        l.make_defaults_use.hash(&mut h);
        l.package_use.hash(&mut h);
    }

    h.finish()
}

/// #114: a content fingerprint of every `is_visible`-relevant config
/// field the USE-context base does not already cover -- `package.mask`/
/// `package.unmask`, the `LICENSE`/`PROPERTIES`/`RESTRICT` accept lists,
/// `package.accept_keywords` and `license_groups`.
///
/// `resolve_config` computes this once and stores it in
/// [`Config::is_visible_base`]; the `is_visible` memo hashes that `u64`
/// plus the live `autounmask_use` instead of re-hashing these lists per
/// call.
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
