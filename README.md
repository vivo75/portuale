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
  `matches_version`/`matches_slot`/`match_from_list` -- at the time this
  paragraph was written, full enforcement was scoped out because this
  pilot's `Candidate`/`match_from_list` model had no per-candidate
  IUSE/USE state to check a use-dep against at all. This wasn't an
  invented gap, though: verified empirically, real `match_from_list`
  given this pilot's own plain-string candidates (no `.use`/`.iuse`
  attributes) already skips its own USE-dep filtering entirely too --
  `dev-libs/foo[bar]` and `dev-libs/foo[-bar]` return the identical
  match set against real `match_from_list` itself, not just this pilot's
  port of it. **Now stale**: once `package.use`/`package.use.mask`/
  `.force` gave every candidate real, computable IUSE/effective-USE
  state for other reasons, this stopped being a hard blocker -- see the
  dedicated **USE-dep enforcement** paragraph further below for the
  follow-up that closes most of this gap (the `opt=`/`opt?` forms stay
  genuinely out of scope, for the reason named above -- they're
  conditional on the *atom-owning* package's own USE state, not just the
  candidate's, a wholly different mechanism).

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
- **`required-use-harness`**: ports real `check_required_use` (PMS
  7.3.4/8.2 -- `|| ( )`/any-of, `^^ ( )`/exactly-one-of, `?? ( )`/
  at-most-one-of, `flag? ( )`/use-conditional, negation, and a bare
  `( )` all-of group, PMS 8.2's own "all-of" production, permitted at
  REQUIRED_USE's own top level with no wrapping parens needed) as
  `portage_required_use::check_required_use`. Unlike real
  `check_required_use`, which builds and returns a full navigable tree
  purely so a caller can later pretty-print exactly which sub-expression
  failed, this is a much simpler direct recursive-descent boolean
  evaluator with no tree bookkeeping -- this pilot only ever needs the
  final yes/no verdict (see the `emerge --pretend` paragraph below for
  how a violation gets reported). Verified against real
  `check_required_use` directly via 37 cases (28 satisfied/unsatisfied
  plus 9 malformed-syntax/undeclared-flag error cases, plus batch mode)
  in the shared contract suite, the same wraps-the-real-thing
  verification pattern `use-reduce-harness` already established -- not
  just my own reasoning about equivalence. One EAPI-conditional
  real behavior is deliberately not replicated: real `eapi <= Eapi("6")`
  treats an empty group (`( )`, `|| ( )`) as vacuously satisfied; this
  pilot always uses the newer (EAPI 7+) behavior of evaluating an empty
  group with the ordinary per-operator rule instead (unsatisfied for
  `||`/`^^`, satisfied for `??`/a use-conditional) -- consistent with
  this pilot's established "no EAPI parametrization" precedent elsewhere
  (`portage-dep`'s own grammar, `use.mask`/`.force`'s atom-specificity
  ordering), and pinned on the Python reference side by passing
  `eapi="8"` explicitly to the real function rather than leaving it
  EAPI-agnostic (`eapi=None`), so both sides agree on exactly the same
  attribute values.
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
  rather than the real system profile), wildcard `_*` expansion, and
  every `USE_ORDER` layer except `defaults` (profile) and `conf`
  (make.conf) -- see the doc comment at the top of
  `rust/portage-profile/src/lib.rs`. `USE_EXPAND` itself is no longer on
  this list -- see its own paragraph further below for the follow-up
  that closed it.

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
  `portage-profile` can compute on its own). **Now stale**: the
  `USE_EXPAND`-prefix shorthand real `package.use` supports (`VIDEO_CARDS:
  nvidia` lines applying a `video_cards_` prefix to subsequent flags,
  reset at the start of every physical line) used to be out of scope
  here -- see the dedicated paragraph further below for the follow-up
  that closed it (real portage's own reset condition turned out to be
  "every line," not "a blank line" as this sentence used to claim).

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

  **`ebuild`'s own `--help`/`-h`**. Real bin/ebuild is pure `argparse`,
  which auto-adds `-h`/`--help` for free -- but neither was ever in
  `ebuild_options.rs`'s own `OPTIONS` table (only the six explicitly
  declared options are, "none with short aliases" per that table's own
  doc comment), so a bare `ebuild --help` used to be rejected as an
  unrecognized option instead of printing help and exiting `0`. Checked
  unconditionally before anything else in `args`, same "wins regardless
  of position, valid or not" precedent `emerge --help` already set --
  but simpler here, since real `ebuild` declares no short aliases for
  any option at all, so there's no bundling concept to scan through
  (unlike `emerge`'s own `-pv`-style bundling). The help text itself is
  a short, honest, pilot-specific summary, not a port of real
  `argparse`'s own generated usage block, matching `emerge --help`'s own
  precedent there too. `--version` was scoped alongside it but turned
  out to be a considerably worse fit than expected: real `bin/ebuild`'s
  own `print("Portage", portage.VERSION)` looks like a simple static
  read, but `portage.VERSION` (`lib/portage/__init__.py`) is a fixed,
  build-substituted string only for an *installed* copy -- for a
  from-source checkout (confirmed to be exactly what this repo itself
  is, by there being no `lib/portage/VERSION` file anywhere in it),
  `VERSION` is instead derived live via `git describe --dirty --long
  --match "portage-*"` against the current commit and working-tree state
  (verified directly against this repo: returns
  `portage-3.0.81-272-g1cb1941de`, parsed into
  `3.0.81.dev272+g1cb1941de`) -- the same host/git-state-dependent,
  non-deterministic-output problem that already ruled out `emerge
  --version`/`-V`'s own (differently-sourced) real value, so `--version`
  stays an ordinary recognized-but-unimplemented option here too.

  **`--deselect`/`-W`: the first real emerge *action*, not an option.**
  Every flag implemented so far modifies ordinary `--pretend` resolution;
  `--deselect` is different in kind -- real `lib/_emerge/main.py` turns a
  bare `--deselect`/`-W` into its own standalone action (`if myaction is
  None and myoptions.deselect is True: myaction = "deselect"`, the same
  shape as `--depclean`/`--sync`), dispatched here to a new `run_deselect`
  before any of the ordinary target-atom/resolve machinery even runs.
  Grounded directly against real `action_deselect`
  (`lib/_emerge/actions.py`, lines 1740-1835): a genuinely smaller port
  than every masking slice before it, since real `action_deselect`
  touches only the world file and the vdb, never any repo/config
  resolution at all -- so, unlike every other implemented flag,
  `portage-repo`'s own repo/profile-resolution machinery is never called
  here. Each given target is expanded into its own actually-installed
  `category/package:slot` form(s) -- a bare package name (no `/`) via
  real portage's own "null category" mechanism (scanning the world file
  for a same-named atom to borrow its category from), then an
  `installed_candidates` (`vardb.match`-equivalent) lookup either way --
  and each expanded form is matched against every world-file atom,
  printing `>>> Would remove <atom> from "world" favorites file...` for
  each one discarded (pretend mode; real portage only writes the world
  file outside of `--pretend`, so this pilot's own "never merges"
  invariant holds here unchanged), or `>>> No matching atoms found in
  "world" favorites file...` if none matched at all. A documented scope
  cut versus real `Atom.intersects()`: `pretend.rs`'s own `run_deselect`
  uses a narrower category/package(+slot) equality check rather than the
  full version-range/USE-dep algebra, sufficient for the dominant plain-
  atom case; the Python reference, by contrast, reuses the real
  `match_from_list` directly (the same "why re-derive it" reasoning
  `_matches_config_entry` already established), and both are verified to
  agree on every case this pilot's own contract suite exercises. A
  `@`-prefixed world entry is never matched, consistent with
  `read_world_atoms`'s own pre-existing cut for `@world` itself, not a
  new gap. CLI-wise, real `--deselect` is `argument_options` with an
  *optional* `y`/`n` value, the identical shape `--verbose`/`-v` already
  has: a bare `--deselect`/`-W` or `--deselect y` enables it, `--deselect
  n` explicitly disables it (falling through to ordinary resolution
  instead, chosen by real `main.py`'s own `is True` check rather than
  truthiness); a bundled `-W` (e.g. `-pW`) never consumes a value, always
  enabling, the same reasoning already established for a bundled `-v`/
  `-D`. Deliberately out of scope: `--ask` interactive confirmation
  (needs no special-casing at all -- it already falls through to this
  pilot's existing "not yet implemented" rejection) and `--json` output
  (simply not offered for deselect mode).

  **`--with-bdeps y|n`: build-time deps for an already-installed
  package's own `--deep` walk.** Grounded against real
  `create_depgraph_params.py`'s own `bdeps` param and `depgraph.py`'s
  `_add_pkg_dep_string` (`if pkg.built and not removal_action: ... else:
  ignore_build_time_deps = True`): real portage only ever drops
  `DEPEND`/`BDEPEND` for a package that's *already built* (installed),
  never for one being freshly resolved from an ebuild -- so, like
  `--deep` itself, this has zero effect on New/Upgrade/Reinstall
  packages, and only ever matters for an `AlreadyInstalled` package's own
  dependency walk once `--deep` says to walk it at all. The real default,
  `auto` (`create_depgraph_params.py`'s own `myparams["bdeps"] = "auto"`
  whenever `--usepkg` isn't given -- which this pilot's own
  `--usepkg`-less CLI always satisfies), and the real `y` are collapsed
  into one caller-facing `true`, since `depgraph.py` itself only ever
  tests `bdeps in ("y", "auto")`, never distinguishing the two -- so this
  pilot's own pre-existing "walk all five dependency-string keys
  uniformly" behavior (see the dependency-recursion paragraph above) was
  already exactly the real default, and `--with-bdeps=n` is the one value
  that actually changes anything. Unlike `--exclude` (arbitrary text) or
  `--deep`/`--verbose` (an optional peek), real `--with-bdeps` is
  `argument_options` with a REQUIRED, closed `"choices": ("y", "n")`
  value -- a missing value is a real, immediate usage error (same shape
  as `--exclude`'s own), and a value that's neither `y` nor `n` is *also*
  a real, immediate usage error (real `argparse`'s own choices
  validation, reproduced here as a pilot-specific message rather than
  argparse's own multi-line usage banner, consistent with every other
  CLI error in this pilot). It has no short alias and isn't
  bundle-compatible either -- real `main.py` declares no `shortopt` for
  it at all, so unlike `--exclude`'s own deliberate bundling cut, there's
  no bundling concept to begin with. `--with-bdeps-auto` (the only other
  real lever on this same `bdeps` value, relevant only once
  `--usepkg`/binary-package support exists) stays a deliberate,
  documented out-of-scope cut. New fixture packages `withbdepspkg`
  (installed, `DEPEND`s on `builddeponlypkg`, `BDEPEND`s on
  `hostdeponlypkg`, `RDEPEND`s on the existing `newpkg`) prove the
  distinction end to end: under `--deep`, the default walks all three;
  `--with-bdeps=n` walks only `newpkg` (`RDEPEND`), leaving the other two
  entirely unmentioned.

  **`--changed-deps[=y|n]`: reinstall a package whose own recorded
  dependencies changed.** Grounded against real `depgraph.py`'s own
  `_changed_deps`: reinstalls an already-installed package once its own
  vdb-recorded `DEPEND`/`RDEPEND`/etc strings differ from the repo's
  *current* ebuild for that exact version -- catching the case where an
  ebuild's own dependencies were edited upstream since this package was
  last merged, something `--newuse`/`--changed-use` (USE-driven reasons
  only) can never detect. Needed a genuinely new capability this pilot
  didn't have before: reading an installed package's own recorded
  `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/`IDEPEND` from the vdb (`read_
  vdb_string`, alongside the pre-existing `read_vdb_flag_set` for `USE`/
  `IUSE`) -- every prior slice only ever needed vdb presence/`USE`/
  `SLOT`, never a package's own recorded dependency strings. Both sides
  of the comparison are flattened (`use_reduce_flat`) against the
  *installed* package's own recorded `USE` (real `_changed_deps`'s own
  `uselist=pkg.use.enabled`, used for both sides), so a difference
  driven purely by a USE change is never what this detects -- confirmed
  independent of, and freely combinable with, `--newuse`/`--changed-use`
  (real portage treats them as separate triggers feeding one reinstall
  decision, not mutually exclusive ones; `PretendOutcome::Reinstall`
  gained a `deps_changed: bool` field alongside the pre-existing
  `changed_flags`, and the `(reinstall for ...)` note's own text
  combines both reasons when both fired). Which dependency keys get
  compared respects `--with-bdeps` exactly the same way `--deep`'s own
  `AlreadyInstalled` walk already does. **Deliberate, documented scope
  cut, not an oversight**: real `_changed_deps` compares genuine
  *structured* `use_reduce` output (`||`-group boundaries preserved) key
  by key; this pilot has no structured, non-flat `use_reduce` anywhere
  (the same limitation the `LICENSE` slice's own bespoke `LicenseNode`
  parser exists specifically to work around, for that one different
  mechanism) -- so this reuses the same flat comparison every other
  dependency-recursion path in this pilot already uses, consistent with,
  not a new exception to, the rest of this pilot's own dependency
  handling. A narrower, real-but-out-of-scope sibling,
  `--changed-deps-report` (a cosmetic-only "you might want
  `--changed-deps`" notice, no reinstall of its own), stays
  recognized-but-unimplemented. New fixture package `changeddepspkg`
  (installed, vdb-recorded `RDEPEND="dev-libs/samepkg"`, but its current
  ebuild's own `RDEPEND="dev-libs/newpkg"`) proves the whole path end to
  end, including that the reinstalled package's own recursion walks the
  *current* ebuild's dependency (`newpkg`), not the vdb's stale one --
  the same "current tree wins" precedent `enqueue_dependencies` already
  established for `--deep`'s own `AlreadyInstalled` walk.

  **`--changed-deps` ignores a libc-only dependency change
  (`strip_libc_deps`).** A later, independent follow-up to `--changed-deps`
  above, grounded against real `portage.dep.libc.find_libc_deps`/
  `strip_libc_deps`: practically every ebuild silently gains or loses an
  implicit dependency on whichever package provides `virtual/libc`
  across revisions, and real portage strips that noise out of both sides
  of its own `_changed_deps` comparison before comparing, rather than
  reporting a reinstall for it. `find_libc_deps(vardb, realized=False)`
  is itself a call into real `expand_new_virt` -- this pilot ports a
  simplified, one-level version of it (`libc_provider_cps`): find the
  installed `virtual/libc` package in the vdb (if any), read *its own*
  vdb-recorded `RDEPEND`, flatten it against its own installed `USE`,
  and collect the `category/package` identity of every resulting atom.
  Real `virtual/libc`'s own `RDEPEND` is always a flat `|| (
  sys-libs/glibc sys-libs/musl ... )` of real, non-virtual packages, so
  this doesn't replicate `expand_new_virt`'s own further case of
  recursing into a *second* virtual reached this way -- a case real
  `virtual/libc` never actually needs, the same "ported faithfully where
  it matters, simplified where the simplification is provably safe"
  judgment this pilot already applies elsewhere. This was a real,
  previously-named gap, not a new discovery: both this pilot's own
  `deps_changed`/`_deps_changed` doc comments already flagged
  `strip_libc_deps` explicitly as "unaddressed... no fixture in this
  pilot's own tree represents a libc package" -- closing it needed a new
  fixture-side `virtual/libc` vdb entry (`RDEPEND="|| ( sys-libs/glibc
  sys-libs/musl )"`, no repo ebuild needed at all, since `find_libc_deps`
  only ever reads the vdb) plus `dev-libs/libcnoisepkg` (vdb-recorded
  `RDEPEND="sys-libs/glibc dev-libs/samepkg"`, current ebuild's own
  `RDEPEND="sys-libs/musl dev-libs/samepkg"` -- different libc atom text
  on each side, but both are real libc providers, so once stripped both
  sides reduce to the identical `{dev-libs/samepkg}` and no reinstall
  fires even with `--changed-deps` given), proving the stripping engages
  without disturbing `changeddepspkg`'s own already-tested genuine
  dependency-change detection.

  **`--changed-slot[=y|n]`: reinstall a package whose own recorded `SLOT`
  changed.** Grounded against real `depgraph.py`'s own `_changed_slot`:
  `ebuild = self._equiv_ebuild(pkg); return ebuild is not None and
  (ebuild.slot, ebuild.sub_slot) != (pkg.slot, pkg.sub_slot)` --
  reinstalls an already-installed package once its own vdb-recorded
  `SLOT` (main *and* sub-slot, e.g. an ABI-bump `SLOT="0"` ->
  `SLOT="0/2"`) differs from the repo's *current* ebuild for that exact
  version. The first slice in this pilot to model sub-slots at all: real
  `SLOT="main/sub"` splits on `/`, defaulting `sub_slot` to the slot
  itself when no `/` is present (`split_slot`, shared by both the vdb
  and repo sides) -- deliberately narrow, though, not the general
  `Candidate.sub_slot` threading a full port of real slot-operator
  (`:=`) rebuild tracking would eventually need: this reuses the exact
  same "dedicated, narrow re-read of metadata this pilot's general
  `Candidate` model doesn't carry" approach `--changed-deps` already
  established for `DEPEND`/`RDEPEND`, rather than growing `Candidate`
  itself and touching the whole matching/visibility pipeline for a
  single new flag. Implemented as a third independent,
  freely-combinable `PretendOutcome::Reinstall` trigger alongside the
  USE- and deps-based ones -- confirmed real: `_changed_slot`'s own real
  callers (`_slot_operator_replace_installed`, and the main
  package-selection loop's own `built`/`useoldpkg` branches) live deep
  inside binary-package/slot-operator-rebuild scheduling this pilot has
  none of, so this ports the *effect* (a package whose `SLOT` changed
  upstream gets flagged) via the same "report a reinstall" pattern
  `--changed-deps` already used, rather than replicating real portage's
  own considerably messier, binpkg-entangled control flow -- a
  documented, confirmed scope cut, not a guess. The `(reinstall for
  ...)` reason line now combines up to three independent phrases
  (`reinstall_reason`/`_reinstall_reason` refactored from a fixed
  2-case match into a list of active reasons, joined with `; `, in the
  same push order both languages already agreed on), proven by
  extending the existing `changedslotpkg` fixture with its own stale
  vdb `RDEPEND` too: `--changed-slot` alone reports "changed slot",
  `--changed-deps` alone reports "changed dependencies" for the *same*
  package, and both together report "changed dependencies; changed
  slot" on one line.

  **Nested `@set` references.** Closes the `@world` slice's own
  documented gap (see the correction on that paragraph above), grounded
  by reading real `WorldSelectedSet`/`WorldSelectedSetsSet`/
  `StaticFileSet` (`lib/portage/_sets/files.py`) and `SetConfig`
  (`lib/portage/_sets/__init__.py`) directly rather than assuming from
  the name: real `@world` is the union of *two* separate files, not one
  -- `var/lib/portage/world` (plain atoms, already read) and
  `var/lib/portage/world_sets` (real `WORLD_SETS_FILE`, a list of
  `@name` references, e.g. added by a prior `emerge --noreplace
  @some-set`) -- ported here as a new `read_world_sets`. Each `@name` is
  resolved against `<config_root>/etc/portage/sets/<name>`, real
  portage's own default `usersets` source (`_create_default_config`'s
  own `class = StaticFileSet`, `directory = .../etc/portage/sets`, one
  file per set, the file's own relative path becoming the set's name) --
  ported as `resolve_custom_set`, same atom-per-line format the world
  file itself uses. The "nested" part earns its name genuinely: unlike
  the plain world file (whose own stricter validator rejects a
  `@`-prefixed line outright), a *custom* set file's own validator
  explicitly accepts one, and real `SetConfig.getSetAtoms` recurses into
  each such reference it finds -- so a custom set can reference another
  custom set, which can reference another, and so on. Ported with a
  `seen` cycle guard (real `getSetAtoms`'s own `ignorelist`, a fresh one
  per top-level name in `world_sets`): a name already being expanded on
  the current path contributes nothing further rather than looping
  forever, matching real portage's own silent (not erroring) cycle
  tolerance exactly. Deliberately **not** the same "absence is valid"
  tolerance the world/`world_sets` *files* themselves get: a `@name`
  explicitly listed (in `world_sets`, or referenced by another set) with
  no matching file is a real, immediate error (real `PackageSetNotFound`,
  which every real call site treats as fatal) -- a genuine configuration
  inconsistency, not an implicitly-optional file that simply might not
  exist yet. Deliberately still out of scope, confirmed as a *separate*
  mechanism rather than folded in here: `--deselect`'s own world-atom
  matching (`run_deselect`) is not integrated with `world_sets`/custom
  sets at all -- real `action_deselect` operates against the identical
  combined `world_set` `@world` itself now fully resolves, but
  deselect's own removal semantics (matching installed candidates,
  discarding matched world *entries*) are a genuinely different
  operation from simply resolving `@world` for a dependency walk, not a
  trivial extension of the same code -- see `run_deselect`'s own doc
  comment. New fixture packages `nestedsetpkg`/`innernestedsetpkg`
  (reached only via `PORTING/fixtures/var/lib/portage/world_sets`'s own
  `@nestedtestset`, whose own `etc/portage/sets/nestedtestset` nests a
  further `@innernestedset` reference, which itself references back to
  `@nestedtestset` to exercise the cycle guard) prove the whole path end
  to end.

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
  `resolve_config` is even called. Deliberately still out of scope at the
  time: an *overlay* repo's own repo-level `package.mask`/`.unmask`
  (only the one main repo's was read -- closed by a later follow-up
  below), and `masters` (eclass/mask inheritance across repos, still
  open). `package.accept_keywords`/`.use` remained user-level only for
  now -- real portage has repo/profile-level equivalents for both too,
  but stacking those was a separate, still-open cut this slice didn't
  claim to close (`package.accept_keywords` closed in the follow-up
  below; `package.use` remains open).

  **Overlay repos' own `package.mask`/`.unmask`**: closes the cut named
  just above, grounded against real `MaskManager.py`'s
  `repositories.repos_with_profiles()`, which reads every configured
  repo's own `profiles/package.mask`/`.unmask` *unconditionally* -- not
  just the main one -- each repo's own lines scoped via real
  `append_repo` (`lib/portage/util/__init__.py`) before being folded
  into the combined stack: "atoms without an explicit repo part get one,
  atoms that already have one are left alone", so an overlay's own bare
  `dev-libs/foo` mask entry becomes `dev-libs/foo::overlay`, never
  masking a same-named package in a *different* repo. `-atom` removals
  get the identical scoping (`-cat/pkg` -> `-cat/pkg::overlay`), so an
  overlay's own `package.unmask` can only ever cancel that same overlay's
  own `package.mask` entry, not another repo's. `resolve_config` gained
  a third parameter, `overlay_repos: &[(String, PathBuf)]` in Rust /
  `overlay_repos=()` in Python -- a plain local pair type rather than
  `portage_repo::RepoConfig`, since `portage-profile` can't depend on
  `portage-repo` (the dependency only runs the other way). Digging into
  the real mechanism turned up a genuine scope-narrowing discovery
  before any code was written: an overlay's own `profiles/`/
  `license_groups` are *not* part of this same "every repo,
  unconditionally" mechanism -- real `LicenseManager.__init__`'s own
  `license_group_locations` is tied to `locations_manager.
  profile_locations`, i.e. the profile *chain's* own directories, which
  only reach into an overlay once cross-repo profile parents
  (`reponame:path` syntax) exist -- a separate mechanism, closed by a
  later follow-up below. Also deliberately not
  done: retroactively scoping the *main* repo's own, already-shipped
  `package.mask`/`.unmask` entries with their own `::reponame` (real
  portage does this too, for consistency) -- an unrequested behavior
  change to already-tested main-repo behavior, and a distinct
  correctness question from "add overlay support". Two new overlay
  fixture packages exercise it end to end: `overlaymaskedpkg` (masked
  only in the overlay's own `package.mask`, with an identically-named,
  unaffected copy in the main repo -- an unconstrained atom still
  resolves via the main repo's copy, `::overlay` hits the mask,
  `::testrepo` bypasses it) and `overlaymaskedthenunmaskedpkg` (masked
  and unmasked by two entries in that same overlay's own files, proving
  both get the identical auto-scoping and still cancel out).

  **`repos.conf` `masters`**: a later, independent follow-up to the same
  overlay `package.mask` work above, grounded against real `config.py`
  (lines ~1229-1260) and `MaskManager.py` (lines ~69-100) together. A
  repo with no explicit `masters =` doesn't read its own `package.mask`
  standalone the way this pilot's own doc comments previously claimed --
  real `config.py`'s own `repo.masters = (self.mainRepo(),)` default
  means every non-main repo *implicitly* masters the main repo alone
  (the main repo's own masters default to `()`, since it can't be its
  own master), and `MaskManager.py`'s own per-repo loop stacks each
  master's own `package.mask` lines in *ahead of* the repo's own (real
  `stack_lists(incremental=1)`, i.e. this pilot's own `stack_mask_lines`)
  before the combined result gets `::reponame`-scoped -- so an overlay
  with no explicit masters still inherits the main repo's own masks for
  its own packages. This was a genuine, if narrow, behavior gap this
  pilot's own doc comments mis-described as intentional (`"every repo's
  own entries here are read standalone, which is exactly what real
  portage would also do for a masters-less repo"` -- a masters-less repo
  and a repo with no *explicit* `masters =` are not the same thing, and
  conflating them was the actual bug). A real asymmetry confirmed by
  reading `MaskManager.py`'s own two loops side by side: only
  `package.mask` consults masters at all -- `package.unmask`'s own loop
  never does, so this stays exactly as the previous follow-up left it,
  main-repo-only, no inheritance. Since this pilot doesn't parse an
  explicit `masters =` `repos.conf` key at all (no fixture repo ever
  declares one), only the implicit main-repo default is modeled -- an
  explicit override or a multi-master chain stays unimplemented. Two new
  overlay-only fixture packages exercise it end to end:
  `mastermaskedpkg` (masked only by the *main* repo's own
  `package.mask`, never mentioned in the overlay's own -- the only way
  it can end up masked is via the implicit masters inheritance) and
  `mastermaskedthenoverlayunmaskedpkg` (masked the same
  masters-inherited way, then unmasked by the overlay's own
  `package.unmask`, proving the inherited mask and the overlay's own
  unmask still cancel out once both get the identical `::overlay`
  scoping).

  **Cross-repo profile parents (`reponame:path` syntax)**: closes the
  cut named just above, grounded against real `LocationsManager.
  _addProfile`/`_expand_parent_colon`: a profile's own `parent` file
  entry can name another repo before a `:` (`reponame:some/path`,
  expanding to `<that repo's own location>/profiles/some/path`) or use a
  bare leading `:` (`:some/path`, meaning "this same repo" -- whichever
  repo the *referencing* profile node's own directory belongs to, found
  via the longest matching repo-location prefix, `repo_containing`,
  mirroring real `intersecting_repos`/`max(key=len)`). Previously
  rejected outright ("out of v1 scope"); now resolved by
  `resolve_profile_chain`, which gained a `repos: &[(String, PathBuf)]`
  parameter -- the main repo's own `(name, location)` plus every
  overlay's -- `resolve_config` itself gained one new parameter,
  `main_repo_name: &str` (`main_repo_name=""` default in Python),
  needed alongside the `overlay_repos` parameter the previous follow-up
  already added to build that combined list. One real, deliberate
  simplification: real portage only allows this syntax when the
  *referencing* profile node's own repo declares `profile-formats =
  portage-2` in `layout.conf`; this pilot doesn't model `layout.conf`
  profile-formats at all (a pre-existing cut, unrelated to this one), so
  it's always allowed here -- every real Gentoo profile fixture this
  pilot already ships implies it, the same "real default, ported without
  modeling the mechanism that technically gates it" treatment already
  applied elsewhere (e.g. `ACCEPT_LICENSE`'s own hardcoded `"* -@EULA"`
  default). This closes the exact gap the previous follow-up's own scope
  cut named: since every one of `resolve_config`'s own `for level in
  &chain` loops (license_groups included) already reads from *whichever*
  directories are in the chain, an overlay's own `profiles/`/
  `license_groups` becomes reachable automatically the moment a `parent`
  file actually names it -- no separate code path needed beyond the
  chain-resolution fix itself. `PORTING/fixtures/repo/profiles/default/
  parent` gained a third entry, `overlay:crossrepo-parent`, pointing at
  a new `PORTING/fixtures/overlay/profiles/crossrepo-parent/
  license_groups` that *extends* the main repo's own `EULA` group with
  one more member (`CrossRepoNonfree`, alongside the existing
  `SomeEula`) -- proving the two stack rather than one replacing the
  other. `dev-libs/crossrepolicensepkg` (`LICENSE="CrossRepoNonfree"`)
  is masked by the real default `"* -@EULA"` only if that overlay-level
  entry actually joined the active chain.

  **`package.accept_keywords` profile-chain stacking**: extends this
  same file from user-level-only to profile-chain (in chain order) +
  user-level, grounded directly against real `KeywordsManager.
  getPKeywords`. Digging into the real mechanism first turned up an
  asymmetry worth confirming with the user before assuming
  `package.use` would be an equally simple extension of the same
  pattern: real portage has **no repo-level source for
  `package.accept_keywords` at all** (confirmed by reading
  `KeywordsManager.__init__`, which never reads a repo-location path for
  it), so this is a clean 2-source extension, not 3. **Correction (see
  the `package.accept_keywords` negation paragraph further below): this
  pilot's own matching *was* purely additive at the time this paragraph
  was first written, but real `package.accept_keywords` itself was never
  purely additive -- real `KeywordsManager._getEgroups` supports
  `-atom`/`-*` removal too, ported later, not at this slice.**
  Concatenating profile-chain lines (in order) then user-level lines,
  then parsing once, was equivalent to real portage's own
  per-source-then-extend loop for this slice's own union-only matching;
  see the later paragraph for why a *negating* entry that crosses a
  source boundary still doesn't get real portage's own strict
  per-source precedence even after that fix. `package.use`, by contrast,
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

  **`package.accept_keywords` negation.** A correctness fix, not a new
  feature: grounded against real `KeywordsManager.getMissingKeywords`/
  `_getEgroups` (`lib/portage/package/ebuild/_config/
  KeywordsManager.py`), which folds `-token`/`-*` *removals* over the
  combined global-`ACCEPT_KEYWORDS`-plus-`package.accept_keywords` list,
  not just additions -- so a `package.accept_keywords` entry can revoke
  a keyword the global set already granted, not only extend it (the
  stale claim two paragraphs up, corrected there). This pilot's own
  `keywords_accepted` previously just unioned every matching entry's own
  tokens (including a literal, never-matching `"-amd64"` string) onto
  the global set, silently no-op'ing any negation instead of applying
  it. The fix reuses `specificity_ordered_flags` -- already established
  for `package.use.mask`/`.force`'s own identical "specificity-ordered
  incremental fold" shape -- seeded with the global `accept_keywords`
  set itself (a new `seed` parameter on that function; every
  `package.use.mask`/`.force` caller still passes an empty one, so
  behavior there is unchanged) rather than duplicating the fold logic.
  `"**"` (`accept-any`) is now folded in as an ordinary token too,
  rather than a separate pre-scan that ignored fold order entirely --
  proven to matter by a new test where a more-specific `-*` entry
  revokes an unconditional `"**"` grant from a less-specific one, which
  the old pre-scan-based code couldn't express at all. One deliberate,
  documented cut inherited from the pre-existing architecture, not
  introduced by this fix: like `package.mask`/`package.use.mask`/
  `.force` before it, every source (repo, profile chain, user) is
  concatenated into one flat entry list and folded by atom specificity
  alone -- real portage instead applies each *source* fully before
  moving to the next (specificity only breaks ties *within* one
  source), so a negating entry that crosses a source boundary can, in
  principle, resolve differently here than in real portage. Not
  addressed here since it's a pre-existing simplification spanning
  every one of these masking mechanisms, not something specific to
  keywords -- reopening it would be its own, separately-scoped slice.

  **`ACCEPT_KEYWORDS`/`package.accept_keywords` `"*"`/`"~*"` wildcard
  tokens.** A second correctness fix to the exact same function the
  negation slice above just touched, found by re-reading real
  `_getMissingKeywords`'s own per-candidate-keyword loop (lines
  ~273-300) more carefully: a literal `"*"` in the accepted set means
  "accept any stable keyword," `"~*"` means "accept any testing
  keyword" -- both distinct from `"**"` (accept even an *empty*
  `KEYWORDS`), which was already ported. Before this fix, `"*"`/`"~*"`
  had no special meaning at all in this pilot's own `keywords_accepted`
  -- `apply_incremental` would just insert the literal string `"*"` (or
  `"~*"`) into the accepted set, an inert token that can never equal a
  real `KEYWORDS` entry, so `ACCEPT_KEYWORDS="*"` (real portage's own
  documented "all arches allowed") or a `package.accept_keywords "~*"`
  entry would silently grant nothing at all. Ported directly from real
  portage's own per-keyword classification: each of the candidate's own
  declared keywords is checked for a direct match first (short-
  circuiting immediately); a `-`-prefixed one (explicit "not supported
  here") never matches and is excluded from classification entirely;
  anything else is classified stable or testing (`~`-prefixed) for a
  final fallback -- `"*"` grants acceptance if *any* declared keyword
  was stable-classified, `"~*"` if any was testing-classified. New
  fixture packages `starkeywordpkg` (`KEYWORDS="arm64"`, otherwise
  unmentioned anywhere, visible only via a `package.accept_keywords "*"`
  entry) and `tildestarkeywordpkg` (`KEYWORDS="~arm64"`, visible only
  via a `"~*"` entry -- deliberately proving `"*"` alone would *not*
  have covered it, since it only ever covers stable-classified
  keywords) exercise both wildcards end to end, distinctly from each
  other and from `"**"`.

  **`package.accept_keywords` bare-atom `accept_keywords_defaults`.** A
  third correctness fix to the same file, this time to how a bare
  entry (an atom with *no* keyword tokens at all) is parsed, not how a
  matching entry's tokens are folded. Grounded against real
  `KeywordsManager.__init__`/`getPKeywords`: a bare atom means "~" plus
  every plain (non-`~`/`-`-prefixed) token in the *current* global
  `ACCEPT_KEYWORDS` -- e.g. `ACCEPT_KEYWORDS="amd64"` turns a bare
  `dev-libs/foo` entry into the same thing as `dev-libs/foo ~amd64`
  written by hand. This pilot's own doc comments previously claimed real
  portage only applies this at the *profile*-level source and a bare
  *user*-level entry "never gets" it -- re-reading `__init__` itself
  while investigating this slice showed that's wrong: the user-level
  source gets the identical substitution too, just baked in at
  config-load time (`self.pkeywordsdict`) rather than read time
  (`getPKeywords`'s own per-entry check) -- a real behavior gap, not
  just a stale comment, since this pilot's own bare atom previously
  stayed a true no-op at both levels. Fixed by computing the derived
  `~arch` default list once, right after `config.accept_keywords` is
  fully resolved, and substituting it into every bare `package.
  accept_keywords` entry's own (now-preserved-rather-than-dropped) empty
  token list -- `keywords_accepted` itself needed no change at all,
  since a substituted entry folds through `specificity_ordered_flags`
  exactly like an explicitly-written one would. `parse_package_
  accept_keywords_lines` now keeps a bare atom instead of dropping it;
  since it's shared with `package.license`/`.properties`/
  `.accept_restrict` (none of which get this treatment in real portage),
  `parse_package_license_lines` gained one filter step to keep dropping
  bare atoms for those three specifically. New fixture package
  `bareacceptkeywordspkg` (`KEYWORDS="~amd64"`, testing-only) is visible
  only via a bare `package.accept_keywords` entry naming it, with no
  explicit `~amd64` token anywhere in the fixture tree.

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
  already uses -- not a separate code path. **Correction (see the
  "Nested `@set` references" paragraph further below): a `@`-prefixed
  line really is skipped in *this specific file* (real
  `WorldSelectedPackagesSet`'s own validator rejects it outright, no
  general set-recursion machinery needed to explain that part) -- but
  the claim that this was the *whole* story for real `@world`'s own
  nested-set union was wrong; nested `@set` references live in a
  genuinely separate file this pilot didn't yet read at the time this
  paragraph was written.** A missing world file (a fresh `ROOT` that's
  never had anything merged into it) is treated as a real, valid empty
  state, not an error. Only the literal token `@world` triggers this
  expansion -- `@system` (a separate mechanism, the profile's own
  `packages` file -- now also implemented, see below) or any other
  `@`-prefixed top-level target falls through to the ordinary
  atom-parsing path and gets a clear "invalid atom" error rather than a
  silent no-op.

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
  Two deliberate scope cuts beyond that at the time this paragraph was
  written: no stable-vs-`~arch` KEYWORDS distinction at all (real
  portage's own separate `use.stable.mask`/`.force`/`package.use.stable.
  mask`/`.force` files and `_isStable` check were out of scope entirely
  -- **now stale**, see the dedicated paragraph further below for the
  follow-up that closed it), and comparison-operator atoms (`>`/`<`/`>=`/
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

  **`--update`/`-u`: real default-vs-update package selection**. Grounded
  against real `depgraph.py`'s own `_wrapped_select_pkg_highest_available_imp`
  (`lib/_emerge/depgraph.py`, lines 7814 and 8448): `avoid_update =
  "--update" not in myopts` is real portage's *default*, and when it
  holds, an already-installed version that itself still satisfies the
  requested atom is returned immediately, without ever searching for a
  newer one -- real `emerge cat/pkg`, with no other flags, does NOT
  offer to upgrade a package just because a newer version exists; that's
  what `--update`/`-u` is for. This was a genuine, discovered inaccuracy
  in this pilot's own prior default behavior: every prior slice's
  New/Upgrade/AlreadyInstalled decision unconditionally searched for the
  single best visible version first, with no way to prefer "stay
  installed" at all -- there was no flag whose absence this
  unconditional search could even be said to be gated on. Fixed as an
  early return in `resolve_pretend`, checked before the pre-existing
  "always resolve to the best visible candidate" logic: without
  `update`, if some installed version both matches the atom and still
  has a visible candidate in the tree (mask/keyword-filtered, same as
  the rest of resolution), the highest such version is used as-is.
  Requiring a *visible* candidate rather than checking the vdb directly
  is deliberate, not incidental: it's what lets an installed version
  that's since become masked fall through to the ordinary
  best-visible-candidate path unchanged, matching real portage's own
  "enable upgrade or downgrade to a version with visible KEYWORDS when
  the installed version is masked" comment sitting right above its own
  `avoid_update` check -- a real corner this pilot gets right for free,
  not by accident. `update` threads uniformly through
  `resolve_pretend_graph`'s whole BFS, top-level atoms and dependencies
  alike, the same way `newuse`/`changed_use` already do -- and unlike
  that pair's own whole-graph application (a *documented* simplification
  of real portage's own more selective default), this one isn't a new
  pilot-specific simplification at all: `avoid_update`/`dont_miss_updates`
  are themselves plain `myopts` checks inside the one package-selection
  function every atom resolution, args and dependencies alike, already
  funnels through in real portage too. No new fixture was needed:
  `dev-libs/upgradepkg` (installed at `1.0`, a newer `2.0` visible in the
  tree -- already exercising the *old*, now-corrected default) turned
  out to be exactly the right shape to prove the *new* one too, reused
  as-is. Every pre-existing pinned test/example whose own point was
  something else entirely (`@world`/`@system` expansion, `--onlydeps`)
  but happened to lean on `upgradepkg`'s old default-upgrades-unconditionally
  behavior for its expected output now passes `--update` explicitly,
  noted inline in each case.

  **USE-dep enforcement** (`dev-libs/foo[bar]`/`[-bar]`, `(+)`/`(-)`
  defaults -- PMS 8.3.4). `portage-dep` has parsed the full 7-form USE-dep
  grammar since the slot-operator follow-up, but never enforced it when
  matching -- confirmed by grepping both crates: `matches_version`/
  `matches_slot`/`match_from_list` never once consulted `Atom::use_deps`.
  This was a *deliberate* cut at the time ("this pilot's dependency-graph
  model has no per-package IUSE/USE state to check a use-dep against"),
  but that stopped being true once `package.use`/`package.use.mask`/
  `.force` gave every candidate a real, computable effective-USE set --
  closing part of an existing, explicitly-documented gap, not a redesign.
  Grounded against real `match_from_list`'s own USE-dep post-pass
  (`lib/portage/dep/__init__.py` lines 3143-3188) and, for the trickier
  multi-flag/default-interaction cases, against real portage's own
  authoritative test vectors (`lib/portage/tests/dep/test_match_from_list.py`'s
  `dev-libs/A[...]` cases) -- ported as `use_deps_satisfied`
  (`portage-dep`), called by `portage-repo`'s `resolve_pretend` as a
  post-filter *after* `match_from_list`'s own version/slot/repo matching,
  never inside `match_from_list` itself: real `match_from_list` skips its
  own USE-dep block entirely for a plain-string candidate (its own
  `hasattr(x, "use")` guard), which is exactly what every candidate this
  pilot ever builds already is -- so `portage-repo` only calls
  `use_deps_satisfied` once it has a real candidate's own current-tree
  IUSE and effective USE in hand (`candidate_iuse_and_use`, extracted
  from what `reinstall_flags_for_use_change` already computed the same
  way). A use-dep flag with no `(+)`/`(-)` default -- of *any* form,
  including the four conditional ones (`flag?`/`!flag?`/`flag=`/
  `!flag=`) -- must be a real, declared IUSE flag on the candidate, or
  the atom doesn't match at all; only the two *unconditional* forms
  (`flag`/`-flag`) actually constrain enabled/disabled state, and a
  `(+)`/`(-)` default only ever matters for a flag missing from IUSE.
  The four conditional forms impose no enabled/disabled constraint
  here at all -- genuine real `match_from_list` behavior (their own
  values live in a separate `.conditional` structure it never reads),
  not a pilot simplification: evaluating one for real needs the
  *atom-owning* package's own USE state, a wholly different mechanism
  this pilot doesn't have and `match_from_list` itself doesn't either.
  Wiring this up surfaced a second, real latent bug along the way,
  fixed as part of the same slice: `candidate_iuse_and_use` (and the
  `reinstall_flags_for_use_change` code it was extracted from) used to
  treat a missing `IUSE` key in an md5-cache entry as "unreadable,
  exclude this candidate" -- but a package that simply declares no USE
  flags at all is a real, valid state (same "absence is real, not an
  error" precedent `read_vdb_flag_set` already sets for a missing vdb
  file), and roughly a dozen older fixture packages' own md5-cache
  entries genuinely omit `IUSE=` entirely (predating IUSE being modeled
  in this pilot at all) -- previously harmless, since nothing before
  this slice ever read a non-installed candidate's IUSE this broadly.
  `dev-libs/useflagpkg` (already-established `IUSE="foo missingflag"`,
  `foo` enabled globally, `missingflag` not) needed no new fixture at
  all to prove every combination -- declared+enabled, declared+disabled,
  each negated, and an undeclared flag with and without a `(+)` default;
  `dev-libs/usedeppkg`'s own pre-existing RDEPEND (from the slot-operator/
  USE-dep grammar follow-up) picked up `(+)` defaults so it stays
  genuinely satisfied under real enforcement instead of merely
  unenforced; and a new `dev-libs/usedeprejectedpkg` (RDEPEND
  `dev-libs/useflagpkg[-foo]`, genuinely unsatisfiable) proves a
  rejected *dependency-level* USE-dep atom reports `NoVisibleCandidate`
  for that one entry without failing the whole graph, the same
  "report, don't fail" precedent an unresolvable dependency already had.

  **REQUIRED_USE violation reporting** (PMS 7.3.4/8.2). Grounded against
  real `depgraph.py`'s own integration point, not just `check_required_use`
  in isolation: its own `_add_pkg` has a comment reading "NOTE:
  REQUIRED_USE checks are delayed until after package selection, since
  we want to prompt the user for USE adjustment rather than have
  REQUIRED_USE affect package selection and `||` dep choices" -- a
  genuine *post*-selection check, no part of matching/visibility at all
  (unlike `package.mask`/`package.use`/USE-dep enforcement, all of which
  narrow which *candidate* even resolves). Ported into
  `resolve_pretend_graph` at exactly that point: right after a
  candidate is newly resolved to New/Upgrade/Reinstall (never
  AlreadyInstalled -- matching real `not pkg.built`, trivially always
  true here since this pilot has no binary-package concept at all to
  make that check meaningful), its own `REQUIRED_USE` (if declared and
  non-empty) is checked via `check_required_use` against its own
  already-computed IUSE/effective-USE. On violation (or a malformed/
  undeclared-flag error), this is FATAL to the **whole** `--pretend`
  run -- real portage's own severity for this, verified against
  `depgraph.py` itself: the failure flag it sets
  (`_required_use_unsatisfied`/`_skip_restart`) is a single, global
  piece of state, with no distinction anywhere for whether the
  violating package was reached as a top-level target or a dependency
  deep in the graph -- a materially different, *harsher* severity than
  a dependency's own `NoVisibleCandidate` (report, don't fail the whole
  call) that this pilot already had. Ported as a `Result::Err` returned
  straight out of the BFS loop, reusing the exact same fatal-error
  plumbing a top-level atom's own unsatisfiable `NoVisibleCandidate`
  already needed -- no new plumbing required in `pretend.rs` at all.
  The error message itself is a short, honest, pilot-specific one
  (`REQUIRED_USE not satisfied for cat/pkg-version: "<normalized
  REQUIRED_USE string>"`) showing the package's own full, as-declared
  constraint -- not real portage's own elaborate, colorized report with
  the "reduced," violation-only sub-expression extracted via the tree
  `check_required_use` itself doesn't build here (see
  `required-use-harness`'s own bullet above), matching the same
  "pilot-specific summary, not a port of real formatting" precedent
  `--help` already set. `dev-libs/requireduseokpkg` (`REQUIRED_USE="foo?
  ( bar )"`, `foo` enabled globally by the fixture profile chain, `bar`
  forced on by this package's own `package.use` entry -- genuinely
  satisfied) and `dev-libs/requiredusebadpkg` (identical REQUIRED_USE,
  but no `package.use` entry forcing `bar` on -- genuinely violated)
  needed no new USE-flag machinery at all, just two small ebuilds
  proving both the satisfied and violated paths; `dev-libs/
  requiredusebadparentpkg` (RDEPENDs on the violated package) proves the
  fatal-abort severity really does apply regardless of graph position,
  not just to a top-level atom's own REQUIRED_USE.

  **`USE_EXPAND` support** (PMS 7.3.4). Closes a gap named explicitly in
  `portage-profile`'s own doc comment since the original profile-chain
  slice. Grounded against real `config.py`'s own `regenerate()` --
  genuinely elaborate machinery (incremental-vs-non-incremental
  per-variable handling, a separate `USE_EXPAND_UNPREFIXED` mode,
  IUSE-aware wildcard expansion for cases like `linguas_*`, an
  early-expand pass specifically so sub-profiles get useful incremental
  behavior) considerably bigger than this pilot's own flat accumulation
  model, so v1 ports a deliberately narrower core: `USE_EXPAND` itself
  (the variable-NAME list, e.g. `VIDEO_CARDS PYTHON_TARGETS`) accumulates
  incrementally across the profile chain and `make.conf`, the exact same
  `apply_incremental` mechanism `USE`/`ACCEPT_KEYWORDS` already use; each
  named variable's own VALUE (e.g. `VIDEO_CARDS="nvidia"`) is read via
  this pilot's own pre-existing, already-documented "last-level-wins,
  no incremental merge" scalar mechanism (the same one `ARCH` already
  uses) -- a deliberate simplification of real portage's own genuinely
  per-variable-incremental behavior, extending an already-confirmed cut
  to one more case rather than inventing a new one. Each value's own
  tokens expand into lowercase-`varname_`-prefixed pseudo-USE-flags
  (`VIDEO_CARDS="nvidia"` -> `video_cards_nvidia`), folded directly into
  the same flat `use_flags` set every other USE source already
  populates -- no separate per-variable breakdown is kept, since nothing
  in this pilot (no `--info` action) needs one. Deliberately out of
  scope, all confirmed real, named corners: `USE_EXPAND_UNPREFIXED`,
  IUSE-aware wildcard expansion (needs a specific package's own IUSE,
  which global config resolution has no access to), and
  `USE_EXPAND_HIDDEN`/`_IMPLICIT` (real `emerge --info` display-only
  concerns). **Now stale**: `package.use`'s own `USE_EXPAND`-prefix
  shorthand (`VIDEO_CARDS: nvidia` lines) used to be listed here as a
  separate, not-yet-ported follow-up -- see the dedicated paragraph
  further below for the follow-up that closed it. `dev-libs/useexpandpkg` (`IUSE="video_cards_nvidia
  video_cards_amdgpu"`, RDEPEND gated on each) proves the expanded flag
  genuinely drives dependency recursion, not just USE display:
  `video_cards_nvidia` (declared by `profiles/base/make.defaults`) pulls
  in its dependency, `video_cards_amdgpu` (never declared anywhere)
  doesn't.

  **`package.use`'s own `USE_EXPAND`-prefix shorthand**. The
  explicitly-deferred follow-up to the base `USE_EXPAND` slice above.
  Grounded against real `UseManager._parse_user_files_to_extatomdict`: a
  token ending in `:` (e.g. `VIDEO_CARDS:`) sets a
  `lowercase(name) + "_"` prefix applied to every *following* token on
  that same line (a leading `-` stays outside the new prefix, so
  `-intel` becomes `-video_cards_intel`, not `video_cards_-intel`),
  reset back to none at the start of every physical line -- confirmed by
  reading real `grabdict_package`'s own `newlines=1` marker handling (a
  fresh `"\n"` token is inserted between every line for the same atom,
  and the real code's own loop resets its prefix on each one; the
  original `package.use` slice's own paragraph above had mis-described
  this as "resets on a blank line," now fixed). A genuinely real,
  *user-level-only* restriction, not a pilot-invented one: confirmed by
  reading `UseManager.__init__`, only `_parse_user_files_to_extatomdict`
  (the user-level `package.use` parser) ever applies this shorthand --
  the repo-level/profile-level parsers
  (`_parse_repository_files_to_dict_of_dicts`/
  `_parse_profile_files_to_tuple_of_dicts`) both go through
  `_parse_file_to_dict` instead, which never passes `newlines=1` and has
  no such expansion step at all, so the identical `VIDEO_CARDS:` syntax
  in a repo-level or profile-level `package.use` file is just a literal,
  unexpanded token there. This real distinction meant splitting this
  pilot's own previously-uniform "concatenate all three sources, parse
  once" `package.use` handling into two parses -- repo+profile-level
  lines (no shorthand) and user-level lines (shorthand enabled) --
  concatenated together afterward, rather than adding a new pilot-wide
  simplification. `dev-libs/packageuseexpandpkg`, gated by a
  `PORTING/fixtures/etc/portage/package.use` entry reading
  `dev-libs/packageuseexpandpkg PYTHON_TARGETS: python3_12`, proves the
  shorthand expansion drives real dependency resolution end to end, not
  just token substitution in isolation -- and, since the shorthand
  itself never checks whether `PYTHON_TARGETS` is an actually-declared
  `USE_EXPAND` variable anywhere (confirmed by reading the real parsing
  loop: it's a purely syntactic transform), this fixture needed no
  change to the base `USE_EXPAND` slice's own profile fixture state at
  all.

  **`use.stable.mask`/`.force`/`package.use.stable.mask`/`.force`
  (stable-vs-`~arch` distinction)**. Closes the last named cut in the
  `package.use.mask`/`.force` slice's own paragraph above. Grounded
  against real `KeywordsManager.isStable`, which is genuinely more
  subtle than a raw "no `~` prefix" check: a candidate counts as
  "stable" if replacing *every* one of its own KEYWORDS with its
  `~`-prefixed unstable form would make it invisible under the current
  `ACCEPT_KEYWORDS`/`package.accept_keywords` -- real portage's own
  comment explains why: "this guarantees that the effective use.force/
  mask settings for a particular ebuild do not change when that ebuild
  is stabilized." Ported as `is_stable`, reusing `is_visible`'s own
  keyword-matching logic (factored out into `keywords_accepted`) against
  that artificially-unstabilized list instead of a candidate's real
  KEYWORDS -- not a second, separate matching algorithm. Also confirmed
  by reading real `getUseMask`/`getUseForce`'s own `pkg=None` (global)
  branch: it never even looks at the stable variant at all, since
  "stable" is inherently a per-candidate property with no meaningful
  global value -- so, unlike the already-shipped, config-resolution-time-
  folded `use_force`/`use_mask`, `use_stable_force`/`use_stable_mask`
  stay separate fields on `Config`, applied by `portage-repo`'s own
  `effective_use_flags` conditionally, once it knows a specific
  candidate's own stability (the same layer `package.use.mask`/`.force`
  and USE-dep enforcement already work at). `use.stable.mask`/`.force`
  read from the profile chain only, matching this pilot's own
  already-established (not newly cut) profile-only sourcing for the
  non-stable global `use.mask`/`.force`, rather than also adding the
  repo-level sourcing real per-package `getUseMask`/`getUseForce` has
  for it that this pilot's own global mechanism never had either;
  `package.use.stable.mask`/`.force` read repo-level (main repo only)
  plus profile-chain, mirroring `package.use.mask`/`.force`'s own
  already-confirmed sourcing exactly, no user-level source (same
  `UseManager.__init__` file/variable table confirmation). `dev-libs/
  stableusepkg` (`KEYWORDS="amd64"`, genuinely stable under the fixture's
  own `ACCEPT_KEYWORDS="amd64"`) and `dev-libs/unstableusepkg`
  (`KEYWORDS="~amd64"`, genuinely not) share identical `IUSE`/RDEPEND
  and an identical `package.use` entry enabling `maskflag` -- proving,
  end to end through real `emerge --pretend`, that `use.stable.force`
  forces `stableforceflag` on (pulling in a real dependency) and
  `package.use.stable.mask` masks `maskflag` back off despite
  `package.use` enabling it, for the stable candidate only; the
  unstable one gets neither.
  **`--deep`/`-D`: also recurse into an already-installed package's own
  dependencies.** Grounded against real `lib/_emerge/main.py`'s own
  `"--deep": valid_integers` declaration and
  `create_depgraph_params.py`/`depgraph.py`'s `_too_deep`/`_add_pkg`
  combination: real portage's own default (`deep` absent from
  `myparams` entirely unless `--deep`'s own value is present and
  non-zero) means an already-installed, already-satisfied package's own
  further dependencies are *never* walked, at any depth -- which turned
  out to already be exactly this pilot's own pre-existing, hardcoded
  behavior (an AlreadyInstalled outcome never reads its own DEPEND/
  RDEPEND/etc at all), so implementing `--deep` meant adding the
  recursion this pilot never had, not fixing a gap in what it already
  did. A bare `--deep` means real Python `True` (unlimited depth);
  `--deep=N`/`--deep N` bounds it to `N` levels past a directly-
  requested top-level atom (depth `0`) -- both threaded through the BFS
  as a per-queued-atom depth counter (`portage-repo`'s own `Deep` enum
  and `Deep::recurses_at`), exactly mirroring real depgraph.py's own
  graded, non-boolean cutoff rather than collapsing it to a simpler
  on/off simplification. Like `--verbose`/`-v`, it's real
  `argument_options` with an *optional* value, not a plain boolean, so
  the CLI parsing follows the same `insert_optional_args`-derived
  "peek the next token, consume only if it validates" pattern already
  established there -- a bundled `-D` (e.g. `-pvD`) never consumes a
  value either, same "no ambiguity with another bundled flag character"
  reasoning as a bundled `-v`. An AlreadyInstalled package's own
  dependency metadata is read from the repo's *current* ebuild for that
  version (via `enqueue_dependencies`, factored out of the ordinary New/
  Upgrade/Reinstall dependency walk so both share the same lookup-and-
  flatten logic) rather than from real portage's own vdb-snapshot
  metadata, since this pilot has no vdb-metadata reader at all -- a
  deliberate, documented simplification consistent with every other
  candidate lookup in this pilot already working this way, not a new
  gap introduced here. `dev-libs/deeppkg` (installed, RDEPENDs on
  `dev-libs/deeppkg2`) and `dev-libs/deeppkg2` (also installed, RDEPENDs
  on `dev-libs/newpkg`, New) exercise the exact depth-cutoff semantics:
  without `--deep`, only `deeppkg`'s own "nothing to do" line shows;
  a bare `--deep` reaches all the way to `newpkg`; `--deep=1` reaches
  `deeppkg2` but not `newpkg` (indistinguishable from no `--deep` at all
  in this pilot's own plain-text output, since `deeppkg2` stays a
  silent, non-top-level AlreadyInstalled entry either way); `--deep=2`
  reaches `newpkg`, same as unlimited -- proving the bound is real in
  both directions, not silently ignored. `--deep=0` parses fine but is
  indistinguishable from `--deep` never being given at all, matching
  real `create_depgraph_params.py`'s own `!= 0` check; a negative
  `--deep=N` is a real, immediate parse error (exit `2`), matching real
  `parser.error("Invalid --deep parameter: ...")`.
  **`--exclude`/`-X`: leave a matching package alone.** Grounded against
  real `lib/_emerge/main.py`'s own `"--exclude": {"shortopt": "-X",
  "action": "append", ...}` declaration and depgraph.py's own scattered
  `excluded_pkgs.findAtomForPackage` call sites (`self.excluded_pkgs =
  WildcardPackageSet(atoms)`, checked at ~18 different points throughout
  package selection). Real help text: "Emerge won't install any ebuild
  or binary package that matches any of the given package atoms" -- but
  reading the actual call sites shows two distinct effects, not one:
  (1) `_want_update_pkg`/`_replace_installed_atom` both check
  `excluded_pkgs` *first*, before any `--update`/USE-change logic even
  runs, so an installed package matching an exclude atom is left exactly
  as-is unconditionally -- the dominant real-world use ("pin an
  installed package so `--update`/`--deep` never touch it"); (2) several
  candidate-selection loops (e.g. depgraph.py's own lines 2331 and 5544)
  skip an excluded candidate when picking the best available version, so
  a not-yet-installed package matching an exclude atom is never offered
  either. Both are ported as two checks inside `resolve_pretend`, using
  the exact atom text/version at each point (not a separate "is this
  category/package excluded" shortcut), matching how real portage
  re-checks per specific candidate rather than blacklisting a whole
  category/package once. Deliberately NOT replicated: real depgraph.py's
  remaining `excluded_pkgs` call sites cover interaction points this
  pilot doesn't implement at all (autounmask, binpkg selection,
  `--complete-graph`, ...) -- a documented scope cut, not an oversight.
  Real `WildcardPackageSet` accepts wildcard atoms as well as plain
  ones, so this reuses the exact same two-tier `matches_config_entry`
  matcher `package.mask`/`.unmask` already established (try
  `match_from_list` first, fall back to the bounded wildcard-atom
  matcher). CLI-wise, real `main.py` declares `--exclude` `"action":
  "append"` with each occurrence's own value itself a *space-separated*
  atom list (help text: "A space separated list of package names or
  slot atoms") -- both accumulate here: `--exclude foo --exclude "bar
  baz"` excludes all three. Unlike `--deep`/`-D`'s own optional value,
  `--exclude`'s is required, so this pilot deliberately doesn't support
  bundling it (`-pX` gets a specific "requires an argument, can't be
  bundled" message) -- there's no sensible default the way a bundled
  `-v`/`-D` has.
  **`--json`: structured output, plus `required_by`.** Unlike every
  other flag in this series, `--json` is NOT a port of any real emerge
  behavior -- real portage has no structured-output mode for
  `--pretend` at all. Built at the user's own explicit request, with two
  fields no plain-text line has ever carried: `requested` (was this
  exact category/package one of the atoms given directly, vs. reached
  only via a dependency string) and `required_by` (which package(s), if
  any, pulled it in that way -- a genuinely new piece of graph state,
  not just a different rendering of what already existed). `required_by`
  needed real BFS surgery: the queue now carries each item's own owner
  alongside its atom text and depth, and a `required_by_map` accumulates
  every distinct owner per `(category, package)` throughout the walk,
  merged into `entries` in one pass at the end -- deliberately
  *independent* of the BFS's own `visited_atoms`/`resolved_slots`/
  `other_outcomes` dedup decisions (which only ever decide whether to
  *resolve* an atom again), so a diamond dependency's second, deduped
  owner still gets recorded even though it never triggers a new
  resolution -- verified against the existing `dev-libs/diamond` fixture
  (`shared-a`/`shared-b` both RDEPEND on `dev-libs/common`): `common`'s
  own `required_by` lists both, sorted, not just whichever branch the
  BFS happened to resolve first. `source` is always `"ebuild"` -- this
  pilot has no binary-package support anywhere (no `--usepkg`/
  `--getbinpkg`, no binpkg reading in `portage-repo` at all), so nothing
  else is ever possible; included so a JSON consumer doesn't have to
  assume it, not because this pilot actually distinguishes binary from
  source (confirmed with the user directly, choosing this over omitting
  the field entirely). Output is deliberately unaffected by `--onlydeps`
  (a display-only concern for the plain-text loop): `--json` always
  dumps the *whole* resolved graph, so a consumer can filter on
  `requested` themselves instead. Hand-rolled JSON on both sides
  (`json_escape`/`json_string`/`entry_to_json`/`print_json` in
  `pretend.rs`, `_json_escape`/`_entry_to_json`/`_print_json` in the
  Python reference) rather than a JSON library on either side -- the
  same "two independent implementations building the identical string
  via the identical algorithm" approach this whole pilot uses
  everywhere else, verified to produce genuinely byte-for-byte identical
  output (not just structurally-equal-as-JSON) via the shared contract
  suite.
  **`LICENSE`/`ACCEPT_LICENSE`/`package.license` masking** (PMS 7.3.2).
  A new, real visibility-gating mechanism this pilot had zero handling
  for before this slice -- grounded against real
  `LicenseManager.getMissingLicenses`/`_getPkgAcceptLicense`
  (`lib/portage/package/ebuild/_config/LicenseManager.py`) and
  `Package.py`'s own `settings._getMissingLicenses` call, alongside
  `package.mask` as another independent masking reason. The real
  algorithm turned out to need genuine `||`-group *structure* (real
  `use_reduce(..., opconvert=True)`) that this pilot's own existing
  `use_reduce_flat` deliberately discards (the same DEPEND/RDEPEND `||`-
  flattening simplification `resolve_pretend_graph` already documents),
  so the Rust side needed a bespoke recursive-descent parser
  (`portage-repo`'s own `LicenseNode`/`parse_license_tree`) -- the same
  reasoning that already made `portage_required_use` its own separate
  algorithm rather than a mode of `use_reduce_flat`, not a new kind of
  exception. Verified directly against real `portage.dep.use_reduce`'s
  own empirical output (not just its docstring) that a `||` group's own
  members stay flat (`['||', 'MIT', 'BSD']`, not double-nested) while a
  *plain* sub-group sitting directly inside that same `||`'s member list
  stays a genuine nested "this whole bundle is one alternative" unit
  (`['||', ['GPL-2', 'MIT'], 'BSD']` for `|| ( ( GPL-2 MIT ) BSD )`) --
  a real, structurally-significant distinction a naive "always flatten"
  parser would have gotten semantically wrong, caught by grounding
  against real output before implementing rather than assumed. The
  Python reference, unlike the Rust side, calls real `use_reduce`
  directly (same "call the real function" approach `check_required_use`
  already established for REQUIRED_USE) rather than needing its own
  parser, and both sides' masking-decision walk (AND at the top level or
  a plain nested group, satisfied-once-any-alternative-clean under
  `||`) are verified to agree via the shared contract suite regardless.
  `license_groups` (`@FREE`-style named groups, recursively expandable
  with negation and a cycle guard -- e.g. `-@EULA` negates every
  expanded member, not just the group reference) and `package.license`
  (atom-specificity ordered, reusing the exact machinery already built
  for `package.use.mask`/`.force`) round out the real mechanism.
  `ACCEPT_LICENSE` itself is a deliberate, documented simplification:
  real portage genuinely accumulates it incrementally across every
  config source (`prune_incremental` over each source's own raw
  tokens); this pilot instead extends its own pre-existing "any variable
  other than USE/ACCEPT_KEYWORDS is a plain last-level-wins scalar" cut
  to this one too, rather than inventing a new, single-variable-only
  incremental mechanism -- real portage's own meaningful default when
  `ACCEPT_LICENSE` is never set anywhere at all, `"* -@EULA"`, is still
  replicated exactly. `dev-libs/eulapkg` (`LICENSE="SomeEula"`, masked
  by the real default once `license_groups` defines `EULA="SomeEula"`),
  `dev-libs/anyoflicensepkg` (`LICENSE="|| ( GPL-2 SomeEula )"`, visible
  via the accepted `GPL-2` alternative), `dev-libs/packagelicensepkg`
  (identical to `eulapkg`, but unmasked for that one package via
  `package.license`), and `dev-libs/uselicensepkg`/
  `uselicensepkgforced` (an identical USE-conditional `LICENSE`, visible
  with the flag off by default, masked once `package.use` forces it on
  for the `forced` sibling specifically) exercise the full mechanism end
  to end through real `emerge --pretend`.
  **`PROPERTIES`/`ACCEPT_PROPERTIES`/`package.properties` and
  `RESTRICT`/`ACCEPT_RESTRICT`/`package.accept_restrict` masking.** A
  natural, smaller follow-up to the `LICENSE` slice above, grounded
  against real `config.py`'s own `_getMissingProperties`/
  `_getMissingRestrict` -- and its own comment says it plainly:
  "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without groups". Unlike
  `LICENSE`, neither `PROPERTIES` nor `RESTRICT` has any `||`-any-of
  semantics at all, so this reuses `use_reduce_flat` directly (every
  flattened token individually needs to be accepted) instead of the
  bespoke `LicenseNode` tree -- confirmed by reading both real functions
  side by side with `getMissingLicenses`, not assumed from the name
  alone. The shared `*`/`-*`/`-token`/`token` acceptance algorithm and
  the atom-specificity-ordered `package_accept` layering were factored
  out of `license_accepted` into `resolve_accept_tokens`/
  `resolve_acceptable_tokens`/`use_flags_if_conditional` (Rust) and
  their Python mirrors, verified behavior-preserving for `LICENSE`
  itself (all of that slice's own tests still pass unchanged) before
  building `PROPERTIES`/`RESTRICT` on top of the same three functions,
  rather than copy-pasting the algorithm a second time. Real portage's
  own default -- `"*"` (accept everything) -- comes from
  `cnf/make.globals`, a real, always-sourced config layer this pilot
  doesn't model as an actual read file (unlike the profile chain/
  make.conf), so it's replicated as a hardcoded fallback, the same "real
  default, ported without modeling the file it technically comes from"
  treatment `accept_license`'s own `"* -@EULA"` already gets (there, the
  default is a genuine Python-level hardcoded fallback even in real
  portage itself, not read from any file at all -- a slightly different
  real mechanism arriving at the same pilot-side treatment).
  `dev-libs/propertiespkg` (`PROPERTIES="live"`, visible under the real
  default) and `dev-libs/restrictedpkg`/`interactivepkg`
  (`RESTRICT="bindist"`/`PROPERTIES="interactive"`, each individually
  masked once `package.accept_restrict`/`package.properties` narrows
  that one package's own effective accept set with a `-token` layered on
  top of the otherwise-permissive global `"*"` -- a real, meaningful
  per-package narrowing mechanism, not just an additive one) exercise
  the mechanism end to end.

  **`--with-test-deps[=y|n]`**: grounded against real `depgraph.py`'s own
  `_add_pkg`, which additionally pulls in a package's own `test?`-gated
  dependencies -- but only for a top-level atom (`pkg.depth == 0` --
  this pilot's own `depth == 0`, since every depth-0 atom here already
  came from `atoms` itself or a `@world`/`@system` expansion of it, both
  of which real portage also counts as an "argument" for this exact
  purpose), only when its own IUSE declares a `"test"` flag not already
  enabled and not use-masked (global `use_mask` or a matching
  `package_use_mask` entry, mirroring real `"test" not in
  pkg.use.mask"` exactly). The extraction itself -- real `use_reduce(dep_
  string, uselist=use_enabled | {"test"}, ..., subset={"test"})` -- is
  the first real use of `use_reduce`'s own `subset` parameter in this
  pilot, previously an explicit, documented cut on the Rust side
  (`use_reduce_flat`'s own module doc comment) -- the Python side needed
  no new code at all, since it already calls real `portage.dep.
  use_reduce` directly and real `use_reduce` already implements `subset`
  as a genuine two-pass operation (`select_subset` over the *full*
  nested `paren_reduce` structure runs first, *then* `flat=True`'s own
  ordinary reduction on the result) rather than something that composes
  naturally into a single flat pass. The Rust side ports that same
  two-pass shape as `use_reduce_flat_subset` (`portage-use-reduce`): a
  new `DepNode` tree type plus its own `build_dep_tree`/`select_subset`/
  `serialize_dep_tree` pipeline, feeding into the *unmodified*
  `use_reduce_flat` for the final flattening -- verified to agree with
  real `portage.dep.use_reduce(..., subset=...)` directly (not just
  against the Python side's own mirror) on seven hand-picked cases,
  including one with a `test?`-gated alternative nested inside an `||`
  group, before relying on the Rust-side unit tests or the shared
  contract suite. Additive on top of a package's own normal dependency
  walk, never a replacement for it, and queued exactly like any other
  dependency (same blocker extraction, same `depth + 1`) via a small
  shared `enqueue_flat_deps` helper factored out of the pre-existing
  normal-deps queueing so the two code paths can't drift apart. New
  fixture packages: `withtestdeppkg` (`IUSE="test"`,
  `RDEPEND="dev-libs/newpkg test? ( dev-libs/testonlydep )"` --
  `testonlydep` only ever appears with `--with-test-deps` given) and
  `withtestdepconsumer` (`RDEPEND="dev-libs/withtestdeppkg"`, proving the
  depth-0-only gate: even with `--with-test-deps`, resolving
  `withtestdepconsumer` reaches `withtestdeppkg` at depth 1, so
  `testonlydep` still doesn't appear).
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
  involved, and a real-but-unimplemented option like `--jobs` being
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
paragraphs in "What this proves" above; USE deps are parsed and
`atom-harness`/`match_from_list` itself still never enforces them --
matching real `match_from_list`'s own behavior for the same
plain-string candidates this pilot always uses -- but `portage-repo`
now enforces them as a separate post-filter, once it has a real
candidate's own IUSE/effective-USE in hand; see the "USE-dep
enforcement" paragraph in "What this proves" above). Candidates for matching are plain
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
(`dev-libs/newpkg[bar(+)]`; `dev-libs/multislotpkg:1[baz(+)?]`, combined
with a plain slot restriction -- both `(+)`-defaulted so they stay
genuinely satisfied under real USE-dep enforcement too, not just
grammar-parseable; see the "USE-dep enforcement" paragraph above), and
(for profile resolution) a three-level
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

`dev-libs/useflagpkg`'s own already-established `IUSE="foo missingflag"`
(`foo` enabled globally, `missingflag` not) needed no new fixture at all
to exercise USE-dep enforcement as a top-level atom: `[foo]`/`[-missingflag]`
(declared, matching state) resolve normally; `[-foo]`/`[missingflag]`
(declared, opposite state) and `[nonexistentflag]` (undeclared, no
default) all report no visible candidate; `[nonexistentflag(+)]`
(undeclared, `(+)`-defaulted) resolves anyway. `dev-libs/usedeprejectedpkg`
(RDEPEND `dev-libs/useflagpkg[-foo]`, genuinely unsatisfiable since `foo`
is enabled globally) proves the same rejection at the *dependency* level:
the parent still resolves, `useflagpkg` gets its own `NoVisibleCandidate`
entry, reported on stderr, not silently dropped or accepted.

`dev-libs/requireduseokpkg` and `dev-libs/requiredusebadpkg` (identical
`IUSE="foo bar"`/`REQUIRED_USE="foo? ( bar )"`, `foo` enabled globally by
the fixture profile chain either way) exercise REQUIRED_USE: a
`dev-libs/requireduseokpkg bar` entry in
`PORTING/fixtures/etc/portage/package.use` forces `bar` on for the first
package only, genuinely satisfying its own conditional group, while the
second has nothing forcing `bar` on at all, genuinely violating it.
`dev-libs/requiredusebadparentpkg` (RDEPEND
`dev-libs/requiredusebadpkg`) proves the resulting fatal abort applies
the same way when the violation is reached only as a dependency.

`PORTING/fixtures/repo/profiles/base/make.defaults`'s own
`USE_EXPAND="VIDEO_CARDS"`/`VIDEO_CARDS="nvidia"` lines exercise
`USE_EXPAND`: `dev-libs/useexpandpkg` (`IUSE="video_cards_nvidia
video_cards_amdgpu"`) RDEPENDs on `dev-libs/newpkg` only when
`video_cards_nvidia` (the expanded pseudo-flag) is enabled, and on the
never-reached `dev-libs/hiddendep` when `video_cards_amdgpu` (declared
nowhere at all) is -- proving the expansion feeds real dependency
resolution, not just `-v`'s own USE display.

`PORTING/fixtures/etc/portage/package.use`'s own
`dev-libs/packageuseexpandpkg PYTHON_TARGETS: python3_12` entry
exercises `package.use`'s own `USE_EXPAND`-prefix shorthand:
`dev-libs/packageuseexpandpkg` (`IUSE="python_targets_python3_12"`)
RDEPENDs on `dev-libs/newpkg` only once that entry's own shorthand
expands to `python_targets_python3_12`, exactly as if it had been
written out in full.

`dev-libs/stableusepkg` (`KEYWORDS="amd64"`) and `dev-libs/unstableusepkg`
(`KEYWORDS="~amd64"`, visible only via its own
`package.accept_keywords` entry) exercise `use.stable.mask`/`.force`/
`package.use.stable.mask`/`.force`: `PORTING/fixtures/repo/profiles/base/
use.stable.force` (`stableforceflag`) and `PORTING/fixtures/repo/
profiles/package.use.stable.mask` (`dev-libs/stableusepkg maskflag`)
both apply only to the genuinely-stable `stableusepkg` (real
`KeywordsManager.isStable`'s own "would masking every keyword make this
invisible" check) -- `unstableusepkg` shares the identical `IUSE`/
RDEPEND and the identical `package.use`-enabled `maskflag`, but gets
neither the force nor the mask.

`PORTING/fixtures/var/lib/portage/world` (real portage's own `WORLD_FILE`
location, `ROOT`-relative) exercises `@world` expansion: it lists
`dev-libs/newpkg` directly, `dev-libs/withdeps` (which recurses into
`newpkg` again -- deduped -- and `dev-libs/upgradepkg` via its own
RDEPEND, proving `@world`'s expanded atoms feed the same recursion
machinery any other target does), and a `@some-nested-set-reference`
line proving a `@`-prefixed line in the world *file* itself is silently
skipped rather than mishandled (real portage's own `WorldSelectedPackagesSet`
validator would reject it too).

`PORTING/fixtures/var/lib/portage/world_sets` (real `WORLD_SETS_FILE`,
the genuinely separate file real `@world` also unions in) lists
`@nestedtestset`, resolved against
`PORTING/fixtures/etc/portage/sets/nestedtestset` -- a plain atom
(`dev-libs/nestedsetpkg`) plus a further nested `@innernestedset`
reference (`PORTING/fixtures/etc/portage/sets/innernestedset`), which
itself contributes `dev-libs/innernestedsetpkg` and references back to
`@nestedtestset`, exercising the cycle guard (contributes nothing
further, doesn't loop or error).

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
read). The overlay also has its own `profiles/package.mask`/`.unmask`,
exercising overlay repo-level masking: `overlaymaskedpkg` (masked only
by the overlay's own bare-atom `package.mask` entry, auto-scoped to
`::overlay` -- an identically-named main-repo copy stays unaffected) and
`overlaymaskedthenunmaskedpkg` (masked and unmasked by two entries in
that same overlay's own files). The main repo's own
`profiles/package.mask` has two more entries exercising the overlay's
own implicit `masters` inheritance: `mastermaskedpkg` (exists only in
the overlay, masked purely via inheriting the main repo's own
`package.mask` -- the overlay's own file never mentions it) and
`mastermaskedthenoverlayunmaskedpkg` (masked the same inherited way,
then unmasked by the overlay's own `package.unmask`). The main repo's
own `profiles/default/parent` also has a third entry,
`overlay:crossrepo-parent`, real cross-repo profile parent syntax
reaching into `PORTING/fixtures/overlay/profiles/crossrepo-parent/
license_groups` (extending `EULA` with one more member,
`CrossRepoNonfree`, alongside the main repo's own `SomeEula`) --
`dev-libs/crossrepolicensepkg` (`LICENSE="CrossRepoNonfree"`) is masked
by the real default `"* -@EULA"` only once that overlay-level entry
actually joins the active chain.

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

# REQUIRED_USE ("^^ ( a b )", exactly-one-of, with only "a" enabled --
# satisfied): Python then Rust, same output either way
python3 PORTING/python/required_use_harness.py check a a,b "^^" "(" a b ")"
PORTING/rust/target/release/required-use-harness check a a,b "^^" "(" a b ")"
# true
```

Try `emerge --pretend` against the fixture tree:

```sh
ln -sf "$(realpath PORTING/rust/target/release/multicall)" /tmp/emerge
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/newpkg              # -> [ebuild  N] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg # -> [ebuild  U] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/samepkg             # -> already installed

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

# real USE-dep dependency atoms are resolved AND enforced now: both
# "[bar(+)]"/"[baz(+)?]" are (+)-defaulted flags missing from their own
# target's IUSE, so both are genuinely, trivially satisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeppkg
# [ebuild  N] dev-libs/usedeppkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/multislotpkg-2.0

# USE-dep enforcement, top-level: useflagpkg's own IUSE="foo missingflag",
# "foo" enabled globally -- "[foo]" (declared, enabled) is satisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[foo]'
# [ebuild  N] dev-libs/useflagpkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
# "[-foo]" (declared, but enabled, not disabled) is genuinely unsatisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[-foo]'
# emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".  (exit 1)
# a flag not declared in IUSE at all, with no (+)/(-) default, never
# matches -- real _use_dep.required, checked before enabled/disabled at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[nonexistentflag]'
# emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[nonexistentflag]".  (exit 1)
# ...but a "(+)" default rescues a flag missing from IUSE, standing in
# for "as if enabled"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[nonexistentflag(+)]'
# [ebuild  N] dev-libs/useflagpkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# USE-dep enforcement, dependency level: usedeprejectedpkg's own RDEPEND
# is "dev-libs/useflagpkg[-foo]", genuinely unsatisfiable -- the parent
# still resolves, the rejected dependency is reported, not silently
# dropped or accepted (same "report, don't fail" spirit as an
# unresolvable dependency)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeprejectedpkg
# [ebuild  N] dev-libs/usedeprejectedpkg-1.0
# !!! no visible ebuild for dependency "dev-libs/useflagpkg"  (stderr)

# REQUIRED_USE is real and implemented: requireduseokpkg's own
# "foo? ( bar )" is genuinely satisfied (foo enabled globally, bar
# forced on by this package's own package.use entry)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requireduseokpkg
# [ebuild  N] dev-libs/requireduseokpkg-1.0
# requiredusebadpkg has the identical constraint but nothing forcing
# "bar" on -- genuinely violated, which aborts the WHOLE run (exit 1),
# a harsher severity than a merely unresolvable dependency
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadpkg
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"  (exit 1)
# ...and still aborts the whole run even when only reached as a
# dependency, not just as a top-level atom
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadparentpkg
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"  (exit 1)

# real profile/make.conf resolution: "foo" is enabled by the fixture's
# profile chain, so this package's foo?-gated dependency is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# USE_EXPAND is real and implemented: profiles/base/make.defaults'
# VIDEO_CARDS="nvidia" expands into the pseudo-USE flag
# "video_cards_nvidia", which genuinely gates a dependency, not just -v
# display
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/useexpandpkg
# [ebuild  N] dev-libs/useexpandpkg-1.0  USE="-video_cards_amdgpu video_cards_nvidia"
# [ebuild  N] dev-libs/newpkg-1.0

# package.use's own USE_EXPAND-prefix shorthand is real and implemented
# too: "dev-libs/packageuseexpandpkg PYTHON_TARGETS: python3_12" in
# fixtures/etc/portage/package.use expands to
# "python_targets_python3_12", user-level package.use only
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/packageuseexpandpkg
# [ebuild  N] dev-libs/packageuseexpandpkg-1.0  USE="python_targets_python3_12"
# [ebuild  N] dev-libs/newpkg-1.0

# use.stable.force/package.use.stable.mask are real and implemented too:
# stableusepkg's own KEYWORDS="amd64" (no "~") is genuinely stable, so
# both apply -- stableforceflag forced on (pulling in a real dependency)
# and maskflag masked back off despite package.use enabling it first
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/stableusepkg
# [ebuild  N] dev-libs/stableusepkg-1.0  USE="-maskflag stableforceflag"
# [ebuild  N] dev-libs/newpkg-1.0
# unstableusepkg shares the identical IUSE/RDEPEND/package.use entry,
# but its own KEYWORDS="~amd64" is genuinely NOT stable -- neither
# applies: stableforceflag stays off, maskflag stays on
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/unstableusepkg
# [ebuild  N] dev-libs/unstableusepkg-1.0  USE="maskflag -stableforceflag"

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

# package.accept_keywords negation ("-amd64") revokes a keyword the
# global ACCEPT_KEYWORDS="amd64" already granted -- for this one
# genuinely stable package specifically
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/keywordrevokedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/keywordrevokedpkg".  (exit 1)

# package.accept_keywords "*"/"~*" wildcards: accept any stable/testing
# keyword respectively, distinct from "**" -- "*" alone would NOT have
# covered the second package below, since it's testing-only (~arm64)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/starkeywordpkg
# [ebuild  N] dev-libs/starkeywordpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/tildestarkeywordpkg
# [ebuild  N] dev-libs/tildestarkeywordpkg-1.0

# package.accept_keywords bare atom: no keyword tokens at all, real
# accept_keywords_defaults still grants an implicit "~amd64" (global
# ACCEPT_KEYWORDS="amd64", "~"-prefixed) -- not a no-op
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/bareacceptkeywordspkg
# [ebuild  N] dev-libs/bareacceptkeywordspkg-1.0

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

# overlay repos' own package.mask: overlaymaskedpkg is masked only in the
# overlay's own profiles/package.mask (a bare atom, auto-scoped to
# "::overlay" by real append_repo) -- an unconstrained atom still
# resolves via the main repo's own, unaffected copy
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedpkg
# [ebuild  N] dev-libs/overlaymaskedpkg-1.0

# an explicit "::overlay" atom does hit that same auto-scoped mask
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedpkg::overlay
# emerge: there are no ebuilds to satisfy "dev-libs/overlaymaskedpkg::overlay".  (exit 1)

# the overlay's own package.unmask cancels that same overlay's own
# package.mask entry (both get the identical "::overlay" auto-scoping)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedthenunmaskedpkg
# [ebuild  N] dev-libs/overlaymaskedthenunmaskedpkg-1.0

# repos.conf masters: the overlay has no explicit "masters =", so it
# implicitly masters the main repo -- mastermaskedpkg exists only in the
# overlay and is masked purely by the MAIN repo's own package.mask
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/mastermaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/mastermaskedpkg".  (exit 1)

# the overlay's own package.unmask still cancels a masters-inherited
# mask, since both get the identical "::overlay" auto-scoping
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/mastermaskedthenoverlayunmaskedpkg
# [ebuild  N] dev-libs/mastermaskedthenoverlayunmaskedpkg-1.0

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

# @world expands in place to the union of the fixture world file's own
# atoms (newpkg directly, withdeps -- which recurses into newpkg again,
# deduped, and upgradepkg -- and a "@some-nested-set-reference" line
# that's silently skipped there, not mishandled, since a "@"-prefixed
# line genuinely fails real portage's own world-FILE validation too) and
# the fixture world_sets file's own "@nestedtestset" (nestedsetpkg
# directly, plus a further nested "@innernestedset" reference --
# innernestedsetpkg, which itself cycles back to "@nestedtestset" without
# looping). --update is added here purely so upgradepkg actually
# upgrades (see the --update example further below) rather than staying
# silently already-installed -- unrelated to @world itself
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update @world
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/withdeps-1.0
# [ebuild  N] dev-libs/nestedsetpkg-1.0
# [ebuild  N] dev-libs/innernestedsetpkg-1.0
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)

# @world combines with an explicit atom in the same invocation
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/samepkg @world
# dev-libs/samepkg-1.0 is already installed; nothing to do
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/withdeps-1.0
# [ebuild  N] dev-libs/nestedsetpkg-1.0
# [ebuild  N] dev-libs/innernestedsetpkg-1.0
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)

# an unresolvable "@name" listed in world_sets is a real, immediate
# error (real PackageSetNotFound) -- unlike a missing world/world_sets
# *file* itself (a real, valid "nothing selected" state)
mkdir -p /tmp/badset-root/var/lib/portage
echo '@doesnotexist' > /tmp/badset-root/var/lib/portage/world_sets
PORTAGE_CONFIGROOT="$FX" ROOT="/tmp/badset-root" /tmp/emerge --pretend @world
# emerge: set 'doesnotexist' not found  (exit 1)

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
# deduped, and upgradepkg). --update again just so upgradepkg upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update @system
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

# --update/-u is real and implemented: without it, real emerge does NOT
# offer to upgrade a package just because a newer version exists --
# upgradepkg is installed at 1.0, a newer 2.0 is visible in the tree, but
# plain "emerge dev-libs/upgradepkg" leaves it alone (real depgraph.py's
# own avoid_update, lines 7814/8448)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/upgradepkg
# dev-libs/upgradepkg-1.0 is already installed; nothing to do
# --update (or its short alias -u) is what makes the newer version show up
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)
# --update threads through the whole dependency graph, not just a
# top-level atom: here upgradepkg is reached only as withdeps' own
# dependency, and still upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/withdeps
# [ebuild  N] dev-libs/withdeps-1.0
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)

# --deep/-D is real and implemented: without it, real emerge never walks
# an already-installed package's own further dependencies, no matter how
# deep the graph goes -- deeppkg is installed and RDEPENDs on deeppkg2
# (also installed), which itself RDEPENDs on newpkg (New), but neither
# ever shows up here
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# a bare --deep (unlimited depth) walks the whole already-installed
# chain -- deeppkg2 itself stays silent (already installed, not a
# top-level atom), but newpkg's own [ebuild N] line now appears
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --deep dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# [ebuild  N] dev-libs/newpkg-1.0
# --deep=N bounds the depth: 1 level reaches deeppkg2 but not newpkg
# (identical output to no --deep at all); 2 levels reaches all the way
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --deep=1 dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --deep=2 dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# [ebuild  N] dev-libs/newpkg-1.0

# --exclude/-X is real and implemented: without it, --update offers the
# visible upgrade normally
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)
# --exclude matching the installed package overrides --update entirely --
# it's checked first, unconditionally, before --update/--newuse/
# --changed-use ever get a say
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --exclude dev-libs/upgradepkg dev-libs/upgradepkg
# dev-libs/upgradepkg-1.0 is already installed; nothing to do
# excluding a package that isn't installed at all means there's no
# eligible candidate left -- the same fatal "no ebuilds to satisfy"
# outcome any other unsatisfiable top-level atom already gets
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --exclude dev-libs/newpkg dev-libs/newpkg
# emerge: there are no ebuilds to satisfy "dev-libs/newpkg".  (exit 1)
# real "action": "append" -- repeatable, and each occurrence's own value
# is itself a space-separated atom list, so both accumulate
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --exclude "dev-libs/does-not-exist dev-libs/upgradepkg" dev-libs/upgradepkg
# dev-libs/upgradepkg-1.0 is already installed; nothing to do

# --json is pilot-specific (NOT a real emerge option): the whole
# resolved graph as one line of JSON instead of the plain-text lines
# above, including "requested" and "required_by" -- two fields no
# plain-text line has ever carried
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/newpkg
# {"entries":[{"category":"dev-libs","package":"newpkg","outcome":"new","version":"1.0","slot":"0","source":"ebuild","requested":true,"required_by":[],"blockers":[]}],"slot_conflicts":[]}
# dev-libs/common is a diamond dependency (both shared-a and shared-b
# RDEPEND on it) -- required_by lists both owners, sorted
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/diamond | python3 -c 'import json,sys; print(next(e["required_by"] for e in json.load(sys.stdin)["entries"] if e["package"] == "common"))'
# [{'category': 'dev-libs', 'package': 'shared-a'}, {'category': 'dev-libs', 'package': 'shared-b'}]

# LICENSE/ACCEPT_LICENSE/package.license masking (PMS 7.3.2) is real and
# implemented: neither the fixture profile chain nor make.conf sets
# ACCEPT_LICENSE at all, so real portage's own "* -@EULA" default
# applies -- profiles/base/license_groups defines EULA="SomeEula", so
# this package's own LICENSE="SomeEula" is masked
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/eulapkg
# emerge: there are no ebuilds to satisfy "dev-libs/eulapkg".  (exit 1)
# a || any-of LICENSE group is visible via any one accepted alternative
# -- GPL-2 is accepted by the real default's own "*" token
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/anyoflicensepkg
# [ebuild  N] dev-libs/anyoflicensepkg-1.0
# package.license unmasks an otherwise EULA-masked package for that one
# package specifically (etc/portage/package.license accepts SomeEula
# just for this atom)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packagelicensepkg
# [ebuild  N] dev-libs/packagelicensepkg-1.0

# cross-repo profile parents: the main repo's own profiles/default/parent
# names "overlay:crossrepo-parent" -- that overlay directory's own
# license_groups extends EULA with "CrossRepoNonfree", so this package
# is masked only once the overlay-level entry actually joins the chain
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/crossrepolicensepkg
# emerge: there are no ebuilds to satisfy "dev-libs/crossrepolicensepkg".  (exit 1)

# a USE-conditional LICENSE is visible with the flag off (the default);
# its sibling has package.use forcing the same flag on, activating the
# conditional and masking it
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/uselicensepkg
# [ebuild  N] dev-libs/uselicensepkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/uselicensepkgforced
# emerge: there are no ebuilds to satisfy "dev-libs/uselicensepkgforced".  (exit 1)

# PROPERTIES/ACCEPT_PROPERTIES/package.properties and RESTRICT/
# ACCEPT_RESTRICT/package.accept_restrict masking are real and
# implemented: real portage's own default (from cnf/make.globals) is
# "*", accepting everything, so a plain declared PROPERTIES is visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/propertiespkg
# [ebuild  N] dev-libs/propertiespkg-1.0
# package.properties/package.accept_restrict can still narrow
# acceptance for one specific package via a "-token", even under the
# otherwise-permissive global "*" default
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/interactivepkg
# emerge: there are no ebuilds to satisfy "dev-libs/interactivepkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/restrictedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/restrictedpkg".  (exit 1)

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
# withdeps' own [ebuild N] line is suppressed. --update is added again
# just so upgradepkg's own dependency-level entry actually upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --onlydeps dev-libs/withdeps
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
# --changed-use/-U, --nodeps/-O, --onlydeps/-o, --update/-u, --deep/-D,
# --exclude/-X, --deselect/-W, --with-bdeps, --changed-deps,
# --changed-slot, and --help/-h are implemented so far; see PROMPT.md)
# (exit 2)

# --help/-h is real and implemented: a short, honest, pilot-specific
# summary, not a port of real emerge's own (157-line, colorized,
# ~130-flag) help text -- wins unconditionally, regardless of position
# or what else accompanies it
/tmp/emerge --help
# emerge (pilot v1): command-line interface to the Rust porting pilot
# ...
# See PORTING/README.md and PORTING/PROMPT.md for this pilot's current scope.
/tmp/emerge --jobs --help          # --help wins even combined with other flags
/tmp/emerge -ph                    # ...and even bundled with other short flags

# CLI surface recognition: a real emerge option this pilot doesn't
# implement is named specifically, not lumped in with a typo
/tmp/emerge --jobs dev-libs/newpkg
# emerge (pilot v1): option "--jobs" is a real emerge option, but is not
# implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N,
# --changed-use/-U, --nodeps/-O, --onlydeps/-o, --update/-u, --deep/-D,
# --exclude/-X, --deselect/-W, --with-bdeps, --changed-deps,
# --changed-slot, and --help/-h are implemented so far; see PROMPT.md)
# (exit 2)

# a token that isn't a real emerge option/action at all gets a
# different message
/tmp/emerge --totally-fake-option dev-libs/newpkg
# emerge: unrecognized option "--totally-fake-option"

# or against the Python reference implementation directly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    python3 PORTING/python/emerge_pretend_reference.py --pretend dev-libs/newpkg

# --deselect/-W is a standalone action, not a --pretend modifier -- it
# needs no repos.conf/profile at all, only ROOT's own world file and
# vdb, so this uses a small throwaway ROOT instead of $FX
mkdir -p /tmp/deselect-demo-root/var/lib/portage /tmp/deselect-demo-root/var/db/pkg/dev-libs/foo-1.0
echo "dev-libs/foo" > /tmp/deselect-demo-root/var/lib/portage/world
echo "dev-libs" > /tmp/deselect-demo-root/var/db/pkg/dev-libs/foo-1.0/CATEGORY
echo "0" > /tmp/deselect-demo-root/var/db/pkg/dev-libs/foo-1.0/SLOT
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/foo
# >>> Would remove dev-libs/foo from "world" favorites file...

# a target that isn't actually installed (or isn't in the world file at
# all) never becomes an expanded atom, so nothing is reported for it
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/bar
# >>> No matching atoms found in "world" favorites file...

# --with-bdeps: withbdepspkg is already installed, DEPENDs on
# builddeponlypkg, BDEPENDs on hostdeponlypkg, RDEPENDs on newpkg --
# --deep's default (--with-bdeps=y/auto) walks all three
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --deep dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N] dev-libs/builddeponlypkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/hostdeponlypkg-1.0

# --with-bdeps=n: DEPEND/BDEPEND are skipped, but RDEPEND is unaffected
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --deep --with-bdeps n dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N] dev-libs/newpkg-1.0

# --changed-deps: changeddepspkg's own vdb-recorded RDEPEND (samepkg)
# differs from its current ebuild's own RDEPEND (newpkg) -- reinstalls
# and recurses into the CURRENT ebuild's own dependency, not the vdb's
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/changeddepspkg
# [ebuild  r] dev-libs/changeddepspkg-1.0 (reinstall for changed dependencies)
# [ebuild  N] dev-libs/newpkg-1.0

# --changed-deps ignores a libc-only dependency change (strip_libc_deps):
# libcnoisepkg's own vdb RDEPEND names sys-libs/glibc, its current
# ebuild names sys-libs/musl -- both are real virtual/libc providers per
# the fixture vdb's own virtual/libc entry, so no reinstall fires
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/libcnoisepkg
# dev-libs/libcnoisepkg-1.0 is already installed; nothing to do

# --changed-slot: changedslotpkg's own vdb-recorded SLOT ("0") differs
# from its current ebuild's own SLOT ("0/2", an ABI-bump sub-slot change)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-slot dev-libs/changedslotpkg
# [ebuild  r] dev-libs/changedslotpkg-1.0 (reinstall for changed slot)
# [ebuild  N] dev-libs/newpkg-1.0

# --changed-deps/--changed-slot are independent, freely-combinable
# reinstall triggers -- changedslotpkg's own vdb RDEPEND is *also* stale,
# so giving both prints both reasons on the same line
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps --changed-slot dev-libs/changedslotpkg
# [ebuild  r] dev-libs/changedslotpkg-1.0 (reinstall for changed dependencies; changed slot)
# [ebuild  N] dev-libs/newpkg-1.0

# --with-test-deps: withtestdeppkg's own RDEPEND is "dev-libs/newpkg
# test? ( dev-libs/testonlydep )" -- without the flag, only the
# unconditional dev-libs/newpkg is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/withtestdeppkg
# [ebuild  N] dev-libs/withtestdeppkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# --with-test-deps additionally pulls in the test?-gated dep too
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --with-test-deps dev-libs/withtestdeppkg
# [ebuild  N] dev-libs/withtestdeppkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
# [ebuild  N] dev-libs/testonlydep-1.0

# ...but only for a top-level (depth 0) atom -- withtestdepconsumer's own
# RDEPEND reaches withtestdeppkg at depth 1, so testonlydep stays absent
# even with --with-test-deps given
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --with-test-deps dev-libs/withtestdepconsumer
# [ebuild  N] dev-libs/withtestdepconsumer-1.0
# [ebuild  N] dev-libs/withtestdeppkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0
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

# --help/-h are real and implemented too, winning unconditionally
# regardless of position or what else accompanies them
/tmp/ebuild --help
# ebuild (pilot stub): command-line interface to the Rust porting pilot
# ...
/tmp/ebuild --not-a-real-option -h
# (same help text -- wins even alongside an otherwise-invalid option)

# --version is deliberately NOT specially implemented (see
# ebuild_options.rs's own doc comment: real portage.VERSION is derived
# live via "git describe" for a from-source checkout, not a static
# string) -- it's still a real, recognized option though, just a no-op
# like the other five, unlike emerge's own CLI philosophy of rejecting
# every merely-recognized-but-unimplemented flag by name
/tmp/ebuild --version foo-1.0.ebuild merge
# ebuild (pilot stub): dry-run only, no phase execution yet ...
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
