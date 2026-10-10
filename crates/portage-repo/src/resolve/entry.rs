//! Graph entries and their edges.

use crate::*;

/// What resolves a blocker row that is not
/// [`BlockerConflict::unsolvable`] (#68/#72 B1). Real's
/// `_serialize_tasks` only *appends* a solved blocker to the merge list
/// when its uninstall task was scheduled (`depgraph.py:10190`,
/// `:10351-10358`), and a `--columns` display suppresses it regardless
/// (`output.py:120`); carrying the reason lets the renderer decide
/// placement without re-deriving it from the entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerSatisfiedBy {
    /// A merge-bound entry of the blocked cp replaces the installed match
    /// **in its own slot** (`_package_tracker.add_pkg` discards the
    /// superseded installed node), so the replacement merge resolves the
    /// block. `cp` is `(category, package)`; `slot` is the replaced
    /// instance's slot. `TEST/findings/l0.md` "#68/#72 B0b": real prints
    /// the inline `b` for this shape exactly when the replacement waits
    /// on the owner's merge (p2c/p2d/p2e), never otherwise (flat p2b).
    Replacement { cp: (String, String), slot: String },
    /// An uninstall task resolves the block: `cpv` is the installed
    /// package real's `operation="uninstall"` task removes, and is the
    /// `PretendOutcome::Uninstall` entry's own cpv. Two arms produce one
    /// (`_validate_blockers`' `depends_on_order` step):
    ///
    /// - a **merging** owner with an installed match removes the match
    ///   (`depends_on_order.add((pkg, parent))`, `depgraph.py:9195-9197`);
    ///   `anchor` is then the owner.
    /// - a **nomerge** (installed) owner whose blocker meets a merge-bound
    ///   package removes the *owner* (`depends_on_order.add((parent,
    ///   pkg))`, `:9204-9206`); `anchor` is the merge-bound blocked
    ///   instance. #77 A1 adds this arm (the vdb-scan owner is never
    ///   walked, so its row rides its own removal entry).
    Uninstall {
        cpv: String,
        /// Real's `inst_task` (`:9226-9242`): the `cat/pkg` the removal is
        /// ordered after. `merge_order::build_digraph`'s dedicated
        /// `Uninstall.required_by` pass adds the owner-first edge, so this
        /// is exactly which side must be selected before the removal.
        anchor: (String, String),
    },
}

/// A blocker atom (from a package's own dependency strings) that matches
/// either a currently-installed package or another package this same
/// `resolve_pretend_graph` run would also newly merge/upgrade. Purely
/// informational (see `resolve_pretend_graph`'s doc comment): v1 makes no
/// attempt to resolve or refuse anything on account of a blocker, the
/// same "report, don't enforce" spirit as `PretendOutcome::NoVisibleCandidate`
/// for an unresolvable dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerConflict {
    /// The raw blocker atom text, e.g. `"!!dev-libs/foo"`.
    pub atom_str: String,
    /// `true` for a strong (`!!`) blocker, `false` for a weak (`!`) one.
    pub strong: bool,
    pub matched_category: String,
    pub matched_package: String,
    pub matched_version: String,
    /// Real `_serialize_tasks`' `unresolved_blocks`: the blocked package
    /// is installed, can't be replaced or unmerged (it still has other
    /// installed consumers), so the block cannot be resolved -- real
    /// counts it in `Conflict: N blocks (M unsatisfied)`, prints the
    /// `* Error: The above package list …` block, and exits 1.
    pub unsolvable: bool,
    /// What resolves this row (#68/#72 B1). Only the `Replacement` arm is
    /// classified in B1, so today this is `Some(Replacement { .. })` for a
    /// replaced-in-slot installed match and `None` for every other row
    /// (unsolvable and not-yet-classified alike); #72 B2/B2c/B3/B4 narrow
    /// `None` to the unsolvable rows and tag the uninstall-resolved ones.
    /// `Replacement` rows are carried on the owner but hidden by the
    /// renderer until B2 models real's scheduling predicate.
    pub satisfied_by: Option<BlockerSatisfiedBy>,
    /// Backlog #81: whether tree-mode serialization solves this row.
    /// Real's tree serializer disables the greedy leaf pop, so it can
    /// stall with this blocker pending and schedule the replaced
    /// instance's uninstall (`scheduled_uninstalls`), appending the
    /// solved row -- while the flat greedy pop drains past it and the
    /// row stays hidden. Set resolve-side by the tree-mode simulation
    /// (`merge_order::tree_solved_replacements`); read only by `--tree`
    /// display (row visibility) and the tree counters. Flat display,
    /// `--json` and exit codes never consult it.
    pub tree_scheduled_uninstall: bool,
}

/// One installed package portuale's renderer needs to name in an
/// `[old-ver]` column -- version plus the slot/sub-slot/repo `emerge -pv`
/// decorates it with (real `convert_myoldbest` at verbosity 3). `repo`
/// is the vdb `repository` file's contents, or `"__unknown__"` when it's
/// absent (real `portage.versions._unknown_repo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRef {
    pub version: String,
    pub slot: String,
    pub sub_slot: String,
    pub repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEntry {
    pub category: String,
    pub package: String,
    pub outcome: PretendOutcome,
    /// Only ever non-empty for `New`/`Upgrade` entries -- an entry's own
    /// DEPEND/RDEPEND (and therefore its blockers) are only read when it
    /// would newly merge or upgrade, same as dependency recursion itself.
    pub blockers: Vec<BlockerConflict>,
    /// `Some(slot)` only for `New`/`Upgrade` entries (the ones a slot can
    /// meaningfully be tracked for -- see `resolve_pretend_graph`'s doc
    /// comment on multi-slot support), `None` for `AlreadyInstalled`/
    /// `NoVisibleCandidate`.
    pub slot: Option<String>,
    /// The resolved candidate's own sub-slot and repo name -- `Some` for
    /// the same `New`/`Upgrade`/`Downgrade`/`Reinstall` entries `slot` is
    /// `Some` for. Feed real `output.py::_append_slot` /
    /// `_append_repository`, which decorate the bracket cpv with
    /// `:slot`/`:slot/sub_slot`/`::repo` at `emerge -pv` (verbosity 3).
    pub sub_slot: Option<String>,
    pub repo_name: Option<String>,
    /// The installed version(s) this entry replaces -- real
    /// `pkg_info.oldbest_list`, the `blue("[…]")` column after the cpv.
    /// One entry for an `Upgrade`/`Downgrade` (the in-slot installed
    /// version); every installed version (all slots) for a new-slot
    /// `New`; empty for a brand-new `New` and
    /// `AlreadyInstalled`/`NoVisibleCandidate`; the same-version
    /// installed instance for a `Reinstall` whose slot, sub-slot or
    /// repo drifts (else empty). Carries each old
    /// package's own slot/sub_slot/repo so `convert_myoldbest` can
    /// decorate it the same way `_append_slot`/`_append_repository`
    /// decorate the main cpv at `-pv`.
    pub oldbest: Vec<InstalledRef>,
    /// This package's own IUSE-declared flags (default markers stripped),
    /// each paired with whether `effective_use_flags` resolved it enabled
    /// -- alphabetically sorted, matching real `--pretend -v`'s own
    /// ordering. Only ever non-empty for `New`/`Upgrade` entries whose
    /// md5-cache metadata was readable, same as `blockers`. Always
    /// computed regardless of `--verbose` (cheap; the CLI layer decides
    /// whether to print it) -- see pretend.rs.
    pub use_flags_display: Vec<(String, bool)>,
    /// The same flags as `use_flags_display`, but rendered the way real
    /// `emerge --pretend -v` shows them: split into the plain `USE` group
    /// plus one group per `Config::use_expand` variable whose
    /// `lowercase(name)_` prefixes the flag (prefix stripped), with
    /// `Config::use_expand_hidden` groups omitted -- real
    /// `output.py::_display_use`/`map_to_use_expand`/`_create_use_string`
    /// (see `build_use_expand_display`). `(VAR_NAME, "flag -flag …")`
    /// pairs, `USE` first, then the `USE_EXPAND` vars in sorted order;
    /// only non-empty groups appear, so this is `[]` whenever
    /// `use_flags_display` is (and `pretend.rs` prints no `USE=` line).
    /// The `--json` `use_flags` map keeps the raw, ungrouped flags
    /// instead -- more useful programmatically, and real `--json` has no
    /// USE display of its own to match.
    ///
    /// This field is the `-pv` form (real `all_flags = verbosity == 3`,
    /// always on for `emerge -pv`): every flag shown, unchanged ones
    /// plain, plus the `(-flag%)` removed-from-IUSE list. See
    /// `use_expand_display_p` for the plain `-p` form.
    pub use_expand_display: Vec<(String, String)>,
    /// The plain `emerge -p` (`all_flags` off) form of `use_expand_display`:
    /// real `_create_use_string` still runs at the default verbosity 2
    /// (`print_use_string = verbosity != 1`), but leaves an *unchanged*
    /// flag (and any removed-from-IUSE flag) omitted. So for a
    /// `Reinstall`/`Upgrade`/`Downgrade` this is only the changed flags
    /// (`flag%*`/`flag*`/`-flag%`/`-flag*`), often empty; for a `New`
    /// (`is_new` -> every flag renders) it equals `use_expand_display`.
    pub use_expand_display_p: Vec<(String, String)>,
    /// Real `output.py::gen_mask_str`'s own one-character mask column
    /// (`PkgAttrDisplay.mask`). Real `set_pkg_info` fills it in `if
    /// self.include_mask_str()` (`verbosity > 1`), and real default
    /// `emerge -p` verbosity is 2 -- so the column shows at plain `-p`
    /// *and* `-pv`, absent only under `--quiet` (verbosity 1), which
    /// portuale doesn't model (`pretend.rs::attr_display_field` always
    /// renders it). `'#'` for a candidate hard-masked in some
    /// profile/`package.mask` but pulled in anyway (`isHardMasked`, wins
    /// first), `'~'` for one visible only via a `~<our-arch>` testing
    /// keyword (`get_keyword_mask` "unstable"), `'*'` for one visible
    /// only via `**` or a different arch's keyword ("missing"). `None`
    /// for an ordinary, globally-keyword-visible, unmasked candidate
    /// (rendered as a bare space). See `keyword_mask_marker`.
    pub keyword_mask: Option<char>,
    /// Real `output.py::_get_installed_best`'s own `new_slot` flag -- the
    /// `S` bracket column (`PkgAttrDisplay.new_slot`, rendered
    /// unconditionally, not just under `-v` like `keyword_mask`): `true`
    /// for a `New` entry whose `category/package` *is* installed, just in
    /// some other slot (real `not myinslotlist` while `vardb.match(pkg.cp)`
    /// is non-empty). `resolve_pretend` already makes the New/Upgrade/
    /// Downgrade/Reinstall decision slot-aware (installed matching is
    /// filtered to the resolved candidate's own main slot), so a `New`
    /// outcome here already means nothing is installed *in this slot* --
    /// this field only additionally records whether another slot is.
    /// Always `false` for every non-`New` outcome.
    pub new_slot: bool,
    /// Real `output.py:833`'s own `attr_display.interactive` (the `I`
    /// bracket column, rendered before the `N`/`r` code letter):
    /// `"interactive" in pkg.properties and pkg.operation == "merge"` --
    /// `pkg.properties` being `PROPERTIES` after real USE-conditional
    /// evaluation against this candidate's own effective USE
    /// (`_PackageMetadataWrapper.__getitem__`, gated on `"?" in v`).
    /// `true` only for a merge-bound entry (`New`/`Upgrade`/`Downgrade`/
    /// `Reinstall`) whose evaluated `PROPERTIES` contains `interactive`;
    /// always `false` for `AlreadyInstalled`/`NoVisibleCandidate`.
    pub interactive: bool,
    /// Real `output.py:633`'s own `attr_display.fetch_restrict` (the
    /// `f`/`F` bracket column, after the `S`/`R` one): `true` for a
    /// merge-bound *ebuild* entry (`not pkg.built`) whose evaluated
    /// `RESTRICT` contains `fetch` -- a package portage will not
    /// auto-download, only ever `false` for a binary candidate. See
    /// `fetch_restrict_satisfied` for the `f`-vs-`F` split.
    pub fetch_restrict: bool,
    /// Real `output.py:636`: `not getfetchsizes(cpv, useflags=…,
    /// only_restricted=True)` -- every one of this candidate's own
    /// `SRC_URI` distfiles (flattened against its effective USE) is
    /// already present in `DISTDIR` at the size its `Manifest` records.
    /// Only meaningful when `fetch_restrict` is `true`: `true` -> the
    /// green `f` column ("nothing to fetch"), `false` -> the red `F`
    /// ("fetch these by hand"). A `SRC_URI` portuale can't parse, or a
    /// missing `Manifest` entry, counts as not-satisfied (`F`, the loud
    /// choice).
    pub fetch_restrict_satisfied: bool,
    /// Real `output.py:300-332`'s own `_calc_size` input to
    /// `counters.totalsize` -- `(filename, size)` for each of this
    /// merge-bound *ebuild* entry's own `SRC_URI` distfiles not already
    /// in `DISTDIR` at its `Manifest` size (`db.getfetchsizes(cpv,
    /// useflags=pkg.use)`, no `only_restricted`). `(filename, _)` is
    /// carried, not just a summed count, so `package_counters_summary`
    /// can dedup a distfile shared by two entries once, exactly as real
    /// portage's own `myfetchlist` does. Empty for every non-`New`/
    /// `Upgrade`/`Downgrade`/`Reinstall` entry, for a binary candidate
    /// (real `_calc_size` runs for binaries too, but portuale has no
    /// remote-binpkg fetch so a local `PKGDIR` binary is always already
    /// present -> 0), and for an unparsable `SRC_URI` / incomplete
    /// `Manifest` (real `getfetchsizes` returns `None` and `_calc_size`
    /// adds nothing).
    pub download_files: Vec<(String, u64)>,
    /// Every `(category, package)` that reached this entry via its own
    /// DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND (sorted, deduplicated) --
    /// empty for a directly-requested top-level atom with no other
    /// owner. A package required by more than one parent (a diamond
    /// dependency) lists every one of them, not just whichever reached
    /// it first -- tracked separately from the BFS's own dedup/
    /// recursion decisions (`visited_atoms`/`resolved_slots`/
    /// `other_outcomes`), which only ever decide whether to *resolve*
    /// an atom again, never whether to *record* who asked for it. See
    /// `resolve_pretend_graph`'s own doc comment.
    pub required_by: Vec<(String, String)>,
    /// Ebuild or binary -- only meaningful for `New`/`Upgrade`/
    /// `Reinstall` entries (defaults to `Ebuild` for everything else,
    /// unobserved since `AlreadyInstalled`/`NoVisibleCandidate` never
    /// print a bracket word distinguishing the two). See
    /// `CandidateSource`'s own doc comment; `pretend.rs` uses this to
    /// print `"[binary"` instead of `"[ebuild"`.
    pub source: CandidateSource,
    /// `--json`'s own state-change trace -- see `VisibilityProvenance`'s
    /// own doc comment. `Default` (every field `None`) for
    /// `AlreadyInstalled`/`NoVisibleCandidate` entries, which never pick
    /// a fresh repo/PKGDIR candidate to trace at all, same scope cut as
    /// `slot`/`use_flags_display` above.
    pub provenance: VisibilityProvenance,
    /// `--autounmask`'s own keyword-suggestion sub-feature (see
    /// `resolve_pretend_graph`'s own doc comment), extended to a
    /// *dependency's* own `NoVisibleCandidate` -- previously deliberately
    /// out of scope (portuale's own v1 only ever suggested something
    /// for a top-level atom's own fatal `NoVisibleCandidate`, which
    /// aborts the whole call and never reaches a `GraphEntry` at all).
    /// `(version, keyword)` of the best `suggested_keyword_candidate`
    /// result, or `None` when `--autounmask-keep-keywords=n` isn't in
    /// effect (`autounmask_suggest_keywords` off) or no real candidate is
    /// masked by `KEYWORDS` alone. Only ever `Some` for a
    /// `NoVisibleCandidate` entry -- every other outcome had a visible
    /// candidate, so there's nothing to suggest unmasking.
    pub keyword_suggestion: Option<(String, String)>,
    /// `--autounmask-use`'s own suggestion sub-feature (see
    /// `resolve_pretend_graph`'s own doc comment): `(version, [(flag,
    /// desired_state)])` from the best `suggested_use_candidate` result,
    /// or `None` under the same gating `keyword_suggestion` already has
    /// (`autounmask_suggest_use` off, or no real candidate masked by a
    /// plain USE-dep mismatch alone). Only ever `Some` for a
    /// `NoVisibleCandidate` entry, same as `keyword_suggestion`.
    pub use_suggestion: Option<(String, Vec<(String, bool)>)>,
    /// `--autounmask-use`'s own second, architecturally distinct
    /// suggestion sub-feature -- see `suggested_parent_use_candidate`'s
    /// own doc comment for the full real grounding (`opt?`/REQUIRED_USE-
    /// conditional atoms, flipping the *requesting parent's* own flag,
    /// not the candidate's). `(parent_category, parent_package,
    /// parent_version, [(flag, desired_state)])`.
    ///
    /// `Some` for a `NoVisibleCandidate` entry whose parent flip would
    /// resolve the dep: with `--autounmask-backtrack` off the flip is
    /// recorded in `GraphResult::autounmask_use_changes` and the resolve
    /// fails (record-and-fail, real `_apply_parent_use_changes`), so this
    /// stays `Some` on the failed entry -- and doubles as the abort
    /// renderer's skip signal (a collected dep renders no row; the
    /// change block is its whole output). With
    /// `--autounmask-backtrack=y` the flip is folded and the graph
    /// re-walked instead, so a still-failing entry keeps `Some` only
    /// when the re-walk could not use it. Kept as a field for the
    /// `--json` provenance trace and symmetry with `use_suggestion`.
    pub parent_use_suggestion: Option<ParentUseSuggestion>,
    /// `--root-deps`'s own real `ESYSROOT`-vs-running-root distinction
    /// (see `running_root_satisfies_atom`'s own doc comment for the full
    /// real `depgraph.py:4207-4271` grounding): `true` for an entry this
    /// portuale resolved as a real `DEPEND`/`BDEPEND` (or, one recursion
    /// level deeper, `RDEPEND`/`IDEPEND`) atom that isn't satisfied by
    /// the running root's own vdb and needs building *there*, not under
    /// the target `ROOT` at all -- real portage's own "recursively pull
    /// in and build new packages against the running root" behavior.
    /// `false` for every ordinary `ROOT`-targeted entry (every entry this
    /// portuale ever produced before this field existed). Resolved via
    /// `resolve_root_deps_build_entries`, which walks such an entry's own
    /// `DEPEND` + `BDEPEND` + `RDEPEND` + `IDEPEND` against the running
    /// root recursively, cycle-guarded by the shared `root_deps_build_seen`
    /// set (see that function's own doc comment). `blockers` stay empty
    /// for such an entry (not computed -- a documented cut); the USE
    /// displays are built like every other entry's and repainted from the
    /// running profile at display time (backlog #242 Slice D). Residual:
    /// a *top-level* package's own `IDEPEND` still resolves against `ROOT`
    /// (real portage targets the running root for it too), and the full
    /// multi-root graph architecture, both still approximated edge by
    /// edge.
    pub targets_running_root: bool,
    /// Real `output.py:648`'s own `attr_display.remote_binary = pkg.remote`
    /// (the `g` bracket column, in the `f`/`F` slot): `true` for a
    /// merge-bound entry whose resolved candidate is a `--getbinpkg`
    /// *remote* binary not yet in `$PKGDIR` (`Candidate::remote`).
    /// Mutually exclusive with `fetch_restrict`/`_satisfied` in real
    /// `PkgAttrDisplay.__str__` (a binary is never `not pkg.built`), so
    /// this alone drives the column. Always `false` for an ebuild entry,
    /// a local `$PKGDIR` binary, and `AlreadyInstalled`/`NoVisibleCandidate`.
    pub remote_binary: bool,
    /// The resolved candidate's `BUILD_ID` (`Candidate::build_id`) --
    /// `Some` only for a merge-bound *binary* entry whose `Packages`
    /// index carried `BUILD_ID`. Real `output.py::_append_build_id`
    /// appends `-{build_id}` to this entry's displayed version, before
    /// the `-pv` `:slot` / `::repo` decoration. `None` for an ebuild
    /// entry or `AlreadyInstalled`/`NoVisibleCandidate`.
    pub build_id: Option<String>,
    /// This entry's own direct dependencies, in real portage's own
    /// graph-*discovery* order -- `RDEPEND`, `IDEPEND`, `PDEPEND`,
    /// `DEPEND`, `BDEPEND` (real `_add_pkg_dep_string`'s own `deps`
    /// tuple, `depgraph.py:4253-4289`), each key's own atom list left in
    /// written/`use_reduce`d token order -- each carrying the real
    /// `DepPriority` its originating key (and the atom's own slot
    /// operator) implies. This is the entire input to
    /// `merge_order::serialize_merge_order`, portuale's port of real
    /// `_serialize_tasks`: it supplies both the digraph's edges and, via
    /// first-occurrence order, real's own `.order`.
    ///
    /// Populated for a New/Upgrade/Downgrade/Reinstall entry and for an
    /// `AlreadyInstalled` entry that `--deep` actually recurses into
    /// (real's own `_add_pkg` gate: an installed package whose deps
    /// aren't recursed into is pushed onto `_ignored_deps`, never
    /// `.order`); empty for `NoVisibleCandidate` and for the handful of
    /// entry-construction sites that don't populate it (slot-operator-
    /// rebuild/`--rebuild-if-*` synthetic entries, `--nodeps`) -- those
    /// fall back to array position plus their `required_by` edges.
    pub deps: Vec<DepEdge>,
    /// This entry's own index in the resolver's pre-sort BFS discovery
    /// order (backlog #155 S1): real's digraph parent lists follow edge-
    /// insertion order, which is resolution order, and the `--tree`
    /// display walk (`print_tree`'s `add_parents`) picks the first
    /// untraversed parent -- so display edges must be enumerated in
    /// discovery order, not in merge-sorted array position (a dependent
    /// discovered later sorts earlier in merge order and would otherwise
    /// precede the earlier puller in every parent list it shares).
    /// Stamped by `topological_merge_order` (whose input vec IS discovery
    /// order) just before sorting; `0` for entries fabricated outside a
    /// resolve (unit tests), which all tie and keep array order via the
    /// stable sort at the single enumeration site.
    pub discovery: usize,
}

/// A root-qualified `cat/pkg`: real's graph nodes are `Package` objects
/// carrying their own `root_config` (`depgraph.py:4218-4291` deps queue,
/// `Package.root_config` / `pkg.root`), so the same cp merged in both
/// roots is two nodes. Track X Slice C (#242): every node-keyed map uses
/// this instead of the bare cp, or a both-roots package collapses into
/// one key and the second root's edges demote, misattribute, or vanish.
pub type RootCp = (bool, String, String);

/// `((child root, child cp), (owner root, owner cp)) -> (has_hard,
/// has_soft)`: for each dependency edge the BFS walked, whether it was
/// ever pulled as an unsatisfied build-time-only dep
/// (`QueueItem::buildtime_hard`) and/or via any softer route. An edge is
/// unbreakable in a cycle when `(true, false)` -- or, since backlog #228,
/// when the owner's entry carries an unsatisfied non-optional
/// slot-operator priority toward the dep (`find_hard_cycles`' own deps
/// rule, which the map's token-text view cannot see: a run-time-key `:=`
/// atom is never `buildtime_hard`). Track X Slice C (#242): keyed by
/// [`RootCp`] on both ends (real's `PackageTracker` keys `(root, cp)`,
/// `resolver/package_tracker.py:240`), so a target-root edge into a
/// running-root instance no longer merges with the running-root self
/// edge it depends on. Built in `resolve_pretend_graph`, consumed by
/// `topological_merge_order` (and Commit 2's cycle detection).
pub(crate) type EdgeKindMap = HashMap<(RootCp, RootCp), (bool, bool)>;

/// Real `_add_pkg_deps`' five-group `deps` queue (`depgraph.py:4255-4291`)
/// as a resolving root per dep key: which root a dependency edge's child
/// lands in, given the owner's own root. `RDEPEND`/`PDEPEND` stay with
/// the owner (`myroot`); `IDEPEND`/`DEPEND`/`BDEPEND` resolve against the
/// running root (native builds: `ESYSROOT` collapses to the running root
/// -- Track X Slice B's rule, kept here). Track X Slice C (#242): the
/// merge-order digraph and the cycle detectors use this to edge each atom
/// at the instance the walk actually resolved, instead of collapsing
/// every same-cp edge onto the first scheduled instance. Key ordinals are
/// `DepEdge::key`'s (`merge_order.rs`): 0=`RDEPEND`, 1=`IDEPEND`,
/// 2=`PDEPEND`, 3=`DEPEND`, 4=`BDEPEND`.
pub(crate) fn dep_child_root(owner_targets_running_root: bool, key: u8) -> bool {
    match key {
        // `RDEPEND` / `PDEPEND`: the owner's own root (`myroot`).
        0 | 2 => owner_targets_running_root,
        // `IDEPEND` / `DEPEND` / `BDEPEND`: the running root.
        1 | 3 | 4 => true,
        // Unreachable (`DepEdge::key` is always one of the five keys),
        // kept owner-local like the run-time keys.
        _ => owner_targets_running_root,
    }
}

/// Backlog #242 Slice D: attribute a skipped-update row's missed node and
/// merge parent to their resolving roots, so the renderer paints real's
/// per-node `to '<root>'` suffix and per-root USE (`Package.__str__` +
/// `pkg_use_display`, S0 map
/// `LLM/evidence/2026-10-02-242-display-sites.md` §1e).
///
/// A merge parent sits in its entry's root (looked up by cpv; first
/// merge-match wins — a dual-root same-cp-same-version parent is
/// ambiguous from the cpv alone, an unpinned shape). The missed instance
/// sits in the root the rejecting parent's edge points at (real's
/// five-group `deps` queue — [`dep_child_root`] over the parent entry's
/// own recorded edge to the missed cp, preferring the rejecting atom's
/// edge): a buildtime edge off a target parent lands the miss in the
/// running root (the blk0 shape: `<blk0x-2` off target `blk0b-1` rejects
/// `blk0x-3` into `/`), a runtime edge keeps it with the parent. Both
/// collapse to `false` on a single-root run (no entry is ever
/// running-rooted there, so the edge rule's running arm is gated on
/// that). Installed parents resolve from the walk root's own vdb, which
/// every producer passes as `root` — always the target walk here — so
/// they attribute target (documented, not derived).
pub(crate) fn skipped_update_roots(
    entries: &[GraphEntry],
    missed: (&str, &str),
    parent: (&str, &str, &str),
    atom: &str,
) -> (bool, bool) {
    fn merge_version(e: &GraphEntry) -> Option<&str> {
        match &e.outcome {
            PretendOutcome::New { version } => Some(version.as_str()),
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                Some(to.as_str())
            }
            PretendOutcome::Reinstall { version, .. } => Some(version.as_str()),
            _ => None,
        }
    }
    let (cc, cp2, cv) = parent;
    let parent = entries
        .iter()
        .find(|e| {
            e.category.as_str() == cc
                && e.package.as_str() == cp2
                && merge_version(e).is_some_and(|v| v == cv)
        })
        .or_else(|| {
            entries.iter().find(|e| {
                e.category.as_str() == cc && e.package.as_str() == cp2 && merge_version(e).is_some()
            })
        });
    let Some(parent) = parent else {
        return (false, false);
    };
    let consumer_running = parent.targets_running_root;
    let edge = parent
        .deps
        .iter()
        .find(|d| d.category == missed.0 && d.package == missed.1 && d.atom == atom)
        .or_else(|| {
            parent
                .deps
                .iter()
                .find(|d| d.category == missed.0 && d.package == missed.1)
        });
    let cross_active = entries.iter().any(|e| e.targets_running_root);
    let missed_running = match edge {
        Some(d) => cross_active && dep_child_root(parent.targets_running_root, d.key),
        None => false,
    };
    (missed_running, consumer_running)
}

/// Arguments of [`topological_merge_order`].
pub(crate) struct TopologicalMergeOrderArgs<'a> {
    pub(crate) entries: Vec<GraphEntry>,
    pub(crate) top_level_atoms: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) implicit_system_deps: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) dynamic_deps: bool,
    pub(crate) circular: &'a HashMap<RootCp, Vec<CircularDepChild>>,
}

/// Put `entries` in real portage's dependency-first *merge* order.
///
/// Real portage's `mylist` (`Display.__call__`'s input) is a genuine
/// merge schedule -- its `Scheduler` runs every install after the
/// installs it depends on. `merge_order::serialize_merge_order` is a
/// port of the function that produces it, real
/// `depgraph._serialize_tasks`, run over a typed digraph built from
/// every entry's own `GraphEntry::deps` (which carry real's per-key
/// `DepPriority`) plus its `required_by` edges as a fallback. See that
/// module's own header for what the port does and does not cover, and
/// for how it was validated against real portage's `--debug` digraph
/// dump on a live system.
///
/// `edge_kind_map` no longer feeds the ordering itself (the digraph's
/// own `DepPriority` edges are strictly richer than its
/// `(has_hard, has_soft)` summary); it stays a parameter because
/// `find_hard_cycles` -- the `* Error: circular dependencies:` reporter
/// -- still reads it from the same call site.
///
/// `--json`'s `merge_order` field and `emerge --buildpkgonly`'s build
/// loop both read this order directly; `--tree` walks the tree-mode
/// serialization (`merge_order::tree_display_order`) over this same Vec,
/// so the Vec order seeds its discovery exactly like the flat list does.
///
/// `circular` is backlog #221's settling pass's recorded cycle edges:
/// the suppression inside the scheduler must demote exactly like the
/// walk did, or a phantom edge re-closes the broken cycle and the order
/// falls back to bias (bug 705986's `pypy-exe` request merging ahead of
/// its own buildtime dep).
/// Returns the sorted entries plus, per sorted position, the entry's
/// rank in the scheduler digraph's pre-bias insertion order (backlog
/// #278: real `digraph`'s node insertion order, which `get_cycles`
/// iterates -- the cycle reporter threads it to `find_hard_cycles`).
/// Short-circuited inputs (< 2 entries) never reach the scheduler, so
/// their rank is plain array position.
pub(crate) fn topological_merge_order(
    args: TopologicalMergeOrderArgs<'_>,
) -> (Vec<GraphEntry>, Vec<usize>) {
    let TopologicalMergeOrderArgs {
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        circular,
    } = args;
    // Backlog #155 S1: stamp BFS discovery order before the merge
    // sort consumes it -- the input vec IS discovery order (a package's
    // entry is pushed before its dependencies are ever queued; see the
    // caller), and the `--tree` display walk needs it to order display
    // parents the way real's resolution-insertion edge order does.
    let mut entries = entries;
    for (seq, entry) in entries.iter_mut().enumerate() {
        entry.discovery = seq;
    }
    if entries.len() < 2 {
        // Real still emits the `digraph:` dump for a single-package merge
        // (its scheduler runs regardless); portuale short-circuits the
        // scheduler, so dump here explicitly under `--pretend --debug`.
        if entries.len() == 1 && resolver_debug() {
            merge_order::debug_dump_graph_only(&entries, top_level_atoms, root, circular);
        }
        let rank: Vec<usize> = (0..entries.len()).collect();
        return (entries, rank);
    }
    let (order, insertion_rank) =
        merge_order::serialize_merge_order(merge_order::SerializeMergeOrderArgs {
            entries: &entries,
            top_level_atoms,
            config,
            root,
            implicit_system_deps,
            repos,
            dynamic_deps,
            circular,
        });
    let mut slots: Vec<Option<GraphEntry>> = entries.into_iter().map(Some).collect();
    let rank: Vec<usize> = order.iter().map(|&i| insertion_rank[i]).collect();
    let entries = order
        .into_iter()
        .map(|i| slots[i].take().expect("each index emitted once"))
        .collect();
    (entries, rank)
}
