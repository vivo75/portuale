// Profile-chain + make.conf + package.mask/.unmask/.accept_keywords
// resolution for real USE/ACCEPT_KEYWORDS/visibility (see
// docs/agent-context.md's depgraph/config-resolution follow-up work, and
// docs/what-this-proves.md for the full scope writeup). Replaces the base
// `emerge --pretend` slice's hardcoded `ACCEPT_KEYWORDS="amd64"`/`USE=""`
// with the real mechanism: a profile inheritance chain (`make.profile` ->
// `parent` files) plus `/etc/portage/make.conf`, each level's
// `make.defaults` contributing incremental USE/ACCEPT_KEYWORDS tokens,
// plus per-package overrides from `/etc/portage/package.mask`,
// `package.unmask`, and `package.accept_keywords`.
//
// KNOWN, DOCUMENTED SCOPE CUTS (confirmed with the user before
// implementing):
//   - Cross-repo profile parent references (`reponame:path` syntax, a
//     bare `:path` too) ARE resolved -- see `expand_parent_colon`/
//     `repo_containing` -- and now correctly gated on the current
//     profile node's own repo declaring `profile-formats = portage-2`
//     in `metadata/layout.conf` (real `_config/LocationsManager.py:47`/
//     `259`'s own `_allow_parent_colon` frozenset). `resolve_config`
//     reads each repo's `layout.conf` directly (`repo_profile_formats`)
//     rather than threading a param through every call site. Real
//     portage's EAPI-conditional `profile-formats` *default* when the
//     key is absent (`portage-1`/`portage-1-compat`) isn't modeled --
//     absent just means "no `portage-2`".
//   - ARCH-based KEYWORDS-format validation is out of scope. Wildcard
//     `_*` IUSE-aware expansion (e.g. `linguas_*`) IS now done, but in
//     `portage_repo::effective_use_flags` (it needs a specific
//     candidate's own IUSE, which this global config layer doesn't
//     have) -- see that function's own `_*` block. **Now stale**:
//     `USE_EXPAND` itself
//     and `package.use`'s own `USE_EXPAND`-prefix shorthand (`VIDEO_CARDS:
//     nvidia` lines) used to be listed here too -- see the dedicated
//     `USE_EXPAND` bullet further below for the follow-up that closed
//     both.
//   - Real config.py's `USE_ORDER` is
//     `env:pkg:conf:defaults:pkginternal:features:repo:env.d`. This pilot
//     now models `repo` (repo-level `package.use` only, not repo
//     `make.defaults`), `pkginternal` (the ebuild's own IUSE `+`/`-`
//     defaults), `defaults` (profile `make.defaults` + profile
//     `package.use`), `conf` (make.conf), and `pkg` (user
//     `package.use`) -- see `Config::package_use{,_repo,_user}` and
//     `portage-repo::effective_use_flags`. Still absent: `env` (the
//     `$USE` env var, `/etc/portage/env`, `package.env`), `features`
//     (`FEATURES`-implied USE), and `env.d`.
//   - No line continuation / multi-line quoted values, and no trailing
//     `# comment` after a real assignment (a `#` is only recognized as a
//     comment when it starts the (trimmed) line). Real make.defaults/
//     make.conf files in practice don't rely on either.
//   - Any variable other than USE/ACCEPT_KEYWORDS is tracked only as a
//     plain scalar for `${VAR}` substitution purposes (last-value-wins,
//     no incremental merge), matching how it's actually used in real
//     profiles (e.g. ARCH feeding `ACCEPT_KEYWORDS="${ARCH}"`).
//   - `package.mask`/`.unmask` are stacked from all three real sources --
//     the main repo's own repo-level `profiles/package.mask`/`.unmask`,
//     every profile level's own pair (in chain order), and the
//     user-level `/etc/portage` files -- with `-atom` removal applying
//     across the whole combined stream, exactly matching real
//     `MaskManager.py`'s `stack_lists(incremental=1)` (see
//     `stack_mask_lines`). An *overlay* repo's own repo-level
//     `package.mask`/`.unmask` is now read too (every configured repo,
//     unconditionally, each auto-scoped to its own `::reponame` -- see
//     `scope_repo_mask_lines`/`resolve_config`'s own doc comment). An
//     overlay's own `package.mask` (only `package.mask` -- real portage
//     never consults masters for `package.unmask`) also now stacks with
//     its implicit `masters` default (the main repo alone, since this
//     pilot doesn't parse an explicit `masters =` repos.conf key). Still
//     out of scope: eclass inheritance via `masters`, and an explicit
//     `masters =` override/multi-master chain.
//   - `package.accept_keywords` is stacked from profile-chain (in chain
//     order) + user-level sources, mirroring real `KeywordsManager.
//     getPKeywords` exactly -- confirmed by reading it, there's no
//     repo-level source for this file in real portage at all (unlike
//     `package.mask`'s repo-level `profiles/package.mask`). Purely
//     additive, like the pilot's own pre-existing user-level-only
//     handling always was: no `-atom` removal exists for this file in
//     real portage either, so every matching source's keyword tokens are
//     just unioned together (see `is_visible`). A bare atom with no
//     keyword tokens now gets real `accept_keywords_defaults`'s own
//     implicit `~arch` meaning at *both* levels (see
//     `parse_package_accept_keywords_lines`'s own doc comment) --
//     confirmed by reading `KeywordsManager.__init__` itself: contrary
//     to this pilot's own earlier assumption, the *user*-level source
//     gets the identical substitution too (baked in at load time,
//     `self.pkeywordsdict`), not just the profile-level one
//     (`getPKeywords`'s own read-time substitution).
//   - `package.use` is stacked from all three real sources, and -- as of
//     the "Config depth" slice (SCOPE_BACKLOG Part 2.C) -- each source
//     now lands in its own `Config` field at its own real `USE_ORDER`
//     position, instead of the earlier flat "one incremental list
//     regardless of source" model:
//       * repo-level (`<repo>/profiles/package.use`, every configured
//         repo, overlays `::repo`-scoped via
//         `scope_repo_package_use_lines`) -> `Config::package_use_repo`,
//         real `configdict["repo"]` -- applied *before* the ebuild's own
//         IUSE `+`/`-` defaults, the weakest USE layer this pilot models.
//       * every profile level's own `package.use` (chain order) ->
//         `Config::package_use`, real `configdict["defaults"]` -- applied
//         after the profile `make.defaults` USE tokens, *before*
//         `make.conf`. (Real portage interleaves each level's
//         `package.use` right after that same level's `make.defaults`;
//         this pilot applies all profile `package.use` as one group after
//         all profile `make.defaults` -- a narrow simplification that
//         only matters when a child profile's `make.defaults` and a
//         parent profile's package-specific `package.use` disagree, which
//         essentially never happens.)
//       * user-level (`/etc/portage/package.use`) ->
//         `Config::package_use_user`, real `configdict["pkg"]` -- applied
//         after `make.conf`, the strongest USE layer before the final
//         `use.force`/`use.mask` step.
//     Still not modeled: the `env`/`features`/`env.d` `USE_ORDER` layers,
//     and repo-level `make.defaults` USE (real `_repo_make_defaults`,
//     folded into `configdict["repo"]` alongside repo `package.use`).
//     Each source is still purely additive within itself (no `-atom`
//     removal exists for this file -- see `parse_package_use_lines`), and
//     still applied per package (not globally): a matching entry's tokens
//     are layered on with the same incremental semantics as `USE` itself
//     (see `apply_incremental`), scoped to the one package being
//     resolved -- see `portage-repo::effective_use_flags` for the full
//     per-package `USE_ORDER` walk and `resolve_pretend_graph` for where
//     it is driven (it needs the candidate's SLOT to match slotted
//     entries, which only exists at that later, repo-aware layer).
//   - `packages` (`@system`'s real source -- `PackagesSystemSet` in
//     `lib/portage/_sets/profiles.py`) IS now read: every profile level's
//     own `<level>/packages` file, in chain order, stacked with the
//     identical `stack_lists(incremental=1)` semantics `package.mask`
//     already ports (see `stack_mask_lines`) -- confirmed by reading
//     `PackagesSystemSet.load`, which calls the exact same real
//     `stack_lists` function `MaskManager` does, on the *raw* lines
//     (`*foo` and plain `foo` alike -- `-foo` only ever removes an
//     earlier exact-text `foo`, never a `*foo`, same plain string
//     equality `stack_mask_lines` already uses). Only *after* stacking
//     does real portage keep the subset starting with `*` (stripping the
//     `*`) as the actual `@system` atom list -- every other stacked line
//     is a "this package is known to the profile but not part of the
//     base system" hint with no `@system`-set meaning of its own, so
//     `system_packages` applies that same post-stack filter. No
//     repo-level or user-level source exists for this file in real
//     portage at all -- confirmed by reading (only
//     `PackagesSystemSet.__init__`'s `profiles` list, never
//     `config_root`), unlike `package.mask`'s repo-level
//     `profiles/package.mask`.
//   - `use.mask`/`use.force` (global USE forcing) ARE now read: every
//     profile level's own `<level>/use.mask`/`use.force` file, in chain
//     order, stacked with the identical `stack_lists(incremental=True)`
//     semantics `package.mask`/`packages` already port (see
//     `stack_mask_lines`) -- confirmed by reading `UseManager.
//     getUseMask`/`getUseForce`'s own `pkg=None` case, the one real
//     `config.py`'s `regenerate()` actually calls to build the *global*
//     `USE` value this pilot's flat model corresponds to: it returns
//     `stack_lists(self._usemask_list/self._useforce_list,
//     incremental=True)` directly, never touching a repo-level or
//     per-package source at all here -- those only exist on the
//     *per-package* path (`pkg` not `None`), see the
//     `package.use.mask`/`.force` bullet further below for that follow-up.
//     Deliberately NOT folded into `use_flags` here at all (an earlier
//     version of this pilot did, which was wrong): real `regenerate()`
//     applies `self.useforce`/`self.usemask` (which `setcpv()` sets to
//     the *per-package* `getUseForce(pkg)`/`getUseMask(pkg)` -- global
//     `use.force`/`use.mask` combined with the atom-scoped
//     `package.use.force`/`.force`) as the literal *last* step of its
//     own incremental USE walk (`myflags.update(useforce)` followed by
//     `myflags.difference_update(usemask)`), strictly *after* the `pkg`
//     (`package.use`) tier -- so `portage-repo`'s own
//     `effective_use_flags` applies `use_force`/`use_mask` at that same
//     relative position instead, alongside the atom-scoped
//     `package_use_force`/`package_use_mask` it already positions
//     correctly (force-add first, THEN force-remove, so a flag listed in
//     both ends up masked, not forced). Exposed on `Config` directly
//     either way, since real portage's own `forced_flags` (consumed by
//     `--newuse`'s `reinstall_flags_for_newuse` in `portage-repo`,
//     previously always empty -- see that function's own doc comment)
//     is `use.force ∪ use.mask`, not either alone.
//   - `package.use.mask`/`package.use.force` (per-package USE forcing) ARE
//     now read too: repo-level (every repo, `::repo`-scoped the same way
//     `package.mask` scopes an overlay's own entries -- but with no
//     `masters`-merge step, unlike `package.mask`: real `UseManager.py`
//     never combines an overlay's own file with its master's own at load
//     time, only later, per-package, in `getUseMask`/`getUseForce`)
//     plus every profile level's own file, in chain order --
//     confirmed by reading
//     `UseManager.__init__`'s own file/variable table that there's no
//     user-level source for either file at all (unlike `package.use`
//     itself), so this pilot doesn't invent one. Stored flat, same as
//     `package_use`; `portage-repo`'s own `effective_use_flags` decides
//     which entries actually apply to a given candidate and in what
//     order -- see that crate's doc comment for the atom-specificity
//     algorithm this needed (real `ordered_by_atom_specificity`, ported)
//     and a deliberate simplification made along the way: comparison-
//     operator atoms lose real `best_match_to_list`'s "closest version
//     wins a tie" refinement, since real-world `package.use.mask`/
//     `.force` files essentially never use `>`/`<`/`>=`/`<=` atoms in
//     practice. The stable-vs-`~arch` KEYWORDS distinction named as a
//     cut here at the time this comment was written -- real portage's
//     own *separate* `use.stable.mask`/`.force`/`package.use.stable.
//     mask`/`.force` files and `_isStable` check -- is **now stale**:
//     see the `use.stable.mask`/`.force` bullet further below for the
//     follow-up that closed it.
//   - `USE_EXPAND` itself (PMS 7.3.4) IS now read too: the variable-NAME
//     list (e.g. `VIDEO_CARDS PYTHON_TARGETS`) accumulates incrementally
//     across the profile chain and `make.conf`, the same mechanism
//     `USE`/`ACCEPT_KEYWORDS` already use; each named variable's own
//     VALUE is read via this pilot's own pre-existing "last-level-wins,
//     no incremental merge" scalar mechanism (the same one `ARCH`
//     already uses -- real portage is genuinely per-variable-incremental
//     here, but this pilot's own blanket "no incremental merge outside
//     USE/ACCEPT_KEYWORDS" cut, confirmed above, already covers every
//     other such variable, so this just extends it rather than inventing
//     a new one) and expanded into lowercase-`varname_`-prefixed
//     pseudo-USE-flags folded directly into `use_flags`. `package.use`'s
//     own `USE_EXPAND`-prefix shorthand (`VIDEO_CARDS: nvidia` lines,
//     confirmed real-portage-user-level-only by reading
//     `UseManager.__init__` -- the repo-level/profile-level parsers go
//     through a different real function that never applies it) IS now
//     read too, which is why `package.use`'s own bullet above parses
//     repo-level/profile-level lines separately from user-level ones.
//     `USE_EXPAND_UNPREFIXED` (real `config.py`'s own companion to
//     `USE_EXPAND` -- a separate mode where the variable's own value
//     folds into `use_flags` with *no* prefix at all, not
//     `lowercase(name)_`) IS now read too, via the exact same
//     variable-NAME accumulation + last-level-wins scalar VALUE read:
//     real Gentoo's own `profiles/arch/amd64/make.defaults` sets
//     `USE_EXPAND_UNPREFIXED="ARCH"`, which is literally how `amd64`/
//     `x86`/`arm64` exist as plain USE flags at all -- there is no other
//     mechanism that defines them. See `Config::use_expand_unprefixed`'s
//     own field doc. `USE_EXPAND_IMPLICIT` + `IUSE_IMPLICIT` +
//     `USE_EXPAND_VALUES_<v>` are now read too, feeding the real EAPI
//     5+ `IUSE_EFFECTIVE` computation (`Config::iuse_effective`) -- a
//     candidate's `is_valid_flag` domain, not just an `emerge --info`
//     display concern. IUSE-aware `_*` wildcard expansion (`linguas_*`)
//     is done too, in `portage_repo::effective_use_flags` (see the `_*`
//     bullet near the top of this comment). `USE_EXPAND_HIDDEN` is read
//     too now (`Config::use_expand_hidden`) -- genuinely display-only
//     for EAPI 5+, honoured only by `emerge --pretend -v`'s own
//     `USE_EXPAND` grouping (`portage_repo`), never by resolution.
//   - `use.stable.mask`/`.force`/`package.use.stable.mask`/`.force`
//     (PMS 5+, always recognized here per this pilot's own "no EAPI
//     parametrization" precedent) ARE now read too, closing the
//     stable-vs-`~arch` cut named above: ported as `portage-repo`'s own
//     `is_stable`, grounded against real `KeywordsManager.isStable` --
//     genuinely more subtle than a raw "no `~` prefix" check (a
//     candidate counts as stable if replacing *every* one of its own
//     KEYWORDS with its `~`-prefixed unstable form would make it
//     invisible under the current `ACCEPT_KEYWORDS`/
//     `package.accept_keywords`). `use_stable_force`/`use_stable_mask`
//     stay separate `Config` fields rather than folding into
//     `use_flags` like `use_force`/`use_mask` do, since real
//     `getUseMask`/`getUseForce`'s own global (`pkg=None`) branch never
//     even looks at the stable variant at all -- "stable" is inherently
//     a per-candidate property with no meaningful global value, so
//     `portage-repo`'s own `effective_use_flags` applies these
//     conditionally instead, once it knows a specific candidate's own
//     stability. `use.stable.mask`/`.force` read from the profile chain
//     only (matching this pilot's own already-established profile-only
//     sourcing for the non-stable global files, not real per-package
//     `getUseMask`/`getUseForce`'s own additional repo-level source for
//     them, which this pilot's global mechanism never had either);
//     `package.use.stable.mask`/`.force` read repo-level (every repo,
//     `::repo`-scoped) plus profile-chain, mirroring
//     `package.use.mask`/`.force`'s own sourcing exactly, no user-level
//     source (same `UseManager.__init__` file/variable table
//     confirmation).
//
// One real, deliberately-preserved quirk from lib/portage/package/ebuild/
// config.py (see the comment above its `expand_map.pop("USE", None)`):
// `${VAR}` substitution persists across profile levels for every variable
// *except* USE, which is reset before each level's make.defaults is
// parsed. This stops a parent profile's accumulated USE from leaking into
// a child's own `USE="${USE} flag"` self-append. In the flat, single-set
// consumption model this pilot uses (no package.use interaction, unlike
// the real bug this quirk guards against), that particular scenario
// usually doesn't change the final *set* of enabled flags -- it's ported
// anyway for fidelity with the real algorithm, since it's cheap to do and
// it's what real portage actually does.

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    /// `+`/`-` defaults -- the weakest USE layer this pilot models.
    /// Tokens use the same `-flag`/`flag`/`+flag` incremental syntax as
    /// `USE` itself -- see `apply_incremental`.
    pub package_use_repo: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, raw USE tokens) pairs from every
    /// **profile** level's own `package.use` (chain order) -- real
    /// `configdict["defaults"]` (`_pkgprofileuse`). Applied by
    /// `effective_use_flags` after the profile `make.defaults` tokens
    /// ([`Config::use_tokens`]), before `make.conf`
    /// ([`Config::conf_use_tokens`]).
    pub package_use: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, raw USE tokens) pairs from the
    /// user-level `/etc/portage/package.use` -- real `configdict["pkg"]`
    /// (`_pusedict` / `getPUSE`). Applied by `effective_use_flags` after
    /// `make.conf`, the strongest USE layer before the final
    /// `use.force`/`use.mask` step. This is the only one of the three
    /// `package.use` sources that gets the `USE_EXPAND`-prefix shorthand
    /// (`VIDEO_CARDS: nvidia` lines) -- real portage's own user-only
    /// `extended_syntax`; see `parse_package_use_lines`.
    pub package_use_user: Vec<(String, Vec<String>)>,
    /// `@system`'s real atom source: every profile level's own `packages`
    /// file, stacked in chain order and filtered to `*`-prefixed lines
    /// (the `*` stripped) -- see the module doc comment's `packages`
    /// bullet and `PackagesSystemSet.load`.
    pub system_packages: Vec<String>,
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
    /// "=" + line)` and drops invalid ones with a warning; this pilot
    /// carries every stacked line through and lets `match_from_list`
    /// simply never match a malformed one (a documented simplification).
    /// The real EAPI 7+ gate (`allows_package_provided`, disallowed for
    /// EAPI 7+) is NOT ported: this pilot tracks no per-profile-level
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
    /// scope writeup, including the deliberate simplifications this pilot
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
    /// *non-stable* global file (which this pilot's own `use_force`
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
    /// -- a bigger, separate feature this pilot doesn't otherwise model
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
    /// source's own raw tokens) -- extends this pilot's own pre-existing
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
    /// sourcing variant" cut this pilot already makes elsewhere).
    /// Real portage's own `*/*`-line "extract into the global
    /// `ACCEPT_LICENSE` instead of a real per-package entry" quirk
    /// (`extract_global_changes`) is deliberately NOT replicated either
    /// -- a `*/*` line here is just an ordinary (if unusual) per-package
    /// entry, matched via the same wildcard-atom machinery every other
    /// `*/*` entry in this pilot already uses.
    pub package_license: Vec<(String, Vec<String>)>,
    /// `ACCEPT_PROPERTIES`, last-level-wins scalar (same reasoning as
    /// `accept_license`'s own doc comment -- real config.py's own
    /// comment: "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without
    /// groups"; no `@group` expansion exists for this variable at all,
    /// confirmed by reading `config.py`'s own `ACCEPT_PROPERTIES`
    /// handling, which never touches `LicenseManager`). Real portage's
    /// own default when unset anywhere -- `"*"` -- comes from
    /// `cnf/make.globals` (a real, always-sourced config layer this
    /// pilot doesn't model as an actual read file), so it's replicated
    /// here as a hardcoded fallback, the same "real default, ported
    /// without modeling the file it technically comes from" approach
    /// `accept_license`'s own `"* -@EULA"` already takes (there, the
    /// default is a genuine Python-level hardcoded fallback even in
    /// real portage itself, not read from any file at all -- a slightly
    /// different real mechanism arriving at the same pilot-side
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
    /// real portage caches it as `Packages`; this pilot recomputes each
    /// run, so `--pretend` still writes nothing.
    pub scanned_binpkgs: Option<Vec<std::collections::HashMap<String, String>>>,
    /// Every non-USE/ACCEPT_KEYWORDS variable's final scalar value
    /// (last-level-wins across the profile chain + `make.conf`), e.g.
    /// `CHOST`, `CFLAGS`, `GENTOO_MIRRORS`, `CONFIG_PROTECT`,
    /// `PORTAGE_TMPDIR`. This is the `scalars` map `resolve_config`
    /// already builds for `${VAR}` substitution, exposed for
    /// `emerge --info`'s own variable dump.
    pub other_vars: HashMap<String, String>,
}

/// One `--getbinpkg` binary-package repository (real
/// `portage.binrepo.config.BinRepoConfig`, narrowed to what a
/// `--pretend` resolution needs: no fetch/resume commands, no
/// signature-verification config, no `getbinpkg-exclude`/`-include` --
/// those gate an actual download, which `--pretend` never does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinRepo {
    /// The `[section]` name, or -- for an implicit `PORTAGE_BINHOST`
    /// entry -- the real `md5(uri)` hex-digest fallback.
    pub name: String,
    /// `sync-uri`, real `_normalize_uri` (trailing `/` stripped).
    pub sync_uri: String,
    /// `priority` (real `int(priority)`, or `0` on parse failure /
    /// absence -- real sorts a `None` priority as `0`).
    pub priority: i32,
}

impl BinRepo {
    /// The directory holding this binrepo's own `Packages` index on disk
    /// under `root` (real `bintree._populate_remote`'s own
    /// `pkgindex_file`, `bintree.py:1496-1504`):
    /// `<EROOT>/var/cache/edb/binhost/<host>/<url-path>/` for an
    /// `http(s)://`/`ssh://` `sync-uri`; the URI's own path for a
    /// `file://` URI or a bare filesystem path. `--pretend` never
    /// fetches, so a missing index just means "this binrepo contributes
    /// no candidates".
    pub fn packages_dir(&self, root: &std::path::Path) -> std::path::PathBuf {
        let uri = &self.sync_uri;
        if let Some(rest) = uri.strip_prefix("file://") {
            return std::path::PathBuf::from(rest);
        }
        for scheme in ["https://", "http://", "ssh://"] {
            if let Some(rest) = uri.strip_prefix(scheme) {
                let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
                let host = hostport.split(':').next().unwrap_or(hostport);
                return root
                    .join("var/cache/edb/binhost")
                    .join(host)
                    .join(path.trim_matches('/'));
            }
        }
        std::path::PathBuf::from(uri)
    }
}

fn var_ref_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap())
}

/// Substitutes `${VARNAME}` references: a name already set earlier in the
/// config wins, else the process environment (as bash sees it when it
/// sources `make.conf`), else empty (bash's default for an unset var).
/// The environment fallback is what lets a fixture write a relocatable
/// `PKGDIR="${PORTAGE_CONFIGROOT}/pkgdir"` instead of an absolute path.
fn substitute(value: &str, scalars: &HashMap<String, String>) -> String {
    var_ref_re()
        .replace_all(value, |caps: &regex::Captures| {
            scalars
                .get(&caps[1])
                .cloned()
                .or_else(|| std::env::var(&caps[1]).ok())
                .unwrap_or_default()
        })
        .into_owned()
}

/// Parses one `KEY="value"` / `KEY='value'` / `KEY=value` line. Returns
/// `None` for comments, blank lines, or anything that isn't a simple
/// assignment (conditionals, function defs, etc. -- out of scope; real
/// make.defaults/make.conf files don't use them).
fn parse_kv_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    if key.is_empty()
        || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || key.chars().next().unwrap().is_ascii_digit()
    {
        return None;
    }
    let mut value = line[eq + 1..].trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            value = &value[1..value.len() - 1];
        }
    }
    Some((key, value))
}

/// Applies real incremental-variable token semantics: `-*` clears
/// everything accumulated so far, `-flag` removes, `flag`/`+flag` adds
/// (a leading `+` is invalid per PMS but real config.py tolerates it by
/// stripping it, which this mirrors). Public so `portage-repo` can reuse
/// it to apply `package.use` tokens on top of a per-package clone of the
/// base USE set -- see the module doc comment.
pub fn apply_incremental(tokens: &str, set: &mut HashSet<String>) {
    for tok in tokens.split_whitespace() {
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
}

/// Config variables the pilot honours from the **process environment** --
/// real `config.regenerate()`'s `env` `USE_ORDER` layer, the
/// highest-priority config source (`ACCEPT_KEYWORDS=~amd64 emerge foo`,
/// `USE="-X" emerge bar`, …). Real portage passes nearly all of
/// `os.environ` through `backupenv`; this pilot uses a curated allowlist
/// instead -- it only reads a fixed set of config vars, and folding
/// `PATH`/`HOME`/… into `other_vars` would pollute `emerge --info`.
///
/// The first list is real `INCREMENTALS` the pilot models (stacked onto
/// the profile chain + `make.conf` via `apply_incremental`); the second
/// is plain last-wins scalars the pilot reads out of `scalars` /
/// `other_vars` later. `USE_EXPAND` *variable values* (`VIDEO_CARDS=…`)
/// are handled separately, in the expansion loop, once their names are
/// known.
const ENV_INCREMENTAL_VARS: &[&str] = &[
    "USE",
    "ACCEPT_KEYWORDS",
    "USE_EXPAND",
    "USE_EXPAND_UNPREFIXED",
    "USE_EXPAND_IMPLICIT",
    "USE_EXPAND_HIDDEN",
    "IUSE_IMPLICIT",
];
const ENV_SCALAR_VARS: &[&str] = &[
    "ACCEPT_LICENSE",
    "ACCEPT_PROPERTIES",
    "ACCEPT_RESTRICT",
    "PKGDIR",
    "DISTDIR",
    "PORTAGE_TMPDIR",
    "PORTAGE_LOGDIR",
    "PORTAGE_BINHOST",
    "PORTAGE_NICENESS",
    "PORTAGE_IONICE_COMMAND",
    "PORTAGE_ELOG_SYSTEM",
    "PORTAGE_ELOG_CLASSES",
    "PORTAGE_ELOG_MAILURI",
    "FEATURES",
    "CHOST",
    "CBUILD",
    "CTARGET",
    "CFLAGS",
    "CXXFLAGS",
    "CPPFLAGS",
    "LDFLAGS",
    "FFLAGS",
    "FCFLAGS",
    "MAKEOPTS",
    "EMERGE_DEFAULT_OPTS",
    "PORTAGE_RSYNC_EXTRA_OPTS",
    "GENTOO_MIRRORS",
];

/// Apply the `env` `USE_ORDER` layer: for every allowlisted config var
/// present in the process environment, override (scalars) or stack
/// (incrementals) on top of the profile chain + `make.conf`. Called once,
/// right after `make.conf` -- the same position real `regenerate()`
/// gives the env layer (after `conf`, before the final USE calculation).
fn apply_env_layer(scalars: &mut HashMap<String, String>, config: &mut Config) {
    for &name in ENV_INCREMENTAL_VARS {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        match name {
            "USE" => {
                apply_incremental(&value, &mut config.use_flags);
                // Real `USE_ORDER` puts `env` above `pkg` (user
                // `package.use`); this pilot lands env `USE` at the
                // `conf` layer instead (`conf_use_tokens`, applied after
                // `defaults` + profile/repo `package.use`, before the
                // user-level `package.use`) -- a documented narrowing of
                // the existing "`package.use` applied as one group" cut.
                // The global `config.use_flags` above is still env-last
                // (correct) for every package with no `package.use` entry.
                config.conf_use_tokens.push(value.clone());
            }
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            "USE_EXPAND" => apply_incremental(&value, &mut config.use_expand),
            "USE_EXPAND_UNPREFIXED" => apply_incremental(&value, &mut config.use_expand_unprefixed),
            "USE_EXPAND_IMPLICIT" => apply_incremental(&value, &mut config.use_expand_implicit),
            "USE_EXPAND_HIDDEN" => apply_incremental(&value, &mut config.use_expand_hidden),
            "IUSE_IMPLICIT" => apply_incremental(&value, &mut config.iuse_implicit),
            _ => {}
        }
        scalars.insert(name.to_string(), value);
    }
    for &name in ENV_SCALAR_VARS {
        if let Ok(value) = std::env::var(name) {
            scalars.insert(name.to_string(), value);
        }
    }
}

/// Processes one file's lines against the shared scalar/USE/ACCEPT_KEYWORDS
/// state, without any `source` support (used for make.defaults; make.conf
/// wraps this with `source` handling -- see `process_make_conf_file`).
fn process_lines(text: &str, scalars: &mut HashMap<String, String>, config: &mut Config) {
    for line in text.lines() {
        let Some((key, raw_value)) = parse_kv_line(line) else {
            continue;
        };
        let value = substitute(raw_value, scalars);
        match key {
            "USE" => {
                apply_incremental(&value, &mut config.use_flags);
                config.use_tokens.push(value.clone());
            }
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            "USE_EXPAND" => apply_incremental(&value, &mut config.use_expand),
            "USE_EXPAND_UNPREFIXED" => apply_incremental(&value, &mut config.use_expand_unprefixed),
            "USE_EXPAND_IMPLICIT" => apply_incremental(&value, &mut config.use_expand_implicit),
            "IUSE_IMPLICIT" => apply_incremental(&value, &mut config.iuse_implicit),
            "USE_EXPAND_HIDDEN" => apply_incremental(&value, &mut config.use_expand_hidden),
            _ => {}
        }
        scalars.insert(key.to_string(), value);
    }
}

fn read_parent_lines(profile_dir: &Path) -> Result<Vec<String>, String> {
    let parent_path = profile_dir.join("parent");
    if !parent_path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&parent_path)
        .map_err(|e| format!("reading {}: {e}", parent_path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}

/// Finds which of `repos` (each `(name, location)`) `dir` lives inside,
/// via the longest matching location prefix -- mirrors real
/// `LocationsManager._addProfile`'s own `intersecting_repos`/
/// `max(key=len)` logic, needed to resolve a same-repo `:path` profile
/// parent shorthand. Repo locations are canonicalized before comparing
/// (falling back to the raw path if that fails, e.g. a repo location
/// that doesn't exist on disk in a test fixture -- it simply won't match
/// any real profile dir either way), since `dir` itself is always
/// already canonicalized by `visit_profile`.
/// The `profile-formats` token list from a repo's own
/// `metadata/layout.conf` (real `parse_layout_conf`), or an empty list
/// when the file/key is absent. A tiny dedicated reader: `portage-profile`
/// can't depend on `portage-repo` (where the fuller `parse_layout_conf`
/// lives), and this is the only `layout.conf` key this crate needs --
/// the same "duplicate a small, stable chunk rather than share across a
/// dependency edge that only runs one way" precedent `keyword_masked_
/// only`/`is_visible` already set in `portage-repo`.
fn repo_profile_formats(repo_location: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(repo_location.join("metadata/layout.conf")) else {
        return Vec::new();
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "profile-formats" {
                return value.split_whitespace().map(String::from).collect();
            }
        }
    }
    Vec::new()
}

fn repo_containing(dir: &Path, repos: &[(String, PathBuf)]) -> Option<(String, PathBuf)> {
    repos
        .iter()
        .filter_map(|(name, loc)| {
            let canon_loc = loc.canonicalize().unwrap_or_else(|_| loc.clone());
            dir.starts_with(&canon_loc)
                .then_some((name.clone(), canon_loc))
        })
        .max_by_key(|(_, loc)| loc.as_os_str().len())
}

/// Expands a profile `parent` file line's real cross-repo `:path`/
/// `reponame:path` syntax (`LocationsManager._expand_parent_colon`):
/// a `:` with nothing before it means "this same repo" (`current_repo`),
/// anything else before the `:` is another repo's own name, looked up in
/// `repos`. Both forms expand to `<repo_location>/profiles/<rest>`. A
/// line with no `:` at all (the plain relative-path form) is returned
/// unchanged. Real portage only allows this syntax when the *current*
/// profile node's own repo declares `profile-formats = portage-2` in
/// `layout.conf` (real `_config/LocationsManager.py:47`/`259`'s own
/// `_allow_parent_colon = frozenset(["portage-2"])` gate) --
/// `parent_colon_repos` is the set of repo names that do (built from
/// `portage_repo::RepoConfig::profile_formats` at the CLI boundary). A
/// `parent` line containing a `:` whose current repo isn't in that set
/// is returned *unchanged*, matching real portage: the `:` then becomes
/// a literal path component that fails to resolve downstream exactly as
/// it would there.
fn expand_parent_colon(
    parent: &str,
    current_repo: Option<&(String, PathBuf)>,
    repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    parents_file: &Path,
    parent_colon_repos: &HashSet<String>,
) -> Result<String, String> {
    let Some(colon) = parent.find(':') else {
        return Ok(parent.to_string());
    };
    // Real `if not abs_parent and allow_parent_colon:` gate
    // (`_config/LocationsManager.py:207`/`259`): `allow_parent_colon`
    // defaults `True` and is only *overridden* when the current profile
    // node intersects a known repo -- so a node outside any repo keeps
    // the permissive default, and a node inside a repo is gated on that
    // repo declaring `portage-2` in `layout.conf`.
    let allowed = match current_repo {
        None => true,
        Some((name, _)) => parent_colon_repos.contains(name),
    };
    if !allowed {
        return Ok(parent.to_string());
    }
    let repo_loc = if colon == 0 {
        &current_repo
            .ok_or_else(|| {
                format!(
                    "parent {parent:?} not found: {} (not inside any known repo)",
                    parents_file.display()
                )
            })?
            .1
    } else {
        let repo_name = &parent[..colon];
        // Real `repositories.get_location_for_name`: canonical names and
        // aliases share one namespace (an alias never shadows a
        // canonical name -- real `config.py`'s own alias-registration
        // loop skips an already-taken name), so check `repos` first,
        // then `repo_aliases`.
        repos
            .iter()
            .chain(repo_aliases.iter())
            .find(|(name, _)| name == repo_name)
            .map(|(_, loc)| loc)
            .ok_or_else(|| {
                format!(
                    "parent {parent:?} not found: {} (no repo named {repo_name:?})",
                    parents_file.display()
                )
            })?
    };
    Ok(repo_loc
        .join("profiles")
        .join(&parent[colon + 1..])
        .to_string_lossy()
        .into_owned())
}

/// Recursively resolves the profile inheritance chain starting at `leaf`,
/// ancestors before descendants (parents listed in a level's `parent`
/// file are visited in the order given), cycle/diamond-safe via a visited
/// set keyed on the canonicalized directory. `repos` (main + every
/// overlay, name + location) is only needed to resolve a cross-repo
/// `parent` entry (see `expand_parent_colon`); a chain with none never
/// consults it.
fn resolve_profile_chain(
    leaf: &Path,
    repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    parent_colon_repos: &HashSet<String>,
) -> Result<Vec<PathBuf>, String> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut chain: Vec<PathBuf> = Vec::new();
    visit_profile(
        leaf,
        repos,
        repo_aliases,
        parent_colon_repos,
        &mut visited,
        &mut chain,
    )?;
    Ok(chain)
}

fn visit_profile(
    dir: &Path,
    repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    parent_colon_repos: &HashSet<String>,
    visited: &mut HashSet<PathBuf>,
    chain: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let canon = dir
        .canonicalize()
        .map_err(|e| format!("resolving profile {}: {e}", dir.display()))?;
    if !visited.insert(canon.clone()) {
        return Ok(());
    }
    let current_repo = repo_containing(&canon, repos);
    let parents_file = canon.join("parent");
    for parent in read_parent_lines(&canon)? {
        let expanded = expand_parent_colon(
            &parent,
            current_repo.as_ref(),
            repos,
            repo_aliases,
            &parents_file,
            parent_colon_repos,
        )?;
        visit_profile(
            &canon.join(&expanded),
            repos,
            repo_aliases,
            parent_colon_repos,
            visited,
            chain,
        )?;
    }
    chain.push(canon);
    Ok(())
}

/// Resolves `source <path>` directives against `config_root` as if it
/// were `/` (chroot-style), matching PORTAGE_CONFIGROOT/ROOT semantics
/// elsewhere in this pilot -- an absolute `source /etc/make.local` reads
/// `<config_root>/etc/make.local`, not the real host path. A missing
/// sourced file is silently skipped (lenient default; real bash would
/// error, but no fixture or real usage in this pilot relies on that).
fn process_make_conf_file(
    path: &Path,
    config_root: &Path,
    scalars: &mut HashMap<String, String>,
    config: &mut Config,
    visited_sources: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let canon = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => return Ok(()), // missing file: lenient no-op, see doc comment
    };
    if !visited_sources.insert(canon.clone()) {
        return Ok(());
    }
    let text =
        fs::read_to_string(&canon).map_err(|e| format!("reading {}: {e}", canon.display()))?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("source ") {
            let sourced = rest.trim();
            let sourced_path = Path::new(sourced);
            let resolved = if sourced_path.is_absolute() {
                config_root.join(sourced_path.strip_prefix("/").unwrap_or(sourced_path))
            } else {
                canon
                    .parent()
                    .map(|p| p.join(sourced_path))
                    .unwrap_or_else(|| sourced_path.to_path_buf())
            };
            process_make_conf_file(&resolved, config_root, scalars, config, visited_sources)?;
            continue;
        }
        let Some((key, raw_value)) = parse_kv_line(trimmed) else {
            continue;
        };
        let value = substitute(raw_value, scalars);
        match key {
            "USE" => {
                apply_incremental(&value, &mut config.use_flags);
                config.conf_use_tokens.push(value.clone());
            }
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            "USE_EXPAND" => apply_incremental(&value, &mut config.use_expand),
            "USE_EXPAND_UNPREFIXED" => apply_incremental(&value, &mut config.use_expand_unprefixed),
            "USE_EXPAND_IMPLICIT" => apply_incremental(&value, &mut config.use_expand_implicit),
            "IUSE_IMPLICIT" => apply_incremental(&value, &mut config.iuse_implicit),
            "USE_EXPAND_HIDDEN" => apply_incremental(&value, &mut config.use_expand_hidden),
            _ => {}
        }
        scalars.insert(key.to_string(), value);
    }
    Ok(())
}

/// Reads every non-comment, non-blank, trimmed line from `path`, which
/// may be a single file or (like `repos.conf` elsewhere in this pilot) a
/// directory of files merged in sorted-filename order. A missing path
/// yields an empty list, not an error.
fn read_config_lines(path: &Path) -> Result<Vec<String>, String> {
    fn read_file_lines(path: &Path) -> Result<Vec<String>, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect())
    }

    let mut lines = Vec::new();
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for entry in entries {
            lines.extend(read_file_lines(&entry)?);
        }
    } else if path.is_file() {
        lines.extend(read_file_lines(path)?);
    }
    Ok(lines)
}

/// Scopes each of `lines` (raw `package.mask`/`.unmask` entries, a
/// leading `-` meaning removal) to `repo_name` by appending a `::name`
/// suffix to the atom portion -- real `append_repo`'s own "atoms
/// without an explicit repo part get one; atoms that already have one
/// are left alone" rule (`lib/portage/util/__init__.py`), applied here
/// to an overlay's own repo-level entries so they can never silently
/// mask/unmask a same-named package in a *different* repo. A leading
/// `-` (removal) is preserved ahead of the atom, not swallowed into it
/// -- `-cat/pkg` scopes to `-cat/pkg::name`, matching real portage's own
/// behavior of scoping a removal atom exactly like an addition one, so
/// it can only ever cancel a same-repo-scoped entry.
fn scope_repo_mask_lines(lines: &[String], repo_name: &str) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let (prefix, atom) = match line.strip_prefix('-') {
                Some(rest) => ("-", rest),
                None => ("", line.as_str()),
            };
            if atom.contains("::") {
                line.clone()
            } else {
                format!("{prefix}{atom}::{repo_name}")
            }
        })
        .collect()
}

/// Same real "add `::repo` to every atom without one" rule as
/// `scope_repo_mask_lines`, applied to the `package.use`/`.mask`/`.force`/
/// `.stable.mask`/`.stable.force` line shape instead
/// (`<atom> <flag> <flag> ...`, see `parse_package_use_lines`): only the
/// leading atom token gets scoped, the flag tokens after it are passed
/// through untouched. Unlike `package.mask`/`.unmask`, none of these
/// files has `-atom` whole-entry removal syntax (confirmed by
/// `parse_package_use_lines`'s own doc comment: real portage only ever
/// `.extend()`s these, and a leading `-` inside the flag list masks/
/// forces a single flag off, not an atom) -- so there's no leading-`-`
/// case to preserve here, unlike `scope_repo_mask_lines`.
fn scope_repo_package_use_lines(lines: &[String], repo_name: &str) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let (atom, rest) = match line.split_once(char::is_whitespace) {
                Some((a, r)) => (a, r.trim_start()),
                None => (line.as_str(), ""),
            };
            let scoped_atom = if atom.contains("::") {
                atom.to_string()
            } else {
                format!("{atom}::{repo_name}")
            };
            if rest.is_empty() {
                scoped_atom
            } else {
                format!("{scoped_atom} {rest}")
            }
        })
        .collect()
}

/// Stacks ordered `package.mask`/`.unmask` lines from multiple sources
/// (earlier sources first) with real portage's own `-atom` removal
/// semantics -- see `MaskManager.py`'s `stack_lists(incremental=1)`: a
/// `-atom` line removes the exact matching atom text added by ANY
/// earlier source in this same stack, not just within its own source
/// (e.g. a user-level `-atom` in `package.mask` can remove an atom the
/// repo or a profile level added). Shared between `package.mask` and
/// `package.unmask`, which real portage stacks identically -- unlike
/// this pilot's previous, user-level-only `package.unmask` handling,
/// which treated a leading `-` there as meaningless; it's meaningful
/// once more than one source can contribute an unmask entry.
///
/// Ports real `stack_lists`'s own `ignore_repo=True` behavior (the flag
/// real `MaskManager.__init__` always passes for its own final
/// `[repo_pkgmasklines, profile_pkgmasklines, user_pkgmasklines]`
/// combination -- confirmed by reading it directly): "let `-cat/pkg`
/// remove `cat/pkg::repo`" -- an unscoped removal token (no `::` of its
/// own, which is all a profile-level or user-level `-atom` can ever be,
/// since only repo-level entries ever get `::repo`-scoped at all, see
/// `scope_repo_mask_lines`) strips any `::repo` suffix off every
/// existing entry before comparing, so it cancels a repo-scoped atom
/// from *any* repo, not just an identically-unscoped one -- without
/// this, a profile's `-dev-libs/foo` could never again cancel the main
/// repo's own (now `::reponame`-scoped) `dev-libs/foo` mask entry, a
/// real regression the main-repo-scoping follow-up above would
/// otherwise have introduced. A removal token that's already `::repo`-
/// scoped itself (rare -- only possible if a user writes one by hand)
/// keeps exact-match semantics instead, matching real `stack_lists`'s
/// own `"::" not in token` guard exactly.
fn stack_mask_lines(sources: &[Vec<String>]) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for lines in sources {
        for line in lines {
            match line.strip_prefix('-') {
                Some(removed) if removed.contains("::") => {
                    list.retain(|x| x != removed);
                }
                Some(removed) => {
                    list.retain(|x| {
                        x.split_once("::").map_or(x.as_str(), |(atom, _)| atom) != removed
                    });
                }
                None => list.push(line.clone()),
            }
        }
    }
    list
}

/// `package.accept_keywords`: each line is `<atom-or-wildcard>
/// <keyword...>`. A line with no keyword tokens after the atom is kept
/// here with an *empty* token list, not dropped -- real portage gives a
/// bare atom an implicit `~arch` meaning at *both* levels, confirmed by
/// reading `KeywordsManager.__init__` (the user-level source: a bare
/// `pkgdict` entry's value is replaced with `accept_keywords_defaults`
/// right there, before it's ever stored in `self.pkeywordsdict`) and
/// `getPKeywords` (the profile-level source: the identical substitution
/// happens at read time instead, per matching entry) -- the same
/// `accept_keywords_defaults` formula either way: `"~" + keyword` for
/// each *plain* (non-`~`/`-`-prefixed) token in the *current* global
/// `ACCEPT_KEYWORDS`. `resolve_config` fills in the actual defaults
/// once `config.accept_keywords` is final (see its own call site) --
/// this function only preserves the bare atom itself so that
/// substitution has something to act on; every other caller of this
/// shape (`parse_package_license_lines` below) filters bare atoms back
/// out immediately after, since no other file gets this treatment.
fn parse_package_accept_keywords_lines(lines: &[String]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(atom) = parts.next() else {
            continue;
        };
        let keywords: Vec<String> = parts.map(String::from).collect();
        result.push((atom.to_string(), keywords));
    }
    result
}

/// `package.license`/`package.properties`/`package.accept_restrict`:
/// each line is `<atom-or-wildcard> <token...>`. Same shape as
/// `package.accept_keywords`, reused directly for all three real files
/// (unlike `parse_package_use_lines`'s own deliberate separateness from
/// `parse_package_accept_keywords_lines`, which exists only because of
/// `package.use`'s own `USE_EXPAND`-shorthand parameter -- these three
/// have no such per-file divergence to justify three near-identical
/// wrapper functions) -- except for a bare atom's own meaning: none of
/// these three files gets `package.accept_keywords`'s own implicit
/// `~arch`-default treatment in real portage, so a bare atom is filtered
/// back out here as a genuine no-op, unlike the shared parser's own
/// bare-atom-preserving behavior.
fn parse_package_license_lines(lines: &[String]) -> Vec<(String, Vec<String>)> {
    parse_package_accept_keywords_lines(lines)
        .into_iter()
        .filter(|(_, tokens)| !tokens.is_empty())
        .collect()
}

/// `license_groups` (real `LicenseManager._read_license_groups`): each
/// non-comment, non-blank line is `<group-name> <license or @group
/// ...>` (`grabdict` format -- no `-atom`/removal semantics at all,
/// unlike `package.mask`'s own file format). Later sources *extend*
/// (never replace or stack-remove) whatever the same group name already
/// has, matching real `self._license_groups.setdefault(k, []).extend(v)`
/// exactly.
fn parse_license_groups_lines(lines: &[String]) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        groups
            .entry(name.to_string())
            .or_default()
            .extend(parts.map(String::from));
    }
    groups
}

/// Expands a single `ACCEPT_LICENSE`/`package.license` token against
/// `groups`: a plain license name (or `*`/`-*`, real portage's own
/// symbolic wildcard tokens) passes through unchanged; an `@group-name`
/// token (optionally `-`-negated) expands to every one of that group's
/// own members, each recursively expanded the same way (a member that's
/// itself a nested `@group` reference, real portage's own
/// `_expandLicenseToken` "elif license_group:" recursion) -- negation
/// applies to every expanded member, not just the group reference
/// itself (`-@EULA` with `EULA = "Eula1 Eula2"` expands to `-Eula1
/// -Eula2`, matching real portage's own tail `if negate: rValue = ["-" +
/// token for token in rValue]`). `traversed` guards against a circular
/// group reference (matching real portage's own cycle guard): a group
/// already being expanded higher up this same call stack is left as its
/// own literal `@group-name` text instead of recursing infinitely, same
/// for a genuinely undefined group name (never declared in any
/// `license_groups` file at all) -- both deliberately silent here (no
/// stderr warning), unlike real portage's own `writemsg`, since this
/// pilot has no precedent for emitting warnings to stderr from deep
/// inside config resolution (every other real portage warning-only path
/// in this pilot is silently skipped the same way).
fn expand_license_token(token: &str, groups: &HashMap<String, Vec<String>>) -> Vec<String> {
    fn expand(
        token: &str,
        groups: &HashMap<String, Vec<String>>,
        traversed: &mut HashSet<String>,
    ) -> Vec<String> {
        let (negate, license_name) = match token.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, token),
        };
        let Some(group_name) = license_name.strip_prefix('@') else {
            return vec![token.to_string()];
        };
        let mut result: Vec<String> = if traversed.contains(group_name) {
            vec![format!("@{group_name}")]
        } else if let Some(members) = groups.get(group_name) {
            traversed.insert(group_name.to_string());
            let mut out = Vec::new();
            for member in members {
                // Real portage: "Skipping invalid element %s in license
                // group '%s'" -- a group's own member list is never
                // itself allowed to contain a "-"-negated entry.
                if !member.starts_with('-') {
                    out.extend(expand(member, groups, traversed));
                }
            }
            traversed.remove(group_name);
            out
        } else {
            vec![format!("@{group_name}")]
        };
        if negate {
            result = result.iter().map(|t| format!("-{t}")).collect();
        }
        result
    }
    expand(token, groups, &mut HashSet::new())
}

/// Expands every token in `tokens` against `groups`, in order -- see
/// `expand_license_token`'s own doc comment. Mirrors real
/// `LicenseManager.expandLicenseTokens` exactly.
fn expand_license_tokens(tokens: &[String], groups: &HashMap<String, Vec<String>>) -> Vec<String> {
    tokens
        .iter()
        .flat_map(|t| expand_license_token(t, groups))
        .collect()
}

/// `package.use`: each line is `<atom-or-wildcard> <use-token...>`. A line
/// with no tokens after the atom is a documented no-op, matching
/// `parse_package_accept_keywords_lines`. Purely additive across sources,
/// like `package.accept_keywords` and unlike `package.mask`/`.unmask`:
/// real portage's own `package.use` consumption (`config.py`'s
/// `regenerate` -- see the module doc comment on `USE_ORDER`) only ever
/// `.extend()`s a growing token list per source, never removes a
/// previous entry, so there's no `-atom` semantics to port here at all.
///
/// `use_expand_shorthand`, when true, ports real
/// `UseManager._parse_user_files_to_extatomdict`'s own `VIDEO_CARDS:
/// nvidia intel` syntax: a token ending in `:` sets a
/// `lowercase(name) + "_"` prefix applied to every following token on
/// *that same line* (a leading `-` stays outside the new prefix, e.g.
/// `-flag` becomes `-video_cards_flag`, not `video_cards_-flag`) --
/// reset back to none at the start of every line, confirmed by reading
/// real `grabdict_package`'s own `newlines=1` marker handling (a fresh
/// `"\n"` token is inserted between every physical line for the same
/// atom, and the real code's own loop resets its prefix on each one).
/// Callers pass `false` for repo-level/profile-level lines: confirmed by
/// reading `UseManager.__init__`, only the *user*-level source
/// (`_parse_user_files_to_extatomdict`) ever applies this shorthand at
/// all -- `_parse_repository_files_to_dict_of_dicts`/
/// `_parse_profile_files_to_tuple_of_dicts` both go through
/// `_parse_file_to_dict` instead, which never passes `newlines=1` and
/// has no such expansion step, so a `VIDEO_CARDS:` token in a
/// repo-level or profile-level `package.use` file is (in real portage)
/// just a literal, almost-certainly-invalid USE token, not shorthand at
/// all. This is genuine real behavior, not a pilot-invented
/// restriction -- see `resolve_config`'s own call sites.
fn parse_package_use_lines(
    lines: &[String],
    use_expand_shorthand: bool,
) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(atom) = parts.next() else {
            continue;
        };
        let mut prefix = String::new();
        let mut tokens: Vec<String> = Vec::new();
        for tok in parts {
            if use_expand_shorthand {
                if let Some(name) = tok.strip_suffix(':') {
                    prefix = format!("{}_", name.to_lowercase());
                    continue;
                }
            }
            if prefix.is_empty() {
                tokens.push(tok.to_string());
            } else if let Some(rest) = tok.strip_prefix('-') {
                tokens.push(format!("-{prefix}{rest}"));
            } else {
                tokens.push(format!("{prefix}{tok}"));
            }
        }
        if tokens.is_empty() {
            continue;
        }
        result.push((atom.to_string(), tokens));
    }
    result
}

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
/// stay unscoped, matching this pilot's own pre-existing (unchanged)
/// behavior -- real portage scopes *every* repo's own repo-level
/// entries this same way, including the main repo's, so a `package.mask`
/// entry from main only masking main's own packages (not an
/// identically-named overlay package) is a separate, distinct
/// correctness question this slice doesn't also take on. Real
/// `masters` (each repo's own `package.mask` -- and *only*
/// `package.mask`, real `MaskManager.py`'s own `package.unmask` loop
/// never consults masters at all -- stacks with its declared masters'
/// own lines before repo-scoping) is now modeled to the extent every
/// fixture in this pilot ever needs: a repo with no explicit
/// `masters =` (this pilot doesn't parse that `repos.conf` key at all
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
/// step, and this pilot matches that.)
pub fn resolve_config(
    config_root: &Path,
    main_repo_location: &Path,
    overlay_repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    main_repo_name: &str,
    repo_masters: &HashMap<String, Vec<PathBuf>>,
) -> Result<Config, String> {
    let mut config = Config::default();
    let mut scalars: HashMap<String, String> = HashMap::new();

    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();

    // Real `layout.conf` `profile-formats = portage-2` gate on a profile
    // `parent` line's own `reponame:path`/`:path` cross-repo syntax
    // (real `_config/LocationsManager.py:47`/`259`): the set of repo
    // names that declare it. Read here directly (a tiny dedicated
    // layout.conf reader -- see `repo_profile_formats` -- rather than
    // threading a param through every `resolve_config` call site) so
    // `masters` resolution in `portage_repo::find_repos` and this gate
    // stay independently minimal.
    let parent_colon_repos: HashSet<String> = all_repos
        .iter()
        .filter(|(_, loc)| repo_profile_formats(loc).iter().any(|f| f == "portage-2"))
        .map(|(name, _)| name.clone())
        .collect();

    let make_profile = config_root.join("etc/portage/make.profile");
    let chain: Vec<PathBuf> = if make_profile.exists() {
        resolve_profile_chain(&make_profile, &all_repos, repo_aliases, &parent_colon_repos)?
    } else {
        Vec::new()
    };
    for level in &chain {
        let make_defaults = level.join("make.defaults");
        if !make_defaults.is_file() {
            continue;
        }
        // Real config.py quirk: USE is excluded from cross-level
        // substitution -- see the module doc comment.
        scalars.remove("USE");
        let text = fs::read_to_string(&make_defaults)
            .map_err(|e| format!("reading {}: {e}", make_defaults.display()))?;
        process_lines(&text, &mut scalars, &mut config);
    }

    let make_conf = config_root.join("etc/portage/make.conf");
    if make_conf.is_file() {
        let mut visited_sources = HashSet::new();
        process_make_conf_file(
            &make_conf,
            config_root,
            &mut scalars,
            &mut config,
            &mut visited_sources,
        )?;
    }

    // Real `config.regenerate()`'s `env` `USE_ORDER` layer -- the process
    // environment overrides / stacks on the profile chain + `make.conf`.
    // See `apply_env_layer` / `ENV_*_VARS`.
    apply_env_layer(&mut scalars, &mut config);

    // USE_EXPAND (PMS 7.3.4; real config.py's own regenerate(), "Do the
    // USE calculation last because it depends on USE_EXPAND"): now that
    // every profile level's own make.defaults plus make.conf have been
    // read, `config.use_expand` holds the final, incrementally-stacked
    // set of USE_EXPAND variable NAMES (e.g. "VIDEO_CARDS"). Each named
    // variable's own current VALUE -- read from `scalars`, the same
    // last-level-wins mechanism every other non-USE/ACCEPT_KEYWORDS
    // variable already uses (see the module doc comment; a deliberate
    // simplification of real portage's own genuinely-incremental
    // per-USE_EXPAND-variable behavior -- extending the pre-existing
    // "no incremental merge outside USE/ACCEPT_KEYWORDS" cut to these
    // variables too, not a new one) -- is expanded into
    // lowercase-prefixed pseudo-USE-flags via the exact same
    // apply_incremental token semantics (`-flag`/`flag`/`+flag`/`-*`)
    // USE itself already uses, folded directly into `use_flags`.
    // `USE_EXPAND_UNPREFIXED` (real `config.py`'s own companion
    // mechanism -- no prefix at all, applied in the loop right below
    // this one) IS now read too. `USE_EXPAND_IMPLICIT` + `IUSE_IMPLICIT`
    // + `USE_EXPAND_VALUES_<v>` are now read as well, feeding the real
    // EAPI 5+ `IUSE_EFFECTIVE` computation just after the unprefixed loop
    // -- **the earlier "`USE_EXPAND_HIDDEN`/`USE_EXPAND_IMPLICIT` are
    // display-only" note here was wrong for `USE_EXPAND_IMPLICIT`**: it
    // drives `pkg.iuse.is_valid_flag` (so a `foo[elibc_glibc]` USE-dep
    // matches a `foo` that never lists `elibc_glibc`), not just
    // `emerge --info` display. `USE_EXPAND_HIDDEN` genuinely *is*
    // display-only for EAPI 5+ (see `Config::iuse_effective`); it is read
    // now (`Config::use_expand_hidden`) and honoured only by
    // `emerge --pretend -v`'s own `USE_EXPAND` grouping in `portage_repo`.
    // IUSE-aware `_*`
    // wildcard expansion (`linguas_*`) IS now done, but in
    // `portage_repo::effective_use_flags` (it needs a specific
    // candidate's own IUSE). `package.use`'s own USE_EXPAND-prefix
    // shorthand (`VIDEO_CARDS: nvidia` lines) is read too (see the
    // `package.use` bullet in this comment).
    //
    // `env` `USE_ORDER` layer for the USE_EXPAND / USE_EXPAND_UNPREFIXED
    // variable *values* (`VIDEO_CARDS=nouveau emerge …`): their names
    // aren't known until `USE_EXPAND` itself is finalized above, so the
    // env override is folded into `scalars` here, right before both
    // expansion loops read them. Real portage treats each as its own
    // incremental; the pilot's last-wins `scalars` model just lets the
    // env value replace the profile/`make.conf` one.
    let env_expand_names: Vec<String> = config
        .use_expand
        .iter()
        .chain(config.use_expand_unprefixed.iter())
        .cloned()
        .collect();
    for var in env_expand_names {
        if let Ok(value) = std::env::var(&var) {
            scalars.insert(var, value);
        }
    }

    let use_expand_vars: Vec<String> = config.use_expand.iter().cloned().collect();
    for var in use_expand_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        let prefix = var.to_lowercase();
        let prefixed: String = value
            .split_whitespace()
            .map(|tok| {
                // Real config.py's own early-expand loop: "-flag" keeps
                // its own "-" outside the new prefix ("-video_cards_x",
                // still recognized as a removal by apply_incremental
                // below); a leading "+" is stripped first, matching
                // ordinary USE token handling, so it doesn't get
                // literally baked into the prefixed flag name.
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
        apply_incremental(&prefixed, &mut config.use_flags);
        config.conf_use_tokens.push(prefixed);
    }

    // USE_EXPAND_UNPREFIXED: real config.py's own companion to
    // USE_EXPAND -- the exact same mechanism (variable NAMES
    // incrementally stacked into `config.use_expand_unprefixed` via
    // "USE_EXPAND_UNPREFIXED" during make.defaults/make.conf processing
    // above, each named variable's own current VALUE read from
    // `scalars`), except the value is folded into `use_flags` via
    // `apply_incremental` *directly*, with no `lowercase(name)_` prefix
    // at all -- real Gentoo's own profile sets
    // `USE_EXPAND_UNPREFIXED="ARCH"`, so `ARCH="amd64"` contributes the
    // bare `amd64` flag, not `arch_amd64` (this is literally how
    // `amd64`/`x86`/`arm64` etc. exist as real USE flags at all).
    let use_expand_unprefixed_vars: Vec<String> =
        config.use_expand_unprefixed.iter().cloned().collect();
    for var in use_expand_unprefixed_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        apply_incremental(value, &mut config.use_flags);
        config.conf_use_tokens.push(value.clone());
    }

    // Real EAPI 5+ `IUSE_EFFECTIVE` (`config.py::_calc_iuse_effective`) --
    // computed now that `use_expand`/`use_expand_unprefixed`/
    // `use_expand_implicit`/`iuse_implicit` are all fully stacked and
    // every `USE_EXPAND_VALUES_<v>` scalar has been read. See
    // `Config::iuse_effective`'s own doc comment for what this grants a
    // candidate's `is_valid_flag` domain on top of its declared `IUSE`.
    // Deliberately a plain scalar read of each `USE_EXPAND_VALUES_<v>`
    // (this pilot's established "no incremental merge outside USE/
    // ACCEPT_KEYWORDS" cut -- `USE_EXPAND_VALUES_*` isn't even in real
    // portage's own INCREMENTALS list anyway).
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

    // use.mask/use.force: every profile level's own file (in chain
    // order), stacked with the same -atom removal semantics
    // package.mask uses (see stack_mask_lines) -- confirmed by reading
    // UseManager.getUseMask/getUseForce's own pkg=None (global) case,
    // which returns stack_lists(self._usemask_list/self._useforce_list,
    // incremental=True) directly, never consulting a repo-level or
    // per-package source at all (those only exist on the *per-package*
    // path, out of scope for this pilot's flat/global USE model, same
    // as package.use's own repo/profile/user-only sourcing already is).
    // NOT folded into `use_flags` here -- real config.py's own
    // `regenerate()` applies `self.useforce`/`self.usemask` (which
    // `setcpv()` sets to the *per-package* `getUseForce(pkg)`/
    // `getUseMask(pkg)` -- global use.force/use.mask combined with the
    // atom-scoped package.use.force/.mask this pilot already applies
    // per-candidate) as the literal *last* step of its own incremental
    // USE walk (`lib/portage/package/ebuild/config.py`, ~line 3024:
    // `myflags.update(self.useforce); ...;
    // myflags.difference_update(self.usemask)`), strictly *after* the
    // `pkg` (`package.use`) tier -- not folded in early alongside
    // `defaults`/`conf` the way this pilot previously (incorrectly) did,
    // which let a `package.use` entry override a global force/mask
    // decision real portage never lets it override. See
    // `portage-repo`'s own `effective_use_flags` doc comment for where
    // `use_force`/`use_mask` actually get applied now -- alongside the
    // atom-scoped `package_use_force`/`package_use_mask` it already
    // positions correctly, force-add first then force-remove, so a flag
    // in both ends up masked, not forced, exactly like real portage.
    let mut usemask_sources: Vec<Vec<String>> = Vec::new();
    let mut useforce_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        usemask_sources.push(read_config_lines(&level.join("use.mask"))?);
        useforce_sources.push(read_config_lines(&level.join("use.force"))?);
    }
    config.use_force = stack_mask_lines(&useforce_sources).into_iter().collect();
    config.use_mask = stack_mask_lines(&usemask_sources).into_iter().collect();

    // PORTAGE_ARCHLIST: same chain, same stacking semantics as
    // use.mask/use.force just above -- see `archlist`'s own doc comment.
    let mut archlist_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        archlist_sources.push(read_config_lines(&level.join("arch.list"))?);
    }
    config.archlist = stack_mask_lines(&archlist_sources).into_iter().collect();

    // Real append_repo scopes EVERY repo's own repo-level package.mask/
    // .unmask, including the main repo's own -- not just an overlay's,
    // confirmed by reading MaskManager.py's own repo_pkgmasklines/
    // repo_pkgunmasklines loop (`for repo in repositories.
    // repos_with_profiles()`, unconditional). This pilot previously left
    // the main repo's own entries unscoped, a genuine, documented gap
    // (see this function's own earlier doc comment on the overlay
    // scoping work): an identically-named package.mask atom from main
    // would incorrectly also mask a same-named-but-different overlay
    // package that no mask file ever actually mentions. Note this is
    // NOT the same as the profile-chain's own package.mask/.unmask below
    // (`for level in &chain`) or the user-level one further down --
    // real MaskManager.py's own profile_pkgmasklines/user_pkgmasklines
    // never get append_repo'd at all, only the repo-level ones do, so
    // those two stay exactly as unscoped as they already were.
    let main_repo_mask_lines =
        read_config_lines(&main_repo_location.join("profiles/package.mask"))?;
    let main_repo_unmask_lines =
        read_config_lines(&main_repo_location.join("profiles/package.unmask"))?;
    let mut mask_sources: Vec<Vec<String>> =
        vec![scope_repo_mask_lines(&main_repo_mask_lines, main_repo_name)];
    let mut unmask_sources: Vec<Vec<String>> = vec![scope_repo_mask_lines(
        &main_repo_unmask_lines,
        main_repo_name,
    )];
    for (repo_name, repo_location) in overlay_repos {
        // Real masters (`repo_masters`, resolved by the caller from real
        // `repos.conf`'s own `masters = name1 name2 ...` key --
        // `portage_repo::find_repos`'s own `RepoConfig::masters` doc
        // comment has the full real grounding, `config.py:1229-1260`):
        // an overlay's own package.mask is stacked *on top of* every one
        // of its declared masters' own package.mask, in declared order,
        // before the combined result gets the usual "::reponame"
        // scoping. No entry for `repo_name` in `repo_masters` at all
        // (every pre-existing test call site, which never populates this
        // map) falls back to the same "main repo alone" default this
        // pilot always used before `masters =` parsing existed, so
        // nothing already-tested changes behavior. Simplified from real
        // `MaskManager.py`'s own per-master `stack_lists` (which stacks
        // each master separately against the repo's own lines, then
        // concatenates every one of those per-master results, so a
        // "-atom" removal line's own unmatched-master warning can be
        // attributed correctly) to one flat `stack_mask_lines` call over
        // every master's lines followed by the repo's own -- produces
        // the identical final masked-atom set for the common case (no
        // fixture in this pilot exercises the exotic case where two
        // masters' own "-atom" removals would need to be told apart).
        let masters = repo_masters
            .get(repo_name)
            .cloned()
            .unwrap_or_else(|| vec![main_repo_location.to_path_buf()]);
        let mut mastered_lines_stack: Vec<Vec<String>> = Vec::with_capacity(masters.len() + 1);
        for master_location in &masters {
            mastered_lines_stack.push(read_config_lines(
                &master_location.join("profiles/package.mask"),
            )?);
        }
        mastered_lines_stack.push(read_config_lines(
            &repo_location.join("profiles/package.mask"),
        )?);
        let mastered_mask_lines = stack_mask_lines(&mastered_lines_stack);
        mask_sources.push(scope_repo_mask_lines(&mastered_mask_lines, repo_name));
        // package.unmask deliberately does NOT get the same masters
        // treatment -- confirmed by reading MaskManager.py's own two
        // loops side by side: the package.mask loop stacks each
        // master's own lines in first, but the package.unmask loop
        // only ever stacks a repo's own lines against itself
        // (`stack_lists([repo_lines], incremental=1, ...)`, no masters
        // iteration at all), a real asymmetry in real portage itself,
        // not a simplification on this pilot's part.
        unmask_sources.push(scope_repo_mask_lines(
            &read_config_lines(&repo_location.join("profiles/package.unmask"))?,
            repo_name,
        ));
    }
    for level in &chain {
        mask_sources.push(read_config_lines(&level.join("package.mask"))?);
        unmask_sources.push(read_config_lines(&level.join("package.unmask"))?);
    }
    mask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.mask"),
    )?);
    unmask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.unmask"),
    )?);

    config.package_mask = stack_mask_lines(&mask_sources);
    config.package_unmask = stack_mask_lines(&unmask_sources);

    // package.accept_keywords: profile-chain (in chain order), then
    // user-level -- real KeywordsManager.getPKeywords iterates its own
    // per-profile-level dicts first, then the user-level one, extending
    // the same accumulating "extra accepted keywords" list each time (no
    // "-atom" removal exists for this file at all, unlike package.mask,
    // so a flat concatenation-then-parse is equivalent to parsing each
    // source separately and concatenating the results). No repo-level
    // source exists for this file in real portage at all (unlike
    // package.mask's repo-level profiles/package.mask) -- confirmed by
    // reading KeywordsManager.__init__, which never reads a
    // repo-location path for either package.accept_keywords or its
    // package.keywords alias.
    let mut accept_keywords_lines: Vec<String> = Vec::new();
    for level in &chain {
        accept_keywords_lines.extend(read_config_lines(&level.join("package.accept_keywords"))?);
    }
    accept_keywords_lines.extend(read_config_lines(
        &config_root.join("etc/portage/package.accept_keywords"),
    )?);
    config.package_accept_keywords = parse_package_accept_keywords_lines(&accept_keywords_lines);
    // A bare atom (empty token list, preserved by the parser above) gets
    // real `accept_keywords_defaults`'s own implicit meaning: "~" plus
    // every *plain* (non-`~`/`-`-prefixed) token in the *final* global
    // ACCEPT_KEYWORDS -- computed once here, against `config.
    // accept_keywords` as already fully resolved by this point (every
    // profile-level/make.conf ACCEPT_KEYWORDS contribution above already
    // folded in), exactly matching what real portage computes it from at
    // both of its own two call sites (`KeywordsManager.__init__`'s own
    // `global_accept_keywords` parameter, `getPKeywords`'s own `pgroups`
    // -- both already-resolved global ACCEPT_KEYWORDS by the time either
    // runs). Sorted only for deterministic test assertions; downstream
    // consumption (`specificity_ordered_flags`) folds these into a
    // `HashSet`, so fold order was never semantically significant.
    let mut accept_keywords_defaults: Vec<String> = config
        .accept_keywords
        .iter()
        .filter(|k| !k.starts_with('~') && !k.starts_with('-'))
        .map(|k| format!("~{k}"))
        .collect();
    accept_keywords_defaults.sort();
    for (_, tokens) in &mut config.package_accept_keywords {
        if tokens.is_empty() {
            *tokens = accept_keywords_defaults.clone();
        }
    }

    // package.use: three real sources, each into its own field at its own
    // real USE_ORDER position (the "Config depth" slice, SCOPE_BACKLOG
    // Part 2.C -- see the module doc comment's `package.use` bullet and
    // `portage-repo::effective_use_flags` for the per-package walk):
    //   - repo-level (`<repo>/profiles/package.use`, every configured
    //     repo -- real UseManager._parse_repository_files_to_dict_of_dicts
    //     iterates repositories.repos_with_profiles()) -> package_use_repo,
    //     real configdict["repo"]. Overlay lines are `::repo`-scoped via
    //     scope_repo_package_use_lines (the same "package.mask, widened"
    //     precedent); the main repo's stay unscoped, since real portage's
    //     own masters-chain lookup (`repos = masters + [pkg.repo]`) always
    //     includes it (every overlay here implicitly masters main, no
    //     explicit `masters =` support yet).
    //   - every profile level's own package.use (chain order) ->
    //     package_use (real configdict["defaults"], _pkgprofileuse).
    //   - user-level `/etc/portage/package.use` -> package_use_user (real
    //     configdict["pkg"], _pusedict / getPUSE).
    // Each source is parsed independently (rather than one concatenated
    // pass) so only the user-level one gets the USE_EXPAND-prefix
    // shorthand's real user-only `extended_syntax` -- see
    // parse_package_use_lines. Still purely additive within each source
    // (no `-atom` removal for this file). Still a simplification: repo
    // `make.defaults` USE (real _repo_make_defaults, folded into
    // configdict["repo"] alongside repo package.use) is not modeled, and
    // profile package.use is applied as one group after all profile
    // make.defaults rather than interleaved per level.
    let mut repo_use_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_use_lines = read_config_lines(&repo_location.join("profiles/package.use"))?;
        repo_use_lines.extend(scope_repo_package_use_lines(&overlay_use_lines, repo_name));
    }
    let mut profile_use_lines: Vec<String> = Vec::new();
    for level in &chain {
        profile_use_lines.extend(read_config_lines(&level.join("package.use"))?);
    }
    let user_use_lines = read_config_lines(&config_root.join("etc/portage/package.use"))?;
    config.package_use_repo = parse_package_use_lines(&repo_use_lines, false);
    config.package_use = parse_package_use_lines(&profile_use_lines, false);
    config.package_use_user = parse_package_use_lines(&user_use_lines, true);

    // package.use.mask/package.use.force: repo-level (every repo, not
    // just main -- see the package.use bullet just above for the same
    // `::repo`-scoping this now applies here too) plus every profile
    // level's own file (in chain order) -- NO
    // user-level source at all, unlike package.use: confirmed by reading
    // UseManager.__init__'s own file/variable table (the "user config"
    // section lists only "package.use -> _pusedict", nothing for
    // mask/force). Unlike package.mask/.unmask, real UseManager.py never
    // merges an overlay's own file with its master's own at load time
    // (no `stack_lists`-equivalent combination here at all -- confirmed
    // by reading `_parse_repository_files_to_dict_of_tuples`/`_of_dicts`,
    // which just parse each repo's own file independently); the masters
    // chain is only consulted later, per-package, in
    // `getUseMask`/`getUseForce` (`repos = masters + [pkg.repo]`, each
    // repo's own already-independent dict appended in that order) -- so
    // no `stack_mask_lines`-style merge is needed here either, just the
    // same scope-then-append `package.use` above already does. Flat-
    // concatenated the same way package_use already is; which entry
    // actually wins when more than one matches the same candidate is
    // decided later, at application time -- see `portage-repo`'s own doc
    // comment on why (atom-specificity ordering).
    let mut use_force_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.force"))?;
    let mut use_mask_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.mask"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_force = read_config_lines(&repo_location.join("profiles/package.use.force"))?;
        use_force_lines.extend(scope_repo_package_use_lines(&overlay_force, repo_name));
        let overlay_mask = read_config_lines(&repo_location.join("profiles/package.use.mask"))?;
        use_mask_lines.extend(scope_repo_package_use_lines(&overlay_mask, repo_name));
    }
    for level in &chain {
        use_force_lines.extend(read_config_lines(&level.join("package.use.force"))?);
        use_mask_lines.extend(read_config_lines(&level.join("package.use.mask"))?);
    }
    // No shorthand here either: package.use.force/.mask have no
    // user-level source at all (see this crate's own doc comment), and
    // real portage's own shorthand support is genuinely user-only.
    config.package_use_force = parse_package_use_lines(&use_force_lines, false);
    config.package_use_mask = parse_package_use_lines(&use_mask_lines, false);

    // use.stable.mask/use.stable.force (PMS 5+; real
    // eapi_supports_stable_use_forcing_and_masking's own EAPI floor,
    // always recognized here -- see this crate's own module doc comment
    // for this pilot's established "no EAPI parametrization"
    // precedent): profile-chain only, same -atom-removal stacking
    // use_force/use_mask already get -- deliberately NOT folded into
    // use_flags here; see use_stable_force's own doc comment for why.
    let mut use_stable_force_sources: Vec<Vec<String>> = Vec::new();
    let mut use_stable_mask_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        use_stable_force_sources.push(read_config_lines(&level.join("use.stable.force"))?);
        use_stable_mask_sources.push(read_config_lines(&level.join("use.stable.mask"))?);
    }
    config.use_stable_force = stack_mask_lines(&use_stable_force_sources)
        .into_iter()
        .collect();
    config.use_stable_mask = stack_mask_lines(&use_stable_mask_sources)
        .into_iter()
        .collect();

    // package.use.stable.mask/package.use.stable.force: repo-level
    // (every repo, not just main -- same `::repo`-scoped, no-masters-
    // merge treatment package.use.mask/.force just above now gets,
    // confirmed by the same UseManager.__init__ table entries for
    // `_repo_pusestablemask_dict`/`_repo_pusestableforce_dict`) plus
    // every profile level's own file (in chain order) -- NO user-level
    // source at all, mirroring package_use_force/_mask's own confirmed
    // sourcing exactly. No shorthand either, same reasoning.
    let mut use_stable_force_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.stable.force"))?;
    let mut use_stable_mask_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.stable.mask"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_stable_force =
            read_config_lines(&repo_location.join("profiles/package.use.stable.force"))?;
        use_stable_force_lines.extend(scope_repo_package_use_lines(
            &overlay_stable_force,
            repo_name,
        ));
        let overlay_stable_mask =
            read_config_lines(&repo_location.join("profiles/package.use.stable.mask"))?;
        use_stable_mask_lines.extend(scope_repo_package_use_lines(
            &overlay_stable_mask,
            repo_name,
        ));
    }
    for level in &chain {
        use_stable_force_lines.extend(read_config_lines(&level.join("package.use.stable.force"))?);
        use_stable_mask_lines.extend(read_config_lines(&level.join("package.use.stable.mask"))?);
    }
    config.package_use_stable_force = parse_package_use_lines(&use_stable_force_lines, false);
    config.package_use_stable_mask = parse_package_use_lines(&use_stable_mask_lines, false);

    // packages (@system): every profile level's own file, in chain
    // order, stacked with the same -atom removal semantics package.mask
    // uses (see stack_mask_lines) -- confirmed by reading
    // PackagesSystemSet.load, which calls the identical real
    // stack_lists(incremental=1) function. Only *after* stacking are the
    // "*"-prefixed lines kept (with the "*" stripped) as the real
    // @system atom list -- see the module doc comment's `packages`
    // bullet for why every other stacked line is read/stacked but never
    // contributes an atom of its own.
    let mut packages_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        packages_sources.push(read_config_lines(&level.join("packages"))?);
    }
    config.system_packages = stack_mask_lines(&packages_sources)
        .into_iter()
        .filter_map(|line| line.strip_prefix('*').map(String::from))
        .collect();

    // package.provided (real config.py:970-1027): every profile level's
    // own file (chain order) + the user-level
    // /etc/portage/profile/package.provided, stacked with the same
    // stack_lists(incremental=1) `-atom` removal package.mask/`packages`
    // use. See `Config::package_provided`.
    let mut pprovided_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        pprovided_sources.push(read_config_lines(&level.join("package.provided"))?);
    }
    pprovided_sources.push(read_config_lines(
        &config_root.join("etc/portage/profile/package.provided"),
    )?);
    config.package_provided = stack_mask_lines(&pprovided_sources);

    // license_groups: real `LicenseManager._read_license_groups`
    // (`LicenseManager.py:47`) over `LocationsManager.profile_locations`
    // (`LocationsManager.py:432`) -- which is the `profiles/` directory
    // of the *main repo and each overlay*, NOT the individual
    // profile-chain levels (real gentoo puts `license_groups` at
    // `<repo>/profiles/license_groups`, never in a `profiles/<foo>/`
    // profile dir -- verified live against a real tree). Then the
    // user-level `/etc/portage/license_groups`. "extend, don't
    // stack/replace" semantics -- see `parse_license_groups_lines`. Read
    // before ACCEPT_LICENSE/package.license below, both of which need
    // the full, final group map to expand `@group` tokens against.
    let mut license_groups: HashMap<String, Vec<String>> = HashMap::new();
    for (_name, location) in &all_repos {
        for (name, members) in parse_license_groups_lines(&read_config_lines(
            &location.join("profiles").join("license_groups"),
        )?) {
            license_groups.entry(name).or_default().extend(members);
        }
    }
    for (name, members) in parse_license_groups_lines(&read_config_lines(
        &config_root.join("etc/portage/license_groups"),
    )?) {
        license_groups.entry(name).or_default().extend(members);
    }
    config.license_groups = license_groups;

    // ACCEPT_LICENSE: last-level-wins scalar (see `accept_license`'s own
    // doc comment for why, and the real "* -@EULA" default this pilot
    // replicates when it's never set anywhere at all) -- `scalars`
    // already holds whatever the profile chain + make.conf left it as,
    // via the same catch-all `scalars.insert` every other scalar
    // variable already goes through in `process_lines`/
    // `process_make_conf_file`.
    let accept_license_str = scalars
        .get("ACCEPT_LICENSE")
        .cloned()
        .unwrap_or_else(|| "* -@EULA".to_string());
    let accept_license_tokens: Vec<String> = accept_license_str
        .split_whitespace()
        .map(String::from)
        .collect();
    config.accept_license = expand_license_tokens(&accept_license_tokens, &config.license_groups);

    // package.license: user-level only -- see `package_license`'s own
    // doc comment for the deliberately-not-replicated `profile-license`
    // profile-format and `*/*`-global-extraction real quirks.
    let package_license_lines =
        read_config_lines(&config_root.join("etc/portage/package.license"))?;
    config.package_license = parse_package_license_lines(&package_license_lines)
        .into_iter()
        .map(|(atom, tokens)| (atom, expand_license_tokens(&tokens, &config.license_groups)))
        .collect();

    // ACCEPT_PROPERTIES/ACCEPT_RESTRICT: last-level-wins scalars, real
    // "*" default (see `accept_properties`'s own doc comment) -- no
    // `@group` expansion for either, unlike ACCEPT_LICENSE/
    // package.license just above.
    config.accept_properties = scalars
        .get("ACCEPT_PROPERTIES")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);
    config.accept_restrict = scalars
        .get("ACCEPT_RESTRICT")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);

    // package.properties/package.accept_restrict: user-level only, same
    // "atom + raw tokens" shape package.license already reads (reused
    // directly -- see parse_package_license_lines's own doc comment).
    config.package_properties = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.properties"),
    )?);
    config.package_accept_restrict = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.accept_restrict"),
    )?);

    // PKGDIR: last-level-wins scalar, real "/var/cache/binpkgs" default
    // (see `pkgdir`'s own doc comment).
    config.pkgdir = scalars
        .get("PKGDIR")
        .cloned()
        .unwrap_or_else(|| "/var/cache/binpkgs".to_string());

    // binrepos.conf + PORTAGE_BINHOST (see `binrepos`'s own doc comment).
    config.binrepos = parse_binrepos(
        &read_to_string_opt(&config_root.join("etc/portage/binrepos.conf")),
        scalars
            .get("PORTAGE_BINHOST")
            .map(String::as_str)
            .unwrap_or(""),
    );

    config.other_vars = scalars;

    Ok(config)
}

/// A missing file reads as an empty string (the same "absence is a valid
/// state" tolerance the rest of this crate uses).
fn read_to_string_opt(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Real `BinRepoConfigLoader` (`lib/portage/binrepo/config.py:97-172`),
/// narrowed. Parses `binrepos.conf`'s own `[section]` / `key = value`
/// INI (only `sync-uri` and `priority` are read), then appends one
/// implicit `BinRepo` per whitespace-separated `PORTAGE_BINHOST` URI
/// that isn't already a section's `sync-uri` (real "Convert
/// PORTAGE_BINHOST entries into implicit binrepos.conf ones", iterated in
/// reverse with an incrementing `priority`). Each URI is
/// `_normalize_uri`'d (trailing `/` stripped). Returns the repos sorted
/// by `(priority, name)` -- real `sorted(repos, key=lambda repo:
/// (repo.priority or 0, repo.name or repo.name_fallback))`.
///
/// **Documented narrowings**: the implicit-entry name is `md5(uri)` in
/// real portage; this pilot has no md5, so it uses the URI's own
/// `host/path` (still stable and unique per URI, only ever surfaced as a
/// sort key here). `[DEFAULT]` interpolation, `getbinpkg-exclude`/
/// `-include`, `fetchcommand`/`resumecommand`, signature-verification
/// settings, and the `location =` fallback for a section without
/// `PORTAGE_BINHOST` are all out (none affect a `--pretend` resolution).
fn parse_binrepos(binrepos_conf: &str, portage_binhost: &str) -> Vec<BinRepo> {
    let normalize = |u: &str| u.trim_end_matches('/').to_string();
    let mut repos: Vec<BinRepo> = Vec::new();
    let mut seen_uris: std::collections::HashSet<String> = std::collections::HashSet::new();

    // INI sections.
    let mut section: Option<String> = None;
    let mut sync_uri: Option<String> = None;
    let mut priority: i32 = 0;
    let flush = |section: &mut Option<String>,
                 sync_uri: &mut Option<String>,
                 priority: &mut i32,
                 repos: &mut Vec<BinRepo>,
                 seen: &mut std::collections::HashSet<String>| {
        if let (Some(name), Some(uri)) = (section.take(), sync_uri.take()) {
            let uri = uri.trim_end_matches('/').to_string();
            seen.insert(uri.clone());
            repos.push(BinRepo {
                name,
                sync_uri: uri,
                priority: *priority,
            });
        }
        *priority = 0;
    };
    for line in binrepos_conf.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            flush(
                &mut section,
                &mut sync_uri,
                &mut priority,
                &mut repos,
                &mut seen_uris,
            );
            section = Some(inner.trim().to_string());
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                // `${VAR}` in `sync-uri` expands from the environment
                // (pilot convenience -- real configparser doesn't -- so a
                // fixture can use `file://${PORTAGE_CONFIGROOT}/binhost`
                // instead of an absolute, non-relocatable path).
                "sync-uri" => sync_uri = Some(substitute(v.trim(), &HashMap::new())),
                "priority" => priority = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    flush(
        &mut section,
        &mut sync_uri,
        &mut priority,
        &mut repos,
        &mut seen_uris,
    );
    repos.retain(|r| r.name != "DEFAULT");

    // Implicit PORTAGE_BINHOST entries (real: reversed, incrementing priority).
    let mut current_priority = 0;
    for uri in portage_binhost.split_whitespace().rev() {
        let uri = normalize(uri);
        if seen_uris.insert(uri.clone()) {
            current_priority += 1;
            let name = uri
                .split_once("://")
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_else(|| uri.clone());
            repos.push(BinRepo {
                name,
                sync_uri: uri,
                priority: current_priority,
            });
        }
    }

    repos.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    repos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_binrepos_reads_sections_and_implicit_portage_binhost_entries() {
        let conf = "\
[gentoobinhost]
sync-uri = https://binhost.example.org/amd64/
priority = 5

[low]
sync-uri = file:///srv/pkgs
";
        let repos = parse_binrepos(
            conf,
            "https://binhost.example.org/amd64/  https://other.example/b/",
        );
        // `low` (prio 0), then the implicit `other.example/b` (prio 1),
        // then `gentoobinhost` (prio 5). The already-listed
        // binhost.example.org URI is not re-added as implicit.
        assert_eq!(
            repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["low", "other.example/b", "gentoobinhost"]
        );
        assert_eq!(repos[0].sync_uri, "file:///srv/pkgs");
        assert_eq!(repos[2].sync_uri, "https://binhost.example.org/amd64");
    }

    #[test]
    fn binrepo_packages_dir_maps_scheme_to_the_real_edb_cache_layout() {
        let root = Path::new("/eroot");
        let http = BinRepo {
            name: "h".into(),
            sync_uri: "https://gpkg.example.org/seed-desk".into(),
            priority: 0,
        };
        assert_eq!(
            http.packages_dir(root),
            Path::new("/eroot/var/cache/edb/binhost/gpkg.example.org/seed-desk")
        );
        let file = BinRepo {
            name: "f".into(),
            sync_uri: "file:///srv/pkgs".into(),
            priority: 0,
        };
        assert_eq!(file.packages_dir(root), Path::new("/srv/pkgs"));
    }

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .expect("fixtures must exist")
    }

    #[test]
    fn expand_license_token_passes_through_a_plain_license_name() {
        let groups = HashMap::new();
        assert_eq!(expand_license_token("GPL-2", &groups), vec!["GPL-2"]);
    }

    #[test]
    fn expand_license_token_passes_through_the_star_wildcard_tokens() {
        let groups = HashMap::new();
        assert_eq!(expand_license_token("*", &groups), vec!["*"]);
        assert_eq!(expand_license_token("-*", &groups), vec!["-*"]);
    }

    #[test]
    fn expand_license_token_expands_a_group_reference() {
        let groups = HashMap::from([(
            "FREE".to_string(),
            vec!["GPL-2".to_string(), "MIT".to_string()],
        )]);
        assert_eq!(expand_license_token("@FREE", &groups), vec!["GPL-2", "MIT"]);
    }

    #[test]
    fn expand_license_token_negates_every_expanded_member() {
        let groups = HashMap::from([(
            "EULA".to_string(),
            vec!["Eula1".to_string(), "Eula2".to_string()],
        )]);
        assert_eq!(
            expand_license_token("-@EULA", &groups),
            vec!["-Eula1", "-Eula2"]
        );
    }

    #[test]
    fn expand_license_token_recurses_into_a_nested_group() {
        let groups = HashMap::from([
            ("FREE".to_string(), vec!["@FSF-APPROVED".to_string()]),
            ("FSF-APPROVED".to_string(), vec!["GPL-2".to_string()]),
        ]);
        assert_eq!(expand_license_token("@FREE", &groups), vec!["GPL-2"]);
    }

    #[test]
    fn expand_license_token_guards_against_a_circular_group_reference() {
        // A -> B -> A: expanding "@A" recurses into B, which tries to
        // re-expand "@A" while "A" is still on the traversal stack, so
        // that innermost attempt falls back to the literal "@A" text
        // (matching real portage's own writemsg-then-literal-fallback
        // behavior) rather than looping forever.
        let groups = HashMap::from([
            ("A".to_string(), vec!["@B".to_string()]),
            ("B".to_string(), vec!["@A".to_string()]),
        ]);
        assert_eq!(expand_license_token("@A", &groups), vec!["@A"]);
    }

    #[test]
    fn expand_license_token_leaves_an_undefined_group_as_literal_text() {
        let groups = HashMap::new();
        assert_eq!(expand_license_token("@NOPE", &groups), vec!["@NOPE"]);
    }

    #[test]
    fn parse_license_groups_lines_extends_a_repeated_group_name() {
        let lines = vec![
            "FREE GPL-2".to_string(),
            "FREE MIT".to_string(),
            "EULA MyEula".to_string(),
        ];
        let groups = parse_license_groups_lines(&lines);
        assert_eq!(
            groups.get("FREE"),
            Some(&vec!["GPL-2".to_string(), "MIT".to_string()])
        );
        assert_eq!(groups.get("EULA"), Some(&vec!["MyEula".to_string()]));
    }

    /// End-to-end check against fixtures/repo/profiles (base,
    /// arch/amd64 -> multi-parent -> default) + fixtures/etc/portage/make.conf
    /// (which sources fixtures/etc/make.local). Traced by hand:
    ///   base:            USE="foo"; USE="${USE} bar"      -> {foo, bar}
    ///   arch/amd64:      ARCH="amd64"; ACCEPT_KEYWORDS="${ARCH}" -> keywords {amd64}
    ///   default:         USE="-bar baz"                   -> {foo, baz}
    ///   make.local (sourced first from make.conf):
    ///                    USE="${USE} localflag"            -> {foo, baz, localflag}
    ///   make.conf:       USE="confflag"                    -> {foo, baz, localflag, confflag}
    #[test]
    fn resolves_fixture_profile_chain_and_make_conf() {
        let root = fixtures_root();
        // The fixture's own default/parent has a real cross-repo entry
        // ("overlay:crossrepo-parent"), so this test needs "overlay" in
        // its own repo list too -- mirroring exactly what production
        // (portuale's pretend.rs, built from real find_repos) already
        // passes for this same fixture tree.
        let overlay_repos = [("overlay".to_string(), root.join("overlay"))];
        let config = resolve_config(
            &root,
            &root.join("repo"),
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("fixture config must resolve");
        assert_eq!(
            config.use_flags,
            HashSet::from([
                "foo".to_string(),
                "baz".to_string(),
                "localflag".to_string(),
                "confflag".to_string(),
                "video_cards_nvidia".to_string(),
                // USE_EXPAND_UNPREFIXED="ARCH" in profiles/arch/amd64/
                // make.defaults, real Gentoo's own mechanism for how
                // "amd64" exists as a plain USE flag at all.
                "amd64".to_string(),
                // ELIBC="glibc" in profiles/base/make.defaults (ELIBC is
                // in USE_EXPAND), same USE_EXPAND folding video_cards_*
                // already exercises -- and ELIBC is also in
                // USE_EXPAND_IMPLICIT, see the iuse_effective assert below.
                "elibc_glibc".to_string(),
                // CPU_FLAGS_X86="sse2" (CPU_FLAGS_X86 is in USE_EXPAND);
                // it is also USE_EXPAND_HIDDEN, but that only affects the
                // `emerge -pv` display, never the resolved flag set.
                "cpu_flags_x86_sse2".to_string(),
            ])
        );
        assert_eq!(
            config.use_expand,
            HashSet::from([
                "VIDEO_CARDS".to_string(),
                "ELIBC".to_string(),
                // LINGUAS is in USE_EXPAND for the _* wildcard fixture
                // (dev-libs/wildexpandpkg) -- no LINGUAS= value set, so
                // it folds nothing here.
                "LINGUAS".to_string(),
                // PYTHON_TARGETS + CPU_FLAGS_X86 exist for the
                // `emerge -pv` USE_EXPAND-grouping fixtures
                // (packageuseexpandpkg / hiddenexpandpkg).
                "PYTHON_TARGETS".to_string(),
                "CPU_FLAGS_X86".to_string(),
            ])
        );
        assert_eq!(
            config.use_expand_hidden,
            HashSet::from(["CPU_FLAGS_X86".to_string()])
        );
        assert_eq!(
            config.use_expand_implicit,
            HashSet::from(["ELIBC".to_string()])
        );
        // Real EAPI 5+ IUSE_EFFECTIVE: ELIBC is in USE_EXPAND ∩
        // USE_EXPAND_IMPLICIT, so every USE_EXPAND_VALUES_ELIBC value gets
        // the lowercase "elibc_" prefix -- valid implicit IUSE for every
        // package even when unlisted in its own IUSE.
        assert_eq!(
            config.iuse_effective,
            HashSet::from(["elibc_glibc".to_string(), "elibc_musl".to_string()])
        );
        assert_eq!(config.accept_keywords, HashSet::from(["amd64".to_string()]));
        // Neither the fixture profile chain nor make.conf sets
        // ACCEPT_LICENSE at all -- real portage's own "* -@EULA"
        // default applies. Real `LicenseManager` reads `license_groups`
        // from each repo's own `profiles/` dir (`LocationsManager.
        // profile_locations`): `repo/profiles/license_groups` defines
        // EULA="SomeEula", `overlay/profiles/license_groups` extends it
        // with "CrossRepoNonfree" -- so "@EULA" expands to both members,
        // main repo first then overlay, rather than staying literal.
        assert_eq!(
            config.accept_license,
            vec![
                "*".to_string(),
                "-SomeEula".to_string(),
                "-CrossRepoNonfree".to_string()
            ]
        );
        assert_eq!(
            config.license_groups.get("EULA"),
            Some(&vec![
                "SomeEula".to_string(),
                "CrossRepoNonfree".to_string()
            ])
        );
    }

    #[test]
    fn license_groups_come_from_each_repo_profiles_dir_not_the_profile_chain() {
        // A `license_groups` file dropped into a profile-chain directory
        // (`repo/profiles/base/`) must be IGNORED -- real portage reads
        // it only from `<repo>/profiles/license_groups`
        // (`LocationsManager.profile_locations`). The fixture's real
        // groups live at `repo/profiles/license_groups` +
        // `overlay/profiles/license_groups`.
        let root = fixtures_root();
        let stray = root.join("repo/profiles/base/license_groups");
        std::fs::write(&stray, "EULA StrayShouldBeIgnored\n").unwrap();
        let config = resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            &[],
            "testrepo",
            &HashMap::new(),
        );
        std::fs::remove_file(&stray).ok();
        let config = config.expect("fixture config must resolve");
        // Only the two real repo-level members, in repo-then-overlay
        // order -- the stray chain-level entry is not present.
        assert_eq!(
            config.license_groups.get("EULA"),
            Some(&vec![
                "SomeEula".to_string(),
                "CrossRepoNonfree".to_string()
            ])
        );
    }

    #[test]
    fn missing_profile_and_make_conf_yield_empty_config() {
        let empty_root = std::env::temp_dir().join("portage-profile-test-empty-root");
        let _ = fs::create_dir_all(&empty_root);
        let config = resolve_config(
            &empty_root,
            &empty_root.join("repo"),
            &[],
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("missing profile/make.conf is not an error");
        assert_eq!(config.use_flags, HashSet::new());
        assert_eq!(config.accept_keywords, HashSet::new());
        assert_eq!(
            config.accept_license,
            vec!["*".to_string(), "-@EULA".to_string()]
        );
        assert_eq!(config.accept_properties, vec!["*".to_string()]);
        assert_eq!(config.accept_restrict, vec!["*".to_string()]);
    }

    #[test]
    fn cross_repo_profile_parent_resolves_a_named_repos_own_profile() {
        // The leaf profile lives outside any known repo, but its own
        // "parent" file names "otherrepo:base" -- expand_parent_colon
        // must resolve that to otherrepo's own profiles/base directory
        // regardless, proving repo-name lookup doesn't depend on the
        // referencing node's own repo membership.
        let root = std::env::temp_dir().join("portage-profile-test-cross-repo-named");
        let main_repo = root.join("repo");
        let other_repo = root.join("otherrepo");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(main_repo.join("profiles")).unwrap();
        fs::create_dir_all(other_repo.join("profiles/base")).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(
            other_repo.join("profiles/base/make.defaults"),
            "USE=\"crossrepoflag\"\n",
        )
        .unwrap();
        fs::write(leaf.join("parent"), "otherrepo:base\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let overlay_repos = [("otherrepo".to_string(), other_repo.clone())];
        let config = resolve_config(
            &root,
            &main_repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("cross-repo parent must resolve");
        assert!(config.use_flags.contains("crossrepoflag"));
    }

    #[test]
    fn cross_repo_profile_parent_resolves_an_aliased_repo_name() {
        // Same as above, but the `parent` line names the repo by an
        // *alias* ("ovl") rather than its canonical name ("otherrepo").
        // Real `_expand_parent_colon` -> `get_location_for_name` is keyed
        // on aliases too, so `resolve_config` gets `repo_aliases`.
        let root = std::env::temp_dir().join("portage-profile-test-cross-repo-alias");
        let main_repo = root.join("repo");
        let other_repo = root.join("otherrepo");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(main_repo.join("profiles")).unwrap();
        fs::create_dir_all(other_repo.join("profiles/base")).unwrap();
        fs::create_dir_all(&leaf).unwrap();
        fs::write(
            other_repo.join("profiles/base/make.defaults"),
            "USE=\"aliasedflag\"\n",
        )
        .unwrap();
        fs::write(leaf.join("parent"), "ovl:base\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        // `otherrepo` is a canonical name, `ovl` its alias.
        let overlay_repos = [("otherrepo".to_string(), other_repo.clone())];
        let repo_aliases = [("ovl".to_string(), other_repo.clone())];
        let config = resolve_config(
            &root,
            &main_repo,
            &overlay_repos,
            &repo_aliases,
            "testrepo",
            &HashMap::new(),
        )
        .expect("aliased cross-repo parent must resolve");
        assert!(config.use_flags.contains("aliasedflag"));

        // ...and without the alias registered, it's the same clear error
        // an unknown repo name gets.
        let err = resolve_config(
            &root,
            &main_repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .unwrap_err();
        assert!(err.contains("no repo named \"ovl\""), "{err}");
    }

    #[test]
    fn cross_repo_profile_parent_unknown_repo_name_is_a_clear_error() {
        let root = std::env::temp_dir().join("portage-profile-test-cross-repo-unknown");
        let profile_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("parent"), "doesnotexist:base\n").unwrap();
        let make_profile = profile_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let err = resolve_config(
            &root,
            &root.join("repo"),
            &[],
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect_err("unknown repo name must be rejected");
        assert!(err.contains("no repo named"), "unexpected error: {err}");
    }

    #[test]
    fn same_repo_colon_profile_parent_resolves_within_the_current_repo() {
        // A bare ":base" parent entry (no repo name before the colon)
        // means "this same repo" -- resolved via whichever repo the
        // *referencing* profile node's own directory belongs to, not
        // necessarily the main repo (repo_containing's own longest-
        // prefix-match), so the leaf profile here lives inside the main
        // repo itself.
        let root = std::env::temp_dir().join("portage-profile-test-same-repo-colon");
        let main_repo = root.join("repo");
        fs::create_dir_all(main_repo.join("profiles/leaf")).unwrap();
        fs::create_dir_all(main_repo.join("profiles/base")).unwrap();
        fs::write(
            main_repo.join("profiles/base/make.defaults"),
            "USE=\"samerepocolon\"\n",
        )
        .unwrap();
        fs::write(main_repo.join("profiles/leaf/parent"), ":base\n").unwrap();
        // Real `_allow_parent_colon` gate: a profile node inside a repo
        // needs that repo's own `layout.conf` to declare `portage-2`.
        fs::create_dir_all(main_repo.join("metadata")).unwrap();
        fs::write(
            main_repo.join("metadata/layout.conf"),
            "profile-formats = portage-2\n",
        )
        .unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(main_repo.join("profiles/leaf"), &make_profile).unwrap();

        let config = resolve_config(&root, &main_repo, &[], &[], "testrepo", &HashMap::new())
            .expect("same-repo colon parent must resolve");
        assert!(config.use_flags.contains("samerepocolon"));
    }

    #[test]
    fn colon_profile_parent_is_not_expanded_when_the_repo_lacks_portage_2() {
        // Real `_allow_parent_colon` gate: the identical setup as
        // `same_repo_colon_profile_parent_resolves` but with NO
        // `metadata/layout.conf` (so `profile-formats` doesn't contain
        // `portage-2`). The `:base` line is then left literal, and
        // resolution fails trying to open `<leaf>/:base` -- proving the
        // gate actually gates.
        let root = std::env::temp_dir().join("portage-profile-test-colon-no-portage2");
        let main_repo = root.join("repo");
        fs::create_dir_all(main_repo.join("profiles/leaf")).unwrap();
        fs::create_dir_all(main_repo.join("profiles/base")).unwrap();
        fs::write(main_repo.join("profiles/base/make.defaults"), "USE=\"x\"\n").unwrap();
        fs::write(main_repo.join("profiles/leaf/parent"), ":base\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(main_repo.join("profiles/leaf"), &make_profile).unwrap();

        let err = resolve_config(&root, &main_repo, &[], &[], "testrepo", &HashMap::new())
            .expect_err("a colon parent in a non-portage-2 repo must not resolve");
        assert!(err.contains(":base"), "unexpected error: {err}");
    }

    #[test]
    fn same_repo_colon_profile_parent_outside_any_known_repo_is_a_clear_error() {
        let root = std::env::temp_dir().join("portage-profile-test-same-repo-colon-outside");
        let profile_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("parent"), ":base\n").unwrap();
        let make_profile = profile_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let err = resolve_config(
            &root,
            &root.join("repo"),
            &[],
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect_err("same-repo colon outside any known repo must be rejected");
        assert!(
            err.contains("not inside any known repo"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn diamond_profile_inheritance_does_not_double_apply_a_shared_ancestor() {
        // level "top" has two parents ("left", "right") that both inherit
        // from the same "shared" ancestor; "shared" must only contribute
        // its USE flag once, not twice (which -- since it's a set -- isn't
        // observable via *membership*, but the visited-set/cycle-safety
        // mechanism is exactly what also protects a genuine cycle, so this
        // proves that mechanism doesn't accidentally block a legitimate
        // multi-path DAG from resolving at all).
        let root = std::env::temp_dir().join("portage-profile-test-diamond");
        let profile_dir = root.join("etc/portage");
        fs::create_dir_all(&profile_dir).unwrap();
        for name in ["shared", "left", "right", "top"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::write(root.join("shared/make.defaults"), "USE=\"sharedflag\"\n").unwrap();
        fs::write(root.join("left/parent"), "../shared\n").unwrap();
        fs::write(root.join("right/parent"), "../shared\n").unwrap();
        fs::write(root.join("top/parent"), "../left\n../right\n").unwrap();
        fs::write(root.join("top/make.defaults"), "USE=\"${USE} topflag\"\n").unwrap();
        let make_profile = profile_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("top"), &make_profile).unwrap();

        let config = resolve_config(
            &root,
            &root.join("repo"),
            &[],
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("diamond inheritance must resolve");
        assert_eq!(
            config.use_flags,
            HashSet::from(["sharedflag".to_string(), "topflag".to_string()])
        );
    }

    #[test]
    fn package_mask_unmask_accept_keywords_load_correctly() {
        let root = std::env::temp_dir().join("portage-profile-test-package-star");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();

        // package.mask: two atoms added, then one removed via "-atom" --
        // only the surviving one should remain.
        fs::write(
            portage_dir.join("package.mask"),
            "dev-libs/foo\ndev-libs/bar\n-dev-libs/bar\n",
        )
        .unwrap();
        // package.unmask: unrelated to the removal above -- a completely
        // separate list, checked per-candidate by the caller.
        fs::write(portage_dir.join("package.unmask"), "dev-libs/baz\n").unwrap();
        // package.accept_keywords: a normal entry, a "**" entry, and a
        // bare-atom (no keywords) line -- this fixture sets no global
        // ACCEPT_KEYWORDS at all, so the bare atom's own real
        // accept_keywords_defaults substitution has nothing to derive a
        // "~arch" set from and it stays present with an empty token
        // list (see the dedicated accept_keywords_defaults tests below
        // for the substitution actually producing tokens).
        fs::write(
            portage_dir.join("package.accept_keywords"),
            "dev-qt/* ~amd64\nsci-misc/live-thing **\ndev-libs/bare-no-op\n",
        )
        .unwrap();

        let config = resolve_config(
            &root,
            &root.join("repo"),
            &[],
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config with package.* files must resolve");
        assert_eq!(config.package_mask, vec!["dev-libs/foo".to_string()]);
        assert_eq!(config.package_unmask, vec!["dev-libs/baz".to_string()]);
        assert_eq!(
            config.package_accept_keywords,
            vec![
                ("dev-qt/*".to_string(), vec!["~amd64".to_string()]),
                ("sci-misc/live-thing".to_string(), vec!["**".to_string()]),
                ("dev-libs/bare-no-op".to_string(), Vec::new()),
            ]
        );
    }

    #[test]
    fn package_mask_atom_removal_applies_across_repo_profile_and_user_sources() {
        // repo-level: masks a and b -- gets "::testrepo"-scoped (the
        // main repo's own repo-level entries now get the same scoping
        // an overlay's own already did).
        // profile-level (the one chain level): "-a" removes the
        // repo-level entry (its own unscoped "-dev-libs/a" still cancels
        // the now-scoped "dev-libs/a::testrepo" -- see stack_mask_lines's
        // own "ignore_repo" doc comment), adds c.
        // user-level: "-c" removes the profile-level entry, adds d.
        // Final: b::testrepo (repo, survives) + d (user) -- a and c were
        // each removed by a LATER source than the one that added them,
        // which only works if -atom removal spans all three sources, not
        // just within each file on its own.
        let root = std::env::temp_dir().join("portage-profile-test-cross-source-mask");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&repo_profiles).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(
            repo_profiles.join("package.mask"),
            "dev-libs/a\ndev-libs/b\n",
        )
        .unwrap();
        fs::write(leaf.join("package.mask"), "-dev-libs/a\ndev-libs/c\n").unwrap();
        fs::write(
            portage_dir.join("package.mask"),
            "-dev-libs/c\ndev-libs/d\n",
        )
        .unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_mask,
            vec!["dev-libs/b::testrepo".to_string(), "dev-libs/d".to_string()]
        );
    }

    #[test]
    fn scope_repo_mask_lines_adds_reponame_only_when_no_repo_part_is_present() {
        let lines = vec![
            "dev-libs/a".to_string(),
            "-dev-libs/b".to_string(),
            "dev-libs/c::already-scoped".to_string(),
        ];
        assert_eq!(
            scope_repo_mask_lines(&lines, "overlay"),
            vec![
                "dev-libs/a::overlay".to_string(),
                "-dev-libs/b::overlay".to_string(),
                "dev-libs/c::already-scoped".to_string(),
            ]
        );
    }

    #[test]
    fn overlay_package_mask_is_scoped_to_its_own_repo_only() {
        // Main repo has no mask at all; the overlay masks dev-libs/a
        // with a bare atom (no "::repo" part) -- resolve_config must
        // auto-scope it to "::overlay" via scope_repo_mask_lines, so it
        // never also masks a same-named package in the main repo.
        let root = std::env::temp_dir().join("portage-profile-test-overlay-mask");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(overlay.join("profiles/package.mask"), "dev-libs/a\n").unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let config = resolve_config(
            &root,
            &repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config must resolve");
        assert_eq!(config.package_mask, vec!["dev-libs/a::overlay".to_string()]);
    }

    #[test]
    fn overlay_package_unmask_gets_the_same_scoping_as_overlay_package_mask() {
        // Both files live in the same overlay, so both entries get the
        // identical "::overlay" auto-scoping -- the actual mask/unmask
        // cancellation happens downstream in portage-repo's is_visible,
        // which matches both against the same "::overlay"-suffixed
        // candidate string; this test only checks that resolve_config
        // itself scopes both sources consistently.
        let root = std::env::temp_dir().join("portage-profile-test-overlay-mask-unmask");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(overlay.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
        fs::write(overlay.join("profiles/package.unmask"), "dev-libs/a\n").unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let config = resolve_config(
            &root,
            &repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config must resolve");
        assert_eq!(config.package_mask, vec!["dev-libs/a::overlay".to_string()]);
        assert_eq!(
            config.package_unmask,
            vec!["dev-libs/a::overlay".to_string()]
        );
    }

    #[test]
    fn overlay_package_mask_inherits_the_main_repos_own_entries_via_implicit_masters() {
        // Real masters: an overlay with no explicit "masters =" always
        // implicitly masters the main repo -- so the main repo's own
        // package.mask entry for "dev-libs/a" must also end up
        // "::overlay"-scoped (on top of its own, separate "::testrepo"
        // scoping from being the main repo's own repo-level entry, see
        // this function's own doc comment on that follow-up), even
        // though the overlay's own package.mask never mentions
        // "dev-libs/a" at all (only "b").
        let root = std::env::temp_dir().join("portage-profile-test-overlay-masters-mask");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
        fs::write(overlay.join("profiles/package.mask"), "dev-libs/b\n").unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let config = resolve_config(
            &root,
            &repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config must resolve");
        assert_eq!(
            config.package_mask,
            vec![
                "dev-libs/a::testrepo".to_string(),
                "dev-libs/a::overlay".to_string(),
                "dev-libs/b::overlay".to_string(),
            ]
        );
    }

    #[test]
    fn overlay_package_unmask_does_not_inherit_via_masters() {
        // Real MaskManager.py's own package.unmask loop never consults
        // masters at all -- only package.mask does. The main repo's own
        // package.unmask entry must stay "::testrepo"-scoped only (its
        // own repo-level scoping, see this function's own doc comment on
        // that follow-up), never *also* "::overlay"-scoped just because
        // the overlay implicitly masters the main repo.
        let root = std::env::temp_dir().join("portage-profile-test-overlay-masters-unmask");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(repo.join("profiles/package.unmask"), "dev-libs/a\n").unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let config = resolve_config(
            &root,
            &repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config must resolve");
        assert_eq!(
            config.package_unmask,
            vec!["dev-libs/a::testrepo".to_string()]
        );
    }

    #[test]
    fn explicit_repo_masters_overrides_the_implicit_main_repo_default() {
        // Real explicit "masters =" (portage_repo::find_repos, resolved
        // into `repo_masters` by the caller) fully replaces the implicit
        // "masters the main repo alone" default this crate always fell
        // back to before -- an explicit empty masters list means NO
        // package.mask inheritance at all, even from the main repo.
        let root = std::env::temp_dir().join("portage-profile-test-explicit-masters-empty");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let repo_masters = HashMap::from([("overlay".to_string(), Vec::<PathBuf>::new())]);
        let config = resolve_config(&root, &repo, &overlay_repos, &[], "testrepo", &repo_masters)
            .expect("config must resolve");
        // The main repo's own "dev-libs/a" entry still applies to
        // itself (real portage's own repo-level package.mask always
        // masks its own repo, independent of who masters whom) -- but
        // is NOT inherited by the overlay, since its own masters is
        // explicitly empty.
        assert_eq!(
            config.package_mask,
            vec!["dev-libs/a::testrepo".to_string()]
        );
    }

    #[test]
    fn explicit_repo_masters_resolves_a_non_main_master_chain() {
        // A repo can explicitly declare a master other than the main
        // repo -- its own package.mask must be stacked in too, exactly
        // like the implicit main-repo case already is.
        let root = std::env::temp_dir().join("portage-profile-test-explicit-masters-chain");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        let downstream = root.join("downstream");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();
        fs::create_dir_all(downstream.join("profiles")).unwrap();

        // The main repo's own entry must NOT apply (downstream doesn't
        // declare it as a master); the overlay's own entry must.
        fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
        fs::write(overlay.join("profiles/package.mask"), "dev-libs/b\n").unwrap();

        let overlay_repos = [
            ("overlay".to_string(), overlay.clone()),
            ("downstream".to_string(), downstream.clone()),
        ];
        let repo_masters = HashMap::from([("downstream".to_string(), vec![overlay.clone()])]);
        let config = resolve_config(&root, &repo, &overlay_repos, &[], "testrepo", &repo_masters)
            .expect("config must resolve");
        assert!(config
            .package_mask
            .contains(&"dev-libs/b::downstream".to_string()));
        assert!(!config
            .package_mask
            .contains(&"dev-libs/a::downstream".to_string()));
    }

    #[test]
    fn overlay_package_use_is_scoped_to_its_own_repo_only() {
        // Main repo has no package.use at all; the overlay sets a flag
        // for dev-libs/a with a bare atom (no "::repo" part) --
        // resolve_config must auto-scope it to "::overlay" via
        // scope_repo_package_use_lines, the same way package.mask's own
        // overlay entries are scoped, so it never also applies to a
        // same-named package in the main repo.
        let root = std::env::temp_dir().join("portage-profile-test-overlay-package-use");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(overlay.join("profiles/package.use"), "dev-libs/a flag\n").unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let config = resolve_config(
            &root,
            &repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config must resolve");
        assert_eq!(
            config.package_use_repo,
            vec![("dev-libs/a::overlay".to_string(), vec!["flag".to_string()])]
        );
    }

    #[test]
    fn overlay_package_use_mask_and_force_are_scoped_with_no_masters_merge() {
        // Unlike package.mask, real UseManager.py never merges an
        // overlay's own package.use.mask/.force with its master's own at
        // load time (see resolve_config's own doc comment) -- so the
        // main repo's entry for "dev-libs/a" stays unscoped (applies
        // everywhere) and the overlay's own entry for "dev-libs/b" gets
        // "::overlay"-scoped, with no cross-stacking between the two.
        let root = std::env::temp_dir().join("portage-profile-test-overlay-package-use-mask-force");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(repo.join("profiles/package.use.mask"), "dev-libs/a maska\n").unwrap();
        fs::write(
            overlay.join("profiles/package.use.mask"),
            "dev-libs/b maskb\n",
        )
        .unwrap();
        fs::write(
            repo.join("profiles/package.use.force"),
            "dev-libs/a forcea\n",
        )
        .unwrap();
        fs::write(
            overlay.join("profiles/package.use.force"),
            "dev-libs/b forceb\n",
        )
        .unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let config = resolve_config(
            &root,
            &repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config must resolve");
        assert_eq!(
            config.package_use_mask,
            vec![
                ("dev-libs/a".to_string(), vec!["maska".to_string()]),
                ("dev-libs/b::overlay".to_string(), vec!["maskb".to_string()]),
            ]
        );
        assert_eq!(
            config.package_use_force,
            vec![
                ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
                (
                    "dev-libs/b::overlay".to_string(),
                    vec!["forceb".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn overlay_package_use_stable_mask_and_force_get_the_same_scoping() {
        // Mirrors overlay_package_use_mask_and_force_are_scoped_with_no_masters_merge
        // exactly, for the .stable. variant.
        let root =
            std::env::temp_dir().join("portage-profile-test-overlay-package-use-stable-mask-force");
        let repo = root.join("repo");
        let overlay = root.join("overlay");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(overlay.join("profiles")).unwrap();

        fs::write(
            repo.join("profiles/package.use.stable.mask"),
            "dev-libs/a maska\n",
        )
        .unwrap();
        fs::write(
            overlay.join("profiles/package.use.stable.mask"),
            "dev-libs/b maskb\n",
        )
        .unwrap();
        fs::write(
            repo.join("profiles/package.use.stable.force"),
            "dev-libs/a forcea\n",
        )
        .unwrap();
        fs::write(
            overlay.join("profiles/package.use.stable.force"),
            "dev-libs/b forceb\n",
        )
        .unwrap();

        let overlay_repos = [("overlay".to_string(), overlay.clone())];
        let config = resolve_config(
            &root,
            &repo,
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config must resolve");
        assert_eq!(
            config.package_use_stable_mask,
            vec![
                ("dev-libs/a".to_string(), vec!["maska".to_string()]),
                ("dev-libs/b::overlay".to_string(), vec!["maskb".to_string()]),
            ]
        );
        assert_eq!(
            config.package_use_stable_force,
            vec![
                ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
                (
                    "dev-libs/b::overlay".to_string(),
                    vec!["forceb".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn system_packages_stack_across_profile_levels_with_atom_removal_and_star_filter() {
        // base: adds "*dev-libs/a" (a real @system atom) and a bare
        // "dev-libs/hint" line (no "*" -- a real "known but not system"
        // hint, must never contribute an atom on its own).
        // leaf (its own parent -> base): "-*dev-libs/a" removes base's
        // own system atom (proving -atom removal spans levels, not just
        // within one file), and adds "*dev-libs/b".
        // Final @system list: just "dev-libs/b" -- "a" was added then
        // removed, "hint" was never eligible in the first place.
        let root = std::env::temp_dir().join("portage-profile-test-system-packages");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(base.join("packages"), "*dev-libs/a\ndev-libs/hint\n").unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(leaf.join("packages"), "-*dev-libs/a\n*dev-libs/b\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(config.system_packages, vec!["dev-libs/b".to_string()]);
    }

    #[test]
    fn package_provided_stacks_the_profile_chain_plus_the_user_file_with_atom_removal() {
        // base: provides dev-libs/a-1.0 and dev-libs/b-1.0.
        // leaf (parent -> base): removes b, adds dev-libs/c-1.0.
        // user (/etc/portage/profile/package.provided): removes a.
        // Final: just dev-libs/c-1.0.
        let root = std::env::temp_dir().join("portage-profile-test-package-provided");
        let repo = root.join("repo");
        let base = repo.join("profiles/base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();
        fs::write(
            base.join("package.provided"),
            "dev-libs/a-1.0\ndev-libs/b-1.0\n",
        )
        .unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(
            leaf.join("package.provided"),
            "-dev-libs/b-1.0\ndev-libs/c-1.0\n",
        )
        .unwrap();
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(portage_dir.join("profile")).unwrap();
        fs::write(
            portage_dir.join("profile/package.provided"),
            "-dev-libs/a-1.0\n",
        )
        .unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(config.package_provided, vec!["dev-libs/c-1.0".to_string()]);
    }

    #[test]
    fn use_mask_and_use_force_stack_across_levels_but_stay_out_of_use_flags() {
        // base: make.defaults enables "normalflag" and "maskflag"
        // normally; use.force forces on "forceflag" and "bothflag".
        // leaf (its own parent -> base): use.mask masks "maskflag" (an
        // otherwise-normal USE flag) and "bothflag" (proving mask wins
        // when a flag is both forced AND masked, matching real
        // regenerate()'s update-then-difference_update order).
        // use_force/use_mask stack correctly across levels -- but,
        // unlike an earlier version of this pilot, are deliberately NOT
        // folded into use_flags at all: real regenerate() applies them
        // as the literal last step of its own incremental USE walk,
        // strictly after package.use, so `use_flags` here stays exactly
        // what make.defaults/make.conf alone produced (see
        // `use_force`'s own doc comment for the full grounding);
        // `portage-repo`'s own `effective_use_flags` applies
        // `use_force`/`use_mask` at that later, correct position
        // instead.
        let root = std::env::temp_dir().join("portage-profile-test-use-mask-force");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(base.join("make.defaults"), "USE=\"normalflag maskflag\"\n").unwrap();
        fs::write(base.join("use.force"), "forceflag\nbothflag\n").unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(leaf.join("use.mask"), "maskflag\nbothflag\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.use_flags,
            HashSet::from(["normalflag".to_string(), "maskflag".to_string()])
        );
        assert_eq!(
            config.use_force,
            HashSet::from(["forceflag".to_string(), "bothflag".to_string()])
        );
        assert_eq!(
            config.use_mask,
            HashSet::from(["maskflag".to_string(), "bothflag".to_string()])
        );
    }

    #[test]
    fn archlist_stacks_across_profile_levels_with_removal_semantics() {
        // base: arch.list declares "amd64" and "x86". leaf (its own
        // parent -> base): arch.list removes "x86" (a "-x86" line) and
        // adds "arm64" -- proving the same chain-order, "-entry"-removal
        // stacking use.mask/use.force already get (real config.py's own
        // stack_lists(archlist, incremental=1)).
        let root = std::env::temp_dir().join("portage-profile-test-archlist");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(base.join("arch.list"), "amd64\nx86\n").unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(leaf.join("arch.list"), "-x86\narm64\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.archlist,
            HashSet::from(["amd64".to_string(), "arm64".to_string()])
        );
    }

    #[test]
    fn use_tokens_capture_the_ordered_raw_use_values_that_produced_use_flags() {
        // base: make.defaults sets USE="foo". leaf (its own parent ->
        // base): make.defaults sets USE="-foo bar". make.conf: USE="baz".
        // use_flags ends up {bar, baz} either way -- but the token lists
        // must retain each raw contribution *separately*, in real
        // accumulation order: `use_tokens` = the profile chain
        // (`defaults` layer), `conf_use_tokens` = make.conf (`conf`
        // layer, split out by the "Config depth" slice). Replaying
        // use_tokens then conf_use_tokens via apply_incremental from an
        // empty set reproduces use_flags exactly. This is what lets
        // portage-repo's own effective_use_flags replay them on top of
        // a *different* seed (a package's own IUSE defaults) instead of
        // just union-ing the pre-flattened use_flags on top, which could
        // never let "-foo" cancel an IUSE "+foo" default.
        let root = std::env::temp_dir().join("portage-profile-test-use-tokens");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(base.join("make.defaults"), "USE=\"foo\"\n").unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(leaf.join("make.defaults"), "USE=\"-foo bar\"\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();
        fs::write(portage_dir.join("make.conf"), "USE=\"baz\"\n").unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.use_tokens,
            vec!["foo".to_string(), "-foo bar".to_string()]
        );
        assert_eq!(config.conf_use_tokens, vec!["baz".to_string()]);
        let mut replayed = HashSet::new();
        for token in config.use_tokens.iter().chain(&config.conf_use_tokens) {
            apply_incremental(token, &mut replayed);
        }
        assert_eq!(replayed, config.use_flags);
    }

    #[test]
    fn use_expand_variable_names_stack_incrementally_across_profile_levels() {
        // base declares USE_EXPAND="VIDEO_CARDS" and VIDEO_CARDS="nvidia";
        // leaf (its own parent -> base) declares USE_EXPAND="PYTHON_TARGETS"
        // (incremental add, not a replace -- both variable names must end
        // up recognized) and PYTHON_TARGETS="python3_11". Each variable's
        // own value is set at only one level, proving expansion works
        // regardless of which level actually declared USE_EXPAND for it.
        let root = std::env::temp_dir().join("portage-profile-test-use-expand-names-stack");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(
            base.join("make.defaults"),
            "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n",
        )
        .unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(
            leaf.join("make.defaults"),
            "USE_EXPAND=\"PYTHON_TARGETS\"\nPYTHON_TARGETS=\"python3_11\"\n",
        )
        .unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.use_expand,
            HashSet::from(["VIDEO_CARDS".to_string(), "PYTHON_TARGETS".to_string()])
        );
        assert!(config.use_flags.contains("video_cards_nvidia"));
        assert!(config.use_flags.contains("python_targets_python3_11"));
    }

    #[test]
    fn use_expand_variable_value_expands_with_negation_and_plus_stripped() {
        // A single USE_EXPAND variable's own value exercises all three
        // ordinary incremental token forms in one pass (real config.py's
        // own early-expand loop keeps a "-" token's own "-" outside the
        // new prefix, and a "+" token has it stripped, same as any
        // ordinary USE token): "nvidia" adds, "+intel" adds (with the
        // "+" stripped, not baked into the flag name), "-nvidia" then
        // removes the flag "nvidia" itself already added earlier in the
        // very same value list -- final: only video_cards_intel remains.
        let root = std::env::temp_dir().join("portage-profile-test-use-expand-negation");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        fs::create_dir_all(&base).unwrap();

        fs::write(
            base.join("make.defaults"),
            "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia +intel -nvidia\"\n",
        )
        .unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&base, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert!(!config.use_flags.contains("video_cards_nvidia"));
        assert!(config.use_flags.contains("video_cards_intel"));
        assert!(!config.use_flags.contains("+video_cards_intel"));
    }

    #[test]
    fn use_expand_unprefixed_variable_contributes_a_bare_flag_no_prefix() {
        // Real Gentoo's own USE_EXPAND_UNPREFIXED="ARCH" mechanism:
        // ARCH="amd64" must contribute the bare flag "amd64" directly,
        // NOT "arch_amd64" -- the defining difference from an ordinary
        // USE_EXPAND variable, which use_expand_variable_names_stack_
        // incrementally_across_profile_levels above already covers.
        let root = std::env::temp_dir().join("portage-profile-test-use-expand-unprefixed");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        fs::create_dir_all(&base).unwrap();

        fs::write(
            base.join("make.defaults"),
            "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"amd64\"\n",
        )
        .unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&base, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.use_expand_unprefixed,
            HashSet::from(["ARCH".to_string()])
        );
        assert!(config.use_flags.contains("amd64"));
        assert!(!config.use_flags.contains("arch_amd64"));
    }

    #[test]
    fn use_expand_unprefixed_variable_value_expands_with_negation_and_plus_stripped() {
        // Same three ordinary incremental token forms
        // use_expand_variable_value_expands_with_negation_and_plus_stripped
        // above already covers for a prefixed USE_EXPAND variable, but
        // for an unprefixed one: "foo" adds, "+bar" adds ("+" stripped),
        // "-foo" then removes "foo" itself, added earlier in the same
        // value list -- final: only "bar" remains, no prefix on either.
        let root = std::env::temp_dir().join("portage-profile-test-use-expand-unprefixed-negation");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        fs::create_dir_all(&base).unwrap();

        fs::write(
            base.join("make.defaults"),
            "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"foo +bar -foo\"\n",
        )
        .unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&base, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert!(!config.use_flags.contains("foo"));
        assert!(config.use_flags.contains("bar"));
        assert!(!config.use_flags.contains("+bar"));
    }

    #[test]
    fn package_accept_keywords_stacks_profile_chain_then_user_no_repo_source() {
        // Profile-level entry for "a", user-level entry for "b" -- both
        // must appear, in that order (profile-chain first, matching real
        // KeywordsManager.getPKeywords), proving there's no repo-level
        // source at all (only the pilot's existing repo/profiles/package.mask
        // convention would exist if there were one, and this test
        // deliberately never creates that file).
        let root = std::env::temp_dir().join("portage-profile-test-accept-keywords-stack");
        let repo = root.join("repo");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(leaf.join("package.accept_keywords"), "dev-libs/a ~amd64\n").unwrap();
        fs::write(
            portage_dir.join("package.accept_keywords"),
            "dev-libs/b ~amd64\ndev-libs/bare-no-op\n",
        )
        .unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_accept_keywords,
            vec![
                ("dev-libs/a".to_string(), vec!["~amd64".to_string()]),
                ("dev-libs/b".to_string(), vec!["~amd64".to_string()]),
                // No global ACCEPT_KEYWORDS is set anywhere in this
                // fixture, so the bare atom's own accept_keywords_defaults
                // substitution has nothing to derive from and it stays
                // present with an empty token list.
                ("dev-libs/bare-no-op".to_string(), Vec::new()),
            ]
        );
    }

    #[test]
    fn accept_keywords_defaults_substitutes_tilde_arch_for_a_bare_atom() {
        // Real accept_keywords_defaults: a bare package.accept_keywords
        // atom means "~" + every plain token in the *current* global
        // ACCEPT_KEYWORDS -- here just "amd64", so "dev-libs/foo" gets
        // an implicit "~amd64", the same as if the user had written
        // "dev-libs/foo ~amd64" themselves.
        let root = std::env::temp_dir().join("portage-profile-test-accept-keywords-defaults");
        let repo = root.join("repo");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&leaf).unwrap();
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();

        fs::write(leaf.join("make.defaults"), "ACCEPT_KEYWORDS=\"amd64\"\n").unwrap();
        fs::write(
            portage_dir.join("package.accept_keywords"),
            "dev-libs/foo\n",
        )
        .unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_accept_keywords,
            vec![("dev-libs/foo".to_string(), vec!["~amd64".to_string()])]
        );
    }

    #[test]
    fn accept_keywords_defaults_excludes_already_prefixed_tokens() {
        // Real accept_keywords_defaults only derives from *plain* global
        // ACCEPT_KEYWORDS tokens (keyword[:1] not in "~-") -- an
        // already-"~"-prefixed or "-"-prefixed global token is excluded
        // entirely, not doubly-prefixed or left bare.
        let root =
            std::env::temp_dir().join("portage-profile-test-accept-keywords-defaults-filter");
        let repo = root.join("repo");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&leaf).unwrap();
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();

        fs::write(
            leaf.join("make.defaults"),
            "ACCEPT_KEYWORDS=\"amd64 ~x86 -arm\"\n",
        )
        .unwrap();
        fs::write(
            portage_dir.join("package.accept_keywords"),
            "dev-libs/foo\n",
        )
        .unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_accept_keywords,
            vec![("dev-libs/foo".to_string(), vec!["~amd64".to_string()])]
        );
    }

    #[test]
    fn package_use_splits_repo_profile_and_user_into_their_own_fields() {
        // Repo-level entry for "a", profile-level entry for "b",
        // user-level entry for "c" -- each lands in its own `Config`
        // field at its own real USE_ORDER position (the "Config depth"
        // slice), proving no source is silently dropped and no `-atom`
        // removal happens anywhere (package.use is purely additive,
        // unlike package.mask).
        let root = std::env::temp_dir().join("portage-profile-test-package-use-stack");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&repo_profiles).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(repo_profiles.join("package.use"), "dev-libs/a flaga\n").unwrap();
        fs::write(leaf.join("package.use"), "dev-libs/b flagb\n").unwrap();
        fs::write(portage_dir.join("package.use"), "dev-libs/c flagc\n").unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_use_repo,
            vec![("dev-libs/a".to_string(), vec!["flaga".to_string()])]
        );
        assert_eq!(
            config.package_use,
            vec![("dev-libs/b".to_string(), vec!["flagb".to_string()])]
        );
        assert_eq!(
            config.package_use_user,
            vec![("dev-libs/c".to_string(), vec!["flagc".to_string()])]
        );
    }

    /// Serial env-scoping guard -- `resolve_config` reads the process
    /// environment (`apply_env_layer`), which parallel test threads share.
    fn with_env(vars: &[(&str, &str)], f: impl FnOnce()) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        f();
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
    }

    #[test]
    fn env_layer_overrides_config_vars() {
        // A minimal profile: ARCH/ACCEPT_KEYWORDS + a USE_EXPAND var, so
        // the env layer's incremental (ACCEPT_KEYWORDS, USE), last-wins
        // (a USE_EXPAND value + a plain scalar) and USE_EXPAND-name paths
        // are all exercised.
        let root = std::env::temp_dir().join("portage-profile-test-env-layer");
        let repo = root.join("repo");
        let prof = repo.join("profiles/default");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&prof).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::write(
            prof.join("make.defaults"),
            "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n\
             USE_EXPAND_UNPREFIXED=\"ARCH\"\nUSE=\"baseflag\"\n\
             USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n",
        )
        .unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&prof, &make_profile).unwrap();

        // No env: baseline.
        with_env(
            &[
                ("ACCEPT_KEYWORDS", ""),
                ("USE", ""),
                ("VIDEO_CARDS", ""),
                ("CFLAGS", ""),
            ],
            || {
                for k in ["ACCEPT_KEYWORDS", "USE", "VIDEO_CARDS", "CFLAGS"] {
                    std::env::remove_var(k);
                }
                let c = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
                    .expect("resolves");
                assert!(c.accept_keywords.contains("amd64"));
                assert!(!c.accept_keywords.contains("~amd64"));
                assert!(c.use_flags.contains("video_cards_nvidia"));
                assert!(!c.other_vars.contains_key("CFLAGS"));
            },
        );

        with_env(
            &[
                ("ACCEPT_KEYWORDS", "~amd64"),
                ("USE", "-baseflag envflag"),
                ("VIDEO_CARDS", "amdgpu"),
                ("CFLAGS", "-O3"),
            ],
            || {
                let c = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
                    .expect("resolves");
                // ACCEPT_KEYWORDS: incremental -- profile amd64 kept, env ~amd64 added.
                assert!(c.accept_keywords.contains("amd64"));
                assert!(c.accept_keywords.contains("~amd64"));
                // USE: env stacks on the conf layer.
                assert!(c.use_flags.contains("envflag"));
                assert!(!c.use_flags.contains("baseflag"));
                // USE_EXPAND value: env replaces the profile value.
                assert!(c.use_flags.contains("video_cards_amdgpu"));
                assert!(!c.use_flags.contains("video_cards_nvidia"));
                // Plain scalar -> other_vars (emerge --info).
                assert_eq!(c.other_vars.get("CFLAGS").map(String::as_str), Some("-O3"));
            },
        );
    }

    #[test]
    fn package_use_expand_shorthand_applies_prefix_to_following_tokens_on_the_same_line() {
        // User-level "dev-libs/a VIDEO_CARDS: nvidia -intel plainflag" --
        // "nvidia"/"intel" get the video_cards_ prefix (negation kept
        // outside it), "plainflag" (before the "VIDEO_CARDS:" marker)
        // does not.
        let root = std::env::temp_dir().join("portage-profile-test-package-use-expand-shorthand");
        let repo = root.join("repo");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();

        fs::write(
            portage_dir.join("package.use"),
            "dev-libs/a plainflag VIDEO_CARDS: nvidia -intel\n",
        )
        .unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_use_user,
            vec![(
                "dev-libs/a".to_string(),
                vec![
                    "plainflag".to_string(),
                    "video_cards_nvidia".to_string(),
                    "-video_cards_intel".to_string(),
                ]
            )]
        );
    }

    #[test]
    fn package_use_expand_shorthand_resets_at_the_start_of_each_line() {
        // Two lines for the SAME atom: the first sets a shorthand prefix
        // that must not leak into the second line's own tokens.
        let root = std::env::temp_dir().join("portage-profile-test-package-use-expand-reset");
        let repo = root.join("repo");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();

        fs::write(
            portage_dir.join("package.use"),
            "dev-libs/a VIDEO_CARDS: nvidia\ndev-libs/a plainflag\n",
        )
        .unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_use_user,
            vec![
                (
                    "dev-libs/a".to_string(),
                    vec!["video_cards_nvidia".to_string()]
                ),
                ("dev-libs/a".to_string(), vec!["plainflag".to_string()]),
            ]
        );
    }

    #[test]
    fn package_use_expand_shorthand_is_user_level_only() {
        // The same "VIDEO_CARDS:" syntax in a repo-level or profile-level
        // package.use file is genuine real behavior's own literal,
        // unexpanded token -- real portage's shorthand support is
        // user-only (see parse_package_use_lines's own doc comment).
        let root = std::env::temp_dir().join("portage-profile-test-package-use-expand-user-only");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        fs::create_dir_all(&repo_profiles).unwrap();
        fs::create_dir_all(root.join("etc/portage")).unwrap();

        fs::write(
            repo_profiles.join("package.use"),
            "dev-libs/a VIDEO_CARDS: nvidia\n",
        )
        .unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_use_repo,
            vec![(
                "dev-libs/a".to_string(),
                vec!["VIDEO_CARDS:".to_string(), "nvidia".to_string()]
            )]
        );
    }

    #[test]
    fn package_use_mask_and_force_stack_repo_then_profile_only_no_user_source() {
        // Repo-level entry for "a", profile-level entry for "b" -- both
        // must appear, in that order; a user-level package.use.mask/
        // .force file is deliberately written too, and must be
        // completely ignored -- real portage has no such source at all
        // (confirmed by reading UseManager.__init__'s own file/variable
        // table), unlike package.use itself.
        let root = std::env::temp_dir().join("portage-profile-test-package-use-mask-force");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&repo_profiles).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(repo_profiles.join("package.use.mask"), "dev-libs/a maska\n").unwrap();
        fs::write(leaf.join("package.use.mask"), "dev-libs/b maskb\n").unwrap();
        fs::write(portage_dir.join("package.use.mask"), "dev-libs/c maskc\n").unwrap();
        fs::write(
            repo_profiles.join("package.use.force"),
            "dev-libs/a forcea\n",
        )
        .unwrap();
        fs::write(leaf.join("package.use.force"), "dev-libs/b forceb\n").unwrap();
        fs::write(portage_dir.join("package.use.force"), "dev-libs/c forcec\n").unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_use_mask,
            vec![
                ("dev-libs/a".to_string(), vec!["maska".to_string()]),
                ("dev-libs/b".to_string(), vec!["maskb".to_string()]),
            ]
        );
        assert_eq!(
            config.package_use_force,
            vec![
                ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
                ("dev-libs/b".to_string(), vec!["forceb".to_string()]),
            ]
        );
    }

    #[test]
    fn use_stable_mask_and_force_stack_profile_chain_only_and_stay_out_of_use_flags() {
        // use.stable.mask/.force: profile-chain only (no repo-level
        // source, matching use_mask/use_force's own existing profile-
        // chain-only sourcing) -- and, unlike use_mask/use_force,
        // deliberately never folded into use_flags itself (see
        // use_stable_force's own doc comment: stability is per-candidate,
        // so portage-repo applies these conditionally instead).
        let root = std::env::temp_dir().join("portage-profile-test-use-stable-mask-force");
        let repo = root.join("repo");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(leaf.join("use.stable.mask"), "stablemaskflag\n").unwrap();
        fs::write(leaf.join("use.stable.force"), "stableforceflag\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.use_stable_mask,
            HashSet::from(["stablemaskflag".to_string()])
        );
        assert_eq!(
            config.use_stable_force,
            HashSet::from(["stableforceflag".to_string()])
        );
        assert!(!config.use_flags.contains("stablemaskflag"));
        assert!(!config.use_flags.contains("stableforceflag"));
    }

    #[test]
    fn package_use_stable_mask_and_force_stack_repo_then_profile_only_no_user_source() {
        // Mirrors package_use_mask_and_force_stack_repo_then_profile_only_no_user_source
        // exactly, for the stable variant: repo-level entry for "a",
        // profile-level entry for "b", a deliberately-written user-level
        // file completely ignored (no such source exists in real
        // portage).
        let root = std::env::temp_dir().join("portage-profile-test-package-use-stable-mask-force");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&repo_profiles).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(
            repo_profiles.join("package.use.stable.mask"),
            "dev-libs/a maska\n",
        )
        .unwrap();
        fs::write(leaf.join("package.use.stable.mask"), "dev-libs/b maskb\n").unwrap();
        fs::write(
            portage_dir.join("package.use.stable.mask"),
            "dev-libs/c maskc\n",
        )
        .unwrap();
        fs::write(
            repo_profiles.join("package.use.stable.force"),
            "dev-libs/a forcea\n",
        )
        .unwrap();
        fs::write(leaf.join("package.use.stable.force"), "dev-libs/b forceb\n").unwrap();
        fs::write(
            portage_dir.join("package.use.stable.force"),
            "dev-libs/c forcec\n",
        )
        .unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new())
            .expect("config must resolve");
        assert_eq!(
            config.package_use_stable_mask,
            vec![
                ("dev-libs/a".to_string(), vec!["maska".to_string()]),
                ("dev-libs/b".to_string(), vec!["maskb".to_string()]),
            ]
        );
        assert_eq!(
            config.package_use_stable_force,
            vec![
                ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
                ("dev-libs/b".to_string(), vec!["forceb".to_string()]),
            ]
        );
    }

    #[test]
    fn package_use_loads_tokens_and_skips_bare_atom_lines() {
        let root = std::env::temp_dir().join("portage-profile-test-package-use");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        fs::write(
            portage_dir.join("package.use"),
            "dev-libs/foo flag1 -flag2\n*/bar +flag3\ndev-libs/bare-no-op\n",
        )
        .unwrap();

        let config = resolve_config(
            &root,
            &root.join("repo"),
            &[],
            &[],
            "testrepo",
            &HashMap::new(),
        )
        .expect("config with package.use must resolve");
        assert_eq!(
            config.package_use_user,
            vec![
                (
                    "dev-libs/foo".to_string(),
                    vec!["flag1".to_string(), "-flag2".to_string()]
                ),
                ("*/bar".to_string(), vec!["+flag3".to_string()]),
            ]
        );
    }

    #[test]
    fn apply_incremental_is_reusable_for_per_package_use_overrides() {
        let mut set = HashSet::from(["foo".to_string()]);
        apply_incremental("flag1 -foo +flag2", &mut set);
        assert_eq!(
            set,
            HashSet::from(["flag1".to_string(), "flag2".to_string()])
        );
    }
}
