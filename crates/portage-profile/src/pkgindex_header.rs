//! The `<pkgdir>/Packages` index header portuale writes after building or
//! backing up a binpkg: Portage's full settings-derived key set, not just
//! `TIMESTAMP`/`VERSION`.
//!
//! Two snapshots, because Portage writes the header from two different
//! processes. A `--buildpkg`/`package` inject runs in the parent (`bintree.inject`
//! reads `self.settings`), while an `unmerge-backup` `quickpkg` runs in a
//! child that inherits the parent's resolved settings as its environment and
//! stacks them over the files again (`vartree._quickpkg_dblink`). The child
//! re-fold resurrects env-negated tokens (`sandbox` is back in its `FEATURES`)
//! and duplicates pruned ones (`ACCEPT_LICENSE: @FREE @FREE`).
//!
//! Scope cuts: `USE`/`ACCEPT_KEYWORDS`/`IUSE_IMPLICIT`/`USE_EXPAND*` reuse the
//! parent fold for the child (they differ only when the calling env negates
//! one of their tokens, which the per-layer sources below do not track);
//! `REPO_REVISIONS` (needs sync history) is never set; entry sorting stays
//! append order (the bed sorts stanzas before comparing).
// Portage: dbapi/bintree.py, _update_pkgindex_header; getbinpkg.py,
//   PackageIndex.write; package/ebuild/config.py, regenerate;
//   package/ebuild/_config/helper.py, prune_incremental

use crate::*;

/// The header key set for one writer, as `(key, value)` with `None`
/// meaning "ensure absent" (Portage pops falsy settings values on every
/// inject). Sorted by key, like Portage's `keys.sort()` write path.
pub type HeaderSnapshot = Vec<(String, Option<String>)>;

/// The two header snapshots: `parent` for the in-process `--buildpkg`
/// inject, `child` for the `quickpkg`-as-a-subprocess re-fold.
#[derive(Clone, Debug, Default)]
pub struct PkgIndexHeader {
    pub parent: HeaderSnapshot,
    pub child: HeaderSnapshot,
}

fn sorted_join(set: &HashSet<String>) -> Option<String> {
    (!set.is_empty()).then(|| {
        let mut tokens: Vec<&str> = set.iter().map(String::as_str).collect();
        tokens.sort_unstable();
        tokens.join(" ")
    })
}

fn scalar_var(config: &Config, key: &str) -> Option<String> {
    config
        .other_vars
        .get(key)
        .filter(|v| !v.is_empty())
        .cloned()
}

fn incremental_var(config: &Config, key: &str) -> Option<String> {
    config
        .resolved_incremental(key)
        .map(|tokens| tokens.join(" "))
        .filter(|v| !v.is_empty())
}

/// The stacked file-layer tokens for one `ACCEPT_*` key, in Portage's
/// stacking order (`globals`, profile `defaults`, `make.conf`), each layer
/// last-wins, with the `package.*` `*/*` tokens joined onto the `conf`
/// layer like Portage's `extract_global_changes` / `conf[...] += ...`.
fn accept_file_tokens(config: &Config, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for layer in [
        AcceptRawLayer::Globals,
        AcceptRawLayer::Defaults,
        AcceptRawLayer::Conf,
    ] {
        if let Some((_, _, tokens)) = config
            .accept_raw_layers
            .iter()
            .rev()
            .find(|(l, k, _)| *l == layer && k == key)
        {
            out.extend(tokens.iter().cloned());
        }
        if layer == AcceptRawLayer::Conf {
            let wildcards = match key {
                "ACCEPT_LICENSE" => &config.package_license,
                "ACCEPT_PROPERTIES" => &config.package_properties,
                "ACCEPT_RESTRICT" => &config.package_accept_restrict,
                _ => continue,
            };
            if let Some((_, tokens)) = wildcards.iter().find(|(atom, _)| atom == "*/*") {
                out.extend(tokens.iter().cloned());
            }
        }
    }
    out
}

fn env_tokens(config: &Config, key: &str) -> Vec<String> {
    config
        .pkgindex_env_raw
        .get(key)
        .map(|v| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// Portage's stacked `ACCEPT_*` value: the file layers plus the calling-env
/// layer, order-preserving with duplicates kept.
fn accept_parent_value(config: &Config, key: &str) -> Option<String> {
    let mut stacked = accept_file_tokens(config, key);
    stacked.extend(env_tokens(config, key));
    let pruned = prune_token_layers(&stacked);
    (!pruned.is_empty()).then(|| pruned.join(" "))
}

/// The file layers of a tracked incremental for the child's re-fold: every
/// recorded source except the calling-env one (the last entry, pushed by
/// `apply_env_layer`, present exactly when the env set the key).
fn child_file_layers<'a>(config: &'a Config, key: &str) -> Vec<&'a [String]> {
    let Some(sources) = config.incremental_sources.get(key) else {
        return Vec::new();
    };
    let without_env = match config.pkgindex_env_raw.contains_key(key) {
        true => sources.len().saturating_sub(1),
        false => sources.len(),
    };
    sources[..without_env.min(sources.len())]
        .iter()
        .map(Vec::as_slice)
        .collect()
}

/// Build both header snapshots from a resolved config. Values are the
/// resolved settings, the same source the merge already uses; `TIMESTAMP`
/// and `PACKAGES` are set per write by the index writer, not here.
pub fn pkgindex_header(config: &Config) -> PkgIndexHeader {
    // The global USE Portage exposes in settings (and the header): the
    // resolved flags with profile `use.force` folded in and `use.mask`
    // removed. Same fold as `emerge --info`'s display.
    // Portage: config.py, regenerate
    let mut global_use: HashSet<String> = config.use_flags.clone();
    global_use.extend(config.use_force.iter().cloned());
    for masked in &config.use_mask {
        global_use.remove(masked);
    }
    let mut parent: HeaderSnapshot = Vec::new();
    for key in [
        "ARCH",
        "CBUILD",
        "CHOST",
        "ELIBC",
        "GENTOO_MIRRORS",
        "INSTALL_MASK",
        "KERNEL",
    ] {
        parent.push((key.to_string(), scalar_var(config, key)));
    }
    for (key, set) in [
        ("ACCEPT_KEYWORDS", &config.accept_keywords),
        ("IUSE_IMPLICIT", &config.iuse_implicit),
        ("USE", &global_use),
        ("USE_EXPAND", &config.use_expand),
        ("USE_EXPAND_HIDDEN", &config.use_expand_hidden),
        ("USE_EXPAND_IMPLICIT", &config.use_expand_implicit),
        ("USE_EXPAND_UNPREFIXED", &config.use_expand_unprefixed),
    ] {
        parent.push((key.to_string(), sorted_join(set)));
    }
    for key in ["CONFIG_PROTECT", "CONFIG_PROTECT_MASK", "FEATURES"] {
        parent.push((key.to_string(), incremental_var(config, key)));
    }
    for key in ["ACCEPT_LICENSE", "ACCEPT_PROPERTIES", "ACCEPT_RESTRICT"] {
        parent.push((key.to_string(), accept_parent_value(config, key)));
    }
    parent.push((
        "PROFILE".to_string(),
        (!config.pkgindex_profile.is_empty()).then(|| config.pkgindex_profile.clone()),
    ));
    parent.push(("VERSION".to_string(), Some("0".to_string())));
    parent.push((
        "URI".to_string(),
        scalar_var(config, "PORTAGE_BINHOST_HEADER_URI"),
    ));
    parent.push(("TTL".to_string(), scalar_var(config, "PORTAGE_BINHOST_TTL")));
    let mut implicits: Vec<&str> = config
        .use_expand_implicit
        .iter()
        .map(String::as_str)
        .collect();
    implicits.sort_unstable();
    for var in implicits {
        parent.push((
            format!("USE_EXPAND_VALUES_{var}"),
            scalar_var(config, &format!("USE_EXPAND_VALUES_{var}")),
        ));
    }

    let value_of = |snapshot: &[(String, Option<String>)], key: &str| -> Vec<String> {
        snapshot
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_ref())
            .map(|v| v.split_whitespace().map(String::from).collect())
            .unwrap_or_default()
    };
    let mut child = parent.clone();
    for key in ["FEATURES", "CONFIG_PROTECT", "CONFIG_PROTECT_MASK"] {
        let mut layers = child_file_layers(config, key);
        let parent_tokens = value_of(&parent, key);
        layers.push(parent_tokens.as_slice());
        let folded = fold_token_layers(&layers);
        if let Some(slot) = child.iter_mut().find(|(k, _)| k == key) {
            slot.1 = (!folded.is_empty()).then(|| folded.join(" "));
        }
    }
    for key in ["ACCEPT_LICENSE", "ACCEPT_PROPERTIES", "ACCEPT_RESTRICT"] {
        let mut stacked = accept_file_tokens(config, key);
        stacked.extend(value_of(&parent, key));
        let pruned = prune_token_layers(&stacked);
        if let Some(slot) = child.iter_mut().find(|(k, _)| k == key) {
            slot.1 = (!pruned.is_empty()).then(|| pruned.join(" "));
        }
    }

    fn sort_snapshot(snapshot: &mut HeaderSnapshot) {
        snapshot.sort_by(|a, b| a.0.cmp(&b.0));
    }
    sort_snapshot(&mut parent);
    sort_snapshot(&mut child);
    PkgIndexHeader { parent, child }
}
