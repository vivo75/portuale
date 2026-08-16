# Porting pilot

This started as the "Suggested first execution step" pilot from
[`PROMPT.md`](PROMPT.md): a small, complete run of the whole pipeline (Rust
port, Python harness, shared black-box contract suite, multicall dispatch
skeleton) on the smallest meaningful slice. It has since grown three slices
further into depgraph/config-resolution territory: atom matching (the
foundational building block both subsystems are built on), USE-conditional
dependency-string flattening (`use_reduce`, a real, heavily-used config.py/
resolver primitive in its own right), a real, working
`emerge --pretend category/package`, recursive DEPEND/RDEPEND resolution
(so `--pretend` on a package with real dependencies reports the whole
deduped, cycle-safe set of packages that would newly merge, not just the
one you named), real USE/ACCEPT_KEYWORDS computed from an actual profile
inheritance chain and make.conf (not a fixed stand-in), per-package
overrides from `package.mask`, `package.unmask`, and
`package.accept_keywords` -- what's visible at all -- per-package USE
overrides from `package.use`, so a single package's own `DEPEND`/
`RDEPEND` can be flattened against a USE set that differs from every
other package's, the same way real portage does it, blocker
(`!atom`/`!!atom`) reporting: a package's blocker atoms are matched
against both currently-installed packages and the rest of the same
resolution's New/Upgrade set, and shown -- not resolved or enforced,
since `--pretend` itself never touches anything real -- overlays:
candidates for a package now come from every repo `repos.conf` defines,
main plus any number of overlays, not just the one main repo -- and, on
top of all of that, slot conflicts: two atoms landing on the identical
slot at incompatible versions are reported the same "show, don't
enforce" way blockers are, while two atoms simply requesting
*different* slots of the same package now correctly resolve as two
independent, coexisting entries instead of one silently overwriting the
other, matching how real portage genuinely allows multiple slots of the
same package side by side. `virtual/*` atoms needed no new code at all:
verified (against a fixture shaped exactly like the real Gentoo tree's
`virtual/pager`) to already resolve correctly through the existing
category-listing and any-of-group machinery, since a virtual is just an
ordinary ebuild with an any-of `RDEPEND` -- no separate PROVIDE
mechanism exists in modern portage. The `emerge` CLI itself then grew a
full option surface: every real emerge option and action from
`lib/_emerge/main.py` is now recognized by name, so using one this
pilot doesn't implement (of which there are, by design, still many --
only `--pretend`/`-p` is actually implemented) gets a specific "that's a
real emerge option, just not implemented here" message instead of a
generic "unsupported option" one indistinguishable from a typo. Most
recently, `ebuild` got the equivalent treatment for its own, much
smaller CLI (`bin/ebuild`'s six options and `doebuild()`'s 29 valid
commands): still a pure no-op stub -- real phase execution stays
deferred -- but it now recognizes real ebuild syntax and only rejects
input that's genuinely invalid, rather than the original bare stub,
which accepted anything at all without looking at it.

## Layout

```
PORTING/
  PROMPT.md                    planning prompt this pilot implements
  rust/                        Rust workspace
    portage-versions/          shared lib: port of lib/portage/versions.py (vercmp, ververify)
    portage-dep/                shared lib: v1 subset of Atom + match_from_list,
                                 plus a separate bounded wildcard-atom matcher for
                                 package.mask/.unmask/.accept_keywords (extracted
                                 from atom-harness; see lib.rs's doc comment)
    portage-use-reduce/          shared lib: use_reduce(flat=True) (extracted from
                                 use-reduce-harness; see lib.rs's doc comment)
    portage-profile/             shared lib: real USE/ACCEPT_KEYWORDS from a
                                 profile chain + make.conf, plus package.mask/
                                 .unmask/.accept_keywords/.use (see lib.rs's doc comment)
    portage-repo/                multi-repo (main + overlays)/metadata/vdb access +
                                 resolution + recursive, slot-aware dependency-graph
                                 walk + blocker/slot-conflict reporting for
                                 `emerge --pretend` (see lib.rs's doc comment on
                                 resolve_pretend_graph)
    versions-harness/          CLI harness over portage-versions
    atom-harness/               CLI harness over portage-dep
    use-reduce-harness/         CLI harness over portage-use-reduce
    multicall/                 the real emerge/ebuild dispatch binary; `emerge`
                                 implements --pretend + dependency recursion +
                                 real profile/make.conf config (pretend.rs),
                                 recognizes every real emerge option/action by
                                 name (emerge_options.rs) even though only
                                 --pretend/-p is implemented; `ebuild` recognizes
                                 its own real options/commands by name
                                 (ebuild.rs/ebuild_options.rs) but remains a
                                 pure no-op dry-run stub -- real phase execution
                                 is still deferred
  python/
    versions_harness.py        thin CLI wrapper around the real portage.versions
    atom_harness.py             thin CLI wrapper around the real portage.dep
                                 Atom/match_from_list, restricted to the v1 subset
    use_reduce_harness.py       thin CLI wrapper around the real
                                 portage.dep.use_reduce(flat=True)
    emerge_pretend_reference.py  Python reference implementation of the same
                                 emerge --pretend v1 algorithm (including profile/
                                 make.conf resolution), for contract testing
  fixtures/                    synthetic repo+vdb+profile tree emerge --pretend is
                                 contract-tested against (repos.conf, ebuilds,
                                 md5-cache, a fake vdb, a profile chain +
                                 make.conf -- see below)
  bench/                       benchmark-mode timing comparison (the CI perf gate)
    gentoo_snapshot.json         vendored real Gentoo tree snapshot (19442 packages,
                                 32862 version strings; see extract_snapshot.py)
    extract_snapshot.py          (re-)generates gentoo_snapshot.json from a live tree
    dataset.py                  turns the snapshot (default) or seeded-random
                                 synthetic versions (fallback) into batch input lines
    run_benchmark.py            times both harnesses' `batch` mode, reports
                                 speedup, checks/updates baseline.json
    baseline.json                recorded speedup from the last --update-baseline run
  musl/                         musl static-build smoke test (the minimal-Linux CI gate)
    Containerfile                 alpine/musl builder stage -> FROM scratch runtime stage
    smoke_test.sh                 builds the image, runs it, checked as a CI gate
  tests/                       shared, black-box pytest contract suite
    conftest.py                 builds the Rust binaries, exposes both harnesses
    test_versions_contract.py   asserts identical output, Python vs. Rust
    test_atom_contract.py       asserts identical output, Python vs. Rust
    test_use_reduce_contract.py asserts identical output, Python vs. Rust
    test_emerge_pretend_contract.py  real emerge binary vs. the Python
                                 reference, against PORTING/fixtures
    test_multicall.py           tests the compiled dispatch binary via symlinks
    test_benchmark_gate.py      opt-in wrapper around run_benchmark.py for CI
    test_musl_smoke.py          opt-in wrapper around musl/smoke_test.sh for CI
```

## What this proves

- **`versions-harness`**: a faithful Rust port of `vercmp`/`ververify`,
  checked against the real Python implementation through a neutral CLI
  contract (not a product CLI, not FFI/PyO3 bindings) -- see `PROMPT.md`
  hard goal 4 and the "black-box via CLI/API" decision.
- **`multicall`**: proves the `argv[0]`-based dispatch mechanism for
  shipping `emerge`+`ebuild` as one static binary (`PROMPT.md`,
  "emerge/ebuild binary shape"). `ebuild` and everything about `emerge`
  beyond the slice below are still dry-run stubs, per the first-port scope
  decision -- no real merges or phase execution yet.
- **`atom-harness`**: the depgraph/config-resolution follow-up work's first
  slice -- both of those subsystems are built on atom parsing and matching
  a dependency atom against candidate package versions, so it's the
  natural next layer above `versions-harness` (which it depends on for
  `vercmp`). Deliberately scoped to a documented v1 grammar subset (no USE
  deps, wildcards, build-ids, or repo constraints -- see the doc comment
  at the top of `rust/portage-dep/src/lib.rs`); the Python harness
  explicitly rejects anything outside that subset as `INVALID` so both
  sides agree on the same input language. Includes a faithful port of an
  easy-to-miss PMS rule: a bare atom whose package name is followed by
  something version-shaped (`foo-bar-2`) is rejected as ambiguous, not
  silently accepted. (`portage-dep` later gained a *separate*,
  deliberately bounded wildcard-atom matcher --
  `*/*`/`category/*`/`*/package` only -- for package.mask/.unmask/
  .accept_keywords, without touching this grammar or contract at all.)

  **Slot operators** (`:=`, `:*`, `:slot=` -- PMS 8.3.3) were added to the
  grammar later (wildcards, build-ids, and EAPI parametrization remain
  out of scope -- see the `::reponame` paragraph further below for why
  repo constraints no longer do; those are a fundamentally bigger
  undertaking each, slot operators were not). `SlotOperator` mirrors
  real portage's own two-stage parse (`_get_atom_re` captures the raw text
  after `:`, `_get_slot_dep_re` re-parses it), including its two rejection
  rules verified directly against real `Atom`: a bare trailing `:` with
  nothing after it (`dev-libs/foo:`) is invalid, and an explicit slot
  combined with `*` (`dev-libs/foo:0*`) is invalid ("any slot" is
  meaningless alongside a specific one). *Matching* needed zero new code
  at all: real `_match_slot` never consults `slot_operator`, only
  `Atom.slot`/`.sub_slot` -- a bare `:=`/`:*` atom has `slot == None`,
  which `matches_slot`'s existing early-return (mirroring real
  `match_from_list`'s own `if mydep.slot is not None:` guard) already
  treated as "no slot restriction" before this slice touched anything.
  This closed a genuine, previously-silent **parity bug**, not just a
  scope gap: `resolve_pretend_graph`'s BFS loop treats a dependency token
  that fails to parse as "not a dependency, skip it," not as an
  unresolvable one, so any real DEPEND/RDEPEND using a slot operator
  (extremely common in real ebuilds, e.g. `dev-libs/foo:0=` for
  ABI-rebuild tracking) was silently dropped from the graph on the Rust
  side -- no entry, no `NoVisibleCandidate`, no warning. The Python
  reference side never had this bug: its own dependency-atom parsing
  always used the real, unrestricted `portage.dep.Atom` (only the CLI's
  top-level-atom validation narrowed the grammar), so it already resolved
  slot-operator dependencies correctly, undetected only because no
  fixture had ever exercised the case.

  **USE deps** (`foo[bar]`, all 7 real per-flag forms plus 4-style
  `(+)`/`(-)` defaults -- PMS 8.3.4) closed the same class of bug for a
  second, even more common real-world case: any DEPEND/RDEPEND using
  e.g. `sys-libs/zlib[static-libs?]` was silently dropped from the graph
  exactly like a slot-operator dependency used to be, confirmed
  empirically before starting this slice (the Python reference side,
  again, never had the bug). Unlike slot operators, this is a
  deliberate, confirmed-with-the-user **parse-only** slice: `UseDep`/
  `UseDepOp`/`UseDepDefault` capture the full grammar (mirroring real
  `Atom.__init__`'s own two-stage validation -- a per-token regex, then a
  check that only the 6 real `prefix`+`suffix` combinations appear, e.g.
  `-flag=`/`-flag?` are syntactically matched but not real operators, and
  that a flag's `(+)`/`(-)` default stays consistent across every token
  mentioning it in the same atom -- both rules verified directly against
  real `Atom`), but the *values* are never consulted by
  `matches_version`/`matches_slot`/`match_from_list`. Full enforcement
  was scoped out because the `opt=`/`opt?` forms are conditional on the
  *atom-owning* package's own USE state, not just the candidate's --
  genuinely new matching architecture, not a grammar extension -- and
  this pilot's `Candidate`/`match_from_list` model has no per-candidate
  IUSE/USE state to check a use-dep against at all. This isn't an
  invented gap, though: verified empirically, real `match_from_list`
  given this pilot's own plain-string candidates (no `.use`/`.iuse`
  attributes) already skips its own USE-dep filtering entirely too --
  `dev-libs/foo[bar]` and `dev-libs/foo[-bar]` return the identical
  match set against real `match_from_list` itself, not just this pilot's
  port of it.

  **The `=*` glob version operator** (PMS 8.3.1) closes the last named
  scope cut in `portage-dep`'s own module doc comment. PMS: "if the
  version specified has an asterisk immediately following it, then only
  the given number of version components is used for comparison, i.e.
  the asterisk acts as a wildcard for any further components" -- and, per
  the PMS's own historical note, this component-wise semantic is the
  *current* one: a raw string-prefix match (e.g. `=foo-5.2*` matching
  `foo-5.22.0`) was the original EAPI 0-5 behavior, retroactively dropped
  in October 2015, well before this repo's EAPI 5+ floor. Real portage
  still implements it as a literal string-prefix match rather than a
  `vercmp`-based one (its own comment: "Nasty special casing for leading
  zeros / Required as =* is a literal prefix match, so can't use
  vercmp"), so this pilot ports that same algorithm: a prefix match on
  `version[-rN]`, accepted only at a genuine component boundary (fixing
  real bug 560466 -- `"1*"` must not match `"10"`, even though `"10"`
  literally starts with `"1"`: the boundary check is "next character is
  `.`/`_`/`-`, or its digit-ness differs from the matched prefix's last
  character"), plus the "leading zeros" normalization (`"01"` and `"1"`
  compare identically as prefixes, `"00.5"`'s redundant second zero is
  dropped, `"0.5"`'s single leading zero is kept since it's a real digit)
  -- both verified empirically against real `match_from_list` (a `python3
  -c` probe over several leading-zero/boundary cases) before relying on
  either. Unlike slot operators/USE deps, this operator's Python side
  needed no new harness code at all: `atom_harness.py` already wraps real
  `match_from_list`, so letting `"=*"` through its own
  `_SUPPORTED_OPERATORS` allow-list was the only change required for it
  to run against real portage's actual `=*` branch, unmodified. PMS is
  also explicit that "an asterisk used with any other operator is
  illegal" (e.g. `>=foo-1.2*`) -- ported as an explicit rejection, not a
  silent truncation to `>=foo-1.2` or a silent accept under the wrong
  operator.

  **The `::reponame` repo constraint** (PMS 3.1.5) closes the last
  remaining item in `portage-dep`'s own scope-cut list. Ported from real
  `match_from_list`'s own final post-pass filter (only run `if
  mydep.repo:`): a candidate is rejected only if it carries a *known*
  repo that differs from the atom's -- a candidate with no repo info at
  all always passes, matching real `dep_getrepo`'s own "unknown, not
  absent" semantics for a plain string. This pilot's own candidate
  strings never carried repo identity at all before this slice, so it
  pairs a `portage-dep` grammar/matching addition (`Atom::repo`/
  `Candidate::repo`, mirroring the existing `:slot` suffix convention)
  with a `portage-repo` wiring change: every candidate string that crate
  builds for `match_from_list` -- `resolve_pretend`'s own top-level *and*
  dependency-atom matching, `is_visible`'s package.mask/.unmask/
  .accept_keywords matching, and `effective_use_flags`'s package.use
  matching -- now appends `::name` using each repo's own already-tracked
  `RepoConfig::name` (its `repos.conf` section name, reused as-is rather
  than reading a second, separate `profiles/repo_name` file real portage
  also cross-checks against). Verified end to end against the fixture's
  own two repos, not just parsed: `dev-libs/foo::testrepo` resolves the
  main repo's copy, `dev-libs/foo::overlay` correctly finds none (`foo`
  only exists in the main repo), and `dev-libs/overlayonlypkg::overlay`
  vs. `::testrepo` prove the reverse. Two paths deliberately keep their
  pre-existing, repo-less candidate strings, a narrower scope cut than
  the rest of this feature's wiring: blocker matching (`resolve_blockers`,
  which builds candidates from vdb/graph `(version, slot)` pairs that
  never tracked repo identity to begin with) and a top-level atom's own
  slot-conflict re-verification against an *already-resolved* candidate
  from a different atom (the `GraphEntry` it's checked against doesn't
  carry repo identity either) -- both real, if narrow, corners a
  `::repo`-constrained atom could theoretically still get wrong.
- **`use-reduce-harness`**: ports `use_reduce(flat=True)` -- USE-conditional
  (`flag? ( ... )`) and any-of (`|| ( ... )`) dependency-string flattening.
  Unlike `atom-harness`, this is *not* a narrowed grammar: flat mode's
  tokenizer/bracket/conditional handling is fully self-contained in the
  real implementation and is ported as-is, error behavior included (bad
  brackets, dangling `flag?`/`||`, literal empty parens, invalid USE flag
  names in a conditional all fail the same way in both). What's out of
  scope is a set of optional parameters the harness doesn't exercise
  (`masklist`, `excludeall`, SRC_URI's `->` arrow token, `opconvert`,
  non-flat structured output, `subset`, atom validation via `token_class`)
  -- see the doc comment at the top of `rust/use-reduce-harness/src/use_reduce.rs`.
  `flat=True` itself is a real, heavily-used mode, not a convenience
  fiction: `config.py`, the resolver, and `_emirrordist` all call it this
  way for RESTRICT/PROPERTIES/IUSE-shaped values. Building this caught a
  genuinely surprising real rule worth a regression test: a USE flag name
  is allowed to *start* with a digit (`1notaflag` is valid per the real
  `useflag_re`), which is easy to assume is invalid and get wrong.
- **`emerge --pretend category/package`** (the answer to "what's missing
  for this to succeed?"): a real, working slice, built on `portage-dep`,
  `portage-versions`, `portage-use-reduce`, and `portage-profile`.
  `portage-repo` finds every configured repo via `repos.conf` (INI,
  `[DEFAULT] main-repo` / any number of `[name] location` sections -- the
  main repo plus overlays), lists candidate versions from ebuild
  filenames across all of them, reads `metadata/md5-cache/<cat>/<pf>`
  (plain `KEY=value` text -- confirmed against a real vendored tree) for
  KEYWORDS/SLOT/DEPEND/RDEPEND *without executing any bash*, and checks
  the vdb (`<ROOT>/var/db/pkg`) for what's installed. There's still no
  backtracking -- an explicitly confirmed scope cut before implementing,
  not a silent omission (see the doc comment at the top of
  `rust/portage-repo/src/lib.rs`).
  Config/target roots come from the real `PORTAGE_CONFIGROOT`/`ROOT`
  environment variables (portage's own mechanism, not a pilot invention --
  see `lib/portage/const.py`), which is what lets `PORTING/fixtures` be
  used hermetically in tests instead of the real system tree. Output is a
  documented, simplified subset of real `--pretend` formatting
  (`[ebuild  N] cat/pkg-1.2.3`, `[ebuild  U] cat/pkg-2.0 (upgrade from 1.0)`,
  or an already-installed/no-visible-candidate message), not
  byte-identical to real emerge. Building this surfaced a real bug before
  it ever shipped: an early version of the vdb directory scan let a
  sibling package sharing a name prefix (`foo-bar` installed) get misread
  as an installed version of `foo`; `rust/portage-repo/src/lib.rs`'s
  `sibling_package_prefix_does_not_contaminate_vdb_scan` unit test
  reproduces it and pins the fix (verified by temporarily reverting the
  fix and confirming the test fails the way predicted).

  **Dependency recursion** (`resolve_pretend_graph`): walks DEPEND+RDEPEND
  (flattened via `portage-use-reduce` with `USE=""`, deduped across both
  fields and across packages via a visited set, so diamond dependencies
  and cycles are both handled -- a cycle fixture pair proves this
  terminates rather than looping forever) for every package that would
  newly merge or upgrade; an already-installed package's own deps are
  presumed satisfied. Two more scope calls confirmed with the user before
  implementing: `||` (any-of) groups resolve *every* alternative rather
  than picking one, because `use_reduce(flat=True)` deliberately discards
  group boundaries so there's no reliable way to identify "the first
  alternative" without reimplementing non-flat structured mode (a
  considerably bigger, separately out-of-scope piece of work); and an
  unresolvable dependency doesn't fail the whole graph -- it still shows
  up (flagged, on stderr) rather than being silently dropped. That
  "silently dropped" framing was actually my first draft of the doc
  comment; the `recursion_survives_an_unresolvable_dependency` unit test
  caught that the code didn't match what the comment claimed, which is
  what led to fixing the comment (the code's behavior -- report, don't
  drop -- was the better one).

  **Real USE/ACCEPT_KEYWORDS** (`portage-profile`): replaces the
  `ACCEPT_KEYWORDS="amd64"`/`USE=""` hardcoding with an actual profile
  inheritance chain (`make.profile` -> recursive `parent` files, same-repo
  only, multi-parent levels processed in listed order, cycle/diamond-safe)
  plus `/etc/portage/make.conf` (including its own `source <path>`
  directive, resolved against `PORTAGE_CONFIGROOT` chroot-style). Each
  level's `make.defaults` contributes real incremental-variable semantics
  (`-*` clears, `-flag` removes, `flag`/`+flag` adds), including
  `${VAR}` shell-style substitution -- necessary because virtually every
  real profile level in the vendored tree does e.g. `USE="${USE} xattr"`
  to self-append rather than overwrite. One genuine, easy-to-miss quirk
  ported faithfully from `config.py` (found by reading the real code's own
  comment on it, not by guessing): `USE` specifically is excluded from
  cross-level `${VAR}` substitution, so a parent profile's accumulated USE
  can't leak into a child's own `USE="${USE} flag"` self-append -- every
  other variable (e.g. `ARCH`, which real profiles use to set
  `ACCEPT_KEYWORDS="${ARCH}"`) persists normally across levels. Explicitly
  out of scope: cross-repo profile parents (`reponame:path` syntax, which
  the real dev machine's own profile actually uses -- so testing this
  mechanism needed a new synthetic same-repo, multi-parent fixture chain
  rather than the real system profile), `USE_EXPAND`, wildcard `_*`
  expansion, and every `USE_ORDER` layer except `defaults` (profile) and
  `conf` (make.conf) -- see the doc comment at the top of
  `rust/portage-profile/src/lib.rs`.

  **`package.mask`/`.unmask`/`.accept_keywords`**: the last piece of
  "which packages are even visible" -- a candidate is masked if it matches
  a `package.mask` entry and no `package.unmask` entry (a simpler
  masked-unless-also-unmasked check, not real portage's own incremental
  `-atom` stacking across repo/profile/user sources -- see below), and
  `package.accept_keywords` extends the globally-accepted keyword set
  per-atom, with a `"**"` token meaning "accept unconditionally" (even a
  package with no `KEYWORDS` at all, like a live/9999 ebuild). Grounding
  this against the real dev machine's own `/etc/portage/package.*` files
  surfaced two things worth scoping explicitly: real `package.mask` is
  actually stacked from *three* sources (repo-level `profiles/package.mask`,
  per-profile `package.mask` in the inheritance chain, and user-level
  `/etc/portage/package.mask`) -- replicating that fully would be close to
  the size of the whole profile-chain slice again, so v1 only implements
  the user-level file (with its own `-atom` removal, e.g. across multiple
  files if `package.mask` is a directory); and real `package.use`/
  `package.accept_keywords` lean heavily on wildcard atoms in practice
  (`dev-qt/*`, `*/*`) that `portage-dep`'s v1 grammar explicitly excludes,
  so matching a candidate against a `package.*` entry is two-tier: try the
  real, already-verified atom grammar first (covers versioned/slotted
  entries), and fall back to a new, deliberately bounded wildcard matcher
  (`*/*`/`category/*`/`*/package` only, no partial-string globs like
  `cat/pkg-*`) only if that fails to parse the entry at all. On the Python
  side this fallback isn't needed at all: real `portage.dep.Atom(allow_wildcard=True)`
  already handles exactly those bounded forms correctly via its own
  `extended_syntax` path (verified empirically to agree with the Rust
  side's bounded matcher for them, before relying on it). See the doc
  comments in `rust/portage-profile/src/lib.rs` and
  `rust/portage-dep/src/lib.rs` for the full scope writeup.

  **`package.use`**: per-package USE overrides, layered on top of
  `package.mask`/`.unmask`/`.accept_keywords`'s "which packages are even
  visible" with "which USE flags does *this specific package* see". Unlike
  every other `Config` field, which is a fixed value for the whole
  resolution, `package.use` is applied once per graph node in
  `resolve_pretend_graph` (see `effective_use_flags`): each package's own
  `DEPEND`/`RDEPEND` are flattened against a clone of the base USE set with
  any matching `package.use` entry's tokens layered on top via the same
  incremental `-flag`/`flag`/`+flag` semantics real `USE` itself uses (a
  deliberate, non-trivial reuse of `portage-profile`'s
  `apply_incremental`, now made `pub` for exactly this) -- never leaking
  into a sibling or dependency's own resolution. Matching an entry against
  a candidate reuses the same two-tier atom/wildcard matcher as
  `package.mask`/`.unmask`/`.accept_keywords`, which is why it needs the
  candidate's resolved `SLOT` (only available at `portage-repo`'s
  repo-aware layer, unlike `USE`/`ACCEPT_KEYWORDS`/`package.mask`, which
  `portage-profile` can compute on its own). Out of scope: the
  `USE_EXPAND`-prefix shorthand real `package.use` supports (`VIDEO_CARDS:
  nvidia` lines applying a `video_cards_` prefix to subsequent flags until
  a blank line resets it) -- only plain tokens are read.

  **Blockers**: `!atom`/`!!atom` tokens found while flattening a New/
  Upgrade package's own DEPEND/RDEPEND (see `BlockerConflict`,
  `PendingBlocker`, and `resolve_blockers`) are matched -- via the same
  `match_from_list` every other atom-vs-candidate check in this crate
  uses, which turns out to ignore an atom's blocker marker entirely, so a
  `!`/`!!`-prefixed atom string matches candidates by
  category/package/version/slot exactly like a normal one (verified
  empirically before relying on it, same as everywhere else this pilot
  reaches for real portage code to settle a question rather than
  guessing) -- against both currently-installed packages
  (`installed_candidates`, a small SLOT-aware sibling of
  `installed_versions`) and the rest of the same graph's own New/Upgrade
  set, resolved in a single post-pass once the whole graph is known (so a
  match doesn't depend on BFS discovery order: two packages can block
  each other regardless of which one the queue reaches first). This is
  reporting only, matching real `--pretend`'s own "calculate and show,
  don't touch anything" behavior: v1 makes no attempt to resolve a
  conflict (no merge reordering, no refusing to proceed), and a strong
  (`!!`) match doesn't change the exit code any differently than a weak
  (`!`) one -- printed as `[blocks] cat/pkg-version hard|soft blocks
  cat2/pkg2-version2 ("!!cat2/pkg2")` right after the blocking package's
  own `[ebuild ...]` line. "Strong" is determined the same way real
  portage's own `--pretend` output does (`blocker.atom.blocker.overlap.forbid`,
  the real "hard blocking" vs "soft blocking" signal -- not the `!!`
  prefix text, which was checked and confirmed empirically to agree with
  it for exactly the weak/strong split portage-dep's `Blocker` enum
  already carries).

  **Overlays**: `find_repos` now returns every `[reponame]` section in
  `repos.conf` that has a `location` -- the main repo plus any number of
  overlays -- not just `[DEFAULT] main-repo`. `list_candidates` gathers
  ebuilds from ALL of them for a given category/package, mirroring real
  `portdbapi.cp_list` (an overlay isn't "consulted only if the main repo
  has nothing"; every repo's ebuilds are real candidates), and each
  resulting `Candidate` remembers which repo it actually came from
  (`repo_location`/`repo_priority`) so a package's own DEPEND/RDEPEND can
  later be re-read from the *right* repo, not always the main one.
  Repos are sorted ascending by `(priority, name)`, exactly matching real
  portage's own `prepos_order` (see
  `lib/portage/repository/config.py`) -- a repo's priority is its
  explicit `repos.conf` value if set, else `-1000` for the main repo
  (real portage's own default) or `0` for anything else -- so a tie
  between two repos providing the *identical* version is broken toward
  the higher-priority one, both for version selection
  (`resolve_pretend`'s final `max_by`) and for which repo's metadata gets
  read for that package's own dependencies (`resolve_pretend_graph`).
  Deliberately out of scope: per-repo `package.mask`/`.unmask`/
  `profiles/`, `masters` (eclass inheritance across repos), and
  `::repo`-constrained atoms (already excluded by `portage-dep`'s v1
  grammar) -- overlays only widen *which ebuilds are candidates*, nothing
  about how they're evaluated once found.

  **Slot conflicts**: `resolve_pretend_graph` now dedupes and recurses by
  `(category, package, slot)` instead of `category/package` alone --
  which fixes a real, latent gap the visited-set had ever since
  recursion existed: two atoms requesting *different* slots of the same
  package (`dev-lang/python:3.11` and `:3.12`, say) used to have the
  second one silently swallowed by the package-only visited check,
  exactly as if it had never been a dependency at all. Now they're two
  genuinely independent `GraphEntry` values, each recursed into on its
  own -- matching how real portage actually treats multiple slots of one
  package (normal, valid coexistence, the entire point of `SLOT`). A
  *conflict* (`SlotConflict`) only exists when a **second** atom lands on
  a slot some earlier atom already resolved, and that earlier resolution
  doesn't satisfy the second atom's own constraint (checked with the same
  `match_from_list` every other atom-vs-candidate comparison in this
  crate uses) -- e.g. one dependency wants any version of `foo:0` and
  gets `2.0`, while another wants `<foo-2.0`, also slot `0`. Same "report,
  don't enforce" spirit as blockers: real portage's own depgraph treats
  an unresolved slot conflict as fatal and refuses to proceed; v1 instead
  keeps whichever version was resolved first, reports the conflict, and
  moves on -- `--pretend` itself never touches anything real, so nothing
  here is truly "fatal" to calculate. Two smaller pieces of bookkeeping
  changed to make this correct: cycle/duplicate termination now dedupes
  on exact *atom text* (a `visited_atoms` set) rather than on
  category/package, since the slot a bare atom resolves to isn't known
  until after resolution; and `SLOT` moved from a side-table
  (`portage-repo`'s former `graph_slots`) onto `GraphEntry` itself
  (`slot: Option<String>`, `None` for `AlreadyInstalled`/
  `NoVisibleCandidate`, which don't carry one), which also let
  `resolve_blockers` be fixed to check *every* graph-resolved slot of a
  blocker's target package, not just whichever one happened to be
  recorded last.

  **Virtuals**: unlike every other follow-up in this series, this one is
  pure verification -- no resolution code changed at all. Real
  `virtual/*` packages (checked directly against the vendored Gentoo
  tree's own `virtual/pager`, not assumed) turn out to be nothing more
  than ordinary ebuilds in a category named `virtual`, whose `RDEPEND`
  is a plain `|| ( ... )` any-of group of real providers -- there is no
  separate PROVIDE-based virtuals mechanism in modern portage for this
  pilot to have missed. Since `portage-repo` already treats every
  category identically (no special-casing anywhere) and already
  resolves every alternative of an any-of group (the documented v1
  simplification from the recursion follow-up), a `virtual/foo` atom --
  whether given directly or reached via another package's own
  DEPEND/RDEPEND -- was already being resolved correctly before this
  slice existed. What this slice actually adds is a fixture package
  (`virtual/texteditor`, deliberately shaped like `virtual/pager`: an
  any-of `RDEPEND` over two real fixture packages) and contract tests
  that pin this down, so it stays proven rather than merely assumed.

  **CLI surface recognition**: `multicall/src/emerge_options.rs`
  transcribes real emerge's entire option surface from
  `lib/_emerge/main.py` into three tables -- boolean flags (the
  `options` list), value-taking options (the `argument_options` dict,
  each with its `"shortopt"` if any), and actions (the `actions`
  frozenset, e.g. `--depclean`/`--sync`/`--search`, with short aliases
  from `shortmapping`) -- around 130 entries in total. `pretend.rs`'s
  arg loop now looks every `-`-prefixed token up against these tables:
  a real option/action other than `--pretend`/`-p` gets a message
  naming it specifically and saying whether it's an "option" or an
  "action" and that it's real-but-unimplemented, while a token that
  isn't in any of the three tables at all gets a different message
  ("unrecognized option") -- so a user hitting a genuine pilot gap
  (say, from `EMERGE_DEFAULT_OPTS` or a script) can tell that apart
  from a typo. Unlike every other change in this series, this one adds
  *zero* new behavior for any of those ~130 flags -- it only makes the
  CLI's *refusal* to handle them more specific. Deliberately out of
  scope: any actual argument-value semantics for an unimplemented option
  (its value, if it takes one, is never inspected -- the CLI reports and
  exits immediately, before ever needing to skip over it; short-flag
  bundling and `--help`/`-h` were *also* out of scope at this point, but
  each got its own, later follow-up -- see below).

  **`ebuild`'s own CLI surface**: the same treatment, applied to
  `ebuild`'s much smaller real surface -- `multicall/src/ebuild_options.rs`
  transcribes `bin/ebuild`'s own `argparse` setup (six options:
  `--force`/`--color`/`--debug`/`--version`/`--ignore-default-opts`/
  `--skip-manifest`, none with short aliases) and, more usefully,
  `doebuild()`'s own `validcommands` list (29 real commands -- not just
  the EAPI phase names in `lib/portage/const.py`'s `EBUILD_PHASES`, but
  the full set `doebuild()` itself accepts, including non-phase actions
  like `clean`/`digest`/`manifest`/`merge`/`qmerge`/`rpm`/`unmerge`/
  `depend`/`fetch`/`fetchall`/`cleanrm`/`help`). Unlike `emerge`, where
  a recognized-but-unimplemented flag is fatal, `ebuild`'s pre-existing
  behavior was to accept *anything at all* as a silent no-op -- and
  `PORTING/tests/test_multicall.py`'s own dispatch-proof tests (`ebuild
  foo-1.0.ebuild merge`, asserting success) depend on that continuing to
  work. So the split here is different: a real option/command (even
  though none of them do anything) still succeeds, still prints the
  `"ebuild (pilot stub)"` marker those tests check for -- only input
  that isn't valid `ebuild` syntax *at all* (an unrecognized option, a
  filename not ending in `.ebuild`, an unrecognized command, or missing
  required args) is now rejected, with exit codes mirroring real
  `ebuild`'s own conventions (`2` for "missing required args", real
  argparse's `parser.error()`; `1` for everything else, real `ebuild`'s
  own `err()` helper and `doebuild()`'s own return value for an
  unrecognized command). `--color`, the one real value-taking option,
  needed actual value-skipping this time (unlike `emerge_options.rs`,
  where it turned out to be unnecessary): since recognized options don't
  stop parsing here, `ebuild --color y foo.ebuild merge` needs `"y"`
  correctly consumed as `--color`'s value, or it would be misread as the
  ebuild file itself. Deliberately out of scope, and deliberately
  deviating from real `bin/ebuild`'s own quirk: real `ebuild` uses
  argparse's `parse_known_args`, which silently swallows an unrecognized
  flag into the positional-args list rather than rejecting it (usually
  surfacing later as a confusing "does not end with '.ebuild'" error);
  this pilot reports an unrecognized option immediately and specifically
  instead, matching `emerge`'s own clearer philosophy. `ebuild` still has
  no Python reference implementation to contract-test against -- it has
  no real behavior to keep in sync between two implementations, so
  `test_multicall.py`'s black-box tests against the real compiled binary
  are the only test surface, same as before this slice.

  **Multiple top-level atoms**: `emerge --pretend foo bar` -- real
  emerge's most common invocation shape -- was, until this slice,
  explicitly rejected ("only a single package atom is supported"). Now
  `resolve_pretend_graph` takes a slice of atoms instead of one, and
  seeds all of them into the same BFS queue together, in argv order,
  before any dependency is ever pushed: every piece of dedup/slot-
  conflict/blocker bookkeeping the recursion follow-up already built is
  keyed by atom text or `(category, package, slot)`, not by "the one
  root", so it needed zero new code to handle sharing between two
  *targets* the same way it already handled sharing between two
  *dependencies* -- a dependency common to two requested packages dedupes
  like a diamond dependency always did, and a slot conflict between two
  targets (not just between two deps of one target) is now detected too.
  A top-level atom with no visible candidate is fatal to the whole call,
  not reported-and-continued the way a dependency's own
  `NoVisibleCandidate` is -- confirmed with the user before implementing,
  over the alternative of resolving the good atoms and reporting the bad
  one alongside them. Since top-level atoms are always dequeued in argv
  order before any dependency, the *first* bad one aborts the run before
  any later atom, top-level or not, is even attempted, and before
  anything is printed -- matching real portage's own actual behavior:
  grounded against `lib/_emerge/depgraph.py`'s real "there are no ebuilds
  to satisfy" message (not a guess), which the pilot's own top-level
  "not found" message now uses verbatim instead of its previous
  placeholder wording. The old single-atom "already installed; nothing
  to do" shortcut (a `len(entries) == 1` special case) generalizes
  cleanly: it's no longer a special case at all, just the ordinary rule
  that any directly-requested atom resolving `AlreadyInstalled` gets its
  own such line, while one reached only as a dependency stays silent, as
  before.

  **Versioned/slotted top-level atoms**: `emerge --pretend '>=cat/pkg-1.2'`
  or `emerge --pretend cat/pkg:0` -- a target carrying an operator or slot,
  not just a bare `category/package` -- was, until this slice, rejected
  outright ("only a bare category/package atom is supported"), even though
  `resolve_pretend`'s own atom-vs-candidate matching (`portage_dep::match_from_list`)
  already handled operators and slots correctly for every dependency atom
  extracted from DEPEND/RDEPEND. Lifting the CLI-level restriction was the
  entire slice: zero resolution-logic changes were needed, since a
  top-level atom and a dependency atom were always resolved through the
  exact same code path. Grounding this in the grammar surfaced a real,
  pre-existing gap worth fixing at the same time: the old check never
  tested for a blocker (`!`/`!!`) at all, so `emerge --pretend '!!cat/pkg'`
  was silently accepted by the CLI and then silently dropped by
  `resolve_pretend_graph`'s own blocker skip -- exit 0, no output, no
  error, instead of being rejected the way real portage rejects a bare
  blocker as an emerge target -- confirmed with the user before folding
  the fix into this same slice rather than deferring it. On the Python
  reference side, `_parse_atom` uses the *real* `portage.dep.Atom` (richer
  than Rust's own deliberately narrowed `portage-dep` crate), so an input
  using a grammar feature Rust's `Atom` struct has no field for at all
  (at the time this paragraph was written: USE deps, repo constraints,
  wildcards, build-ids, slot operators like `:=`/`:*` -- all but
  wildcards/build-ids have since gained real fields and been removed
  from `_has_unsupported_top_level_features`'s own check, see each
  feature's own later paragraph) needs an explicit
  `_has_unsupported_top_level_features` check to still produce the same
  "invalid atom" outcome Rust's own `parse_atom` would (returning `None`
  outright for that input) -- verified empirically atom-by-atom against
  `atom-harness parse` rather than assumed.

  **USE flags in `--pretend -v` output**: `--verbose`/`-v` moves from
  "recognized, not implemented" to a second real, implemented flag
  alongside `--pretend`/`-p` -- grounded against real portage's own gating
  logic (`lib/_emerge/resolver/output_helpers.py`'s
  `print_use_string = self.verbosity != 1 or "--verbose" in myopts`):
  USE flags are off by default and only shown with `-v`, exactly like real
  `emerge`. Each `New`/`Upgrade` `GraphEntry` now carries
  `use_flags_display`, the already-computed `effective_use_flags` result
  (the same set dependency recursion itself flattens DEPEND/RDEPEND
  against) filtered down to just this package's own IUSE-declared flags
  -- IUSE is newly read from the same md5-cache metadata DEPEND/RDEPEND
  already come from, with its own `+`/`-` default markers stripped to get
  the bare flag name (a flag's default only matters for resolving USE
  when nothing else decides it, which is already handled wherever
  `effective_use_flags` gets its input -- display only needs the name and
  the final enabled/disabled verdict). This is always computed, never
  gated on `--verbose` itself, keeping `resolve_pretend_graph`'s behavior
  otherwise identical either way; the CLI layer alone decides whether to
  print it, appending `  USE="flag1 -flag2"` (two leading spaces,
  alphabetically sorted, enabled plain / disabled `-`-prefixed) after the
  package spec on its `[ebuild ...]` line, or nothing at all for a
  package with no IUSE. Real portage's own USE display is considerably
  more elaborate than this -- colorized, diffed against the previously
  installed version's IUSE with `*`/`%` change markers, forced/masked
  flags in parens, `USE_EXPAND` grouping (`VIDEO_CARDS`, etc.) -- none of
  which this pilot has the underlying data (or terminal-color
  infrastructure, unused anywhere else in this pilot) to reproduce; v1
  shows only the plain enabled/disabled set, which is a real, useful
  subset rather than an invented one, matching the "documented,
  simplified subset" spirit of every other output-formatting decision in
  this pilot.

  **BDEPEND/PDEPEND/IDEPEND in recursion**: dependency recursion now
  concatenates and flattens all five real dependency-string keys --
  `DEPEND`, `RDEPEND`, `BDEPEND` (build-time-only, EAPI 7+), `PDEPEND`
  (post-merge), `IDEPEND` (install-time, EAPI 8+, rare) -- fully closing
  a scope cut this pilot had named explicitly since the original
  recursion follow-up. Real portage's own merge ordering treats these
  differently (BDEPEND must be satisfied on the *build host* before
  compiling starts; PDEPEND only needs to be satisfied *after* this
  package itself merges; IDEPEND only at install time), but that
  distinction is meaningless for a `--pretend`-only pilot with no real
  merge ordering or phase execution to begin with (see `PROMPT.md`'s
  "Deferred: ebuild phase execution") -- so v1 treats all five uniformly
  as "a dependency this package needs, resolve and report it", the same
  simplification already applied to blockers and slot conflicts
  elsewhere in this recursion. Mechanically small: one line changed on
  each side (the list of metadata keys concatenated into the flattened
  dependency string), reusing every other piece of recursion machinery
  (dedup, `||` handling, blocker extraction) unmodified.

  **Full `package.mask`/`.unmask` 3-source stacking**: replaces the
  previous user-level-only check with real portage's actual mechanism,
  grounded directly against `MaskManager.py`: three sources -- the main
  repo's own repo-level `<repo>/profiles/package.mask`/`.unmask` (real
  portage's most common real-world masking source, e.g. security/arch
  masks), every profile level's own `package.mask`/`.unmask` pair (in
  chain order, same order `make.defaults` is processed in), and the
  user-level `/etc/portage` files -- concatenated in exactly that order
  and stacked with `-atom` removal applying across the *whole combined
  stream*, not just within one file, matching real
  `stack_lists(incremental=1)` exactly (`stack_mask_lines`, shared
  between mask and unmask, both of which real portage stacks
  identically). This is a genuine correctness improvement beyond just
  adding sources: the pilot's own `package.unmask` previously treated a
  leading `-` as meaningless, since with only one source there was
  nothing for it to meaningfully remove -- reading real `MaskManager.py`
  showed `-atom` removal is just as real for `package.unmask` as for
  `package.mask` once more than one source exists, so it's honored now
  too. Threading in the repo-level source required one real,
  user-confirmed architecture change: `portage-profile::resolve_config`
  previously had zero knowledge of repos (repo discovery lives entirely
  in `portage-repo`, which already depends on `portage-profile`, so the
  dependency can't run the other way) -- it now takes the main repo's
  own location as a parameter, discovered by the CLI layer via
  `find_repos` (which gained an `is_main` field for exactly this) before
  `resolve_config` is even called. Deliberately still out of scope,
  matching the overlays follow-up's own already-confirmed cut: an
  *overlay* repo's own repo-level `package.mask`/`.unmask` (only the one
  main repo's is read), and `masters` (eclass/mask inheritance across
  repos). `package.accept_keywords`/`.use` remained user-level only for
  now -- real portage has repo/profile-level equivalents for both too,
  but stacking those was a separate, still-open cut this slice didn't
  claim to close (`package.accept_keywords` closed in the follow-up
  below; `package.use` remains open).

  **`package.accept_keywords` profile-chain stacking**: extends this
  same file from user-level-only to profile-chain (in chain order) +
  user-level, grounded directly against real `KeywordsManager.
  getPKeywords`. Digging into the real mechanism first turned up an
  asymmetry worth confirming with the user before assuming
  `package.use` would be an equally simple extension of the same
  pattern: real portage has **no repo-level source for
  `package.accept_keywords` at all** (confirmed by reading
  `KeywordsManager.__init__`, which never reads a repo-location path for
  it), so this is a clean 2-source extension, not 3 -- and it's purely
  additive on both sides (no `-atom` removal exists for this file in
  real portage either), so concatenating profile-chain lines (in order)
  then user-level lines, then parsing once, is equivalent to real
  portage's own per-source-then-extend loop. `package.use`, by contrast,
  turned out to be considerably more entangled: real portage's
  repo-level `package.use` lands in a distinct `configdict["repo"]`
  layer and profile-level in `configdict["defaults"]`, both governed by
  the full `USE_ORDER` precedence sequence this pilot only partially
  implements -- extending it the same way `package.mask` and
  `package.accept_keywords` were would be a further, undocumented
  simplification beyond what's already there, not a precedent-following
  extension, so -- confirmed with the user -- it was deliberately left
  out of this slice for its own, properly-scoped follow-up. One more
  small asymmetry worth noting: real portage gives a bare *profile*-level
  `package.accept_keywords` entry (no keyword tokens after the atom) an
  implicit derived `~arch` meaning (`accept_keywords_defaults` in
  `getPKeywords`) that a bare *user*-level entry never gets; v1 treats a
  bare entry as a no-op at both levels, same simplification the
  pre-existing user-level-only handling already made, kept deliberately
  symmetric rather than adding a profile-only special case.

  **Real `-v` value semantics + short-flag bundling**: closes two related
  CLI gaps at once, both grounded in `lib/_emerge/main.py`'s
  `insert_optional_args` (traced by hand, then verified against real
  `argparse` directly before relying on either finding). First, a
  correction to the earlier USE-flags-in-`-v`-output slice:
  `--verbose`/`-v` is **not** a plain boolean in real emerge -- it's
  registered with `choices=("True", "y", "n")`, and
  `insert_optional_args` inserts `"True"` only when no explicit value
  follows. A standalone (non-bundled) `-v`/`--verbose` now peeks at the
  next token, consuming it if it's exactly `"y"`/`"n"` (explicit
  enable/disable) and otherwise defaulting to enabled without consuming
  anything; `--verbose=y`/`--verbose=n` (`argparse`'s own native `=`
  syntax, a separate mechanism) work the same way. Before this fix,
  `emerge --pretend -v n cat/pkg` silently misparsed `"n"` as a second
  target atom instead of an explicit disable. Second, short-flag
  bundling itself (`-pv`, `-pd`, ...): confirmed empirically that real
  bundling is native `argparse` behavior for plain boolean short
  options, not (as the CLI-surface-recognition slice's own doc comment
  had claimed) something `insert_optional_args` provides -- that
  function is real, but it's what lets specific options like `-v` take
  an *optional* value, a separate concern from bundling itself. A
  single-dash token longer than one character now decomposes character
  by character, left to right, reporting on the first
  unimplemented-but-recognized or genuinely unrecognized character
  exactly as a standalone occurrence of it would (same messages, same
  exit code) -- a deliberate simplification of *processing order* versus
  real emerge's own value-scanning recycling algorithm, not of
  *outcome*, since this pilot exits at the first out-of-scope input
  either way. A bundled `-v` (e.g. `-pv`) never consumes a value at all,
  even if followed by a bare `"y"`/`"n"` token -- matching real emerge's
  own `short_arg_opts_n` handling, whose comment explains why: an inline
  or next-token value for a *bundled* single-letter flag would be
  ambiguous with another bundled flag character. Before this slice, a
  bundled token like `-pv` matched no table entry at all and was
  reported as a generic "unrecognized option" -- a worse outcome than
  even a "recognized, not implemented" report, since `-p` and `-v`
  genuinely are both implemented.

  **`--help`/`-h`**: real and implemented, finally closing a gap flagged
  since the original CLI-surface-recognition slice. Checked
  unconditionally, before anything else in `argv` -- matching real
  emerge's own behavior exactly: `main.py`'s `parse_opts` maps `-h`/
  `--help` to the `"help"` action, and `main()` special-cases it
  (`if myaction == "help": emerge_help(); return os.EX_OK`), checked
  once *after* the whole line has already parsed successfully, so it
  wins regardless of position or what other real-but-unimplemented flags
  accompany it -- including bundled, e.g. `-ph`. This pilot's own scan
  is a documented simplification of that: it checks every token
  (including each character of a short-flag bundle) for a literal
  `--help`/`-h`/`h` match unconditionally, rather than first confirming
  the rest of the line would even parse, so `emerge --help
  --this-is-not-a-real-flag-at-all` prints help here where real emerge
  would report a parse error instead (that flag would never reach
  argparse's post-parse action dispatch at all). The help text itself is
  deliberately **not** a port of real emerge's own `_emerge/help.py`
  (157 lines of colorized usage syntax for its full ~130-flag surface,
  most of which this pilot doesn't implement -- reproducing it here
  would be actively misleading); it's a short, honest, pilot-specific
  summary of what's actually implemented, ending with a pointer to
  `PORTING/README.md`/`PORTING/PROMPT.md` for the rest.

  **`package.use` repo+profile stacking**: the follow-up deliberately
  left out of the `package.accept_keywords` slice above, now closing the
  last of the three per-package config files that were still
  user-level-only. Grounded directly against `UseManager.__init__`,
  which confirms repo-level `package.use` lives at
  `<repo>/profiles/package.use` and profile-level at
  `<profile.location>/package.use` -- the exact same file-location
  convention `package.mask` and `package.accept_keywords` both already
  use -- and, like `package.accept_keywords`, purely additive (no
  `-atom` removal exists for this file in real portage at all), so a
  flat concatenation (repo, then every profile level in chain order,
  then user) parsed once is equivalent to parsing each source separately
  and concatenating the results. This remains a **deliberate,
  confirmed-with-the-user simplification**, not a full port of real
  portage's own mechanism: real repo-level `package.use` lands in a
  distinct `configdict["repo"]` `USE_ORDER` layer and profile-level in
  `configdict["defaults"]` (merged per-level with that level's own
  `make.defaults` USE), while this pilot's own per-package application
  (`effective_use_flags`, unchanged by this slice) already flattens
  `package.use` into one incremental list regardless of source --
  extending that flat model from one source to three doesn't add a *new*
  simplification on top of what `package.mask`/`.accept_keywords`
  already established, it just applies the pre-existing one more
  widely.

  **`@world` set support**: the first non-atom target `emerge --pretend`
  accepts. Grounded against `lib/portage/const.py` (`WORLD_FILE` is
  `var/lib/portage/world`, `ROOT`-relative -- the same relative-to-`ROOT`
  convention already used for vdb lookups) and `WorldSelectedSet` in
  `lib/portage/_sets/files.py`, which confirms real `@world` is the
  *union* of that flat file's own atoms with any nested `@set`
  references it may also contain (added by a prior `emerge --noreplace
  @some-set`). This pilot reads the file's plain atom lines and expands
  them in place at whatever position `@world` appears in argv, feeding
  the exact same multi-atom/recursion machinery every other invocation
  already uses -- not a separate code path. This is a **deliberate,
  confirmed-with-the-user simplification**: a `@`-prefixed line is
  silently skipped rather than recursively expanded, since that would
  need general set-recursion machinery this pilot doesn't have; a
  missing world file (a fresh `ROOT` that's never had anything merged
  into it) is treated as a real, valid empty state, not an error. Only
  the literal token `@world` triggers this expansion -- `@system` (a
  separate mechanism, the profile's own `packages` file -- now also
  implemented, see below) or any other `@`-prefixed top-level target
  falls through to the ordinary atom-parsing path and gets a clear
  "invalid atom" error rather than a silent no-op.

  **`--newuse`/`-N` reinstall detection**: closes the exact scope cut
  `resolve_pretend_graph`'s own doc comment named ("v1 has no
  --newuse/--changed-use equivalent"). Ports the `newuse` branch of real
  `depgraph.py`'s `_reinstall_for_flags`: an already-installed package is
  reinstalled if its currently-effective USE differs from what the vdb
  recorded at merge time -- `flags = (orig_iuse ^ cur_iuse) |
  ((orig_iuse∩orig_use) ^ (cur_iuse∩cur_use))`, read from two new vdb
  files (`USE`/`IUSE`, alongside the `CATEGORY`/`SLOT` this pilot already
  read) and the candidate ebuild's own current IUSE/`effective_use_flags`.
  A `Reinstall` entry is walked for dependencies exactly like New/Upgrade
  (the dead end an already-installed package used to always be), printed
  as `[ebuild r ]`, reusing the exact same bracket-column precedent
  `N`/`U` already established, with the changed flags named inline
  (`(reinstall for changed USE: foo)`) for a deterministic, testable
  report -- real emerge instead color-hints changed flags within its
  `-v` USE display, a UI feature out of scope for this pilot's plain-text
  output. Real `_reinstall_for_flags` also subtracts a `forced_flags`
  set (from `use.force`/`use.mask`) before deciding -- this pilot
  initially always treated it as empty (`use.force`/`use.mask` weren't
  modeled at all yet), a deliberate, confirmed-with-the-user
  simplification at the time; see the `use.mask`/`use.force` paragraph
  below for the follow-up that closed it. `--changed-use`/`-U`, a real,
  narrower alternative to `--newuse`, was recognized-but-unimplemented
  at this point too -- see its own paragraph, further below, for the
  follow-up that closed that gap as well.

  **`--nodeps`/`-O`: disable the dependency walk entirely**. Grounded in
  `create_depgraph_params.py`, which pops `"recurse"` out of `myparams`
  when `--nodeps` is given, and `depgraph.py`'s own dependency-walk code,
  which checks for `"recurse"` in `myparams` and returns early without it
  -- ported here as skipping a resolved package's own DEPEND/RDEPEND/etc
  entirely, for every entry (not just top-level atoms), so no dependency
  atom is ever queued and no blocker is ever collected (blockers only
  ever come from a dependency string in this pilot, so this falls out
  for free). A resolved package's own USE display is still computed and
  shown by `-v` regardless -- real portage's USE display is about a
  package's own metadata, unrelated to whether its dependencies get
  walked, confirmed by testing `-O -v` together against a fixture package
  whose own foo?-gated dependency `-O` suppresses.

  **`--onlydeps`/`-o`: the merge list excludes the atom itself**.
  `--nodeps`'s real complement -- man page: "Only merge (or pretend to
  merge) the dependencies of the packages specified, not the packages
  themselves" (the opposite exclusion: `--nodeps` shows the atom but
  hides its dependencies, `--onlydeps` shows the dependencies but hides
  the atom). Unlike `--nodeps`, this needed zero changes to
  `resolve_pretend_graph` at all -- dependency recursion already happens
  identically no matter which top-level entries end up printed, so this
  is purely a `pretend.rs` print-loop change: a directly-requested
  (top-level) entry's own line is suppressed, whatever its outcome
  (New/Upgrade/Reinstall/AlreadyInstalled), while every entry reached
  only as a dependency (which is never a top-level atom) prints exactly
  as before. Applying the same suppression to AlreadyInstalled's own
  "nothing to do" line means an atom that's already installed -- and so
  never had any dependencies walked in the first place, `--onlydeps` or
  not -- now correctly produces *no output at all* under `--onlydeps`,
  not a spurious status line for a package that was asked not to be
  shown.

  **`@system` set support**: closes the gap the `@world` paragraph above
  named as still open. Grounded against `PackagesSystemSet` in
  `lib/portage/_sets/profiles.py`: real `@system` reads a `packages` file
  from *every* profile level in the chain (the same directory
  `make.defaults`/`package.mask` already come from), stacked with the
  identical `stack_lists(incremental=1)` function `MaskManager` uses for
  `package.mask` (confirmed by reading both call sites) -- ported here by
  reusing `stack_mask_lines` as-is, no new stacking logic needed. Real
  portage keeps only the *post-stack* lines starting with `*` (the `*`
  stripped) as the actual `@system` atom list; every other line is a
  "known to the profile but not part of the base system" hint with no
  `@system`-set meaning of its own -- read and stacked (so a later
  `-*atom` can still remove an earlier `*atom`) but never itself
  contributing an atom. `portage_profile::Config::system_packages` is
  computed once in `resolve_config` alongside `package_mask`/etc, and
  `pretend.rs`/`emerge_pretend_reference.py`'s own `run()` needed to be
  reordered to resolve `config` *before* expanding `@world`/`@system`
  (previously config was resolved only after atom validation) -- `@system`'s
  atom list lives inside it, unlike `@world`'s, which only ever needed
  `ROOT`. No repo-level or user-level source exists for this file in real
  portage at all (confirmed by reading `PackagesSystemSet.__init__`,
  which only ever consults the profile chain), unlike `package.mask`'s
  repo-level `profiles/package.mask`.

  **`use.mask`/`use.force`: profile-level global USE forcing**. Closes
  the `forced_flags`-is-always-empty simplification the `--newuse`
  paragraph above named. Grounded against `UseManager.
  getUseMask`/`getUseForce`'s own `pkg=None` case -- the one real
  `config.py`'s `regenerate()` actually calls to build the *global* `USE`
  value this pilot's flat model corresponds to -- which returns
  `stack_lists(self._usemask_list/self._useforce_list, incremental=True)`
  directly: every profile level's own file, stacked with the identical
  machinery `package.mask`/`packages` (`@system`, above) already port, no
  repo-level or per-package source at all for this global case. Applied
  last, after every other real accumulation source: every `use.force`
  flag is force-added, then every `use.mask` flag is force-removed,
  exactly matching real `regenerate()`'s own `update`-then-
  `difference_update` order -- a flag listed in both ends up masked, not
  forced. `use_force`/`use_mask` are also exposed on `Config` directly,
  which let this slice close a second, previously-documented gap for
  free: `--newuse`'s own `forced_flags` (real `_reinstall_for_flags`'s
  `set(chain(pkg.use.force, pkg.use.mask))`, subtracted from the
  IUSE-presence half of its comparison only -- real portage's own `flags
  -= forced_flags` line sits between the `^=` and the final `|=`) was
  always the empty set before this slice; a dedicated fixture
  (`dev-libs/usemaskreinstallpkg`, installed with an empty vdb IUSE, its
  current ebuild now declaring a `use.mask`-masked flag) proves it now
  correctly suppresses a reinstall that would otherwise spuriously
  trigger just because a permanently-masked flag was newly added to
  IUSE -- verified this wasn't already true by temporarily un-masking
  the flag and confirming the spurious reinstall *does* fire without the
  fix.

  **`--changed-use`/`-U`: the narrower reinstall check**. Closes the
  last item the `--newuse` paragraph above named. Ports the `elif
  changed_use` branch of real `depgraph.py`'s `_reinstall_for_flags`,
  which turns out to be *exactly* the term `--newuse`'s own formula
  already computed and shares: `(orig_iuse∩orig_use) ^
  (cur_iuse∩cur_use)` -- which flags were enabled, among those declared
  in IUSE on *both* sides. `--newuse` adds a second term on top (whether
  IUSE gained or lost a flag at all, forced flags aside); `--changed-use`
  never adds it, so it reacts only to an actual enablement change of an
  already-shared flag, never to IUSE simply gaining or losing one.
  `reinstall_flags_for_newuse` (Rust)/`_reinstall_flags_for_newuse`
  (Python) were renamed to `..._for_use_change` and now take a `newuse`
  bool selecting which formula to use, since both flags share almost
  all of their own logic; `resolve_pretend`/`resolve_pretend_graph` gain
  a second, independent `changed_use` parameter alongside `newuse` --
  giving both at once resolves the same way real emerge's own `if
  newuse or (...): ... elif changed_use or (...): ...` does, `newuse`
  winning. A dedicated fixture (`dev-libs/changedusepkg`, installed with
  an empty vdb IUSE, its current ebuild now declaring a real, unmasked,
  not-globally-enabled `brandnewflag`) proves the two flags are
  genuinely different checks, not two names for the same one:
  `--newuse` reports a Reinstall for it (a flag simply exists in IUSE
  now that didn't before), `--changed-use` does not (its own enablement
  never actually changed) -- while `dev-libs/reinstallpkg`'s own `foo`
  flag (shared, enablement-only change) still triggers *both*.

  **`package.use.mask`/`package.use.force`: per-package USE forcing**.
  Grounded against `UseManager.__init__`'s own file/variable comment
  table: unlike `package.use`, there is no user-level source for either
  file at all (the "user config" section lists only `package.use ->
  _pusedict`) -- confirmed real behavior, not a pilot simplification.
  Read from repo-level (main repo only, no `masters` -- the same cut
  `package.mask`'s own repo-level source already makes) plus every
  profile level's own file, in chain order. Unlike real per-instance
  `getUseMask(pkg)`/`getUseForce(pkg)`, which interleave global and
  per-package entries level-by-level in one `stack_lists` pass, this
  pilot applies `package.use.mask`/`.force` as a separate layer on top
  of the already-shipped global `use.mask`/`use.force` and `package.use`
  -- a deliberate, confirmed simplification of the real application
  order, kept separate so the already-tested global implementation
  didn't need reworking. When more than one entry matches the same
  candidate, real `ordered_by_atom_specificity`/`best_match_to_list`
  decides which one wins a conflict; this pilot ports a simplified
  version of that ranking table (`=cpv` highest, then `~cpv`, then
  `=cpv*`, then `cp:slot`, then any comparison-operator atom, then a
  bare `cp`, then this pilot's own bounded wildcard atoms lowest) and
  applies each specificity-ordered entry's tokens via the same
  incremental semantics `package.use` itself uses, so a more-specific
  entry's own `-flag` can cancel a less-specific entry's own mask/force.
  Two deliberate scope cuts beyond that: no stable-vs-`~arch` KEYWORDS
  distinction at all (real portage's own separate `use.stable.mask`/
  `.force`/`package.use.stable.mask`/`.force` files and `_isStable`
  check are out of scope entirely -- this pilot never determines
  stability anywhere), and comparison-operator atoms (`>`/`<`/`>=`/
  `<=`) share one specificity tier without real `best_match_to_list`'s
  "closest version wins a tie" refinement, since real-world
  `package.use.mask`/`.force` files essentially never use these
  operators. `dev-libs/pkgusemaskforcepkg` (`IUSE="forceflag maskflag
  specflag"`) exercises both the forcing and the specificity ordering
  in one fixture: a repo-level `package.use.force` wildcard entry force-
  enables `forceflag`; the base profile's own `package.use.mask` masks
  both `maskflag` and `specflag` via a bare atom; the leaf profile's own
  `package.use.mask` has a *more specific* exact-version atom that
  un-masks `specflag` again -- proving atom-specificity, not just
  profile-chain order, decides the winner, and that a more-specific
  entry from a *later* profile level can still override a less-specific
  one from an earlier level. Final USE: `forceflag -maskflag -specflag`.
- **`PORTING/tests`**: an example of the jointly-owned contract suite
  described in `PROMPT.md` under "Ownership" -- it imports nothing from
  either implementation, driving both purely as subprocesses, so it stays
  valid regardless of how either side's internals evolve.
- **`PORTING/bench`**: the performance-regression gate from `PROMPT.md`
  hard goal 2 ("Rust must be measurably faster... tracked over time in CI
  as a regression gate"). `run_benchmark.py` feeds an identical batch of
  operations to both harnesses' `batch` subcommand (many ops per process,
  so process-spawn overhead doesn't drown out the comparison), takes the
  best of several timed repetitions per side, and refuses to report numbers
  at all if the two implementations' outputs disagree. It exits nonzero if
  Rust isn't at least `--min-speedup` times faster than Python, and
  (`--check-baseline`) if speedup regresses more than 10% below the
  recorded `baseline.json`. Workload defaults to `gentoo_snapshot.json` --
  real package/version pairs from a real Gentoo tree, mostly comparing two
  versions of the *same* package (the realistic vercmp usage pattern) --
  per PROMPT.md's "real, vendored Gentoo tree snapshot" decision; pass
  `--dataset synthetic` to use seeded-random version strings instead. As of
  the last `--update-baseline` run, Rust is **~6x faster** than Python on
  the real snapshot.
- **`PORTING/musl`**: the minimal-Linux gate from `PROMPT.md` hard goal 3
  and "Test/benchmark harness architecture" ("Rust CI also gates on a musl
  static build smoke-tested inside a minimal (scratch/busybox-level)
  container"). `Containerfile`'s build context is `PORTING/` (not just
  `PORTING/rust`) so `PORTING/fixtures` can be copied into the image too.
  It cross-builds the binaries against musl (Alpine's own `rust`/`cargo`
  packages target musl natively, so no rustup/target-add is needed) with
  `relocation-model=static` forced via `rust/.cargo/config.toml` -- the
  resulting binaries have no dynamic section at all, not even a reference
  to musl's own dynamic loader (verified with `ldd`/`readelf`). The
  runtime stage is `FROM scratch`: no libc, no shell, no busybox, nothing
  but the binaries and the fixture tree. `smoke_test.sh` builds that image
  and exercises `versions-harness`, `atom-harness`, `use-reduce-harness`,
  a real `emerge --pretend` resolution (a single package, a multi-package
  dependency graph, a real-profile-derived USE flag gating a dependency,
  a `package.mask`-hidden package staying hidden, a masked-then-
  `package.unmask`-ed package becoming visible again, a `package.use`
  entry both enabling and disabling a per-package flag, a strong and a
  weak blocker match each being reported, an overlay-only package being
  found, a same-version tie across the main repo and the overlay being
  broken toward the higher-priority one, a genuine slot conflict being
  reported, two different slots of the same package correctly
  coexisting instead of one silently overwriting the other, a
  `virtual/*` atom resolving as a dependency with no dedicated code
  involved, and a real-but-unimplemented option like `--deep` being
  named specifically in the CLI's refusal message), `ebuild`-dispatch
  (still succeeding as a no-op for real, valid syntax like `merge`, but
  now rejecting a genuinely unrecognized command by name), and batch
  mode inside it, exiting nonzero on any failure -- including proving
  the fixture's `make.profile` symlink, multi-parent chain, and second
  `repos.conf` repo all survive the image `COPY` and still resolve
  correctly.

Known simplification: `versions-harness`/`portage-versions` compare
version components as `i128` rather than Python's arbitrary-precision
integers. See the comment at the top of
`rust/portage-versions/src/lib.rs`.

Known scope cut: `atom-harness` ports a documented v1 subset of the real
atom grammar (see above and the doc comment in
`rust/portage-dep/src/lib.rs`) -- wildcards, build-ids, and EAPI
parametrization are all deferred (slot operators `:=`/`:*`/`:slot=`,
USE deps `[bar]`, the `=*` glob version operator, and the `::reponame`
repo constraint are now supported -- see the "Slot operators", "USE
deps", "`=*` glob version operator", and "`::reponame` repo constraint"
paragraphs in "What this proves" above; USE deps are parsed
but not enforced by matching, a separately-noted, deliberate cut of their
own). Candidates for matching are plain
`category/package-version[-rN][:slot[/subslot]][::repo]` strings rather
than full Package objects (no package-db/depgraph model exists yet in
this pilot), which mirrors a fallback path the real `match_from_list`
already supports.

`PORTING/fixtures` is a small synthetic repo (not the vendored real tree
used for benchmarking): `repos.conf`, a handful of ebuilds + matching
md5-cache entries, a fake vdb, and now a full profile chain + make.conf,
covering new-install, upgrade, already-installed, not-visible
(`~amd64`-only), nonexistent-package, the sibling-package-prefix-ambiguity
regression case, (for recursion) a basic dependency chain, a diamond
dependency, a dependency cycle, an any-of (`||`) group, an unresolvable
dependency, a dependency listed in both DEPEND and RDEPEND, and three
single-key packages (`bdependpkg`/`pdependpkg`/`idependpkg`) each pulling
in `newpkg` via just BDEPEND, PDEPEND, or IDEPEND alone -- proving each
of the three is actually walked, not just DEPEND/RDEPEND -- a package
(`slotoperatorpkg`) whose own RDEPEND uses real slot-operator syntax
(`dev-libs/newpkg:=`, no explicit slot; `dev-libs/multislotpkg:1=`, an
explicit slot, resolving specifically the SLOT=1 version) proving those
dependency tokens are now resolved rather than silently dropped, a
package (`usedeppkg`) exercising the same fix for USE-dep syntax
(`dev-libs/newpkg[bar]`; `dev-libs/multislotpkg:1[baz?]`, combined with a
plain slot restriction), and (for profile resolution) a three-level
same-repo profile chain
(`profiles/base`, `profiles/arch/amd64`, `profiles/default` -- the latter
inheriting from both of the former, in that order) plus `make.conf`
sourcing `/etc/make.local`, and a package whose dependency is gated on a
USE flag that only a correctly-resolved profile/make.conf stack would
actually enable. `repos.conf`'s repos both use relative `location`s
(resolved against `PORTAGE_CONFIGROOT`) purely so the fixture is
portable across checkouts -- real `repos.conf` files always use absolute
paths; see the comment in `portage-repo/src/lib.rs`.

`PORTING/fixtures/etc/portage/package.mask`, `package.unmask`, and
`package.accept_keywords` exercise the mask/unmask/accept_keywords slice:
`hardmaskedpkg` (masked, never unmasked, so it stays hidden),
`maskedandunmaskedpkg` (masked, then unmasked, so it's visible again),
`samepkg` (masked and then immediately un-masked via a `-dev-libs/samepkg`
line within `package.mask` itself, proving `-atom` removal works, not just
`package.unmask`), `wildcardkeywordpkg` (only `~amd64`, made visible by a
`*/wildcardkeywordpkg ~amd64` wildcard entry), and `livekeywordpkg` (no
`KEYWORDS` at all, like a live/9999 ebuild, made visible by a `**` entry).

`PORTING/fixtures/repo/profiles/package.mask` (repo-level),
`PORTING/fixtures/repo/profiles/base/package.mask` (one profile level's
own mask), and `PORTING/fixtures/repo/profiles/default/package.unmask`
(the leaf profile's own unmask) exercise full 3-source stacking:
`repomaskedpkg` (masked only by the repo-level file, stays hidden),
`profilemaskedpkg` (masked only by the `base` profile level's own file,
stays hidden -- proving a *non-leaf* profile level's own mask is read),
`repomaskedthenprofileunmaskedpkg` (masked by the repo-level file,
unmasked again by the leaf profile's own `package.unmask` -- a
profile-level unmask cancelling a repo-level mask, proving the three
sources are genuinely stacked together), and
`repomaskedthenuserremovedpkg` (masked by the repo-level file, then
un-masked by a `-dev-libs/repomaskedthenuserremovedpkg` line added to
the existing user-level `package.mask` fixture -- `-atom` removal
reaching all the way from the user's own file back to an entry a
different, earlier source added, the specific thing that wasn't
possible before this slice).

`PORTING/fixtures/repo/profiles/arch/amd64/package.accept_keywords` (a
third, previously-untouched profile level, complementing `base`'s own
`package.mask` and `default`'s own `package.unmask`) exercises
`package.accept_keywords` profile-chain stacking:
`profileacceptkeywordspkg` (only `~amd64`, made visible not by the
user-level `package.accept_keywords` fixture -- which has no entry for
it at all -- but by this profile-level one).

`PORTING/fixtures/etc/portage/package.use` exercises the per-package USE
slice: `packageuseenablepkg` (its `pkguseflag?`-gated dependency is only
pulled in because a `*/packageuseenablepkg pkguseflag` wildcard entry
enables a flag that's off everywhere else) and `packageusedisablepkg`
(its `foo?`-gated dependency is *not* pulled in, even though `foo` is
enabled globally by the fixture profile chain -- same as
`useflagpkg`'s own `foo?`-gated dependency, which *is* pulled in -- because
a `dev-libs/packageusedisablepkg -foo` entry disables it for this one
package only).

`PORTING/fixtures/repo/profiles/package.use` (repo-level) and
`PORTING/fixtures/repo/profiles/default/package.use` (the leaf profile's
own) exercise `package.use` repo+profile stacking: `repouseenablepkg`
and `profileuseenablepkg` (each with its own `IUSE`-declared flag,
off everywhere else, pulling in `newpkg` via its own `?`-gated RDEPEND
only because the repo-level or profile-level source, respectively,
enables it).

`PORTING/fixtures/var/lib/portage/world` (real portage's own `WORLD_FILE`
location, `ROOT`-relative) exercises `@world` expansion: it lists
`dev-libs/newpkg` directly, `dev-libs/withdeps` (which recurses into
`newpkg` again -- deduped -- and `dev-libs/upgradepkg` via its own
RDEPEND, proving `@world`'s expanded atoms feed the same recursion
machinery any other target does), and a `@some-nested-set-reference`
line proving a nested-set reference is silently skipped rather than
mishandled.

`PORTING/fixtures/repo/profiles/base/packages` (`*dev-libs/newpkg`, plus
a non-`*`-prefixed `dev-libs/hintonly` hint line that must never
contribute an atom) and `PORTING/fixtures/repo/profiles/default/packages`
(the leaf profile's own, `*dev-libs/withdeps`) exercise `@system`
expansion: proving it stacks across multiple profile levels, not just
the leaf, and that its expanded atoms feed the same recursion machinery
`@world` does too -- `withdeps` recurses into `newpkg` again (deduped)
and `upgradepkg`.

`dev-libs/reinstallpkg` exercises `--newuse` reinstall detection:
installed at `1.0` with `IUSE="foo"` declared but an empty vdb `USE` file
(`PORTING/fixtures/var/db/pkg/dev-libs/reinstallpkg-1.0/USE` -- `foo` was
off at merge time), while the fixture profile chain enables `foo`
globally now, so `--newuse` must report a Reinstall for the changed
`foo` flag; its `RDEPEND="dev-libs/newpkg"` proves a Reinstall entry is
still walked for dependencies, not treated as a dead end the way
AlreadyInstalled is. `dev-libs/samepkg` (already used elsewhere, no
`IUSE` at all) doubles as the negative case: `--newuse` must leave it
AlreadyInstalled, proving it doesn't force a reinstall of every
already-installed package, just ones with an actual USE mismatch.

`PORTING/fixtures/repo/profiles/base/use.mask` (`masked_newly_added_flag`,
a name no other fixture package's `IUSE` declares, so it has zero effect
elsewhere) exercises `--newuse`'s `forced_flags` subtraction:
`dev-libs/usemaskreinstallpkg` is installed with an empty vdb `IUSE`,
but its current ebuild now declares `IUSE="masked_newly_added_flag"` --
a flag that's masked off, so never enabled either before or after.
Without `forced_flags` support this would spuriously report a Reinstall
just because the flag now exists in `IUSE` at all; with it, it correctly
stays `AlreadyInstalled`.

`dev-libs/changedusepkg` exercises the `--newuse`/`--changed-use`
divergence: installed with an empty vdb `IUSE`, its current ebuild now
declares `IUSE="brandnewflag"` -- real, unmasked, not globally enabled.
`--newuse` reports a Reinstall for it (IUSE simply gained a flag);
`--changed-use` doesn't (that flag's own enablement never changed) --
while `dev-libs/reinstallpkg`'s own `foo` (a flag that exists in `IUSE`
on both sides, only its enablement differs) still triggers both.

`dev-libs/pkgusemaskforcepkg` (`IUSE="forceflag maskflag specflag"`)
exercises `package.use.mask`/`package.use.force` plus atom-specificity
ordering, across three config files: `PORTING/fixtures/repo/profiles/
package.use.force` (repo-level, a bare wildcard entry force-enabling
`forceflag`), `PORTING/fixtures/repo/profiles/base/package.use.mask`
(profile-level, a bare atom masking both `maskflag` and `specflag`),
and `PORTING/fixtures/repo/profiles/default/package.use.mask` (the leaf
profile's own, a more specific exact-version atom that un-masks
`specflag` again) -- see the `package.use.mask`/`package.use.force`
paragraph above for why the leaf profile's more-specific entry wins
over the base profile's less-specific one despite coming from a later
chain level.

Four more fixture packages exercise blocker reporting: `blockerpkg`
(RDEPEND `"!!dev-libs/samepkg"`, a strong blocker matching
`dev-libs/samepkg-1.0`, which the fixture vdb already has installed), and
`graphblockerparent`/`blockerpartnerpkg`/`weakblockerpkg` together (the
parent RDEPENDs on both of the other two so they resolve New in the same
graph; `weakblockerpkg`'s own RDEPEND is `"!dev-libs/blockerpartnerpkg"`,
a weak blocker that can only be matched against the graph's own
New/Upgrade set, since `blockerpartnerpkg` isn't installed anywhere).

`PORTING/fixtures/overlay` is a second repo, registered in
`repos.conf` alongside the main one with an explicit `priority = 10`
(the main repo's own priority is left unset, so it defaults to real
portage's own `-1000`), exercising the overlays slice: `overlayonlypkg`
(exists only in the overlay, proving it's actually searched, not just
listed in `repos.conf`), `overlaynewerpkg` (`1.0` in the main repo,
`2.0` in the overlay -- the higher version wins regardless of which repo
has it, since priority only ever breaks a tie on an *identical*
version), and `overlaytiepkg` (identically-versioned `1.0` in both
repos, but only the overlay's copy `RDEPEND`s on `dev-libs/newpkg` --
resolving it pulls `newpkg` in, proving the higher-priority overlay's
copy, not the main repo's, is the one whose own metadata actually got
read).

Six more fixture packages exercise slot conflicts: `slotconflicttarget`
(two versions, `1.0` and `2.0`, both `SLOT="0"`), `slotconflictnewconsumer`
(a bare RDEPEND on `slotconflicttarget`, reached first, resolves the best
version -- `2.0`), `slotconflictoldconsumer` (RDEPEND
`"<dev-libs/slotconflicttarget-2.0"`, which the already-resolved `2.0`
does *not* satisfy), and `slotconflictparent` (RDEPENDs on both
consumers, so the conflict between them surfaces); and, for the
non-conflict case, `multislotpkg` (two versions in *different* slots --
`1.0`/`SLOT="0"` and `2.0`/`SLOT="1"`) and `multislotparent` (RDEPENDs on
`multislotpkg:0` and `multislotpkg:1` explicitly, so both must resolve
as independent entries, not a conflict).

`virtual/texteditor` and `dev-libs/virtualconsumerpkg` exercise
virtuals: the former is deliberately shaped like the real Gentoo tree's
own `virtual/pager` (an ordinary ebuild, `RDEPEND="|| ( dev-libs/newpkg
dev-libs/samepkg )"`), the latter simply `RDEPEND`s on it, proving a
virtual reached as a dependency resolves the same way as one requested
directly.

`gentoo_snapshot.json` was extracted from a full local Gentoo tree
checkout (`/.gentoo/repos/gentoo` on the machine this was vendored on) with
`extract_snapshot.py`, using the real `portage.versions.pkgsplit` as the
authority for parsing "pn-pv" ebuild filenames into package/version pairs
-- not a hand-rolled parser. To refresh it against a newer tree:

```sh
python3 PORTING/bench/extract_snapshot.py /path/to/a/gentoo/tree
```

## Running it

Build both Rust binaries:

```sh
cd PORTING/rust && cargo build --release
```

Try the harnesses directly:

```sh
# Python
python3 PORTING/python/versions_harness.py vercmp 1.0-r1 1.0

# Rust
PORTING/rust/target/release/versions-harness vercmp 1.0-r1 1.0

# batch mode (benchmark-oriented: many ops, one process)
printf 'vercmp 1.0 1.0\nververify 1.0_pre2\n' | PORTING/rust/target/release/versions-harness batch
```

Try the atom-matching harness:

```sh
# Python
python3 PORTING/python/atom_harness.py parse ">=dev-libs/foo-1.2.3-r1:2"

# Rust
PORTING/rust/target/release/atom-harness parse ">=dev-libs/foo-1.2.3-r1:2"

# match_from_list-equivalent: prints the matching candidates, comma-joined
PORTING/rust/target/release/atom-harness match ">=dev-libs/foo-1.2.3" \
    dev-libs/foo-1.0 dev-libs/foo-2.0

# slot operators: ":=" (no explicit slot) matches regardless of slot,
# ":slot=" filters to that slot exactly like a plain ":slot" atom would
PORTING/rust/target/release/atom-harness match "dev-libs/foo:=" \
    dev-libs/foo-1.0:0 dev-libs/foo-2.0:1
# dev-libs/foo-1.0:0,dev-libs/foo-2.0:1
PORTING/rust/target/release/atom-harness match "dev-libs/foo:1=" \
    dev-libs/foo-1.0:0 dev-libs/foo-2.0:1
# dev-libs/foo-2.0:1

# USE deps: parsed, but never enforced by matching -- "[bar]" and
# "[-bar]" return the identical match set, same as real match_from_list
# already does for these same plain-string candidates
PORTING/rust/target/release/atom-harness match "dev-libs/foo[bar]" \
    dev-libs/foo-1.0 dev-libs/foo-2.0
# dev-libs/foo-1.0,dev-libs/foo-2.0
PORTING/rust/target/release/atom-harness match "dev-libs/foo[-bar]" \
    dev-libs/foo-1.0 dev-libs/foo-2.0
# dev-libs/foo-1.0,dev-libs/foo-2.0

# "=*" glob version operator: component-boundary aware, not a naive
# string prefix -- "1*" matches "1.2" (real boundary: ".") but not "10"
# (both digits, no real boundary -- bug 560466)
PORTING/rust/target/release/atom-harness match "=dev-libs/foo-1*" \
    dev-libs/foo-1.2 dev-libs/foo-10
# dev-libs/foo-1.2

# "::reponame" repo constraint: rejects a candidate only if it carries a
# KNOWN, different repo -- the repo-less candidate always passes too
PORTING/rust/target/release/atom-harness match "dev-libs/foo::gentoo" \
    dev-libs/foo-1.0 dev-libs/foo-1.0::gentoo dev-libs/foo-1.0::other
# dev-libs/foo-1.0,dev-libs/foo-1.0::gentoo
```

Try the use_reduce harness:

```sh
# Python
python3 PORTING/python/use_reduce_harness.py reduce normal bar \
    dev-libs/foo bar? "(" dev-libs/baz ")" "!bar?" "(" dev-libs/qux ")"

# Rust
PORTING/rust/target/release/use-reduce-harness reduce normal bar \
    dev-libs/foo bar? "(" dev-libs/baz ")" "!bar?" "(" dev-libs/qux ")"
```

Try `emerge --pretend` against the fixture tree:

```sh
ln -sf "$(realpath PORTING/rust/target/release/multicall)" /tmp/emerge
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/newpkg     # -> [ebuild  N] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/upgradepkg # -> [ebuild  U] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/samepkg    # -> already installed

# dependency recursion: diamond dependency, deduped (see PORTING/fixtures)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/diamond
# [ebuild  N] dev-libs/diamond-1.0
# [ebuild  N] dev-libs/shared-a-1.0
# [ebuild  N] dev-libs/shared-b-1.0
# [ebuild  N] dev-libs/common-1.0

# BDEPEND/PDEPEND/IDEPEND are walked too, not just DEPEND/RDEPEND -- v1
# makes no distinction between any of the five real dependency-string
# keys (no real merge ordering exists yet for the distinction to matter)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/bdependpkg
# [ebuild  N] dev-libs/bdependpkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# real slot-operator dependency atoms (":=" and ":1=") are resolved, not
# silently dropped -- ":1=" specifically resolves multislotpkg's SLOT=1
# version (2.0), not its SLOT=0 version (1.0)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/slotoperatorpkg
# [ebuild  N] dev-libs/slotoperatorpkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/multislotpkg-2.0

# real USE-dep dependency atoms are resolved too, not silently dropped --
# the "[bar]"/"[baz?]" constraints themselves aren't enforced (v1's
# deliberate parse-only scope), so this resolves identically to the same
# dependencies without any USE-dep suffix at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeppkg
# [ebuild  N] dev-libs/usedeppkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/multislotpkg-2.0

# real profile/make.conf resolution: "foo" is enabled by the fixture's
# profile chain, so this package's foo?-gated dependency is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# package.mask: hidden, no matching package.unmask entry
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/hardmaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/hardmaskedpkg".  (exit 1)

# package.mask + package.unmask: masked, then unmasked again -> visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/maskedandunmaskedpkg
# [ebuild  N] dev-libs/maskedandunmaskedpkg-1.0

# repo-level profiles/package.mask (real portage's most common real-world
# masking source, e.g. security/arch masks) hides a package the same way
# a user-level package.mask entry does
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/repomaskedpkg".  (exit 1)

# a repo-level mask, cancelled by a profile-level package.unmask entry --
# proving the three sources (repo, profile chain, user) are genuinely
# stacked together, not checked independently
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedthenprofileunmaskedpkg
# [ebuild  N] dev-libs/repomaskedthenprofileunmaskedpkg-1.0

# a repo-level mask, cancelled by a "-atom" line in the user-level
# package.mask -- -atom removal now spans all three sources, not just
# within the one file that contains the "-atom" line
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedthenuserremovedpkg
# [ebuild  N] dev-libs/repomaskedthenuserremovedpkg-1.0

# package.accept_keywords wildcard ("*/wildcardkeywordpkg ~amd64") makes an
# otherwise ~amd64-only, not-globally-accepted package visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/wildcardkeywordpkg
# [ebuild  N] dev-libs/wildcardkeywordpkg-1.0

# package.accept_keywords "**" accepts a package with no KEYWORDS at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/livekeywordpkg
# [ebuild  N] dev-libs/livekeywordpkg-9999

# package.accept_keywords is now also stacked from the profile chain, not
# just /etc/portage -- this package has no user-level entry at all, only
# a profile-level one (see PORTING/fixtures/repo/profiles/arch/amd64)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/profileacceptkeywordspkg
# [ebuild  N] dev-libs/profileacceptkeywordspkg-1.0

# package.use ("*/packageuseenablepkg pkguseflag") enables a flag that's
# off everywhere else, pulling in its pkguseflag?-gated dependency
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packageuseenablepkg
# [ebuild  N] dev-libs/packageuseenablepkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# package.use ("dev-libs/packageusedisablepkg -foo") disables a flag for
# just this package, even though "foo" is on globally (contrast with
# dev-libs/useflagpkg above, whose own foo?-gated dependency IS pulled in)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packageusedisablepkg
# [ebuild  N] dev-libs/packageusedisablepkg-1.0

# package.use is now stacked from repo+profile too, not just
# /etc/portage -- neither of these packages has any user-level entry
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repouseenablepkg
# [ebuild  N] dev-libs/repouseenablepkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/profileuseenablepkg
# [ebuild  N] dev-libs/profileuseenablepkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# a strong (!!) blocker matching an already-installed package is reported
# (not enforced -- exit code is still 0, same as real --pretend)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/blockerpkg
# [ebuild  N] dev-libs/blockerpkg-1.0
# [blocks] dev-libs/blockerpkg-1.0 hard blocks dev-libs/samepkg-1.0 ("!!dev-libs/samepkg")

# a weak (!) blocker matching another package this same run would also
# newly merge (not just an installed one)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/graphblockerparent
# [ebuild  N] dev-libs/graphblockerparent-1.0
# [ebuild  N] dev-libs/blockerpartnerpkg-1.0
# [ebuild  N] dev-libs/weakblockerpkg-1.0
# [blocks] dev-libs/weakblockerpkg-1.0 soft blocks dev-libs/blockerpartnerpkg-1.0 ("!dev-libs/blockerpartnerpkg")

# overlays: a package that exists only in the overlay repo (see
# PORTING/fixtures/etc/portage/repos.conf) is found
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayonlypkg
# [ebuild  N] dev-libs/overlayonlypkg-1.0

# "::reponame" repo constraint: the same package, constrained to the
# repo it's NOT in, correctly finds nothing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayonlypkg::testrepo
# emerge: there are no ebuilds to satisfy "dev-libs/overlayonlypkg::testrepo".  (exit 1)

# same version in both repos: the higher-priority overlay's own copy is
# the one actually used, proven by its RDEPEND (not the main repo copy's)
# pulling in dev-libs/newpkg
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaytiepkg
# [ebuild  N] dev-libs/overlaytiepkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# slot conflict: slotconflictnewconsumer resolves slotconflicttarget to
# 2.0 first; slotconflictoldconsumer's own "<...-2.0" constraint rejects
# that -- reported, not enforced (exit code and the rest of the graph are
# unaffected)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/slotconflictparent
# [ebuild  N] dev-libs/slotconflictparent-1.0
# [ebuild  N] dev-libs/slotconflictnewconsumer-1.0
# [ebuild  N] dev-libs/slotconflictoldconsumer-1.0
# [ebuild  N] dev-libs/slotconflicttarget-2.0
# [slot conflict] dev-libs/slotconflicttarget:0 resolved to dev-libs/slotconflicttarget-2.0, which does not satisfy "<dev-libs/slotconflicttarget-2.0"

# NOT a conflict: two different slots of the same package coexist as
# independent entries, same as real portage allows
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/multislotparent
# [ebuild  N] dev-libs/multislotparent-1.0
# [ebuild  N] dev-libs/multislotpkg-1.0
# [ebuild  N] dev-libs/multislotpkg-2.0

# virtuals: virtual/texteditor is shaped like the real virtual/pager (an
# ordinary ebuild, any-of RDEPEND) -- no dedicated resolution code exists
# for it, or is needed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/virtualconsumerpkg
# [ebuild  N] dev-libs/virtualconsumerpkg-1.0
# [ebuild  N] virtual/texteditor-0
# [ebuild  N] dev-libs/newpkg-1.0

# multiple top-level atoms: a dependency shared between two REQUESTED
# packages (not just two deps of one package) dedupes the same way a
# diamond dependency always did
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/shared-a dev-libs/shared-b
# [ebuild  N] dev-libs/shared-a-1.0
# [ebuild  N] dev-libs/shared-b-1.0
# [ebuild  N] dev-libs/common-1.0

# a bad top-level atom aborts the whole run immediately, in argv order --
# real portage's own "there are no ebuilds to satisfy" wording (from
# lib/_emerge/depgraph.py), not enforced/reported-and-continued the way a
# dependency's own missing candidate is
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/does-not-exist dev-libs/newpkg
# emerge: there are no ebuilds to satisfy "dev-libs/does-not-exist".  (exit 1)

# a top-level atom can now carry an operator/slot, same as a dependency
# atom always could -- resolve_pretend's own matching needed no changes
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend '>=dev-libs/newpkg-1.0'
# [ebuild  N] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/newpkg:0
# [ebuild  N] dev-libs/newpkg-1.0

# a blocker is still rejected as a target -- fixed to be an explicit,
# reported rejection instead of the pre-existing silent no-op (accepted
# by the CLI, then dropped by resolve_pretend_graph's own blocker skip)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend '!!dev-libs/newpkg'
# emerge (pilot v1): "!!dev-libs/newpkg" is a blocker, not a valid emerge target  (exit 2)

# @world expands in place to the fixture world file's own atoms: newpkg
# directly, withdeps (which recurses into newpkg again -- deduped -- and
# upgradepkg), and a "@some-nested-set-reference" line that's silently
# skipped, not mishandled
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend @world
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/withdeps-1.0
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)

# @world combines with an explicit atom in the same invocation
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/samepkg @world
# dev-libs/samepkg-1.0 is already installed; nothing to do
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/withdeps-1.0
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)

# a missing world file (a fresh ROOT that's never had anything merged
# into it) is a real, valid empty state, not an error -- it hits the same
# "nothing to resolve" error any other empty target list would
mkdir -p /tmp/empty-world-root
PORTAGE_CONFIGROOT="$FX" ROOT="/tmp/empty-world-root" /tmp/emerge --pretend @world
# emerge (pilot v1): no package atoms to resolve (the target list, after
# expanding any @world/@system, is empty)  (exit 2)

# @system is real and implemented too: base/packages contributes newpkg,
# the leaf profile's own default/packages contributes withdeps -- proving
# @system stacks across multiple profile levels and feeds the same
# recursion machinery @world does (withdeps recurses into newpkg again,
# deduped, and upgradepkg)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend @system
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/withdeps-1.0
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)

# only the literal tokens "@world"/"@system" trigger expansion -- any
# other "@"-prefixed target falls through to the ordinary atom-parsing
# path and gets a clear error, not a silent no-op
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend @some-other-set
# emerge: invalid atom "@some-other-set"  (exit 1)

# --verbose/-v is real and implemented: USE flags are off by default,
# same as real emerge, and only shown with -v -- alphabetically sorted,
# limited to this package's own IUSE (foo enabled, missingflag disabled,
# per the fixture profile chain)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"
# [ebuild  N] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0   (no -v: no USE= at all)
# [ebuild  N] dev-libs/newpkg-1.0

# -v/--verbose isn't a plain boolean in real emerge -- an explicit
# following "n" disables it again, same as real insert_optional_args
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v n dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0   (explicit "n": no USE= shown)
# [ebuild  N] dev-libs/newpkg-1.0

# --newuse/-N is real and implemented: reinstallpkg is installed with
# IUSE="foo" declared but an empty vdb USE file (foo was off at merge
# time); the fixture profile chain enables "foo" globally now, so
# --newuse reports a Reinstall for the changed flag -- and still recurses
# into its own RDEPEND, exactly like a New/Upgrade entry would
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/reinstallpkg
# [ebuild  r] dev-libs/reinstallpkg-1.0 (reinstall for changed USE: foo)
# [ebuild  N] dev-libs/newpkg-1.0

# without --newuse, the exact same package stays AlreadyInstalled -- the
# USE mismatch is real, but nothing checks for it unless --newuse is given
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/reinstallpkg
# dev-libs/reinstallpkg-1.0 is already installed; nothing to do

# --newuse is a no-op when USE hasn't changed -- samepkg has no IUSE at
# all (declared or in the vdb), so there's nothing to detect a change in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do

# --newuse's forced_flags subtraction: usemaskreinstallpkg's newly
# IUSE-declared flag is masked off by use.mask, so it never actually
# changed enablement -- without forced_flags this would spuriously
# report a Reinstall just because the flag now exists in IUSE at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/usemaskreinstallpkg
# dev-libs/usemaskreinstallpkg-1.0 is already installed; nothing to do

# --changed-use/-U is real and implemented too, a narrower sibling of
# --newuse: changedusepkg's newly IUSE-declared "brandnewflag" is real
# and unmasked (unlike usemaskreinstallpkg's own above), so --newuse
# still reports a Reinstall for it (IUSE simply gained a flag)...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/changedusepkg
# [ebuild  r] dev-libs/changedusepkg-1.0 (reinstall for changed USE: brandnewflag)
# ...but --changed-use never even looks at IUSE presence, only at
# enablement -- and that flag's own enablement never changed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-use dev-libs/changedusepkg
# dev-libs/changedusepkg-1.0 is already installed; nothing to do
# --changed-use still catches an ENABLEMENT change on a flag shared by
# both IUSE sets, same as reinstallpkg's own --newuse example above
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-use dev-libs/reinstallpkg
# [ebuild  r] dev-libs/reinstallpkg-1.0 (reinstall for changed USE: foo)
# [ebuild  N] dev-libs/newpkg-1.0

# package.use.mask/package.use.force are real and implemented, atom
# specificity included: a repo-level package.use.force wildcard entry
# force-enables "forceflag"; the base profile's own package.use.mask
# masks "maskflag" and "specflag" via a bare atom; the leaf profile's
# own package.use.mask has a more specific exact-version atom that
# un-masks "specflag" again -- the more specific entry wins regardless
# of chain order, so specflag stays off (un-masked but never enabled)
# while maskflag stays masked (nothing un-masks it)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/pkgusemaskforcepkg
# [ebuild  N] dev-libs/pkgusemaskforcepkg-1.0  USE="forceflag -maskflag -specflag"

# --nodeps/-O is real and implemented: withdeps' own RDEPEND (which
# would otherwise pull in newpkg and upgradepkg -- see the plain
# recursion example above) is never even read
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --nodeps dev-libs/withdeps
# [ebuild  N] dev-libs/withdeps-1.0

# --nodeps still shows a resolved package's own USE display with -v --
# it's -N's own foo?-gated dependency recursion that's suppressed, not
# the package's own metadata
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -O -v dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"

# --onlydeps/-o is real and implemented: the exact inverse of --nodeps --
# withdeps' own dependencies (newpkg, upgradepkg) print normally, but
# withdeps' own [ebuild N] line is suppressed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --onlydeps dev-libs/withdeps
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)

# --onlydeps on an already-installed atom: no dependencies were ever
# going to be walked (same as without --onlydeps), and its own "already
# installed" line is suppressed too -- so the whole run prints nothing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --onlydeps dev-libs/samepkg
# (no output)

# short-flag bundling: "-pv" decomposes into -p + -v, both real,
# implemented flags -- native argparse behavior for boolean short
# options, not something requiring emerge-specific parsing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pv dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"
# [ebuild  N] dev-libs/newpkg-1.0

# a bundled flag reports on the first out-of-scope character, left to
# right, exactly like a standalone occurrence of it would
/tmp/emerge -pd dev-libs/newpkg
# emerge (pilot v1): option "--debug" is a real emerge option, but is not
# implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N,
# --nodeps/-O, --onlydeps/-o, and --help/-h are implemented so far; see
# PROMPT.md)  (exit 2)

# --help/-h is real and implemented: a short, honest, pilot-specific
# summary, not a port of real emerge's own (157-line, colorized,
# ~130-flag) help text -- wins unconditionally, regardless of position
# or what else accompanies it
/tmp/emerge --help
# emerge (pilot v1): command-line interface to the Rust porting pilot
# ...
# See PORTING/README.md and PORTING/PROMPT.md for this pilot's current scope.
/tmp/emerge --deep --help          # --help wins even combined with other flags
/tmp/emerge -ph                    # ...and even bundled with other short flags

# CLI surface recognition: a real emerge option this pilot doesn't
# implement is named specifically, not lumped in with a typo
/tmp/emerge --deep dev-libs/newpkg
# emerge (pilot v1): option "--deep" is a real emerge option, but is not
# implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N,
# --nodeps/-O, --onlydeps/-o, and --help/-h are implemented so far; see
# PROMPT.md)  (exit 2)

# a token that isn't a real emerge option/action at all gets a
# different message
/tmp/emerge --totally-fake-option dev-libs/newpkg
# emerge: unrecognized option "--totally-fake-option"

# or against the Python reference implementation directly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    python3 PORTING/python/emerge_pretend_reference.py --pretend dev-libs/newpkg
```

Try the `ebuild` stub (still a dry-run placeholder -- no real phase
execution -- but it now recognizes real ebuild syntax; see
ebuild.rs/ebuild_options.rs):

```sh
ln -sf "$(realpath PORTING/rust/target/release/multicall)" /tmp/ebuild

# a real, valid ebuild command -- still just a no-op stub
/tmp/ebuild foo-1.0.ebuild merge
# ebuild (pilot stub): dry-run only, no phase execution yet (see
# PROMPT.md's "Deferred: ebuild phase execution")
# ebuild file: "foo-1.0.ebuild"
# commands: ["merge"]

# real invocations often chain several phases in one call
/tmp/ebuild foo-1.0.ebuild clean compile install

# a real, value-taking option (--color y) is recognized and its value
# correctly skipped, not misread as the ebuild file
/tmp/ebuild --color y foo-1.0.ebuild merge

# a command that isn't one of doebuild()'s own valid commands is
# rejected, not silently accepted like the original bare stub did
/tmp/ebuild foo-1.0.ebuild not-a-real-phase
# ebuild: "not-a-real-phase" is not one of the valid ebuild commands
```

Run the contract suite (builds the Rust binaries itself; requires `cargo`
on `PATH`):

```sh
python3 -m pytest PORTING/tests -v
```

Run the benchmark / regression gate:

```sh
# report speedup, no gating (uses the vendored real Gentoo tree snapshot)
python3 PORTING/bench/run_benchmark.py --ops 200000

# fall back to synthetic seeded-random version strings instead
python3 PORTING/bench/run_benchmark.py --ops 200000 --dataset synthetic

# CI-style: fail if speedup regressed vs. the recorded baseline
python3 PORTING/bench/run_benchmark.py --check-baseline

# record a new baseline after an intentional, reviewed perf change
python3 PORTING/bench/run_benchmark.py --update-baseline

# same gate, wrapped as a pytest for CI (skipped by default -- see
# PORTING/tests/test_benchmark_gate.py)
PORTING_RUN_BENCHMARK=1 python3 -m pytest PORTING/tests/test_benchmark_gate.py -v
```

Run the musl static-build smoke test (requires `podman` or `docker`; builds
a container image, so it needs network access for the Alpine base layer
and `apk add rust cargo` the first time):

```sh
bash PORTING/musl/smoke_test.sh

# same gate, wrapped as a pytest for CI (skipped by default -- see
# PORTING/tests/test_musl_smoke.py)
PORTING_RUN_MUSL_SMOKE=1 python3 -m pytest PORTING/tests/test_musl_smoke.py -v -s
```
