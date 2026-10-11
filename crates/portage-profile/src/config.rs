//! The resolved configuration (`Config`) and its USE layers.

use crate::*;

/// One profile chain level's `defaults`-tier USE contribution: its
/// `make.defaults` `USE=` values (file order), then its own `package.use`
/// entries. See [`Config::profile_use_layers`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileUseLayer {
    pub make_defaults_use: Vec<String>,
    pub package_use: Vec<(String, Vec<String>)>,
}

/// One profile chain level's contribution to the `use.force`/`use.mask`
/// (+ `.stable.` variants) stacking, in file order. A `Default`
/// (all-empty) level is a valid, common state.
/// See [`Config::use_mask_force_levels`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UseMaskForceLevel {
    pub use_mask: Vec<String>,
    pub use_force: Vec<String>,
    pub use_stable_mask: Vec<String>,
    pub use_stable_force: Vec<String>,
    pub package_use_mask: Vec<(String, Vec<String>)>,
    pub package_use_force: Vec<(String, Vec<String>)>,
    pub package_use_stable_mask: Vec<(String, Vec<String>)>,
    pub package_use_stable_force: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub use_flags: HashSet<String>,
    /// Raw `USE=` values from every profile `make.defaults` (chain
    /// order). A hand-built `Config` must set this, `conf_use_tokens`
    /// and `use_flags` together; `resolve_config` keeps them in sync.
    // Portage: configdict["defaults"]
    pub use_tokens: Vec<String>,
    /// Raw `USE=` values from `make.conf` plus `USE_EXPAND` values,
    /// replayed after [`Config::use_tokens`].
    // Portage: configdict["conf"]
    pub conf_use_tokens: Vec<String>,
    /// Process-environment `USE="..."` values: the highest `USE_ORDER`
    /// tier, replayed after the user-level `package.use`. Populated by
    /// `apply_env_layer` from `$USE`; empty otherwise.
    // Portage: configdict["env"]["USE"]
    pub env_use_tokens: Vec<String>,
    /// `USE=` values from `<eroot>/etc/profile.env`: the lowest
    /// `USE_ORDER` tier, replayed first. Read from `<eroot>`, not
    /// `config_root`; almost always empty.
    // Portage: configdict["env.d"]["USE"]
    pub envd_use_tokens: Vec<String>,
    /// `<eroot>/etc/profile.env` assigns `PATH`: the phase `PATH` is then
    /// the config-file stack's value, not the calling env's.
    // Portage: doebuild.py, _doebuild_path
    pub envd_sets_path: bool,
    /// Raw text of `<eroot>/etc/profile.env` as last read (`""` when
    /// missing). `env_d_changed` compares the live file against this.
    // Portage: config.py, reload
    pub envd_raw: String,
    /// The `eroot` the `profile.env` was read from (empty on a
    /// `Config::default()`): the per-package-task reload re-reads the
    /// same file without a root parameter. Empty disables the reload.
    pub envd_eroot: PathBuf,
    /// The env.d scalar layer as last read, after the `_env_d_blacklist`
    /// pop. Keys a higher layer also assigned stay listed here;
    /// `envd_higher_keys` tells `reload_env_d` which those are.
    // Portage: config.py, init (env.d read)
    pub envd_vars: HashMap<String, String>,
    /// Every key a higher config layer assigned during `resolve_config`
    /// (plus `"USE"` when the globals/profile `USE`-reset dropped env.d's
    /// scalar). `reload_env_d` never touches `other_vars` for these: the
    /// higher layer wins, like Portage's layered `configdict` lookup.
    pub envd_higher_keys: HashSet<String>,
    /// The env.d incremental layer per [`TRACKED_INCREMENTALS`] key as
    /// last read. `reload_env_d` replaces index 0 of
    /// `incremental_sources[key]` with the re-read layer.
    pub envd_incremental_layers: HashMap<String, Vec<String>>,
    /// The `PROFILE_ONLY_VARIABLES` fold as of `resolve_config`, the
    /// profile half of the env.d blacklist. Frozen because Portage
    /// computes the blacklist once at init and `reload()` reuses it.
    // Portage: config.py, init (env.d blacklist)
    pub envd_profile_only: Vec<String>,
    pub accept_keywords: HashSet<String>,
    /// Raw atom or bounded-wildcard-atom strings from `package.mask`, with
    /// `-atom` removal already applied within this source.
    pub package_mask: Vec<String>,
    /// Raw atom or bounded-wildcard-atom strings from `package.unmask`.
    pub package_unmask: Vec<String>,
    /// `(atom-or-wildcard, extra accepted keyword tokens)` pairs from
    /// `package.accept_keywords`. A `"**"` token means "accept any
    /// keyword".
    pub package_accept_keywords: Vec<(String, Vec<String>)>,
    /// Every repo's `profiles/package.use`: the weakest USE layer.
    /// Overlays `::repo`-scoped; the main repo unscoped.
    pub package_use_repo: Vec<(String, Vec<String>)>,
    /// `(repo_name, USE= values)` from each repo's `make.defaults`,
    /// applied at the start of the `repo` layer. Main repo entry applies
    /// to every package; overlays only to their own candidates.
    pub repo_make_defaults_use: Vec<(String, Vec<String>)>,
    /// USE flags implied by `FEATURES` (in practice only `test`),
    /// applied between the `repo` and `pkginternal` tiers.
    pub features_use: Vec<String>,
    /// Flat all-levels `package.use` group: fallback when
    /// [`Config::profile_use_layers`] is empty (a hand-built `Config`).
    pub package_use: Vec<(String, Vec<String>)>,
    /// The `defaults` tier per profile level (weakest first), used in
    /// place of [`Config::use_tokens`] + [`Config::package_use`].
    pub profile_use_layers: Vec<ProfileUseLayer>,
    /// User-level `package.use`: after `make.conf`, the strongest USE
    /// layer before force/mask. Only source with the `USE_EXPAND`-prefix
    /// shorthand (`VIDEO_CARDS: nvidia` lines).
    pub package_use_user: Vec<(String, Vec<String>)>,
    /// `--autounmask-use` changes, replayed as the top USE tier.
    /// Normally empty; the `'backtrack` loop rebuilds it each retry.
    pub autounmask_use: Vec<(String, Vec<String>)>,

    /// Frozen digest of every USE-context field except `autounmask_use`.
    /// `None` until `resolve_config` freezes it (callers then fall back
    /// to the full hash); `Clone` carries it, correct for the `'backtrack`
    /// loop's clones, which differ only in `autounmask_use`.
    pub use_context_base: Option<u64>,

    /// The visibility base: `is_visible` fields the USE-context base
    /// omits. Same `None`-means-fall-back contract as `use_context_base`.
    pub is_visible_base: Option<u64>,
    /// `--autounmask-backtrack` (default off): re-drive the graph on
    /// autounmask changes when on. A CLI-option carrier, not profile state.
    pub autounmask_backtrack: bool,
    /// `@world ∪ @selected ∪ @system` atoms (complete-graph mode only).
    /// Gates the slot-operator-rebuild scan; empty disables it.
    pub complete_seed_atoms: Vec<String>,
    /// Same atoms, populated on every pass; only for blocker
    /// classification, never fed into `complete_seed_atoms`.
    pub blocker_retry_seed_atoms: Vec<String>,
    /// Merge set of the first, non-complete pass; the complete pass
    /// never merges outside it. Empty is a strict no-op.
    pub complete_locked_merges: Vec<String>,
    /// `/etc/portage/package.env` entries; `USE=` half pre-resolved
    /// into [`Config::package_env_use`], scalars into [`Config::package_env_vars`].
    pub package_env: Vec<(String, Vec<String>)>,
    /// `package.env` files' `USE=` values, applied in `pkg` just
    /// before [`Config::package_use_user`]. Missing files are silent.
    pub package_env_use: Vec<(String, Vec<String>)>,
    /// `package.env` files' non-`USE` assignments for the build-phase
    /// env. Not consumed by `--pretend`.
    pub package_env_vars: Vec<(String, Vec<(String, String)>)>,
    /// `@system` atoms: profile `packages` files, stacked, `*`-lines only.
    pub system_packages: Vec<String>,
    /// `@profile` atoms: unstarred `packages` lines of `profile-set`
    /// levels only. Part of every `@world` expansion.
    pub profile_packages: Vec<String>,
    /// `package.provided` CPVs: a matching dependency atom counts as
    /// satisfied. Malformed lines never match; no EAPI 7+ gate.
    pub package_provided: Vec<String>,
    /// Flags forced on by `use.force`. Not folded into `use_flags`:
    /// applied last, after `package.use` (also feeds `--newuse`).
    pub use_force: HashSet<String>,
    /// Flags forced off by every profile level's `use.mask`. See
    /// `use_force` for why this is not folded into `use_flags` either.
    pub use_mask: HashSet<String>,
    /// `package.use.force` entries: repo-level (`::repo`-scoped) plus
    /// profile-chain. No user-level source exists.
    pub package_use_force: Vec<(String, Vec<String>)>,
    /// `(atom-or-wildcard, flag tokens)` pairs from `package.use.mask`.
    /// See `package_use_force`.
    pub package_use_mask: Vec<(String, Vec<String>)>,
    /// `USE_EXPAND` variable names (PMS 7.3.4); values expand into
    /// lowercase-prefixed pseudo-flags folded into `use_flags`.
    pub use_expand: HashSet<String>,
    /// `USE_EXPAND_UNPREFIXED` names: values fold into `use_flags`
    /// unprefixed (how `amd64`/`x86`/`arm64` exist as USE flags at all).
    pub use_expand_unprefixed: HashSet<String>,
    /// `USE_EXPAND_IMPLICIT` names: expanded flags valid IUSE
    /// everywhere. Drives `iuse_effective`.
    pub use_expand_implicit: HashSet<String>,
    /// `IUSE_IMPLICIT` flag names, folded into `iuse_effective`.
    pub iuse_implicit: HashSet<String>,
    /// EAPI 5+ `IUSE_EFFECTIVE`: the extra `is_valid_flag` domain on
    /// top of declared `IUSE` (`USE_EXPAND_HIDDEN` excluded, display-only).
    pub iuse_effective: HashSet<String>,
    /// `USE_EXPAND_HIDDEN` names: display-only grouping hint for
    /// `-pv`, never consulted by resolution.
    pub use_expand_hidden: HashSet<String>,
    /// `use.stable.force`: applied conditionally on stability, never
    /// folded into `use_flags` (stable is per-candidate).
    pub use_stable_force: HashSet<String>,
    /// `use.stable.mask`. See `use_stable_force`.
    pub use_stable_mask: HashSet<String>,
    /// `PORTAGE_ARCHLIST`: profile `arch.list` files, stacked; arch
    /// flags become valid `REQUIRED_USE` references.
    pub archlist: HashSet<String>,
    /// `package.use.stable.force` entries, applied only for a stable
    /// candidate.
    pub package_use_stable_force: Vec<(String, Vec<String>)>,
    /// `(atom-or-wildcard, flag tokens)` pairs from
    /// `package.use.stable.mask`. See `package_use_stable_force`.
    pub package_use_stable_mask: Vec<(String, Vec<String>)>,
    /// The eight force/mask sources per profile level (chain order),
    /// for per-level interleaved stacking the flat fields cannot express.
    /// The flat fields stay for global-only readers.
    // Portage: UseManager.getUseMask/getUseForce
    pub use_mask_force_levels: Vec<UseMaskForceLevel>,
    /// `license_groups`: values extend per group name, raw; `@group`
    /// references expand via `expand_license_token` where used.
    pub license_groups: HashMap<String, Vec<String>>,
    /// `ACCEPT_LICENSE`, `@group`-expanded but symbolic (`*` needs a
    /// candidate's `LICENSE`). Last-level-wins; unset defaults `"* -@EULA"`.
    pub accept_license: Vec<String>,
    /// `package.license` entries (user-level only, `@group`-expanded);
    /// `*/*` lines are ordinary per-package entries.
    pub package_license: Vec<(String, Vec<String>)>,
    /// `ACCEPT_PROPERTIES`: last-level-wins scalar, no groups;
    /// unset-anywhere defaults to `"*"`.
    pub accept_properties: Vec<String>,
    /// `package.properties` entries, user-level only.
    pub package_properties: Vec<(String, Vec<String>)>,
    /// `ACCEPT_RESTRICT`: identical mechanism and default to
    /// `accept_properties`, for `RESTRICT` instead of `PROPERTIES`.
    pub accept_restrict: Vec<String>,
    /// `(atom-or-wildcard, raw tokens)` pairs from
    /// `package.accept_restrict`, user-level only. See
    /// `package_properties`.
    pub package_accept_restrict: Vec<(String, Vec<String>)>,
    /// `PKGDIR` scalar for `--usepkg` (default `"/var/cache/binpkgs"`).
    pub pkgdir: String,
    /// `--getbinpkg` repositories: `binrepos.conf` plus implicit
    /// `PORTAGE_BINHOST` entries, sorted by `(priority, name)`.
    pub binrepos: Vec<BinRepo>,
    /// `$PKGDIR` scan fallback when `Packages` is absent (`None` =
    /// no scan). Recomputed each run, never written back.
    pub scanned_binpkgs: Option<Vec<std::collections::HashMap<String, String>>>,
    /// Every non-USE/ACCEPT_KEYWORDS variable's final scalar value
    /// (last-level-wins across profile chain + `make.conf`): the
    /// `scalars` map `resolve_config` builds for `${VAR}` substitution,
    /// exposed for `emerge --info`'s variable dump.
    pub other_vars: HashMap<String, String>,
    /// Per-source token lists for the displayed `INCREMENTALS`
    /// variables, in db-stack order; see `resolved_incremental`.
    pub incremental_sources: HashMap<String, Vec<Vec<String>>>,
    /// The selected profile's path relative to the main repo's
    /// `profiles/` dir (`""` when no `make.profile` resolves): Portage's
    /// `PROFILE` `Packages`-index header value.
    // Portage: dbapi/bintree.py, _update_pkgindex_header
    pub pkgindex_profile: String,
    /// Raw `ACCEPT_LICENSE`/`ACCEPT_PROPERTIES`/`ACCEPT_RESTRICT`
    /// assignments per file layer, in Portage's stacking order: the
    /// `Packages`-index header stacks them (order-preserving, duplicates
    /// kept), unlike the last-wins scalars the resolver keeps.
    // Portage: package/ebuild/config.py, regenerate
    pub accept_raw_layers: Vec<(AcceptRawLayer, String, Vec<String>)>,
    /// Calling-environment raw values for the header-relevant variables
    /// (`FEATURES`, `CONFIG_PROTECT`, `CONFIG_PROTECT_MASK`,
    /// `ACCEPT_LICENSE`, `ACCEPT_PROPERTIES`, `ACCEPT_RESTRICT`), recorded
    /// by `apply_env_layer`: the `Packages`-index header of a `quickpkg`
    /// child stacks the inherited (already-resolved) values over the
    /// files again.
    // Portage: dbapi/vartree.py, _quickpkg_dblink; bin/quickpkg
    pub pkgindex_env_raw: HashMap<String, String>,
}

/// Which file layer a raw `ACCEPT_*` assignment came from, in Portage's
/// header-stacking order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcceptRawLayer {
    Globals,
    Defaults,
    Conf,
}

impl Config {
    /// The incremental fold for one `const.INCREMENTALS` variable: stack
    /// every source's tokens in db order (`-*` clears, `-tok` discards,
    /// bare or `+`-prefixed adds), then sort. `None` when never assigned.
    // Portage: config.py, regenerate
    pub fn resolved_incremental(&self, key: &str) -> Option<Vec<String>> {
        self.incremental_sources.get(key).map(|sources| {
            fold_token_layers(
                sources
                    .iter()
                    .map(Vec::as_slice)
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
        })
    }

    /// Whether `<eroot>/etc/profile.env` changed since this config was
    /// resolved or last reloaded. A missing file reads as `""`. The
    /// per-package-task gate: when false the caller keeps the old path.
    pub fn env_d_changed(&self, eroot: &Path) -> bool {
        std::fs::read_to_string(eroot.join("etc/profile.env")).unwrap_or_default() != self.envd_raw
    }

    /// Re-read env.d and replace that layer, leaving every higher layer
    /// alone. Keys a higher layer owns or the blacklist drops are
    /// untouched; other keys match the new file. The USE-context
    /// fingerprint is refreshed for the `portage-repo` memos.
    /// Higher-layer expansions are deliberately not redone.
    // Portage: config.py, reload
    pub fn reload_env_d(&mut self, eroot: &Path) {
        let text = std::fs::read_to_string(eroot.join("etc/profile.env")).unwrap_or_default();
        let blacklist: HashSet<String> = self
            .envd_profile_only
            .iter()
            .cloned()
            .chain(ENV_BLACKLIST.iter().map(|s| s.to_string()))
            .collect();
        let parsed = parse_envd_text(&text);
        for (key, value) in &parsed.vars {
            if blacklist.contains(key) || self.envd_higher_keys.contains(key) {
                continue;
            }
            self.other_vars.insert(key.clone(), value.clone());
        }
        for key in self.envd_vars.keys() {
            if !parsed.vars.contains_key(key) && !self.envd_higher_keys.contains(key) {
                self.other_vars.remove(key);
            }
        }
        for key in TRACKED_INCREMENTALS {
            let new_layer = parsed
                .incremental_layers
                .get(key)
                .filter(|_| !blacklist.contains(key));
            let had_old = self.envd_incremental_layers.contains_key(key);
            if new_layer.is_none() && !had_old {
                continue;
            }
            // A recorded env.d layer is still index 0 (lowest db-stack
            // layer): nothing post-resolve mutates these lists.
            let sources = self.incremental_sources.entry(key.to_string()).or_default();
            if had_old && !sources.is_empty() {
                match new_layer {
                    Some(tokens) => sources[0] = (*tokens).clone(),
                    None => {
                        sources.remove(0);
                    }
                }
                if sources.is_empty() {
                    self.incremental_sources.remove(key);
                }
            } else if let Some(tokens) = new_layer {
                // No recorded layer (or its trace is gone, unreachable):
                // the new env.d layer goes first regardless.
                sources.insert(0, (*tokens).clone());
            }
        }
        self.envd_raw = text;
        self.envd_vars = parsed
            .vars
            .into_iter()
            .filter(|(k, _)| !blacklist.contains(k))
            .collect();
        self.envd_incremental_layers = parsed
            .incremental_layers
            .into_iter()
            .filter(|(k, _)| !blacklist.contains(k))
            .collect();
        self.envd_sets_path = parsed.sets_path && !blacklist.contains("PATH");
        self.envd_use_tokens = read_envd_use_tokens_text(&self.envd_raw);
        self.use_context_base = Some(use_context_base_fingerprint(self));
    }
}
