# crates/portage-dep/src/lib.rs
Rust port of a deliberately narrowed subset of Portage's `portage.dep` (`Atom`, `match_from_list`, USE-dep evaluation, `extract_affecting_use`). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## module
- The grammar scope cuts of #336 are closed by backlog #341 (S2–S9, `LLM/02.341-eapi-atom-grammar.opus.md`): the full grammar of each supported EAPI is applied per context. What remains cut is listed in that plan and in `backlog-tasks-2026-10.md` "Deliberate cuts" (installed-deps C-scanner quirk) and residues #345–#348.
- S2 (#341) closed two cuts: build-ids (`=cat/pkg-1.0-2`, via `Atom.build_id` + `AtomOptions.allow_build_id`) and EAPI parametrization of the strict grammar (`parse_atom_with` + `AtomEapi`, EAPI gates verbatim from `Atom.__init__`).
- S3 (#341) closed the wildcard branch: `parse_atom_with` with `allow_wildcard` ports `_get_atom_wildcard_re` (dep:332-372) and `match_from_list` re-parses with `allow_wildcard=True, allow_repo=True` (dep:2901-2906). The default `parse_atom` still rejects extended atoms (`CpBucketIndex::build` routes them to `other` because of it).
- S4 (#341) closed ebuild/binary `*DEPEND` validation: `dep_validate` parses each dep key with the owner's EAPI (`Candidate.eapi`) and validates against its IUSE, masking `invalid:` with Portage's texts (plus the `Improper context` built-atom check, reopening #15). Added `atom_to_string` (Portage `str(atom)` = the original token) for quoting atoms in those messages.
- Origin: the "atom matching" slice from LLM/agent-context.md's depgraph/config resolution follow-up work. Ports `portage.dep.Atom` and `portage.dep.match_from_list` (lib/portage/dep/__init__.py).
- The atom harnesses (Rust `atom-harness`, Python `python-harness/atom_harness.py`) agree on the full S3 input language: EAPI-aware strict atoms incl. build-ids (S2) plus extended/wildcard atoms (S3), differentially tested parse × match. NOTE: the Python mirror no longer exists (see AGENTS.md rule 4).
- Slot operators (`:=`, `:*`, `:slot=` -- PMS 8.3.3): see `SlotOperator`, `atom_regex` (two-stage parse mirroring Portage's `_get_atom_re`/`_get_slot_dep_re` split) and `matches_slot` (matching needed zero changes: Portage's `_match_slot` ignores `slot_operator` entirely and consults only `Atom.slot`/`.sub_slot`, both already modeled correctly before slot operators existed).
- History: adding slot operators closed a real, previously-silent bug in `portage-repo`'s dependency recursion: any DEPEND/RDEPEND token using a slot operator (extremely common, e.g. `dev-libs/foo:0=`) failed to parse under the old grammar and was silently dropped from the graph entirely -- no entry, no `NoVisibleCandidate`, no warning -- because `resolve_pretend_graph`'s BFS loop treats a parse failure as "not a dependency at all" (`let Some(atom) = parse_atom(..) else { continue };`), not as an unresolvable one.
- Candidates for `match_from_list` are plain strings `category/package-version[-rN][:slot[/subslot]]`, not full Package objects (no USE/IUSE/repo metadata), since portuale had no package-db/depgraph model at that point. This mirrors how Portage's `match_from_list` already supports plain strings (via `dep_getslot`'s ":slot" suffix convention) as a fallback when candidates aren't Package objects.
- USE deps (`foo[bar]`, `foo[bar?,!baz=,qux(+)]` -- PMS 8.3.4, all 6 per-flag forms plus 4-style `(+)`/`(-)` defaults) are parsed (see `UseDep`/`UseDepOp`/`UseDepDefault`, `parse_use_deps`) and can be enforced via `use_deps_satisfied` given per-candidate IUSE/USE state. But `matches_version`/`matches_slot`/`match_from_list` never consult `Atom::use_deps`, matching Portage exactly: its own USE-dep filtering is skipped for any candidate that isn't a Package object with `.use`/`.iuse` attributes (the `hasattr` check in `lib/portage/dep/__init__.py`'s `match_from_list`), which is exactly the plain-string case seen here. `portage-repo` calls `use_deps_satisfied` directly as an extra filter after `match_from_list`, once it has each surviving candidate's IUSE/effective USE (computed for other reasons already).
- History: before USE deps were parseable, an atom using one was rejected as `INVALID` outright, which for a dependency atom from DEPEND/RDEPEND meant the BFS silently dropped it (same class of bug as the slot-operator one above).
- `=*` glob operator (PMS 8.3.1): see `Operator::EqGlob`, `atom_regex` (trailing "*" captured generically, not as a second grammar alternative), `matches_version`'s `EqGlob` arm plus `normalize_leading_zeros`/`glob_compare_string` for the boundary-aware prefix match. Portage implements `=*` as a literal string-prefix match, not `vercmp`-based (its comment: "Nasty special casing for leading zeros / Required as =* is a literal prefix match, so can't use vercmp"), with a component-boundary check fixing bug 560466 ("1*" must not match "10").
- PMS historical note on `=*`: the component-wise "wildcard for any further components" semantic here is the *current* one; a raw string-prefix match (e.g. "=foo-5.2*" matching "foo-5.22.0") was the original EAPI 0-5 behaviour, retroactively dropped in October 2015, well before this repo's EAPI 5+ floor.
- `::reponame` (PMS 3.1.5 "Repository names"): see `Atom::repo`/`Candidate::repo` and `matches_repo`. Ported from Portage's `match_from_list` final post-pass filter; a repo-less candidate string always passes, matching `dep_getrepo`'s "unknown, not absent" semantics.
- History: portuale's candidate strings never carried repo identity before the repo slice; `portage-repo` now appends `::reponame` (each repo's `repos.conf` section name, already tracked as `RepoConfig::name`, rather than reading the second `profiles/repo_name` file Portage also cross-checks) to every candidate string it builds for `match_from_list` EXCEPT blocker matching and slot-conflict re-verification (see that crate's own doc comment) -- a deliberately narrower scope than the rest of the feature's wiring.
- Ambiguity rule: a bare (no-operator) atom whose package name is followed by something version-like (`foo-bar-2`) is rejected, not silently accepted with the longest package name. See the `ambiguous` capture group in `atom_regex` and `Atom.__init__`'s check on the "simple" branch in lib/portage/dep/__init__.py.

## const / thread_local (parse caches)
- A single `emerge -puD` on a live tree calls `parse_atom`/`parse_candidate` tens of millions of times, almost always on an already-parsed string (the same handful of `package.*` config atoms and candidate `cat/pkg-ver:slot::repo` strings, re-checked once per package per graph-walk visit). Both parsers run a ~15-group backtracking regex, which `perf` put at ~65% of the whole run. A thread-local memo table makes a hit one hash lookup + one clone of the small parsed struct. Thread-local rather than a locked global so the parse path stays lock-free; the resolver is single-threaded, other threads get their own table. See docs/performances-tuning.md.
- S2 (#341): the default-options cache is untouched (still `HashMap<String, Option<Atom>>` looked up by `&str`, no allocation on a hit); non-default options use a nested `HashMap<AtomOptions, HashMap<String, Result<Atom, AtomError>>>`, also looked up by `&str`. `AtomOptions` is `Copy` (the EAPI is the small `AtomEapi` enum) so the outer key never allocates. `std` has no borrowed tuple-key lookup on stable and no new deps are allowed, hence the nesting (review M-6/I-4).

## const VER
- ASCII `[0-9]`, not `\d`: the `regex` crate's `\d` matches Unicode digits (so `=dev-libs/foo-١` parsed), but Portage compiles every atom regex with `re.ASCII` (oracle Q4). Same for the two `-r[0-9]+` revision groups.

## const SLOT
- Leading `_` accepted (`[A-Za-z0-9_]` first char): Portage `_slot` is `([\w][\w+.-]*)` under `re.ASCII`, so `\w` includes `_` (oracle Q4: `dev-libs/foo:_a` parses).

## enum AtomEapi
- Portage `eapi=` as a `Copy` value (`None` / `Known(u8)` / `Unsupported`); the original string lives with the caller and is passed as `eapi_str` only for messages. `parse` mirrors `Eapi.__init__` (`partition("-")[0]` against `ALL_EAPIS`): unsupported → `Unsupported`, supported-but-unparseable (`3_pre1`, padded) → `Err` (S1's `EapiValueError`; Portage raises `ValueError`, eapi.py:224-229). Never splits on `_`.

## struct AtomOptions
- One parser with options instead of one fixed superset (#341 architecture). Default rule verbatim from dep:1926-1937: `eapi` given ⇒ `allow_repo = repo_deps` (False for every EAPI 0–9), `allow_build_id = false`; `eapi = None` ⇒ both true; explicit `Some(b)` always wins (profile readers rely on it). `parse_atom` is the defaults (= Portage `eapi=None`).

## enum AtomErrorKind / struct AtomError
- Portage `InvalidAtom`'s category mapped: none → `Invalid`, `EAPI.incompatible` → `EapiIncompatible`, `IUSE.missing` → `IuseMissing`. A messageless `InvalidAtom` renders as the atom string itself (`str(e)`).

## struct Atom
- `use_deps`: `foo[]` is invalid, same as Portage. Original doc said "never consulted by matching -- see the module doc comment" (true of `match_from_list`; `use_deps_satisfied` is the explicit second step).
- `build_id` (#341 S2): the ambiguous `-vr` tail of a versioned atom as a digit string (Portage's int is unbounded: `=dev-libs/foo-1.0-18446744073709551616` parses, live). `extended` (#341 S3): true for `_get_atom_wildcard_re` matches, only with `allow_wildcard`.

## fn parse_atom_with
- Transcription of `Atom.__init__` (dep:1900-2153), keeping the crate's two-stage shape (permissive outer regex, validated slot/use stages). The op branch carries Portage's ambiguous tail as `bidtail` + outer `-vr`, tried before a longer package name (the regex backtracking order is the semantics: `<a/1-1-02-1.0` is a leading-zero build-id ERR, not package `1-1-02-1` version `0` — found by the S2 fuzz acceptance, 1 in 60k). `=*` with a tail is always invalid (dep:2023-2033); `=*` with an operator other than `=` is rejected outright (PMS 8.3.1 "an asterisk used with any other operator is illegal": `>=cat/pkg-1.2*` is invalid, not truncated); the simple branch tail is always invalid (`foo-bar-2` ambiguity). Slot parsing follows `_get_slot_dep_re` per `attrs.slot_operator` (without it the whole text must be a plain slot, so `:0/1`/`:0=` are bare `InvalidAtom`s). EAPI gates only when `eapi` is given, in Portage's order (`slot_deps`, `use_deps`, `use_dep_defaults`, `strong_blocks`; messages verbatim with the caller's `eapi_str`). The conditional-flag check is `check_conditional_flags`, not here: it needs the owner's IUSE, which is not hashable for the cache key — and the order is unobservable anyway (strong blocks are off only in EAPI 0/1, where use deps already fail first; review I-1, proven live).

## fn check_conditional_flags
- Portage dep:2125-2144: conditional use-dep flags vs `is_valid_flag`, gated on `eapi is not None`. Types visited in first-occurrence order (Portage iterates the `conditional` dict); within one type the first flag in sorted order is reported (Portage iterates a set there — its pick among several invalid flags is arbitrary; deliberate micro-cut). The message's atom text comes from `render_atom` (Portage prints `str(atom)`, which is the original string).

## fn render_atom
- Private rebuild of the text Portage prints for `str(atom)` (original string): blocker, operator, `cp-ver[-rN][-bid]`, slot, repo, use (tokens in `prefix+flag+default+suffix` order). Only reader is `check_conditional_flags`; Step 6 audit (`pretend/conflicts.rs` gained `-bid`; every other `format!` site builds candidate/entry strings, not atom text, or operates on the raw token).

## fn matches_version
- `Eq` + build-id (dep:2994-3008, oracle `probe_match.py`): the id is consulted only here and only against a candidate that has one — equal ids match, a candidate without one never does (S2 shipped the never half as review C-1; S9 the positive half). Every portuale string candidate models `_pkg_str`/`Package` (a plain string matches regardless in Portage but never occurs here); binary callers set `Candidate.build_id` explicitly since `str(_pkg_str)` drops it. `>=`/`<`/`~` ignore the build-id, like Portage.

## const PKG
- Non-greedy like Python's `_pkg` in lib/portage/versions.py, e.g. "utf8-scanner-1.0" splits as pkg="utf8-scanner", version="1.0".

## const REPO
- Identical to Portage's `_repo_name` (lib/portage/dep/__init__.py): `[\w][\w-]*`, i.e. `[A-Za-z0-9_][A-Za-z0-9_-]*`. Matches PMS 3.1.5 "Repository names" ("may contain [A-Za-z0-9_-], must not begin with a hyphen").

## const USEFLAG
- Identical to Portage's `_useflag_re` (lib/portage/dep/__init__.py) and already mirrored in `portage-use-reduce`'s `useflag_re`; duplicated here rather than a cross-crate dependency since it's a single-line regex literal, not shared logic.

## enum Operator
- `EqGlob` is a real, distinct operator value in Portage too (`Atom.operator == "=*"`), not `Eq` with a flag. PMS 8.3.1: "if the version specified has an asterisk immediately following it, then only the given number of version components is used for comparison, i.e. the asterisk acts as a wildcard for any further components."

## enum SlotOperator
- `Star` = `:*` (any slot acceptable, no explicit slot); `Equals` = `:=`/`:slot=` (any slot if none given, otherwise restricted to that slot -- see `Atom::slot`). Purely a rebuild-trigger signal in Portage (whether a dependency needs rebuilding when the matched package's sub-slot changes); irrelevant to whether an atom matches, which is why `matches_slot` needed no change.

## enum UseDepOp
- Corresponds to the 6 real `prefix`+`suffix` combinations (Portage's `_usedep_re` groups).
- `EqualParent`/`OppositeParent` and `IfParentEnabled`/`IfParentDisabled` are conditional on the *atom-owning* package's own USE state, not just the candidate's. `use_deps_satisfied` still requires their flag to be a declared IUSE flag on the candidate (like any use-dep flag) but, matching Portage's `match_from_list`, imposes no enabled/disabled constraint from them; only `Enabled`/`Disabled` constrain the candidate's own USE state.

## enum UseDepDefault
- Stale claim removed from the old doc: "Parsed for fidelity/round-tripping; never consulted by matching, same as the rest of `UseDep`". It is consulted now, by `use_deps_satisfied`/`use_mismatch_flags`, and round-tripped by `render_use_dep`. PMS: default applies when the ebuild being matched doesn't have `flag` in IUSE_REFERENCEABLE.

## struct Atom
- `use_deps`: `foo[]` is invalid, same as Portage. Original doc said "never consulted by matching -- see the module doc comment" (true of `match_from_list`; `use_deps_satisfied` is the explicit second step).

## impl Atom::full_version
- Mirrors Atom.cpv's version part in the Python original.

## fn atom_regex
- The whole post-":" slot expression is wrapped in its own "slotpart" group (mirroring Portage's two-stage approach: `_get_atom_re` captures the raw text after ":", then `_get_slot_dep_re` re-parses it) so `parse_atom` can tell "no ':' at all" (group absent) from "':' present but empty" (empty match), which PMS says is invalid (the "if self.slot is None and self.slot_operator is None: raise" check in `Atom.__init__`).
- "usedeps" mirrors Portage's permissive `\[.*\]` outer capture (`_use` in lib/portage/dep/__init__.py), validated in a second stage by `parse_use_deps`.
- "glob" (trailing "*" after the version/revision, PMS 8.3.1's "=*") is captured for ANY of the operators, whereas Portage's grammar only allows it after "=" (a dedicated "star" alternative in `_get_atom_re`, distinct from its general "op" alternative). `parse_atom` rejects a captured "glob" with any operator other than "=", which is simpler than duplicating the whole op+cpv sequence into a second alternative to exclude 5 of 6 operators from one optional trailing character.
- "repo" ("::reponame", PMS 3.1.5) sits between the slot part and usedeps, matching `_get_atom_re`'s ordering exactly; shared by the "op" and bare "simple" branches, like slotpart/usedeps.

## fn parse_use_deps
- Mirrors Portage's two-stage approach (`Atom.__init__`'s use-dep loop; no separate `_get_usedep_re`-equivalent function on the Rust side, same algorithm): split on `,`, validate each token against `use_dep_token_regex`, accept only the 6 real `prefix`+`suffix` combinations.
- `-flag=`/`-flag?` are syntactically matched by the per-token regex but not real operators; verified empirically against Portage, which rejects them too.
- A flag's `(+)`/`(-)` default must be consistent across every token mentioning that flag within the atom (`foo[bar(+),bar(-)]` and `foo[bar(+),-bar]` are both invalid -- empirically verified Portage behaviour), so the accept/reject boundary matches `Atom` exactly.
- The `seen_defaults` bookkeeping tracks "has a default at all" per flag so a later token with a different has-default state (regardless of which default) is caught; mirrors `Atom.__init__`'s three-way missing_enabled/missing_disabled/no_default bookkeeping.

## fn without_use
- Portage `Atom.without_use` for a plain atom STRING. `dep_zapdeps` uses it for `all_available` so "does a `||` alternative exist at all" (`mydbapi.match_pkgs(atom.without_use)`, `dep_check.py:447-449` check, soft 469) is independent of whether the pickle's USE flags could be satisfied ("...since we don't want USE settings to adversely affect || preference evaluation", soft 467-468). The Rust port later re-probes the WITH-USE string for the `all_use_satisfied` / bug-515584 split.
- Cut at the first `[` mirrors dep/__init__.py:1792 `s.index("[")` (the same as the `find('[')` used here).

## fn parse_atom_uncached
- Default-options body: `parse_atom_with_uncached` with `AtomOptions::default()`, `Err` → `None`. `parse_atom` (= `eapi=None`) therefore accepts build-ids and `::repo`, like Portage.
- Empty "slotpart": a bare trailing ":" is syntactically matched (slot and operator sub-groups are individually optional) but explicitly invalid per PMS/`Atom.__init__` (see atom_regex).
- "*" with an explicit slot is invalid: "*" means "any slot", meaningless alongside a specific one (`Atom.__init__`'s corresponding check).

## struct Candidate
- `repo`: mirrors `dep_getrepo`'s convention for plain-string candidates. See `matches_repo`.
- `build_id` (#341 S9): the binary build-id (Portage `_pkg_str.build_id`). Always `None` from `parse_candidate` — `str(_pkg_str)` drops it, so a string never carries one; binary callers set it explicitly (`match_from_list_with_build_id`, `match_atom_matches_candidate`). See `matches_version`.

## fn normalize_leading_zeros
- Collapses a leading run of `'0'` characters the way `match_from_list`'s own `=*` branch does before comparing (its comment: "XXX: Nasty special casing for leading zeros / Required as =* is a literal prefix match, so can't use vercmp"). Without it a version's incidental leading zeros ("01" vs "1") would make numerically equal versions compare unequal as prefixes.
- Only applied to the plain version, never the `-rN` revision, matching Portage's `mycpv_cps[2]`/`xs[2]` (the `catpkgsplit`-style "version, no revision" component).
- Empirically verified against Portage's `portage.dep.match_from_list` (`python3 -c` probing leading-zero cases) before relying on the port: `"0" -> "0"`, `"00" -> "0"`, `"01" -> "1"`, `"0.5" -> "0.5"` (single leading zero is a meaningful digit), `"00.5" -> "0.5"` (the redundant second zero is dropped).

## fn glob_compare_string
- Mirrors Portage's targeted `mycpv.replace(cp + "-" + orig_version, cp + "-" + normalized, 1)`, which only rewrites the plain-version substring, never the revision.

## fn matches_version
- Mirrors `match_from_list`'s per-candidate filtering.
- `EqGlob` arm: PMS 8.3.1 wording as above. Portage implements it as a literal string-prefix match (not vercmp-based -- see `normalize_leading_zeros`) on `category/package-version[-rN]`, but only at a genuine component boundary: Portage's bug 560466 fix means "1*" must NOT match "10" (both digits, no real boundary) even though "10" literally starts with "1" -- captured by the digit-adjacency check. category/package equality is already guaranteed by `match_from_list`'s caller-side filter before `matches_version` runs, so comparing only the version[-rN] suffix (rather than the full cpv string Portage slices) is equivalent and simpler.

## fn matches_slot
- Mirrors `_match_slot`. A candidate with no slot info always passes (matches `match_from_list`'s behaviour for plain-string candidates it can't determine a slot for -- see module notes).
- `atom.slot_operator` is never consulted: `_match_slot` doesn't either; only `match_from_list`'s `if mydep.slot is not None:` guard (mirrored by the `atom.slot` check) decides whether slot-filtering happens. A bare `:=`/`:*` has `slot == None`, so falls through the early return and matches any slot; `:slot=` has `slot == Some(..)` and is filtered like a plain `:slot`. This is why adding slot-operator *parsing* needed no change to *matching*.

## fn matches_repo
- Mirrors `match_from_list`'s final post-pass filter (only run `if mydep.repo:`). A candidate with no repo info (`candidate.repo == None`, the default for a plain-string candidate that never had `::repo` appended; `dep_getrepo` returns `None` for a repo-less string) always passes. Portage's justification: a plain string generally means "repo unknown", not "no repo", so it can't positively fail a repo check.

## fn use_deps_satisfied
- Ports `match_from_list`'s USE-dep post-pass (its `if mydep.unevaluated_atom.use:` block, `lib/portage/dep/__init__.py` lines 3143-3188). NOT called from `match_from_list` itself, since that only sees plain candidate strings (Portage skips the same block for a plain-string candidate via its `hasattr(x, "use")` guard). Callers with per-candidate IUSE/USE state (`portage-repo`, which computes both via `read_md5_cache`/`effective_use_flags`) call it directly after `match_from_list`'s filtering.
- `iuse` shape: same as `effective_use_flags`'s callers already extract from md5-cache (`+`/`-` default markers stripped); `enabled` is the effective (computed) USE set.
- Faithful port, not simplified: a flag with no `(+)`/`(-)` default, of ANY form including the four conditional ones, must be a declared IUSE flag on the candidate or the atom doesn't match (`_use_dep.required`, checked via `x.iuse.is_valid_flag(...)` before anything else). A `(+)`/`(-)` default is consulted only for a flag missing from the candidate's IUSE, standing in for "as if enabled/disabled".
- `flag?`/`!flag?`/`flag=`/`!flag=` impose NO enabled/disabled constraint: Portage's `match_from_list` only consults `mydep.use.enabled`/`.disabled`, which `_use_dep.__init__` populates solely from the two unconditional forms; the conditional ones land in a separate `.conditional` structure it never reads. Not a deliberate simplification: evaluating them needs the atom-owning package's USE state, a different mechanism (dependency-string conditional evaluation) that neither portuale's `match_from_list` nor Portage's has.

## fn use_mismatch_flags
- Portage `_prepare_conflict_msg_and_check_for_specificity`'s USE branch (`slot_collision.py:331-389`) for one parent `atom` against one conflicting `other` instance (already matching on version and slot).
- Unconditional: `atom`'s default-less (`required`) flags absent from `other`'s IUSE -- `other_pkg.iuse.get_missing_iuse(atom.unevaluated_atom.use.required)`. Non-empty both keys the flags *and* marks the parent preferred for display (`unconditional_use_deps`); Portage skips the violated computation entirely for such a pair.
- Violated: unconditional-form (`[x]`/`[-x]`) deps contradicted by `other`'s USE -- `violated_conditionals`' `.enabled ∪ .disabled` sets: `[x]` violated iff `x` is off `other` while valid (or carrying a `(-)` default); `[-x]` violated iff `x` is on `other` (or invalid with a `(+)` default).
- Conditional forms (`?`/`=`/`!`...) never yield keys: Portage only reads `.enabled ∪ .disabled` (conditional hits land in a dropped side-dict), and without `parent_use` Portage *raises* -- portuale always has the parent but drops the dict either way, so no raise and no keys (a divergence only against a Portage crash).
- `is_valid_flag` is plain declared-IUSE membership; Portage also consults `_iuse_implicit_match`. Same documented simplification as the `iuse_names` precedent -- implicit-matched flags are rare in slot-conflict atoms.

## fn use_deps_violated
- Portage `Atom.violated_conditionals` (`lib/portage/dep/__init__.py`). This is the walk-time check Portage's complete-graph end-of-walk loop leans on: a deep dependency whose use-deps the scheduled (re)build contradicts counts as unsatisfied even though `match_from_list` (which never reads the four conditional forms, and only the unconditional two against the candidate) still matches it.
- Composed from the two primitives Portage's `violated_conditionals` is equivalent to: `evaluate_conditionals` (parent-relative forms resolved to unconditional demands or dropped) then the match-time satisfied check (`use_deps_satisfied`, incl. IUSE validity and defaults) -- rather than a third evaluator, so the truth table stays in one place (`evaluate_use_dep_conditionals`). `child_iuse` has default markers stripped, same shape as `use_deps_satisfied`.

## fn render_use_dep
- The exact inverse of `parse_use_deps`'s per-token parse; "parsed for fidelity/round-tripping" per `UseDep`'s original doc, now actually round-tripped by `evaluate_atom_conditionals`.

## fn evaluate_use_dep_conditionals
- Ports `Atom.evaluate_conditionals` (`lib/portage/dep/__init__.py:1387`, confirmed by reading it directly) verbatim from its truth table. `parent_use` is the same `uselist` already threaded into `use_reduce_flat` for a dependency string's `flag? ( ... )` groups.
- `x?`/`!x?` are one-directional: they only ever *add* a constraint. A `Vec<UseDep>` that becomes empty is equivalent to "no use-dep at all", the same "empty means unconstrained" convention `use_deps_satisfied`'s callers established (see `resolve_pretend`'s `.filter(|d| !d.is_empty())` gate, portage-repo).

## fn evaluate_atom_conditionals
- Mirrors `use_reduce`'s per-token integration point (`lib/portage/dep/__init__.py:1045-1046`: `if not matchall and hasattr(token, "evaluate_conditionals"): token = token.evaluate_conditionals(uselist)`), called on every dependency atom token as `use_reduce` walks a dependency string, with the same `uselist` used for group conditionals.
- Portuale's `use_reduce_flat` (portage-use-reduce) deliberately stays atom-grammar-agnostic (see its module doc on the atom-parsing/tokenizing split), so this step lives here, applied by portage-repo's `enqueue_flat_deps` to each flattened token once it has the owning package's effective USE.
- Returns `atom_str` literally unchanged (same string) when there are no use-deps or none are conditional, so a plain `[flag]` atom is never needlessly rewritten.

## fn match_from_list
- Mirrors Portage's `match_from_list`. `None` means the atom failed to parse; unparseable candidate strings are silently skipped.
- S3 (#341): re-parses with `allow_wildcard: true, allow_repo: Some(true)` (dep:2901-2906); leading blockers stripped before the re-parse, so a blocker matches like its bare form. Extended cp via `extended_cp_match` (dep:2934-2943); extended `=*` is a substring test on the candidate's `_pkg_str` version (`ver[-rN]`, without `-r0`, dep:2954-2972) — portuale candidates model `_pkg_str`, so `=*/foo-*r0*` matches nothing here while Portage on a plain-string candidate would match (review M-3). `matches_repo` unchanged.
- S9 (#341): `match_from_list_with_build_id` pairs each string with an optional binary build-id (`None` = string-path behaviour); `parse_match_atom` + `match_atom_matches_candidate` are the per-candidate form for callers whose candidates share one string (binpkg-multi-instance builds in `masked_rows_for_pool`, portage-repo). All three share `candidate_hits` (cp/version+`=`-only build-id/slot/repo).

## fn atom_intersects
- Portage `Atom.intersects()` (`lib/portage/dep/__init__.py`): despite the name a deliberately NARROW check; Portage's docstring says so ("atoms with different cpv, operator or use attributes cause this method to return False even though there may actually be some intersection... TODO: Detect more forms of intersection").
- Ported field-for-field, skipping Portage's `self == other` fast-path (redundant: identical atoms satisfy every check and fall through to `true`). `cp`, `use`, `operator` and `cpv` (operator plus the full version/revision, compared as `full_version()`) must ALL match exactly, not overlap or satisfy a range, before slot compatibility decides.
- `repo` is deliberately NOT checked, matching `intersects()`; Portage's `action_deselect` caller adds its own repo check (`and not (arg_atom.repo and not atom.repo)`), ported at `run_deselect`'s call site in `pretend.rs`, not folded in here.

## section: bounded wildcard atoms (removed in S3)
- S3 deleted `WildcardAtom`/`parse_wildcard_atom`/`wildcard_atom_matches` after moving their one caller (`matches_config_entry`) onto `match_from_list`, which now handles the full Portage extended syntax. The old API covered only `*/*`, `cat/*`, `*/pkg` with no partial globs, operators or slots; Portage additionally accepts partial globs (`dev-libs/fo*`), wildcard+slot (`*/*:0`), wildcard+repo, `=*/foo-*1*` substring versions and blockers.

## fn atom_wildcard_regex
- Transcription of `_get_atom_wildcard_re` (dep:332-372): `simple` = `_extended_cat` (`[\w+*][\w+.*-]*`, with `.`) / `pkg_re` (`[\w+*][\w+*-]*?`, no `.`), `star` = `=cat/pkg-\*\w+\*` (ASCII `\w`: no `.`/`-`, so `=dev-libs/fo*-*1.0*` is ERR, oracle probe2). Blockers are stripped before matching (dep:1959-1967), shared with the strict path. The slot groups reuse the strict names so the second-stage slot validation runs unchanged; EAPI gates (slot/strong-blocker) and the repo gate apply to extended atoms too (live: `*/*:0` with eapi=0 → `Slot deps are not allowed`, `!!*/foo` with eapi=1 → `Strong blocks…`).
- The `simple` `-vr` tail check is group `simple+3`, the `(\.\d+)` *inside* the tail — not the tail itself: `*/foo-1` and `*/foo-1-r1` parse as extended atoms with the tail in the cp, `*/foo-1.0` is invalid, and `*/foo-1.r1` never matches at all (live 2026-10-12). The plan's "admits a tail only to reject it" phrasing is the shape, not the rule.
- `**` anywhere in the cpv is rejected (dep:1990).

## fn extended_cp_match
- Portage `extended_cp_match` (dep:2352-2365): each `*` → `[^/]*`, anchored, so a glob never crosses the `/`. Compiled regexes memoised per pattern like Portage's `_extended_cp_re_cache`.

## enum Aff / aff_* helpers
- `Aff` mirrors Portage's `stack` entries in `extract_affecting_use`, which are `str` or `list`.
- `aff_ends_q` = `l[0][-1] == "?"` / `stack[level][-1][-1] == "?"`; `aff_is_barbar` = `stack[level][-1] == "||"`.
- `affecting_useflag_re` = `_get_useflag_re` for the modern EAPI default; EAPI is not parametrized, matching the rest of the crate.
- `aff_cond_flag` = `extract_affecting_use`'s inner `flag(conditional)`; `None` where Portage raises `InvalidDependString`.
- `aff_special_append` = the `special_append()` closure.

## fn extract_affecting_use
- Port of `portage.dep.extract_affecting_use` (`lib/portage/dep/__init__.py`). `None` on malformed `dep` where Portage raises `InvalidDependString`.

## tests: parse_cache_tests
- Guards against a future cache-key bug (e.g. keying on a trimmed/normalised string).

## tests: without_use_tests
- Portage `Atom.without_use`: the trailing `[...]` is stripped, everything before it untouched; a no-block atom must be returned unchanged (`mydbapi_match_pkgs(atom.without_use)` must behave like the full atom for a `||` alternative with no `[use]` deps). The cut at the first `[` is dep/__init__.py:1792 `s.index("[")`; a malformed double-bracket string cuts at the first `[` exactly like Portage.

## tests: use_dep_satisfaction_tests
- `flag_not_in_iuse_at_all_never_matches_without_a_default`: Portage `_use_dep.required`: any default-less flag must be a declared IUSE flag, or the atom doesn't match, regardless of enabled/disabled state.
- `conditional_forms_only_require_...`: flag?/!flag?/flag=/!flag= never constrain state in `match_from_list` itself (see `use_deps_satisfied`).
- `enabled_of`/`iuse_of`: Portage's authoritative test vectors for USE-dep-vs-Package-mock matching: lib/portage/tests/dep/test_match_from_list.py's `testMatch_from_list`, the `dev-libs/A[...]` cases (lines 151-195). Its `Package` mock derives a candidate's `iuse` from `atom.use.required` (flags with NO `(+)`/`(-)` default in the construction atom) and `enabled` from `atom.use.enabled` (bare, non-`-` tokens); reproduced by hand via `use_deps`/`enabled_of` on the same construction atom strings rather than re-deriving the mock's logic. `iuse_of`: "required" = every use-dep flag with no default marker.
- `atom_intersects_rejects_a_different_operator_...`: Portage's docstring: "atoms with different cpv, operator or use attributes cause this method to return False even though there may actually be some intersection". `>=dev-libs/foo-1.0` would be satisfied by 1.0, but the operator must match exactly.

## tests: use_mismatch_tests
- `missing_iuse_is_unconditional...`: Portage skips `violated_conditionals` when `get_missing_iuse` is non-empty.
- `conditional_forms_never_yield_keys`: Portage only reads `violated_conditionals`' `.enabled`/`.disabled`; conditional hits land in a dropped side-dict.

## tests: use_deps_violated_tests
- `parent_disabled_child_enabled_...`: Backlog #222's own shape: installed consumer built with -icu (`[-icu]` raw, `[!icu?]` live) vs the icu-flipped rebuild.

## tests: use_dep_conditional_evaluation_tests
- `evaluate_atom_conditionals_preserves_slot_and_repo`: this crate's atom grammar orders "::repo" before the use-deps bracket (see `atom_regex`'s group order), unlike Portage, which puts use-deps before "::repo"; matching the crate's already-accepted order rather than inventing a new one.

## tests: extract_affecting_use_tests
- `matches_portages_test_corpus`: the 23 passing cases from lib/portage/tests/dep/test_extract_affecting_use.py, verbatim; `malformed_syntax_returns_none`: the 15 cases from its `test_cases_xfail` (Portage raises `InvalidDependString`; portuale returns `None`).
