//! The resolved configuration (`Config`) and its USE layers.

use crate::*;

/// One profile chain level's `defaults`-tier USE contribution -- its
/// `make.defaults` `USE=` value strings (in file order), then its own
/// `package.use` entries. See [`Config::profile_use_layers`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileUseLayer {
    pub make_defaults_use: Vec<String>,
    pub package_use: Vec<(String, Vec<String>)>,
}

/// One profile chain level's contribution to the `use.force`/`use.mask`
/// (+ `.stable.` variants) stacking, in file order -- see
/// [`Config::use_mask_force_levels`]. `use_*` fields hold raw
/// `flag`/`-flag` line tokens (global `use.mask`/`use.force`); the
/// `package_use_*` fields hold `(atom, flag-tokens)` pairs
/// (`package.use.mask` etc.). A `Default` (all-empty) level is a valid,
/// common state.
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
    /// The raw `USE=` value strings from every profile level's own
    /// `make.defaults` (chain order) -- real `configdict["defaults"]`'s
    /// own `make_defaults_use`, the `defaults` USE_ORDER layer, *not yet*
    /// collapsed into a flat set. `portage-repo`'s own
    /// `effective_use_flags` replays this directly (via
    /// `apply_incremental`) on top of a package's own IUSE-defaults
    /// (`pkginternal`) seed, instead of union-ing the already-flattened
    /// `use_flags` on top of it -- a flat union can only ever *add* a
    /// flag, so it could never let `defaults`/`conf` explicitly *cancel*
    /// an IUSE `+default` the way real portage's own single continuous
    /// incremental walk (`repo` -> `pkginternal` -> `defaults` -> `conf`
    /// -> `pkg`) does. See `effective_use_flags`'s own doc comment for
    /// the full grounding.
    ///
    /// Historically this field also held the `make.conf` + `USE_EXPAND`
    /// values; the "Config depth" slice split those into
    /// [`Config::conf_use_tokens`] so profile-level `package.use`
    /// ([`Config::package_use`]) can be applied at its real `USE_ORDER`
    /// position *between* the two (after `defaults`, before `conf`).
    ///
    /// **Consistency note**: `resolve_config` always keeps this,
    /// `conf_use_tokens`, and `use_flags` in sync (all grow from the same
    /// calls); a `Config` literal built by hand (as several test modules
    /// do) must set them together too, or `effective_use_flags` will
    /// silently see an empty (or stale) token list regardless of what
    /// `use_flags` says.
    pub use_tokens: Vec<String>,
    /// The raw `USE=` value strings from `make.conf` (plus any `source`d
    /// files), then every `USE_EXPAND` variable's own value (already
    /// `varname_`-prefixed), then every `USE_EXPAND_UNPREFIXED`
    /// variable's own raw value -- real `configdict["conf"]` plus the
    /// `USE_EXPAND` folding real `regenerate()` does last. Replayed by
    /// `effective_use_flags` immediately after [`Config::use_tokens`] and
    /// the profile-level `package.use`, immediately before the user-level
    /// `package.use`. Split out of `use_tokens` by the "Config depth"
    /// slice -- see that field's own doc comment.
    pub conf_use_tokens: Vec<String>,
    /// The process-environment `USE="..."` value(s) -- real
    /// `configdict["env"]["USE"]`, the **highest** `USE_ORDER` tier.
    /// `effective_use_flags` replays these (via `apply_incremental`,
    /// `+`/`-` syntax preserved) *after* the user-level
    /// `/etc/portage/package.use`, so `USE="-X" emerge foo` overrides
    /// even a `package.use` flag for `foo`. Populated by
    /// `apply_env_layer` from `$USE`; empty otherwise. (The global
    /// [`Config::use_flags`] set is also env-last, which is correct for
    /// any package with no `package.use` entry; this field makes the
    /// per-package walk agree with it.)
    pub env_use_tokens: Vec<String>,
    /// Real `configdict["env.d"]["USE"]` -- the **lowest** `USE_ORDER`
    /// tier (`env:pkg:conf:defaults:pkginternal:features:repo:env.d`),
    /// from `<eroot>/etc/profile.env` (which `env-update` generates from
    /// `/etc/env.d/*`). Read `expand=False`, `export ` prefix stripped.
    /// `effective_use_flags` replays these first, before the `repo` tier,
    /// so everything else overrides them. Read from `<eroot>` (see
    /// `resolve_config`), not `config_root`. `/etc/env.d/*` practically
    /// never sets `USE`, so this is almost always empty.
    pub envd_use_tokens: Vec<String>,
    /// `<eroot>/etc/profile.env` assigns `PATH`. Real `_doebuild_path`
    /// (`doebuild.py:365-368`) then drops the calling env's `PATH` so the
    /// phase `PATH` is the config-file stack's value
    /// (`other_vars["PATH"]`) -- "this allows packages to update our PATH
    /// as they get installed".
    pub envd_sets_path: bool,
    /// The raw text of `<eroot>/etc/profile.env` as last read (`""` when
    /// the file is missing). `env_d_changed` compares the live file
    /// against this, so per-package-task reloads (`reload_env_d`, real
    /// `config.reload()`, `config.py:2691-2699`) only pay for a re-read
    /// when `env-update` actually rewrote it mid-run.
    pub envd_raw: String,
    /// The `eroot` `resolve_config` read `profile.env` from (empty on a
    /// `Config::default()`): the per-package-task reload
    /// (`entry_build_env`) re-reads the same file without a root
    /// parameter. Empty disables the reload.
    pub envd_eroot: PathBuf,
    /// The env.d scalar layer as last read: every `profile.env` key to
    /// its substituted value, **after** the `_env_d_blacklist` pop (real
    /// `config.py:724-743`). Keys a higher layer (`make.globals`, the
    /// profile chain, `make.conf`, the calling env) also assigned stay
    /// listed here when that higher value is what `other_vars` holds --
    /// `envd_higher_keys` tells `reload_env_d` which those are.
    pub envd_vars: HashMap<String, String>,
    /// Every key a higher config layer assigned during `resolve_config`,
    /// whether or not env.d held it then (plus `"USE"` when the
    /// globals/profile `USE`-reset dropped env.d's scalar --
    /// `resolve_config`'s own `scalars.remove("USE")`). `reload_env_d`
    /// never touches `other_vars` for these keys: the higher layer wins,
    /// exactly like real's layered `configdict` lookup, where replacing
    /// `configdict["env.d"]` cannot move a key a higher db owns.
    pub envd_higher_keys: HashSet<String>,
    /// The env.d incremental layer per [`TRACKED_INCREMENTALS`] key as
    /// last read (the file's final assignment's tokens, missing when the
    /// file never assigned the key). `reload_env_d` replaces index 0 of
    /// `incremental_sources[key]` with the re-read layer -- env.d is the
    /// lowest db-stack layer, so its contribution is always first.
    pub envd_incremental_layers: HashMap<String, Vec<String>>,
    /// The `PROFILE_ONLY_VARIABLES` fold as of `resolve_config`, the
    /// profile half of real `_env_d_blacklist`
    /// (`profile_only_variables + _env_blacklist`, `config.py:734-743`).
    /// Frozen here because real computes the blacklist once at init and
    /// `reload()` reuses it -- a re-read env.d layer must not change
    /// which keys the blacklist drops.
    pub envd_profile_only: Vec<String>,
    pub accept_keywords: HashSet<String>,
    /// Raw atom or bounded-wildcard-atom strings (see
    /// `portage_dep::parse_wildcard_atom`) from `package.mask`, with
    /// `-atom` removal already applied within this source.
    pub package_mask: Vec<String>,
    /// Raw atom or bounded-wildcard-atom strings from `package.unmask`.
    pub package_unmask: Vec<String>,
    /// (atom-or-wildcard string, extra accepted keyword tokens) pairs
    /// from `package.accept_keywords`. A `"**"` keyword token means
    /// "accept any keyword" for matching packages.
    pub package_accept_keywords: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, raw USE tokens) pairs from **every
    /// configured repo's own** `profiles/package.use` -- real
    /// `configdict["repo"]` (`_repo_puse_dict`). Overlay entries are
    /// `::repo`-scoped (`scope_repo_package_use_lines`); the main repo's
    /// stay unscoped (it implicitly masters every overlay). Applied by
    /// `effective_use_flags` *first*, before the ebuild's own IUSE
    /// `+`/`-` defaults -- the weakest USE layer portuale models.
    /// Tokens use the same `-flag`/`flag`/`+flag` incremental syntax as
    /// `USE` itself -- see `apply_incremental`.
    pub package_use_repo: Vec<(String, Vec<String>)>,
    /// `(repo_name, USE= value string(s))` pairs from each configured
    /// repo's `<repo>/profiles/make.defaults` -- real `config.py`'s
    /// `_repo_make_defaults`, folded into `configdict["repo"]` alongside
    /// repo `package.use`. Applied by `effective_use_flags` at the very
    /// start of the `repo` layer, **before** [`Config::package_use_repo`]
    /// (real `regenerate()` walks `configdict["repo"]["USE"]` first, then
    /// that tier's package-scoped USE). `${VAR}` in a value is expanded
    /// against the profile-chain + `make.conf` scalars already
    /// accumulated by the time each file is read.
    ///
    /// The **main repo's** entry has an empty `repo_name` -- it applies
    /// to every package (the main repo implicitly masters every overlay
    /// here). An **overlay's** entry is keyed by its real repo name and
    /// applies only to a candidate resolved from that overlay
    /// (`candidate_str`'s `::<repo>` suffix). **Narrowing:** the full
    /// masters-chain stacking order (main, then each master, then the
    /// repo) collapses to "main, then the candidate's own repo".
    pub repo_make_defaults_use: Vec<(String, Vec<String>)>,
    /// Real `configdict["features"]["USE"]` (`config.py` ~2043): the USE
    /// flags implied by `FEATURES`. In practice only `test` -- appended
    /// when `FEATURES` contains `test`, so a package that declares `test`
    /// in IUSE resolves with it enabled (the make.conf/env equivalent of
    /// `--with-test-deps`). Applied by `effective_use_flags` between the
    /// `repo` and `pkginternal` tiers. `["test"]` or empty; portuale's
    /// `FEATURES` is a last-wins scalar (`other_vars["FEATURES"]`), a
    /// documented simplification of real portage's incremental stacking.
    pub features_use: Vec<String>,
    /// (atom-or-wildcard string, raw USE tokens) pairs from every
    /// **profile** level's own `package.use` (chain order) -- real
    /// `configdict["defaults"]` (`_pkgprofileuse`), as one flat group.
    /// **Superseded** for the actual `defaults`-tier walk by
    /// [`Config::profile_use_layers`] (which interleaves each level's
    /// `package.use` right after that level's `make.defaults`, matching
    /// real portage); `effective_use_flags` only falls back to this flat
    /// list when `profile_use_layers` is empty (a hand-built `Config`,
    /// e.g. a test helper). Still populated by `resolve_config` for
    /// inspection / that fallback.
    pub package_use: Vec<(String, Vec<String>)>,
    /// The `defaults` `USE_ORDER` tier, resolved **per profile chain
    /// level** (weakest first) rather than as two flat groups. Real
    /// `config.py::regenerate()` walks `configdict["defaults"]` one
    /// profile at a time, applying that profile's `make.defaults` `USE=`
    /// and then its own `package.use` before moving to the next -- so a
    /// child profile's `make.defaults USE="-foo"` can cancel a parent
    /// profile's `package.use foo` for a matching package.
    /// `effective_use_flags` iterates this in place of the flat
    /// [`Config::use_tokens`] + [`Config::package_use`] whenever it is
    /// non-empty.
    pub profile_use_layers: Vec<ProfileUseLayer>,
    /// (atom-or-wildcard string, raw USE tokens) pairs from the
    /// user-level `/etc/portage/package.use` -- real `configdict["pkg"]`
    /// (`_pusedict` / `getPUSE`). Applied by `effective_use_flags` after
    /// `make.conf`, the strongest USE layer before the final
    /// `use.force`/`use.mask` step. This is the only one of the three
    /// `package.use` sources that gets the `USE_EXPAND`-prefix shorthand
    /// (`VIDEO_CARDS: nvidia` lines) -- real portage's own user-only
    /// `extended_syntax`; see `parse_package_use_lines`.
    pub package_use_user: Vec<(String, Vec<String>)>,
    /// `--autounmask-use` USE changes the resolver has decided are needed
    /// to make the graph resolve -- real `_dynamic_config.
    /// _needed_use_config_changes`, the in-loop config-change feedback
    /// (`backtracking.py::_feedback_config`). `(atom, [+flag | -flag …])`
    /// pairs, exactly `package_use_user`-shaped -- so `effective_use_flags`
    /// replays them the same way, as the **top** USE tier (after `env`,
    /// before `use.force`/`use.mask`): a package under an autounmask flip
    /// is walked with the flipped flag, and its `flag?`-gated deps appear.
    ///
    /// Normally empty. `portage-repo`'s `'backtrack` loop rebuilds it from
    /// its own `autounmask_use_config` accumulator at the top of each
    /// retry iteration (it clones `Config` per iteration to vary this),
    /// so a hand-built `Config` literal never sets it and gets the
    /// pre-autounmask behaviour.
    pub autounmask_use: Vec<(String, Vec<String>)>,

    /// A frozen digest of every USE-context field **except**
    /// [`autounmask_use`](Self::autounmask_use), computed by
    /// [`resolve_config`] once the config is complete
    /// (`use_context_base_fingerprint`). The memo keys in `portage-repo`
    /// (`effective_use_flags`, `resolved_use_mask_or_force`,
    /// `metadata_key_accepted`) rebuild their fingerprint from this base
    /// plus the live `autounmask_use`, instead of re-hashing the big
    /// `HashSet`s and `Vec`s on every call (backlog `#105`: 23.77 % of
    /// the reference workload with children).
    ///
    /// `None` until `resolve_config` freezes it: a hand-built `Config`
    /// (tests, `Config::default()`, a partial resolve) keeps `None` and
    /// the caller falls back to the full content hash, so a later
    /// in-place edit of a context field is still seen. `Clone` carries
    /// the value, which is correct because the `'backtrack` loop's
    /// clones differ only in `autounmask_use` -- the one field the base
    /// deliberately excludes.
    pub use_context_base: Option<u64>,

    /// #114: the visibility base -- the immutable `is_visible`-relevant
    /// config fields the USE-context base does not cover: `package.mask`/
    /// `package.unmask`, the `LICENSE`/`PROPERTIES`/`RESTRICT` accept
    /// lists, `package.accept_keywords`, and `license_groups`. Frozen by
    /// [`resolve_config`] (`is_visible_base_fingerprint`); the
    /// `portage-repo` `is_visible` memo hashes it plus the live
    /// `autounmask_use` (which the check reaches through
    /// `use_flags_if_conditional`) instead of re-hashing those lists per
    /// call. Same `None`-means-fall-back contract as
    /// [`use_context_base`](Self::use_context_base).
    pub is_visible_base: Option<u64>,
    /// `--autounmask-backtrack` (real `emerge` option, `choices: ("y",
    /// "n")`, **disabled by default** -- `man emerge`). When off (the
    /// default), real portage collects autounmask config changes but does
    /// **not** re-drive the whole graph afterwards (`need_config_change`
    /// -> break in `_backtrack_depgraph`): the flipped package's own USE
    /// line reflects the change, but its newly-`flag?`-gated deps do not
    /// appear. When on (or implied by `--autounmask-continue`), the
    /// `'backtrack` loop re-runs the walk on every accumulator growth so
    /// the cascade is fully realised. Like [`autounmask_use`], this is a
    /// CLI-option carrier the resolver reads, not profile state; a
    /// hand-built `Config` literal leaves it `false`.
    pub autounmask_backtrack: bool,
    /// The `@world ∪ @selected ∪ @system` atom list, populated by the CLI
    /// layer **only when complete-graph mode is active** (real
    /// `_complete_graph`'s deep re-walk of the required sets). The
    /// resolver uses it (`required_set_reachable_cps`) to gate the
    /// slot-operator-rebuild scan -- real only slot-op-rebuilds a
    /// consumer reachable from those sets. Empty when not in complete
    /// mode (and for any hand-built `Config`), which disables the scan
    /// entirely, matching real (`emerge -p <atom>` with no installed
    /// change triggers no slot-op rebuild).
    pub complete_seed_atoms: Vec<String>,
    /// The same `@world ∪ @selected ∪ @system` atom list as
    /// [`Config::complete_seed_atoms`], but populated by the CLI layer on
    /// **every** pass, complete or not (#68/#72 B2c). Real's
    /// `_serialize_tasks` re-runs the whole serialization in complete mode
    /// when the first pass selected an uninstall task
    /// (`depgraph.py:10365-10383`), which walks the required sets -- so a
    /// block resolved by an uninstall sees those sets' installed closure
    /// even on a plain `emerge -p <atom>` run, where the surrounding
    /// resolution is *not* complete. The resolver uses it only for that
    /// blocker classification (`ResolveCtx::blocker_retry_closure`); it
    /// deliberately does not feed `complete_seed_atoms`, which gates the
    /// slot-operator-rebuild scan (filling it here would switch #24/#65
    /// behaviour on for non-complete runs).
    pub blocker_retry_seed_atoms: Vec<String>,
    /// `"category/package"` of every package the **first, non-complete**
    /// resolve pass would actually merge. Real `_complete_graph` runs
    /// after the normal graph is built and swaps package selection to
    /// `_select_pkg_from_graph` -- graph-or-installed, **never a new
    /// merge** (`depgraph.py:8662`). Portuale re-resolves the whole graph
    /// in complete mode instead, so the CLI layer passes phase 1's merge
    /// set here: in the complete pass, an atom whose `cp` is not in this
    /// set may only resolve to `AlreadyInstalled`/`NoVisibleCandidate`,
    /// never a merge. Empty for any non-complete pass and every
    /// hand-built `Config` -- a strict no-op there.
    pub complete_locked_merges: Vec<String>,
    /// (atom-or-wildcard string, ordered env-file names) pairs from
    /// `/etc/portage/package.env` -- real `config.py:894`'s
    /// `grabdict_package(.../package.env)`. Each named file lives under
    /// `/etc/portage/env/` and is a `make.conf`-style `KEY=value`
    /// snippet layered onto a matching package's config (real
    /// `_grab_pkg_env`, invoked from `setcpv`). An entry with no file
    /// names is dropped. The `USE=` half of each file is pre-resolved
    /// into [`Config::package_env_use`], the build-relevant scalar half
    /// into [`Config::package_env_vars`].
    pub package_env: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, raw USE tokens) pairs derived from
    /// [`Config::package_env`]: for each entry, every referenced
    /// `/etc/portage/env/<name>` file's `USE=` value(s), concatenated in
    /// file order. Applied by `effective_use_flags` in the `pkg`
    /// `USE_ORDER` layer, immediately **before**
    /// [`Config::package_use_user`] -- real portage fills
    /// `pkg_configdict` from `package.env` first, then appends
    /// `package.use`'s own USE on top (`config.py:2042-2048`), so a
    /// user `package.use` flag wins over a `package.env` one. A
    /// referenced file that doesn't exist contributes nothing (silently
    /// -- portuale's standing "no warnings from deep in config
    /// resolution" precedent).
    pub package_env_use: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, ordered `(KEY, value)` pairs) derived
    /// from [`Config::package_env`]: for each entry, every referenced
    /// `/etc/portage/env/<name>` file's **non-`USE`** `KEY=value`
    /// assignments, concatenated in file order. Real `_grab_pkg_env`
    /// folds every key into `configdict["pkg"]`; portuale's
    /// `ebuild_phases::match_package_env_vars` applies real's acceptance
    /// gate to this list and layers the survivors onto the build-phase
    /// env (incrementals folded, not replaced). `FEATURES`/`PORTAGE_TMPDIR`
    /// are residues #98/#99. Not consumed by `--pretend` (a build-phase
    /// concern only), so unlike [`Config::package_env_use`] it has no
    /// Python-reference mirror.
    pub package_env_vars: Vec<(String, Vec<(String, String)>)>,
    /// `@system`'s real atom source: every profile level's own `packages`
    /// file, stacked in chain order and filtered to `*`-prefixed lines
    /// (the `*` stripped) -- see the module doc comment's `packages`
    /// bullet and `PackagesSystemSet.load`.
    pub system_packages: Vec<String>,
    /// `@profile`'s real atom source (`ProfilePackageSet.load`,
    /// `portage/_sets/profiles.py:13-38`): the **unstarred** lines of the
    /// `packages` files of only those profile levels whose repo declares
    /// `profile-set` in `metadata/layout.conf`, stacked in chain order
    /// with the same `-atom` removal `@system` uses. Real `@world` is
    /// `@profile @selected @system` (`_sets/__init__.py:97-98`), so this
    /// set is part of every `@world` expansion. A level not inside any
    /// configured repo contributes nothing (real gates on the level's own
    /// repo `profile_formats`; the user-config profile node is the same
    /// deliberate narrowing).
    pub profile_packages: Vec<String>,
    /// `package.provided` -- real `config.py:970-1027`'s `pprovideddict`,
    /// flattened: every profile level's own `package.provided` file
    /// (chain order) + the user-level
    /// `/etc/portage/profile/package.provided`, stacked with the same
    /// `stack_lists(incremental=1)` `-atom` removal `package.mask`/
    /// `packages` use, as a flat list of bare `cat/pkg-version` CPVs. A
    /// dependency atom (or a top-level target) that `match_from_list`
    /// against this list is treated as already satisfied and never
    /// pulled in (real `dep_check.py:1052` / `depgraph.py:5497-5615`;
    /// real portage cp-keys this into a dict, but `match_from_list`
    /// already filters by cp, so the flat list is equivalent). Each line
    /// should be a bare CPV -- real portage validates with `isvalidatom(
    /// "=" + line)` and drops invalid ones with a warning; portuale
    /// carries every stacked line through and lets `match_from_list`
    /// simply never match a malformed one (a documented simplification).
    /// The real EAPI 7+ gate (`allows_package_provided`, disallowed for
    /// EAPI 7+) is NOT ported: portuale tracks no per-profile-level
    /// EAPI, consistent with its "no EAPI parametrization within the 5+
    /// floor" precedent (and EAPI 5 -- what every fixture profile is --
    /// does allow it).
    pub package_provided: Vec<String>,
    /// Flags forced on by every profile level's own `use.force` file.
    /// Deliberately *not* folded into `use_flags` -- real config.py's
    /// own `regenerate()` applies this (combined with the atom-scoped
    /// `package.use.force`, via `setcpv()`'s own per-package
    /// `getUseForce(pkg)`) as the literal *last* step of its incremental
    /// USE walk, strictly after `package.use` -- so `portage-repo`'s own
    /// `effective_use_flags` applies this at that same relative
    /// position instead, alongside `package_use_force`. Exposed here too
    /// since real portage's own `forced_flags` (e.g. `--newuse`'s
    /// `reinstall_flags_for_newuse`) is `use.force ∪ use.mask`, not
    /// either one alone. See the module doc comment's `use.mask`/
    /// `use.force` bullet.
    pub use_force: HashSet<String>,
    /// Flags forced off by every profile level's own `use.mask` file.
    /// See `use_force`'s own doc comment for why this is deliberately
    /// *not* folded into `use_flags` either.
    pub use_mask: HashSet<String>,
    /// (atom-or-wildcard string, flag tokens) pairs from `package.use.force`
    /// -- repo-level (every repo, `::repo`-scoped) and every profile
    /// level's own file, flat-concatenated same as `package_use`. Real
    /// portage has no
    /// user-level source for this file at all (confirmed by reading
    /// `UseManager.__init__`'s own file/variable table) -- see the module
    /// doc comment's `package.use.mask`/`.force` bullet for the full
    /// scope writeup, including the deliberate simplifications portuale
    /// makes applying these per package.
    pub package_use_force: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, flag tokens) pairs from `package.use.mask`.
    /// See `package_use_force`'s own doc comment.
    pub package_use_mask: Vec<(String, Vec<String>)>,
    /// `USE_EXPAND` itself (PMS 7.3.4 profiles doc; real `const.py`'s own
    /// `INCREMENTALS` list): the set of variable NAMES (e.g. `VIDEO_CARDS`,
    /// `PYTHON_TARGETS`) accumulated incrementally across every profile
    /// level's own `make.defaults` plus `make.conf`, same mechanism `USE`/
    /// `ACCEPT_KEYWORDS` already use. Each named variable's own VALUE is
    /// expanded into lowercase-prefixed pseudo-USE-flags already folded
    /// into `use_flags` by the time `resolve_config` returns (e.g.
    /// `VIDEO_CARDS="nvidia"` contributes `video_cards_nvidia`) -- see
    /// the module doc comment's own `USE_EXPAND` bullet for the full
    /// scope writeup and its deliberate simplifications. Exposed here
    /// too (not just folded into `use_flags`) purely for
    /// documentation/testability, same reasoning `use_force`/`use_mask`
    /// already have for being separately visible.
    pub use_expand: HashSet<String>,
    /// `USE_EXPAND_UNPREFIXED` (real `config.py`'s own companion to
    /// `USE_EXPAND`): the set of variable NAMES (e.g. `ARCH` -- real
    /// Gentoo's own profile sets `USE_EXPAND_UNPREFIXED="ARCH"`, which is
    /// literally how `amd64`/`x86`/`arm64` etc. exist as plain USE flags
    /// at all) accumulated incrementally the same way `USE_EXPAND` is.
    /// Each named variable's own VALUE is folded into `use_flags`
    /// *without* any prefix at all (unlike `USE_EXPAND`'s own
    /// `lowercase(name)_` prefixing) -- see the module doc comment's own
    /// `USE_EXPAND_UNPREFIXED` bullet for the full scope writeup.
    /// Exposed here too, same reasoning `use_expand` already has.
    pub use_expand_unprefixed: HashSet<String>,
    /// `USE_EXPAND_IMPLICIT` (real `config.py`, an INCREMENTAL): the set
    /// of `USE_EXPAND`/`USE_EXPAND_UNPREFIXED` variable NAMES whose
    /// expanded flags count as *implicitly*-valid IUSE for every package
    /// even when unlisted in its own `IUSE` -- real Gentoo's own
    /// `profiles/base/make.defaults` sets `USE_EXPAND_IMPLICIT="ARCH
    /// ELIBC KERNEL USERLAND"`. Accumulated incrementally the same way
    /// `USE_EXPAND` is; drives `iuse_effective` below. Exposed for
    /// testability, same as `use_expand`.
    pub use_expand_implicit: HashSet<String>,
    /// `IUSE_IMPLICIT` (real `config.py`, an INCREMENTAL): literal flag
    /// names that are implicitly-valid IUSE for every package (real
    /// Gentoo: `prefix prefix-guest ...`). Accumulated incrementally;
    /// folded straight into `iuse_effective` below.
    pub iuse_implicit: HashSet<String>,
    /// Real EAPI 5+ `IUSE_EFFECTIVE` (`config.py::_calc_iuse_effective`):
    /// `iuse_implicit`, plus every value of each `USE_EXPAND_UNPREFIXED`
    /// variable that's also in `use_expand_implicit` (unprefixed), plus
    /// `lowercase(v)_<value>` for each `USE_EXPAND` variable `v` that's
    /// also in `use_expand_implicit` -- the values coming from
    /// `USE_EXPAND_VALUES_<v>`. This is exactly the extra domain real
    /// `pkg.iuse.is_valid_flag` grants an EAPI 5+ package on top of its
    /// own declared `IUSE`, so a USE-dep like `foo[elibc_glibc]` matches
    /// a `foo` that never lists `elibc_glibc` in `IUSE`. `USE_EXPAND_
    /// HIDDEN` is deliberately NOT part of this -- for EAPI 5+ it is a
    /// pure `emerge --info`/`-pv` USE-grouping *display* concern (real
    /// `_get_implicit_iuse` is the pre-EAPI-5 path). See
    /// `use_expand_hidden` below, which `emerge --pretend -v`'s own
    /// `USE_EXPAND` grouping honours.
    pub iuse_effective: HashSet<String>,
    /// `USE_EXPAND_HIDDEN` (real `config.py`, an INCREMENTAL): the subset
    /// of `USE_EXPAND` variable NAMES whose expanded flags real portage
    /// omits from `emerge --info`/`emerge -pv`'s own `USE_EXPAND` grouping
    /// (`output.py::map_to_use_expand`'s own `remove_hidden`). Accumulated
    /// incrementally the same way `USE_EXPAND` is. Display-only: never
    /// consulted by `iuse_effective`/visibility/dependency resolution --
    /// only `portage_repo`'s own `-pv` `USE=`-line grouping reads it.
    pub use_expand_hidden: HashSet<String>,
    /// `use.stable.force`: every profile level's own file (in chain
    /// order), stacked the same `-atom`-removal way `use_force` already
    /// is -- but, unlike `use_force`, deliberately NOT folded into
    /// `use_flags` here: real `getUseForce`'s own `pkg=None` (global)
    /// case never even looks at the stable variant at all (confirmed by
    /// reading it), since "stable" is inherently a per-candidate
    /// property (it depends on that candidate's own KEYWORDS) with no
    /// meaningful "global" value. `portage-repo`'s own `effective_use_flags`
    /// applies this conditionally, once it knows a specific candidate's
    /// own stability -- see that crate's own doc comment for the full
    /// `package.use.stable.mask`/`.force` scope writeup, including the
    /// deliberate simplification of not also adding the repo-level
    /// sourcing real per-package `getUseForce(pkg)` has for the
    /// *non-stable* global file (which portuale's own `use_force`
    /// never had either, profile-chain-only, so the stable variant stays
    /// consistent with it rather than gaining new capability the
    /// non-stable one lacks).
    pub use_stable_force: HashSet<String>,
    /// `use.stable.mask`. See `use_stable_force`'s own doc comment.
    pub use_stable_mask: HashSet<String>,
    /// `PORTAGE_ARCHLIST`: every profile level's own `arch.list` file,
    /// stacked in chain order with the same `-entry` removal semantics
    /// `package.mask` uses (real `config.py`'s own `grabfile` +
    /// `stack_lists(archlist, incremental=1)`, confirmed by reading it
    /// directly). Real `config.py`'s own `_get_implicit_iuse()` feeds
    /// this (plus the profile's own `ARCH`, `use.mask`/`use.force`, and
    /// literal `build`/`bootstrap`) into `IUSE_IMPLICIT`, which makes a
    /// flag like `x86` a *valid* (if not necessarily enabled) REQUIRED_USE
    /// reference even when a specific package's own `IUSE` never
    /// mentions it -- `portage-repo`'s own `resolve_pretend_graph` unions
    /// this into the `iuse_set` it hands `check_required_use`. See the
    /// module doc comment's own "implicit IUSE" bullet for the full scope
    /// writeup, including the deliberate simplification of not also
    /// modeling `USE_EXPAND_HIDDEN`-derived regex flags (`elibc_.*` etc.)
    /// -- a bigger, separate feature portuale doesn't otherwise model
    /// (no ELIBC/KERNEL/USERLAND support at all).
    pub archlist: HashSet<String>,
    /// (atom-or-wildcard string, flag tokens) pairs from
    /// `package.use.stable.force` -- repo-level (every repo, `::repo`-
    /// scoped) and every profile level's own file, flat-concatenated
    /// exactly like
    /// `package_use_force` already is (same no-user-level-source
    /// confirmation from `UseManager.__init__`'s own file/variable
    /// table). Applied by `portage-repo` conditionally, only for a
    /// candidate `effective_use_flags` has already determined to be
    /// stable -- see that crate's own doc comment.
    pub package_use_stable_force: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, flag tokens) pairs from
    /// `package.use.stable.mask`. See `package_use_stable_force`'s own
    /// doc comment.
    pub package_use_stable_mask: Vec<(String, Vec<String>)>,
    /// The same eight `use.*mask`/`use.*force` sources as the flat fields
    /// above, but kept **per profile level in chain order** (with the
    /// repo-level `package.use.*` entries as a synthetic leading level),
    /// so `portage-repo` can reproduce real `UseManager.getUseMask`/
    /// `getUseForce`'s per-level *interleaved* `stack_lists(incremental=1)`:
    /// a global `use.mask` `-flag` line at a later profile level cancels
    /// an earlier level's `package.use.mask` entry (and vice-versa), a
    /// cross-source cancellation the flat-union fields above structurally
    /// cannot express. The flat fields stay for the readers that only
    /// need the global-only / approximate result (`emerge --info`'s
    /// global `USE=`, the REQUIRED_USE valid-IUSE domain, `--newuse`'s
    /// forced-flags filter). See `portage-repo`'s `resolved_use_mask`.
    pub use_mask_force_levels: Vec<UseMaskForceLevel>,
    /// `license_groups` (PMS-adjacent; real `LicenseManager.
    /// _read_license_groups`): every profile level's own file plus the
    /// user-level `/etc/portage/license_groups`, each `<name> <license
    /// or @group ...>` line's own value list *extended* (not stacked/
    /// masked -- real code's own `setdefault(k, []).extend(v)`) onto
    /// whatever the same group name already has from an earlier source.
    /// Raw/unexpanded: a group's own value list may itself reference
    /// another `@group` recursively, resolved by `expand_license_token`
    /// (below) wherever a group is actually expanded -- once, here in
    /// `resolve_config`, for both `accept_license` and `package_license`
    /// (matching real portage's own `set_accept_license_str`/
    /// `_read_user_config`, which both call `expandLicenseTokens` eagerly
    /// at config-read time too, not per-candidate), not pre-flattened
    /// into this map itself, matching real `_expandLicenseToken`'s own
    /// recursive-with-cycle-guard shape exactly.
    pub license_groups: HashMap<String, Vec<String>>,
    /// `ACCEPT_LICENSE`, already `@group`-expanded (see `license_groups`)
    /// but still symbolic -- `*`, `-*`, and `-license` tokens are kept
    /// literally, not resolved into a concrete license set here, since
    /// real portage's own `*`/`-*` resolution depends on a *specific
    /// candidate's own* LICENSE string (PMS 7.3.2), not anything known
    /// at config-resolution time (see `portage-repo`'s own per-candidate
    /// application). Deliberately last-level-wins (profile chain, then
    /// make.conf), not genuinely incremental across sources the way real
    /// portage's own `ACCEPT_LICENSE` is (`prune_incremental` over every
    /// source's own raw tokens) -- extends portuale's own pre-existing
    /// "any variable other than USE/ACCEPT_KEYWORDS is a plain last-
    /// level-wins scalar" cut (see the module doc comment) to this one
    /// too, rather than inventing a new, ACCEPT_LICENSE-specific
    /// incremental mechanism only this one variable would use. Real
    /// portage's own default when `ACCEPT_LICENSE` is never set *at all*
    /// (not even as an explicit empty override) anywhere in the whole
    /// chain -- `"* -@EULA"` -- is replicated exactly (`config.py`'s own
    /// `accept_license_str = " ".join(mysplit) or "* -@EULA"`).
    pub accept_license: Vec<String>,
    /// (atom-or-wildcard string, `@group`-expanded tokens) pairs from
    /// `package.license`, user-level only (real `LicenseManager.
    /// _read_user_config`'s own dominant path -- a profile can opt into
    /// also reading a profile-level `package.license` via its own
    /// `profile-license` profile-format marker, a rare, opt-in newer
    /// feature deliberately NOT replicated here, same "narrow, rare real
    /// sourcing variant" cut portuale already makes elsewhere).
    /// Real portage's own `*/*`-line "extract into the global
    /// `ACCEPT_LICENSE` instead of a real per-package entry" quirk
    /// (`extract_global_changes`) is deliberately NOT replicated either
    /// -- a `*/*` line here is just an ordinary (if unusual) per-package
    /// entry, matched via the same wildcard-atom machinery every other
    /// `*/*` entry in portuale already uses.
    pub package_license: Vec<(String, Vec<String>)>,
    /// `ACCEPT_PROPERTIES`, last-level-wins scalar (same reasoning as
    /// `accept_license`'s own doc comment -- real config.py's own
    /// comment: "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without
    /// groups"; no `@group` expansion exists for this variable at all,
    /// confirmed by reading `config.py`'s own `ACCEPT_PROPERTIES`
    /// handling, which never touches `LicenseManager`). Real portage's
    /// own default when unset anywhere -- `"*"` -- comes from
    /// `cnf/make.globals` (a real, always-sourced config layer this
    /// portuale doesn't model as an actual read file), so it's replicated
    /// here as a hardcoded fallback, the same "real default, ported
    /// without modeling the file it technically comes from" approach
    /// `accept_license`'s own `"* -@EULA"` already takes (there, the
    /// default is a genuine Python-level hardcoded fallback even in
    /// real portage itself, not read from any file at all -- a slightly
    /// different real mechanism arriving at the same portuale-side
    /// treatment).
    pub accept_properties: Vec<String>,
    /// (atom-or-wildcard string, raw tokens) pairs from
    /// `package.properties`, user-level only (real
    /// `LocationsManager.abs_user_config`, confirmed by reading
    /// `config.py`'s own `package.properties` read site -- no
    /// repo-level or profile-level source exists for this file at all).
    pub package_properties: Vec<(String, Vec<String>)>,
    /// `ACCEPT_RESTRICT`. See `accept_properties`'s own doc comment --
    /// identical real mechanism and default, just for `RESTRICT`
    /// instead of `PROPERTIES`.
    pub accept_restrict: Vec<String>,
    /// (atom-or-wildcard string, raw tokens) pairs from
    /// `package.accept_restrict`, user-level only. See
    /// `package_properties`'s own doc comment.
    pub package_accept_restrict: Vec<(String, Vec<String>)>,
    /// `PKGDIR` (`--usepkg`/`--usepkgonly`'s own binary-package
    /// directory, real `lib/portage/dbapi/bintree.py`'s own `pkgdir`):
    /// an ordinary, non-incremental `make.conf` scalar, read the same
    /// "last-level-wins" way every other such variable already is (see
    /// the module doc comment). Real default (`cnf/make.globals`,
    /// `PKGDIR="/var/cache/binpkgs"`) is replicated as a hardcoded
    /// fallback -- the same "real default, ported without modeling the
    /// file it technically comes from" precedent `accept_properties`'s
    /// own doc comment already established. `portage-repo`'s own
    /// `list_binary_candidates` reads `<pkgdir>/Packages` when
    /// `--usepkg`/`--usepkgonly` is given.
    pub pkgdir: String,
    /// Binary-package repositories (`--getbinpkg`/`--getbinpkgonly`), real
    /// `lib/portage/binrepo/config.py`'s own `BinRepoConfigLoader`: every
    /// `[section]` in `<config_root>/etc/portage/binrepos.conf`
    /// (`sync-uri = ...`, optional `priority = ...`), plus one implicit
    /// entry per whitespace-separated `PORTAGE_BINHOST` URI (real
    /// "Convert PORTAGE_BINHOST entries into implicit binrepos.conf ones",
    /// `name_fallback = md5(uri)`, incrementing priority). Sorted by
    /// `(priority or 0, name)`. Empty when neither is configured.
    /// `portage-repo`'s `list_remote_binary_candidates` reads each one's
    /// on-disk `Packages` index (see `BinRepo::packages_dir`).
    pub binrepos: Vec<BinRepo>,
    /// The `$PKGDIR` directory-scan fallback (real
    /// `bintree._populate_local`): when `<pkgdir>/Packages` is *absent*
    /// but `$PKGDIR` holds binpkg *files*, the CLI layer walks them,
    /// reads each file's own embedded metadata (`portuale`'s
    /// `binpkg::scan_pkgdir` -- real `xpak`/`gpkg`), and synthesizes one
    /// `Packages`-style entry per file here. `None` means "no scan was
    /// done" -- either `Packages` exists (use it) or `--usepkg`/
    /// `--usepkgonly` wasn't given (binary candidates aren't eligible
    /// anyway). `portage-repo` builds a `BinaryIndex` from this when set,
    /// or reads `<pkgdir>/Packages` when not. Not written back to disk --
    /// real portage caches it as `Packages`; portuale recomputes each
    /// run, so `--pretend` still writes nothing.
    pub scanned_binpkgs: Option<Vec<std::collections::HashMap<String, String>>>,
    /// Every non-USE/ACCEPT_KEYWORDS variable's final scalar value
    /// (last-level-wins across the profile chain + `make.conf`), e.g.
    /// `CHOST`, `CFLAGS`, `GENTOO_MIRRORS`, `CONFIG_PROTECT`,
    /// `PORTAGE_TMPDIR`. This is the `scalars` map `resolve_config`
    /// already builds for `${VAR}` substitution, exposed for
    /// `emerge --info`'s own variable dump.
    pub other_vars: HashMap<String, String>,
    /// Per-source token lists for the `const.INCREMENTALS` variables real
    /// portage stacks in `config.regenerate()` and stores as
    /// `" ".join(sorted(myflags))`, narrowed to the four `emerge --info`
    /// displays: `CONFIG_PROTECT`, `CONFIG_PROTECT_MASK`, `ENV_UNSET`,
    /// `FEATURES`. One `Vec<String>` per assignment seen, appended in
    /// real's own db-stack order (`env.d` / `/etc/profile.env`, then
    /// `make.globals`, then the profile chain, then `make.conf`, then the
    /// process env). [`Config::resolved_incremental`] folds them with
    /// `-*` / `-tok` semantics and sorts. `ACCEPT_KEYWORDS` (also an
    /// incremental) keeps its own dedicated `accept_keywords` set.
    pub incremental_sources: HashMap<String, Vec<Vec<String>>>,
}

impl Config {
    /// Real `config.regenerate()`'s incremental fold for one
    /// `const.INCREMENTALS` variable: stack every source's tokens in db
    /// order, `-*` clears everything so far, `-tok` discards, a bare (or
    /// `+`-prefixed) token adds, then `sorted(myflags)` (real
    /// `self.configlist[-1][mykey] = " ".join(sorted(myflags))`). Returns
    /// `None` only when the variable was never assigned anywhere.
    pub fn resolved_incremental(&self, key: &str) -> Option<Vec<String>> {
        let sources = self.incremental_sources.get(key)?;
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for tokens in sources {
            for tok in tokens {
                if tok == "-*" {
                    set.clear();
                } else if let Some(rest) = tok.strip_prefix('-') {
                    set.remove(rest);
                } else {
                    set.insert(tok.strip_prefix('+').unwrap_or(tok).to_string());
                }
            }
        }
        Some(set.into_iter().collect())
    }

    /// Whether `<eroot>/etc/profile.env` changed since this config was
    /// resolved (or last reloaded): the live file's text differs from the
    /// recorded [`Config::envd_raw`]. A missing file reads as `""`, so a
    /// deleted file counts as changed unless nothing was ever recorded.
    /// The per-package-task gate (`entry_build_env`): when this is false
    /// the caller keeps the old code path byte for byte.
    pub fn env_d_changed(&self, eroot: &Path) -> bool {
        std::fs::read_to_string(eroot.join("etc/profile.env")).unwrap_or_default() != self.envd_raw
    }

    /// Real `config.reload()` (`config.py:2691-2699`): re-read `env.d`
    /// and replace that layer, leaving every higher layer alone. For
    /// every re-read key a higher layer owns ([`Config::envd_higher_keys`])
    /// or the blacklist drops, `other_vars` is untouched; every other key
    /// is set, changed, or removed to match the new file. The env.d
    /// incremental layer per tracked key is replaced the same way, and
    /// `envd_sets_path`/`envd_use_tokens` are recomputed. Higher-layer
    /// expansions are deliberately not redone (they were computed against
    /// the old env.d at resolve time -- real's `expand=False` env.d read
    /// likewise never re-expands them). The USE-context fingerprint is
    /// refreshed so the `portage-repo` memos keyed on it see the new
    /// `envd_use_tokens`.
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
            // Nothing post-resolve mutates these lists, so a recorded
            // env.d layer is still index 0 (the lowest db-stack layer).
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
                // No recorded layer (or its trace is gone, which is
                // unreachable -- see above): the new env.d layer goes
                // first regardless.
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
