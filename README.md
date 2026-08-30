# Porting pilot

This started as the "Suggested first execution step" pilot from
[`PROMPT.md`](PROMPT.md): a small, complete run of the whole pipeline (Rust
port, Python harness, shared black-box contract suite, portuale dispatch
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
    portuale/                  the real emerge/ebuild dispatch binary; `emerge`
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
    test_portuale.py            tests the compiled dispatch binary via symlinks
    test_benchmark_gate.py      opt-in wrapper around run_benchmark.py for CI
    test_musl_smoke.py          opt-in wrapper around musl/smoke_test.sh for CI
```

## What this proves

- **`versions-harness`**: a faithful Rust port of `vercmp`/`ververify`,
  checked against the real Python implementation through a neutral CLI
  contract (not a product CLI, not FFI/PyO3 bindings) -- see `PROMPT.md`
  hard goal 4 and the "black-box via CLI/API" decision.
- **`portuale`**: proves the `argv[0]`-based dispatch mechanism for
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
  follow-up that closed most of this gap, and the later **`opt=`/`opt?`
  conditional USE-deps** paragraph for the one named here as a "wholly
  different mechanism" (conditional on the *atom-owning* package's own
  USE state, not just the candidate's) -- closed too, once this pilot's
  own dependency recursion had a natural place to plug that state in.

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
  (`[ebuild  N    ] cat/pkg-1.2.3`, `[ebuild     U ] cat/pkg-2.0 [1.0]`,
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

  **`||` (any-of) groups, real semantics.** The paragraph above's own
  "already resolves every alternative of an any-of group" is no longer
  true -- superseded by this later slice. The groundwork already
  existed: `use_reduce_flat_subset` (the `--with-test-deps` follow-up)
  already needed a private `DepNode`/`build_dep_tree` tree preserving
  `||`-group boundaries, just not exposed for regular dependency
  processing. New `use_reduce_flat_disjunctive` (`portage-use-reduce`)
  reuses that same tree, adding a `resolve_disjunctions` walk: for every
  `"||"` group, it picks the first alternative every one of whose own
  atoms a caller-supplied satisfiability closure accepts -- a single
  atom, a bracketed multi-atom group (all must be satisfiable together),
  or a conditional (`flag?`) used directly as an alternative (a real,
  valid dependency-specification shape per PMS, resolved to vacuous
  truth when the flag is off, since an alternative requiring nothing is
  trivially satisfiable). Falls back to keeping every alternative --
  `use_reduce_flat`'s own original behavior, literal `"||"` marker
  included -- whenever *none* is currently satisfiable, so the "never
  silently wrong about whether a dependency exists" invariant the
  original v1 established still holds; nothing regresses for a
  dependency this pilot genuinely can't resolve either way. This crate
  stays atom-agnostic throughout (matching its own established "tokens
  stay opaque strings" architecture) -- `portage-repo`'s own new
  `atom_currently_satisfiable` (the *early* half of `resolve_pretend`'s
  own logic only: `list_candidates` -> filter `is_visible` ->
  `match_from_list` -> USE-dep post-filter, deliberately not the full
  `--update`/`--newuse`/`--exclude`/reinstall-aware function itself,
  since those refinements only matter once an alternative has already
  been chosen) supplies the actual visibility-checking closure, wired
  in at the two real dependency-enqueueing call sites (the main BFS
  loop and `--deep`'s own `AlreadyInstalled` walk -- the other three
  `use_reduce_flat` call sites in `portage-repo`, for LICENSE/
  PROPERTIES/RESTRICT acceptance and `--changed-deps`'s own order-
  independent set comparison, are deliberately untouched: none of them
  is a "pick one alternative to enqueue" decision). Real portage's own
  considerably richer preference order (installed packages first,
  backtracking on a later constraint failure) isn't ported -- this
  pilot has no backtracking architecture at all -- just the single
  "first currently-resolvable alternative wins" rule. `virtual/
  texteditor`'s own RDEPEND (`"|| ( dev-libs/newpkg dev-libs/samepkg )"`)
  now correctly enqueues only `dev-libs/newpkg` (listed first, visible)
  -- `dev-libs/samepkg` (already installed, also satisfiable, but never
  reached) doesn't show up in the graph at all, though `--pretend`'s own
  stdout looked identical either way (an `AlreadyInstalled` dependency
  never prints under plain `--pretend`). New fixture
  `dev-libs/anyofunresolvable` (`"|| ( dev-libs/doesnotexist-anywhere
  dev-libs/alsodoesnotexist-anywhere )"`, neither alternative visible
  anywhere) proves the fallback: both still get reported on stderr,
  neither silently dropped just because they're inside an unresolvable
  `||` group.

  **CLI surface recognition**: `portuale/src/emerge_options.rs`
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
  `ebuild`'s much smaller real surface -- `portuale/src/ebuild_options.rs`
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
  `PORTING/tests/test_portuale.py`'s own dispatch-proof tests (`ebuild
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
  `test_portuale.py`'s black-box tests against the real compiled binary
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
  "world" favorites file...` if none matched at all. (At the time of
  this original slice, `pretend.rs`'s own `run_deselect` used a narrower
  category/package(+slot) equality check versus real `Atom.intersects()`
  as a documented scope cut, and additionally required every target to
  be actually installed; both were closed by a later follow-up below --
  the equality check replaced with a real, field-for-field
  `Atom.intersects()` port, and the installed-check requirement removed
  entirely as a genuine correctness fix, not a scope change.) A
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

  **`emerge --pretend --unmerge` / `-pC <atoms>`: the second real emerge
  action.** Real `main.py` makes `--unmerge`/`-C` a standalone action
  (`myaction = "unmerge"`, same shape as `--deselect`), and real `emerge
  -C` *removes* packages -- this pilot only ever *previews* it (`ebuild
  <file> unmerge` stays its one real removal path), same `--pretend`-only
  gate `--deselect` has. New `run_unmerge_pretend` ports
  `_emerge/unmerge.py::_unmerge_display` for `unmerge_action ==
  "unmerge"`: each target atom is matched against the vdb
  (`installed_candidates` + `match_from_list`, exactly real
  `vartree.dbapi.match`); every match becomes `selected`, every *other*
  installed version of the same `category/package` becomes `omitted`
  (real `vartree.dep_match(cp)` minus selected/protected). `sys-apps/
  portage` itself is force-`protected` with real portage's own "no valid
  reason for Portage to unmerge itself" note (`PORTAGE_PACKAGE_ATOM`). A
  bare name resolves via a real "null-category" vdb scan (ambiguity ->
  the real `AmbiguousPackageName` error); `@world`/`@system`/`@customset`
  targets expand first, via the same set machinery `run()`/`run_deselect`
  already use. The per-cp `selected:`/`protected:`/`omitted:` block
  (label `rjust(14)`, trailing spaces and all), the `All selected
  packages: =cat/pkg-1.0 …` line, and the `>>> 'Selected' …` / `>>>
  'Protected' …` footer are reproduced faithfully; `portage-versions`
  (already transitive) becomes a direct `portuale` dep for the real
  `cpv_sort_key` version ordering. **Documented cuts, a clean
  follow-up:** the `--prune`/`--depclean` variants (best-version pruning
  / reverse-reachability -- since shipped, see below), a bare
  `=<vdb-path>` argument (since shipped -- see "`-pC`: a literal vdb path"
  below), the "currently used Python interpreter" self-skip, and any real
  removal. New fixtures: `dev-libs/unmergepkg` (installed at 1.0 *and*
  2.0), `sys-apps/portage-1.0`.

  **`-pC`: the two `_unmerge_display` warnings.** Completes
  `_unmerge_display` for `unmerge_action == "unmerge"`. (1) `!!! 'cp' is
  part of your system profile. / !!! Unmerging it may be damaging to
  your system.` (to stderr) when a cp that would be *fully* removed
  (nothing `protected`/`omitted`) is a `@system` member -- real `if not
  (protected or omitted) and cp in syslist`, `syslist` built from
  `config.system_packages`. (2) `Package cat/pkg-ver is going to be
  unmerged, / but still listed in the following package sets: @foo` (to
  stdout) when a `selected` package is still in a user-editable set
  reached via `world_sets` -- new `collect_installed_sets` (real
  `_unmerge_display`'s own `installed_sets`, a BFS over the `@`-refs
  keeping each set's *direct* atoms), minus the sets the user is
  `-C`-targeting (real `setconfig.active`). The higher-slot refinement
  (real `unmerge.py:421-441`'s `higher_slot`: (2) is suppressed for a
  set when an installed *newer* version of the same cp *in a different
  slot* also matches the set atom -- removing this version leaves the
  set satisfied) **shipped 2026-08-30** -- see "`emerge -pC`/`-pP`: the
  higher-slot set-protection refinement" below. Still narrowed: a
  referenced-but-missing set is dropped silently here (real `eerror`s
  "Unknown set"). New fixtures: `dev-libs/systempkg` (a `*`-prefixed
  `@system` atom in `profiles/base/packages`, installed);
  `dev-libs/nestedsetpkg` installed (it's in `@nestedtestset`, which
  `world_sets` selects). ~4 pinned `@world`/`@system` tests gained an
  "already installed" line for the two now-installed set members.

  **`-pC`: a literal vdb path.** Real `unmerge.py:137-182`: an
  `--unmerge`/`-C` argument that starts with `.` or `/`, or ends with
  `.ebuild`, is treated as a path into the vdb rather than an atom --
  `emerge -C /var/db/pkg/dev-libs/foo-1.0` (or `.../foo-1.0/foo-1.0.ebuild`).
  New `resolve_vdb_path_arg` (`pretend.rs`, mirrored in
  `emerge_pretend_reference.py`), wired into `run_unmerge_pretend`'s own
  target-expansion loop: the path must exist (`!!! The path '…' doesn't
  exist.`), a `.ebuild` suffix is stripped, the directory must have a
  `CONTENTS` file (`!!! Not a valid db dir: …`) and sit inside
  `<ROOT>/var/db/pkg` (`!!! … is not inside …; aborting.`), and the
  `category/pkg-ver` tail is echoed and selected as `=category/pkg-ver`
  -- exactly real portage's own `print("=" + …)`. Only for
  `--unmerge`/`-C` (real `--prune`/`--clean` reject an ebuild path with
  a different message; `--depclean`/`--prune` here never see a path,
  they feed `=cpv` atoms). The path is resolved with `realpath`
  (`canonicalize`), not `os.path.abspath` -- it follows symlinks, but
  resolves the vdb root the same way, which is what keeps a symlinked
  test `ROOT` working. Real portage's stray `print(sp_absx)` /
  `print(absx)` debug lines (a raw list repr) before the "not inside"
  message are deliberately omitted. New `_vdb_path_root` test fixture.

  **`emerge --pretend --depclean` / `-pc`: the third real emerge action
  (core increment).** Real `action_depclean` + `_calc_depclean` (no
  package arguments): everything nothing in `@world` ∪ `@system` needs,
  at runtime, is the removal list -- reported, never removed
  (`--pretend`-only, same stance as `--unmerge`). New
  `portage_repo::depclean_cleanlist` builds the *installed* dependency
  graph (node = installed package; edge A -> B when B satisfies one of
  A's own vdb `RDEPEND`/`PDEPEND` atoms, flattened against A's own vdb
  `USE` via `flat_dep_atoms`, every `||` branch kept -- the conservative
  choice for a removal decision), roots = installed packages the
  `@world`+`@system` atoms match, and everything unreachable is the
  cleanlist (`+ new all_installed_packages` = real `vardb.cpv_all()`).
  `portuale::run_depclean_pretend` prints real `action_depclean`'s own
  `* ` advisory block, then -- since real `action_depclean` literally
  calls `unmerge(..., "unmerge", cleanlist)` -- feeds each cleanlist cpv
  straight into `run_unmerge_pretend` as an `=cat/pkg-ver` atom (so the
  `sys-apps/portage` skip, set-protection, and system-profile warnings
  all apply to the cleanlist too), with `>>> Calculating removal
  order...` ahead of it and the `Packages installed:` / `in world:` /
  `in system:` / `Required packages:` / `Number to remove:` stats block
  after. **Documented narrowings, this being a first increment** (real
  `_calc_depclean` runs the full backtracking `depgraph` in "remove"
  mode): build-time deps (`DEPEND`/`BDEPEND`, real `bdeps="auto"`) aren't
  followed **[since shipped -- see "`emerge -pc`: build-time deps are
  kept too" below]**;
  `--depclean-lib-check` (a `NEEDED.ELF.2` soname-linkage check) **[since
  shipped -- see "`emerge -pc` / `-pP`: the `--depclean-lib-check`
  soname-consumer scan" below]**, slot-operator rebuild edges, the
  "dependencies could not be resolved, aborting" safety halt **[since
  shipped -- see "`emerge -pc` / `-pP`: the "dependencies could not be
  resolved" safety halt" below]**, and `package.provided` **[since
  shipped -- see "`package.provided`" below]** are all deferred. Tests
  use a self-contained `_depclean_root` (isolated vdb + world file, like
  the `--deselect` tests) so nothing touches the shared fixture tree.

  **`emerge -pc <atoms>`: the `--depclean <atoms>` narrowing.** Traced
  through real `_calc_depclean` + `_complete_graph` in "remove" mode: in
  args mode the world "selected" plain atoms are dropped (the default
  `--deselect` -- `emerge -pc dev-lang/python` removes python *and*
  deselects it) and every installed package NOT matching an args atom
  becomes a protected root, so the cleanlist is just the args-matched
  packages nothing else installed (or `@system`) needs at runtime.
  `depclean_cleanlist` gained an `args` parameter (and `world_atoms` /
  `system_atoms` split out, since world roots drop in args mode);
  `run_depclean_pretend` resolves bare-name args via the vdb
  null-category scan (ambiguity -> real `AmbiguousPackageName`), prints
  `--- Couldn't find 'X' to depclean.` (stderr) for an args atom
  matching nothing and `>>> No packages selected` + exit 1 when none
  match, and skips the `* ` advisory block (real portage only shows it
  with no args). `--deselect=n` (keeps the world atoms as roots even in
  args mode) **shipped 2026-08-30** -- see "`emerge -pc <atoms>
  --deselect=n`" below. **Still deferred:** `world_sets` `@`-refs as
  roots in args mode.

  **`emerge -pc`: build-time deps are kept too (`bdeps="auto"`).** The
  first `--depclean` increment above deliberately walked only the
  runtime keys (`RDEPEND`/`PDEPEND`). But real `_calc_depclean` builds
  its graph through the full `depgraph` in "remove" mode, and
  `create_depgraph_params(myopts, "remove")` sets `bdeps="auto"`
  (`create_depgraph_params.py:100-103`); `depgraph.py:4208-4213` only
  discards `DEPEND`/`BDEPEND` from a removal walk when `--with-bdeps=n`
  is passed explicitly, walking them against the root being cleaned
  (`depend_root = myroot`, `:4218-4219`) otherwise. So a package that is
  *only* a build-time dependency of a kept package is itself kept --
  `emerge --depclean` will not remove something the installed tree still
  needs in order to rebuild what stays. `depclean_cleanlist`'s
  reachability walk (both sides) now follows `["RDEPEND", "PDEPEND",
  "DEPEND", "BDEPEND"]`, reading the build-time keys from the same
  `<root>` vdb as the runtime ones. The `_depclean_root` fixture gained
  `dcworld DEPEND=dev-libs/dcbuilddep` and `dcdep BDEPEND=dev-libs/dcbdep`
  (nothing `RDEPEND`s either) -- both are now kept, not cleaned, and
  `emerge -pc dev-libs/dcbuilddep` reports nothing to remove. The
  `Packages installed:` / `Required packages:` counts in the pinned
  no-args test moved from 7/5 to 9/7 accordingly. **Still deferred**
  (unchanged): slot-operator rebuild edges. (`--depclean-lib-check`,
  `package.provided`, and the "dependencies could not be resolved" safety
  halt have since shipped -- see their own sections below.)

  **`emerge -pc`: `>>> Calculating removal order...` is real now.** The
  first two `--depclean` increments printed that line but the cleanlist
  itself was always `cat`/`pn`/version-sorted, i.e. real
  `_unmerge_display`'s own `ordered=False` branch
  (`unmerge.py:459-474`). Real `_calc_depclean` (`actions.py:1591-1731`)
  builds a digraph over the cleanlist -- an edge `depender -> dep`
  whenever one member satisfies another's
  `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/`IDEPEND` (flattened against the
  depender's own vdb `USE`) -- and topologically sorts it so each
  package is unmerged *before* the ones it depends on, "to avoid
  breaking things that may need to run during pkg_prerm or pkg_postrm".
  Only when there are no edges at all does it fall back to the `cat`/`pn`
  grouping. New `portage_repo::topological_removal_order`: real portage's
  own repeated-root-node pop (every current root emitted at once, cpv
  descending -- `nodes.sort(reverse=True)`), returning `(ordered,
  cleanlist)`. `DepcleanResult` gained an `ordered` field;
  `run_unmerge_pretend` gained a `preserve_order` parameter (real
  `_unmerge_display`'s `ordered` flag) that skips the `cat`/`pn`
  re-sort, set only by `run_depclean_pretend`. New fixture
  `_depclean_order_root` (`mmid` -> `zztop` -> `aabase`, all orphan)
  proves the blocks come out `[mmid, zztop, aabase]`, the reverse of
  alphabetical. The `All selected packages:` line stays sorted in both
  implementations (real portage iterates a `set` there -- not a
  meaningful order to reproduce). **Deliberately out of scope**: the
  slot-operator-built-dep priority bump (bug 916135's `dev-libs/B:0/0=`)
  and the priority-ignoring single-node pop that breaks a genuine
  dependency cycle -- a cleanlist that still holds a cycle here is
  emitted last, in cpv order.

  **`emerge -pc --verbose`: the reverse-dependency display.** Real
  `create_cleanlist` (`actions.py:1324`/`1331`) calls `show_parents(pkg)`
  for every *kept* installed package under `--verbose` (no-args: all of
  them; args: only the `args`-matched ones), cpv-sorted, printing
  `  <cpv> pulled in by:` followed by a `    <parent> requires <atom>,
  <atom>` line per parent -- `<parent>` is a cpv (a `Package` parent) or
  an `@set` label (a `SetArg` parent), lines sorted ascending, atoms
  within a line sorted by atom package-name descending
  (`operator.attrgetter("package")`, `reverse=True`). `--verbose` also
  suppresses the `>>> To see reverse dependencies, use --verbose` hint.
  `depclean_cleanlist` now records real
  `_dynamic_config._parent_atoms` during its BFS -- every dep that
  resolves to an installed package adds a `(parent descriptor, atom)`
  edge -- and `DepcleanResult` gained a `kept_parents` field with the
  already-rendered, already-sorted lines. Seeds are labelled: a `world`
  file line's parent is `@selected`, a `world_sets` nested set's is
  `@<name>` (real `_expand_set_args` nesting -- `run_depclean_pretend`
  passes `(atom, label)` pairs now instead of a flat atom list). The
  args-mode protected-set seeds (real `protected_set_name`) record no
  edge, matching `show_parents`'s own filter. `run_depclean_pretend`
  prints the blocks right after the `* ` advisory and before `>>>
  Calculating removal order...` / the empty-cleanlist message. New
  `_depclean_revdep_root` fixture (a shared dep `dcshared` pulled in by
  two parents). **Deliberately out**: the exact `@selected`-vs-`@world`
  set nesting real portage's `_complete_graph` produces (approximated --
  world-file members are `@selected` here, not `@world`).

  **`emerge --prune --verbose`: `show_parents` for prune.** Real
  `create_cleanlist`'s prune branch (`actions.py:1339`) also calls
  `show_parents(pkg)` under `--verbose` -- but only for an
  `args_set`-matched *kept* version (`for atom in args_set: for pkg in
  vardb.match_pkgs(atom): ... elif "--verbose": show_parents(pkg)`), and
  `show_parents` itself filters out the internal protected-set parent,
  so a highest version pulled in only by the bare-`cp` seed contributes
  nothing. `prune_cleanlist` now records the same `_parent_atoms`
  dep-walk edges `depclean_cleanlist` does and fills `kept_parents` for
  every reachable `matched_by_args` version with a real `Package`
  parent; the `(parent, atom)` -> line rendering is a shared
  `render_show_parents` helper. `run_prune_pretend` gained a `verbose`
  parameter and prints the blocks before `>>> Calculating removal
  order...` / the empty message, and suppresses the `>>> To see reverse
  dependencies` hint. In `_prune_root` only `dev-libs/mm-2.0` (kept by
  `keeper`'s `=dev-libs/mm-2.0` pin) gets a block; `mm-3.0`/`aa-2.0`/
  `zz-2.0` (highest, protected-set-only) get none.

  **`emerge -p --prune` / `-pP`: the fourth real cleanup action.** Real
  modern `--prune` (without `--nodeps`) routes through the same
  `action_depclean` as `--depclean`, with `action="prune"`
  (`actions.py:1059-1110` + `create_cleanlist`'s own prune branch,
  `:1334-1340`). It removes *superseded* installed versions: for every
  cp with more than one version installed, the non-highest ones, kept
  only if something still needs that exact old version. Real portage
  seeds `protected_set` with every installed cp as a bare `cp` atom
  (which resolves to just the *highest* version), then the per-package
  loop explicitly protects the highest version of every cp and every
  non-highest version an argument atom doesn't match; with no `args`,
  `args_set` auto-fills with every multi-version cp. New
  `portage_repo::prune_cleanlist` reproduces that as "seed the closure
  from every installed package except the ones that are both
  non-highest-in-their-cp and matched by `args_set`", reusing the exact
  `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND` closure and
  `topological_removal_order` `depclean_cleanlist` already has.
  `portuale::run_prune_pretend` is deliberately thinner than
  `run_depclean_pretend`: real `action_depclean` returns right after the
  `unmerge()` preview for `action == "prune"` (`:888`), so there is
  **no** `* ` advisory block (only `action == "depclean"` prints it,
  `:840`) and **no** `Packages installed:` / `Required packages:` /
  `Number to remove:` stats block -- just `>>> Calculating removal
  order...` + the `_unmerge_display` block, or `>>> No packages selected
  for removal by prune` followed by both the `--verbose` and (prune-only,
  `:1348`) `>>> To ignore dependencies, use --nodeps` hint lines. The
  bare-name resolution + `--- Couldn't find 'X' to <action>.` handling
  is now a shared `resolve_cleanup_args` helper (real
  `action_depclean:848-863`), used by both `--depclean` and `--prune`.
  Same `--pretend`-only gate as the other cleanup actions. The committed
  fixtures already carry a multi-version cp (`dev-libs/unmergepkg` at
  1.0 and 2.0); the new `_prune_root` test fixture adds a richer case
  (`aa`/`zz`/`mm` multi-version, a `keeper` pinning `mm-2.0`, and a
  `zz-1.0` -> `aa-1.0` ordering edge). **Deliberately out**: the
  `--deselect` world-file rewrite (`--pretend` never writes it), and --
  as with `depclean` -- slot-operator rebuild edges. (`--depclean-lib-check`
  has since shipped for both -- see its own section below.)

  **`emerge --prune --nodeps`: the no-dependency-check branch.** Real
  `actions.py:2684-2697`: `--nodeps` routes prune to `unmerge()`'s own
  `_unmerge_display` prune branch (`unmerge.py:245-272`) *instead of*
  `_calc_depclean` -- so there is no reachability check at all, no `>>>
  Calculating removal order...`, and no `show_parents` (`--verbose` is
  inert). For every cp with more than one version installed the best
  (highest) version is `protected` and *every* other version is
  `selected` -- even one something still needs (in `_prune_root`
  `keeper` pins `=dev-libs/mm-2.0`, but `--prune --nodeps` prunes it
  anyway). New `portage_repo::prune_nodeps_selection` (best/rest split,
  no-args = every `vardb.cp_all()` cp, args = each atom's own
  `vardb.match` set); new `portuale::run_prune_nodeps_pretend` renders
  it -- header, per-cp `selected:`/`protected:`/`omitted:` blocks
  (`omitted` is always `none`), the `sys-apps/portage` self-skip, the
  "still listed in package sets" warning, footer. Empty: `>>> No
  outdated packages were found on your system.` with no args (real
  `global_unmerge`), else `>>> No packages selected for removal by
  prune` -- both **exit 1** (real `_unmerge_display` returns `(1, {})`,
  unlike plain `--prune`'s exit 0). **Narrowed**: `best` is just the
  highest version -- real portage's same-slot `COUNTER` tiebreak (a
  broken-vdb case) is out, the same category as the other `-pC` slot
  refinements.

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
  real lever on this same `bdeps` value) was still a deliberate,
  documented out-of-scope cut at the time -- closed by a later follow-up
  below (it turns out to matter *now*, not "only once `--usepkg`/binary-
  package support exists" as first assumed here, since this pilot's own
  `--usepkg`-less CLI always satisfies the real `--usepkg`-gated half of
  its own condition). New fixture packages `withbdepspkg`
  (installed, `DEPEND`s on `builddeponlypkg`, `BDEPEND`s on
  `hostdeponlypkg`, `RDEPEND`s on the existing `newpkg`) prove the
  distinction end to end: under `--deep`, the default walks all three;
  `--with-bdeps=n` walks only `newpkg` (`RDEPEND`), leaving the other two
  entirely unmentioned.

  **`--with-bdeps-auto[=y|n]`: closes the cut named just above.**
  Grounded against real `create_depgraph_params.py`: `bdeps =
  myopts.get("--with-bdeps"); if bdeps is not None: myparams["bdeps"] =
  bdeps; elif myaction == "remove" or (myopts.get("--with-bdeps-auto")
  != "n" and "--usepkg" not in myopts): myparams["bdeps"] = "auto"` -- an
  explicit `--with-bdeps` always wins outright; only in its *absence*
  does `--with-bdeps-auto=n` matter at all, changing the real default
  from `"auto"` (this pilot's own pre-existing `with_bdeps = true`) down
  to unset (equivalent to `"n"`, since `depgraph.py` itself only ever
  tests `bdeps in ("y", "auto")`). The real `"--usepkg" not in
  myopts` half of that same condition is always true here, since this
  pilot's CLI has no `--usepkg` at all -- so `--with-bdeps-auto` isn't
  gated on anything else in this pilot, unlike the module doc comment's
  own earlier (now-corrected) assumption. Same real
  `argument_options`/`"choices": ("y", "n")` shape `--with-bdeps` itself
  has -- required value, no short alias, not bundle-compatible. Verified
  against `withbdepspkg`, the same fixture `--with-bdeps` itself already
  uses: `--deep --with-bdeps-auto n` (no explicit `--with-bdeps`) walks
  only `newpkg`, exactly like explicit `--with-bdeps n` already does;
  `--deep --with-bdeps y --with-bdeps-auto n` still walks all three,
  proving the explicit flag's own precedence.

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
  handling. (Fully closed by two later follow-ups below --
  "`--changed-deps`: per-key comparison and `strip_slots`" and
  "`--changed-deps`: the structured (non-flat) comparison, in full" --
  the latter porting real `use_reduce`'s own `flat=False` mode. A
  narrower, real sibling, `--changed-deps-report` -- a report-only "you
  might want `--changed-deps`" notice, no reinstall of its own -- was
  deferred at the time, closed by a later follow-up below.) New fixture
  package
  `changeddepspkg`
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

  **`--changed-deps-report[=y|n]`: report, don't reinstall.** Closes the
  deferral named two paragraphs up. Grounded against real
  `depgraph.py::_changed_deps_report`: a `!!! Detected ebuild dependency
  change(s) without revision bump:` WARN, listing every installed
  package still in the graph whose deps differ from the current ebuild
  -- purely informational, never a resolution change, reusing
  `deps_changed` completely unmodified for the comparison itself (the
  same shared function `--changed-deps` already established, not a
  parallel implementation). Real portage's own gating -- "This is
  completely silent... if `--changed-deps` or `--dynamic-deps` is
  enabled" -- is ported as simply never bothering to call `deps_changed`
  at all once `changed_deps` is already true, rather than collecting
  into a dict and discarding it unread at print time: real portage's own
  `_changed_deps_pkgs` dict has no other reader, so the two are
  behaviorally identical, a documented simplification, not a guess.
  (`--dynamic-deps` itself stays unrecognized in this pilot -- real
  portage's own now-defunct alternate resolver strategy -- so only the
  `--changed-deps` half of that real silencing condition is reachable
  here at all.) Detection happens inline in `resolve_pretend_graph`'s own
  BFS loop, right after each atom's `resolve_pretend` call: an
  `AlreadyInstalled` or `Reinstall` outcome (the only two that name a
  genuinely-installed version) triggers an independent `deps_changed`
  check for that exact version, deduplicated by `(category, package,
  version)` the same way real portage's own dict (keyed by the installed
  `Package` object) naturally collapses repeat visits -- so a `Reinstall`
  already triggered by `--newuse`/`--changed-slot` for unrelated reasons
  still gets checked and reported independently, matching real portage's
  own freely-combinable reinstall/report triggers. `repo_name` in each
  report entry stands in for real `pkg.repo` (this pilot has no vdb
  `REPOSITORY` reader) -- a safe substitution, since real
  `_changed_deps_report`'s own `if pkg.repo != ebuild.repo: continue`
  filter requires the two to already match before a package is even
  collected. Extended this pilot's own `--json` schema with a new
  `"changed_deps_report"` array too (a pilot-specific convenience format,
  not real portage's own concern -- real `--json` doesn't exist -- but
  keeping every out-of-band signal representable in both output modes,
  the same way `slot_conflicts` already is, avoids a silently lossy
  gap). New CASES exercise the report firing (bare
  `--changed-deps-report`, no reinstall), the silencing (combined with
  `--changed-deps`, which still reinstalls normally), and the `=n`
  explicit-disable form, all against the pre-existing `changeddepspkg`
  fixture -- no new fixture needed, since this reuses the exact same
  vdb-vs-ebuild `RDEPEND` mismatch `--changed-deps` itself already relies
  on.

  **`--changed-deps`: per-key comparison and `strip_slots`.** Two more
  steps toward real `_changed_deps` (`depgraph.py:3168`), which builds a
  per-key list `[use_reduce(DEPEND), use_reduce(RDEPEND), ...]` on each
  side and compares them element-wise, after `strip_slots` and
  `strip_libc_deps`. The pilot used to concatenate every dep key into one
  string, flatten once, and compare as a single set -- so (1) an atom
  moved from one dep key to another with the same overall set showed no
  change, and (2) a `:=` slot-operator dependency *always* showed a
  change, because the vdb records the built form `dev-libs/foo:2=` (the
  slot it was merged against) while the current ebuild says
  `dev-libs/foo:=`. `deps_changed` now flattens and compares each dep key
  independently (`Vec<HashSet<String>>` per side, index-wise `!=`), and a
  new `strip_slot_operator_slots` ports real `strip_slots`
  (`lib/portage/dep/_slot_operator.py:11`): for any atom whose
  `slot_operator` is `=` and that carries an explicit slot, the slot
  expression is rewritten back to a bare `:=` before comparison (the
  Python mirror uses the real `Atom.with_slot("=")` for the same
  rewrite). The remaining documented cut is now just the `||`-structure
  half -- closed by the next slice. Two new fixtures:
  `dev-libs/movedkeydepspkg` (vdb `RDEPEND= dev-libs/samepkg`, current
  ebuild `PDEPEND=dev-libs/samepkg` -- same atom, different key) proves
  the per-key change registers; `dev-libs/slotopdepspkg` (current ebuild
  `RDEPEND=dev-libs/slotoptarget :=`, vdb recorded
  `dev-libs/slotoptarget:2=`) proves the built slot is *not* a change.

  **`--changed-deps`: the structured (non-flat) comparison, in full.**
  Closes the last cut named just above. Real `_changed_deps` compares
  `use_reduce(k, token_class=Atom)` output -- the `flat=False` *nested*
  form, with `||`-group boundaries preserved -- key by key, as Python
  lists (`built_deps != unbuilt_deps`). New
  `portage_use_reduce::use_reduce_structured` ports real `use_reduce`'s
  own `flat=False`/`opconvert=False` mode: the full nested stack reducer
  with every redundant-bracket optimization
  (`is_single`/`special_append`/`ends_in_any_of_dep`/
  `last_any_of_operator_level`), the EAPI-7+ `|| ( ) ->
  __const__/empty-any-of` placeholder, `|| ( A ) -> A` collapse, `|| (
  || ( ... ) ) -> || ( ... )` flatten -- verified byte-for-byte against
  real `portage.dep.use_reduce` over ~4000 randomized dep strings.
  `deps_changed` reduces each key to that canonical token stream, then
  applies real `Atom.evaluate_conditionals` (`evaluate_atom_conditionals`)
  and real `strip_slots` per atom and real `strip_libc_deps` *top-level
  only* (real `strip_libc_deps` iterates just the outer per-key list, by
  `cp`), and compares the per-key token vectors with `==`. The Python
  mirror drops its hand-rolled helpers entirely and calls the real
  `use_reduce` + `strip_slots` + `strip_libc_deps` -- `_changed_deps`
  ported essentially verbatim. **Faithful to real portage's Python-list
  `!=`, confirmed with the user: order is significant *everywhere*** --
  a `|| ( a b ) -> || ( b a )` reorder and a plain `RDEPEND="a b" -> "b
  a"` reorder both register as changed (real portage does the same);
  only a redundant-bracket difference (`a b` vs `( a b )`, which
  `use_reduce` collapses) does not. Three new fixtures:
  `dev-libs/anyofreorderdepspkg` (`||` alternatives swapped -> changed),
  `dev-libs/orderchangeddepspkg` (plain atoms swapped -> changed),
  `dev-libs/redundantbracketdepspkg` (`( ... )` wrap -> not changed).

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
  and repo sides) -- deliberately narrow at the time, though, not the
  general `Candidate.sub_slot` threading real dependency-atom sub-slot
  matching would eventually need: this reuses the exact same
  "dedicated, narrow re-read of metadata this pilot's general
  `Candidate` model doesn't carry" approach `--changed-deps` already
  established for `DEPEND`/`RDEPEND`, rather than growing `Candidate`
  itself and touching the whole matching/visibility pipeline for a
  single new flag. (That deferred `Candidate.sub_slot` threading is its
  own later follow-up below -- see "Sub-slot modeling".) Implemented as
  a third independent,
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

  **Sub-slot modeling: `Candidate.sub_slot`, and a real, previously-silent
  dependency-matching bug it closes.** Closes the deferral named in the
  `--changed-slot` paragraph just above. `portage-repo`'s own `Candidate`
  struct (repo-sourced) discarded the sub-slot half of a real
  `SLOT="main/sub"` value entirely -- `metadata.get("SLOT").split('/')
  .next()` -- for every candidate it ever built, in both `list_candidates`
  (repo/ebuild side) and `installed_candidates` (vdb side); every
  candidate string this crate builds for `portage_dep::match_from_list`
  (whose own `Candidate` regex already parses a `slot/sub_slot` suffix
  correctly -- it was only ever fed incomplete data) inherited the same
  gap. The practical effect: a real dependency atom restricted on
  sub-slot (`dev-libs/foo:0/2`, PMS 8.3.3, not the `:=`/`:slot=`
  slot-operator forms -- `matches_slot`'s own doc comment already
  established those need no candidate-side sub-slot data at all to match
  correctly) could **never** match anything here, no matter what a
  candidate's real `SLOT` metadata said -- the same "silently drops the
  dependency, no entry, no error" failure mode the slot-operator-grammar
  bug had, just one layer deeper: that earlier bug was in the *atom*
  parser, this one was in the *candidate* data feeding it. Fixed by
  giving `Candidate` a `sub_slot: String` field (populated via the
  already-existing `split_slot` helper, the same one `--changed-slot`
  introduced, reused rather than re-derived) and embedding
  `slot/sub_slot` in every candidate string built from repo/vdb data
  across `is_visible`, `_candidate_iuse_and_use`-equivalent USE lookups,
  `resolve_pretend`'s own atom-vs-candidate matching and `--exclude`
  checks, `resolve_pretend_graph`'s per-entry config re-lookup,
  `enqueue_dependencies`, and `resolve_blockers`' vdb-derived
  contribution -- seven call sites total across the two languages'
  mirrored implementations. `resolve_blockers`' *other* contribution
  (already-resolved New/Upgrade graph entries) is a deliberate, narrower
  scope cut: `GraphEntry`'s own `slot` field stays main-slot-only for
  now, defaulting sub-slot to the main slot itself (the same fallback
  `split_slot` already uses for an unslashed `SLOT`) rather than growing
  `GraphEntry` and its own construction sites too -- a real dependency
  atom blocking a same-run New/Upgrade candidate on a specific sub-slot
  is a narrow enough case to defer honestly rather than half-implement.
  `run_deselect`'s own use of `installed_candidates` is unaffected by
  construction: real `Atom(f"{pkg.cp}:{pkg.slot}")` never included
  sub-slot either, so the added tuple element is simply never consulted
  there. Proven with three new fixtures -- `subslotpkg` (`SLOT="0/2"`),
  `subslotconsumer` (`RDEPEND="dev-libs/subslotpkg:0/2"`, an exact
  match) and `subslotmismatchconsumer` (`RDEPEND=
  "dev-libs/subslotpkg:0/3"`, a genuine mismatch) -- the second pair
  deliberately proving this is real matching, not "always accept
  regardless of sub-slot": before this fix, *both* would have silently
  failed to resolve their own dependency at all.

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
  exist yet. Deliberately still out of scope at the time, confirmed as a
  *separate* mechanism rather than folded in here: `--deselect`'s own
  world-atom matching (`run_deselect`) was not integrated with
  `world_sets`/custom sets at all -- closed by a later follow-up below,
  which turned out to need neither `resolve_custom_set` nor any nested
  expansion at all, once real `action_deselect` was read directly rather
  than assumed to reuse `@world`'s own machinery. New fixture packages
  `nestedsetpkg`/`innernestedsetpkg` (reached only via
  `PORTING/fixtures/var/lib/portage/world_sets`'s own `@nestedtestset`,
  whose own `etc/portage/sets/nestedtestset` nests a further
  `@innernestedset` reference, which itself references back to
  `@nestedtestset` to exercise the cycle guard) prove the whole path end
  to end.

  **`--deselect`'s own `world_sets` integration**: closes the cut named
  just above. Grounded against real `action_deselect` itself
  (`lib/_emerge/actions.py`, lines 1740-1835), read directly rather than
  assumed from `@world`'s own already-ported `world_sets`/nested-set
  machinery -- and the two turn out to be genuinely different real
  mechanisms, not a shared one. Real `action_deselect`'s own combined
  `world_set` (`WorldSelectedSet`) iterates BOTH `world`'s own plain
  atoms AND `world_sets`'s own literal `@name` reference *strings* --
  confirmed by reading `WorldSelectedSet.load`'s own `self._setAtoms(
  chain(self._pkgset, self._setset))`: a `@name` string fails real
  `Atom(...)` parsing and lands in the aggregate's own `_nonatoms`, so
  it's carried through **unexpanded** the whole time, never resolved
  into its own member atoms at all. `action_deselect`'s own matching
  loop confirms this directly: a `@`-prefixed CLI target can only ever
  discard a `@`-prefixed `world_set` entry via *exact string equality*
  -- there is no installed-candidate matching, no member-atom expansion,
  for either side. So despite `resolve_custom_set`'s own real, working
  nested-set expansion (built for -- and still only used by -- `@world`'s
  own dependency-resolution walk, `SetConfig.getSetAtoms`, a genuinely
  different real mechanism), it plays no role here at all: this pilot's
  own equivalent of real `action_deselect`'s own `@`-target handling is
  a plain membership check against `read_world_sets`'s own already-read
  list, nothing more. Each discarded entry is now reported against its
  own real source file (`"world"` for a plain atom, `"world_sets"` for a
  `@name` reference, matching real `filename = "world_sets" if
  str(atom).startswith(SETPREFIX) else "world"` exactly) -- and, since
  real `action_deselect` sorts its *whole* combined `discard_atoms` set
  together (`sorted(discard_atoms, key=str)`), a plain-atom and a
  `@name` discard from the same run are interleaved into one sorted
  list, not printed as two separate "world" then "world_sets" blocks.
  The `_deselect_root` test fixture (isolated from the shared
  `PORTING/fixtures` tree, same reasoning `--deselect`'s own original
  slice already established) gained its own `world_sets` file,
  `@myselectedset` (matchable) alongside `@anotherselectedset` (present
  but never targeted, proving only an actually-requested name is ever
  discarded) -- `--deselect @myselectedset` alone, `--deselect
  @nosuchset` (no match), and `--deselect dev-libs/foo @myselectedset`
  together (proving the combined-sort interleaving) all verified to
  agree between the Rust and Python implementations.

  **`--deselect`'s own real `Atom.intersects()` algebra, and a
  correctness fix uncovered along the way.** Real `Atom.intersects()`
  (`lib/portage/dep/__init__.py`, lines 2213-2240) turned out to be a
  smaller port than its name suggests -- its own docstring says so
  directly: "atoms with different cpv, operator or use attributes cause
  this method to return False even though there may actually be some
  intersection... TODO: Detect more forms of intersection". It's a
  deliberately narrow equality-style check, not a version-range algebra:
  `cp` (category+package), `use` (use-deps), `operator`, and `cpv`
  (effectively `operator`+version+revision together) must ALL match
  exactly -- not overlap, not satisfy a range -- before slot
  compatibility (`None` on either side, or an identical value) decides
  the result; ported field-for-field as `portage_dep::atom_intersects`,
  skipping only real portage's own redundant `self == other` fast path
  (two textually-identical atoms already fall through to `true` the same
  way regardless). This replaces `run_deselect`'s own previous narrower
  category/package(+slot)-only equality check, and closes the Python
  reference's own previously-documented divergence (it now calls real
  `Atom.intersects()` directly, the oracle's usual "why re-derive it"
  reasoning, rather than reusing `match_from_list`).

  Re-deriving the exact match ordering surfaced a genuine correctness
  bug in this pilot's own **pre-existing** `--deselect` port, present
  since its very first slice: both `pretend.rs` and its own doc comments
  assumed every `--deselect` target -- bare name or explicit-category
  alike -- had to be actually installed before it could match anything,
  narrowing candidates through `installed_candidates`/`vardb.match`
  first. Reading real portage's own call chain directly disproves this.
  `action_uninstall`'s own `dep_expand(x, mydb=vardb, ...)`
  (`lib/portage/dbapi/dep_expand.py`) returns an explicit-category atom
  **completely unchanged** -- `if mydep.category != "virtual": return
  mydep` -- before ever reaching `cpv_expand` (the vardb-dependent part,
  only reached for a bare name); `action_deselect` itself then seeds
  `expanded_atoms = set(atoms)` with that same atom, unconditionally, no
  installed check anywhere in the path. The bare-name path fares the
  same: its own null-category-to-real-category substitution is *also*
  unconditional in real `action_deselect`; the accompanying
  `vardb.match(atom)` call real `action_deselect` does make is a
  *separate*, additional contribution (a further bare `category/
  package:slot` candidate for whatever's genuinely installed) -- not a
  gate on the substituted atom, and for a bare name specifically it's
  dead code (real `vardb.match()` can never match anything against the
  still-null-category original atom, since no package is ever
  catalogued under category `"null"`). So `--deselect cat/pkg` (or a
  bare name resolvable via the world file) genuinely discards a matching
  world entry even if that package was never installed at all -- this
  pilot's own earlier tests asserted the opposite, an incorrect
  generalization from "a bare name with *no* matching world entry
  contributes nothing" to "every target needs to be installed",
  conflated two genuinely different real code paths. A further, related
  consequence: since `dep_expand` never adds a slot restriction to an
  explicit-category target on its own, an *unslotted* `--deselect
  dev-libs/pkg` now matches a world entry at any slot at all --
  `Atom.intersects()` only rejects a slot mismatch when *both* sides
  carry one -- while a CLI target that itself specifies a slot
  (`--deselect dev-libs/pkg:1`) still gets narrowed correctly, since both
  sides now have something to compare. All fixed in lockstep across both
  implementations, with the previously-inverted test
  (`dev-libs/qux`, world-listed but never installed) flipped and a
  matching bare-name case (`qux`) added, four new slot-interaction tests
  covering both the unslotted and slotted target forms, and three new
  version/operator tests (`PORTING/fixtures`-independent `_deselect_root`
  entries) exercising the narrow-`intersects()` behavior directly: an
  exact-version target matches, a different version doesn't, and even
  the *same* version under a different operator (`>=dev-libs/vers-1.0`
  against a world `=dev-libs/vers-1.0` entry -- which a real range check
  would actually satisfy) doesn't either, since `Atom.intersects()`
  requires the operator itself to match exactly.

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
  flags in parens, `USE_EXPAND` grouping (`VIDEO_CARDS`, etc.); v1
  shows only the plain enabled/disabled set, which is a real, useful
  subset rather than an invented one, matching the "documented,
  simplified subset" spirit of every other output-formatting decision in
  this pilot. (The `USE_EXPAND` grouping, the `*`/`%` diff markers, the
  `( … )` forced/masked wrap, and the `[ebuild N ~]` bracket-mask column
  were all closed later -- see the "`emerge --pretend -v`: …" slices
  below, as was the `all_flags`-driven "show every flag / `(-flag%)`
  removed-from-IUSE" behavior; only the ANSI colorization is still cut.)

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
  the real mechanism turned up what looked like a scope-narrowing
  discovery: an overlay's own `profiles/`/`license_groups` are *not*
  part of this same "every repo, unconditionally" mechanism -- real
  `LicenseManager.__init__`'s own `license_group_locations` is tied to
  `locations_manager.profile_locations`. **Corrected 2026-08-30** (see
  "`license_groups` read from each repo's `profiles/`" below): real
  `profile_locations` is `[<main_repo>/profiles] + [<overlay>/profiles
  …]` -- the repo `profiles/` *bases*, not the profile-chain
  directories -- so `license_groups` genuinely *is* read from every
  configured repo unconditionally; only an overlay's own profile
  *directory* joining the chain still needs `reponame:path`. Also
  deliberately not
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

  **Overlay repo-level `package.use`/`.mask`/`.force`/`.stable.mask`/
  `.stable.force`**: a later follow-up to the same overlay `package.mask`
  work, grounded against real `UseManager.py`: its own
  `_parse_repository_files_to_dict_of_tuples`/`_of_dicts` both iterate
  `repositories.repos_with_profiles()` -- every configured repo, not
  just main -- for every one of these five files, the exact same
  "every repo, unconditionally" mechanism `package.mask` already ports,
  closing a gap this pilot's own doc comments had explicitly flagged
  ("main repo only") in five separate places. Each overlay's own entries
  get the identical `::repo`-auto-scoping `package.mask`'s own overlay
  entries already get (`scope_repo_package_use_lines`, the same
  atom-only-vs-`<atom> <flag>...`-shaped sibling of
  `scope_repo_mask_lines`) so they can never silently apply to a
  same-named package in a different repo, while the main repo's own
  entries stay unscoped (apply everywhere, since every overlay here
  implicitly masters it). One real asymmetry from `package.mask`,
  confirmed by reading `UseManager.py`'s own parse functions: none of
  these five files gets `package.mask`'s own masters-merge treatment at
  load time (`stack_mask_lines`) -- real portage never combines an
  overlay's own file with its master's own into one stacked list at
  parse time here at all; the masters chain is only consulted later, per
  candidate, in `getUseMask`/`getUseForce` (`repos = masters +
  [pkg.repo]`, each repo's own already-independent dict appended in that
  order), so this pilot's own `resolve_config` just scopes-then-appends
  each overlay's own lines instead, no merge step needed. Three new
  overlay-only fixture packages exercise the three files most likely to
  matter in practice end to end: `overlayuseenablepkg` (its own
  `overlayuseflag?`-gated dependency is pulled in only because the
  overlay's own `profiles/package.use` enables the flag), 
  `overlayuseforcepkg` (same proof for `package.use.force`, forcing a
  flag on that's off everywhere else), and `overlayusemaskpkg` (the
  inverse: `IUSE="+overlaymaskflag"` defaults the flag on, but the
  overlay's own `profiles/package.use.mask` masks it back off, so the
  dependency is *not* pulled in). `package.use.stable.mask`/`.force`
  get the identical scoping too, verified via three new Rust unit tests
  mirroring `package.mask`'s own overlay-scoping test style directly
  (no e2e fixtures added for the `.stable.` variants specifically, since
  the underlying mechanism -- one more `scope_repo_package_use_lines`
  call site -- is already fully proven by the non-stable siblings above,
  and reaching it end-to-end would need a dedicated `~arch`
  stability setup disproportionate to what it'd actually prove).

  **Scoping the main repo's own `package.mask`/`.unmask` too**: an
  immediate follow-up to the overlay `package.use` work above, closing a
  gap this pilot's own doc comment had explicitly named and deferred at
  the time of the original overlay-`package.mask` slice: real
  `append_repo` scopes *every* repo's own repo-level `package.mask`/
  `.unmask`, including the main repo's, not just an overlay's -- this
  pilot previously left the main repo's own entries unscoped. Turned out
  to be a more interesting investigation than expected: an initial live
  repro (masking a package by its bare name in the main repo, then
  querying an identically-named package that exists only in an overlay
  via an explicit `::overlay` atom) looked like it confirmed a real
  scoping leak -- but on closer inspection, that result is *also*
  exactly what real portage would do anyway, via the pre-existing,
  correct `masters` mechanism (every overlay here always implicitly
  masters the main repo, so it inherits the main repo's own
  `package.mask` entries regardless of whether they're separately
  scoped). Since no fixture repo here can currently avoid mastering the
  main repo (no explicit `masters =` override parsing exists -- a
  separate, already-documented gap), this fix's own distinguishing
  effect is presently latent, not currently observable through any
  constructible fixture -- ported anyway for exact correctness with real
  `MaskManager.py`, and because it's genuinely load-bearing already, not
  just inert: scoping main's own entries without a companion fix would
  have broken the pre-existing, already-passing
  `repomaskedthenuserremovedpkg` test, whose own user-level
  `-dev-libs/repomaskedthenuserremovedpkg` (necessarily unscoped, since
  user-level entries never get repo-scoped at all) needs to keep
  cancelling that now-`::testrepo`-scoped repo-level entry. That's what
  the paired fix to `stack_mask_lines` (both `portage-repo`'s -- named
  `scope_repo_mask_lines`'s sibling -- and its Python mirror) actually
  does: ports real `stack_lists`'s own `ignore_repo=True` behavior
  (`lib/portage/util/__init__.py`, confirmed by reading it directly),
  "let `-cat/pkg` remove `cat/pkg::repo`" -- an unscoped removal token
  strips any `::repo` suffix off every existing entry before comparing,
  so a profile-level or user-level `-atom` can still cancel *any*
  repo-scoped mask entry, not just an identically-unscoped one. Without
  it, `repomaskedthenuserremovedpkg` would have regressed from "correctly
  unmasked" to "incorrectly still masked" the moment main's own entries
  started getting `::testrepo`-scoped -- so that pre-existing test is
  itself live, e2e proof this fix is both correct and necessary, even
  though its main-repo-scoping half has no fixture that isolates it on
  its own.

  **Explicit `repos.conf` `masters =` parsing**, closing the exact gap
  just named above and finally making the main-repo `package.mask`
  scoping fix's own distinguishing effect observable through a real
  fixture for the first time. Grounded against real `RepoConfigLoader.
  __init__` (`lib/portage/repository/config.py:1229-1260`): a repo with
  no explicit `masters =` key at all implicitly masters the main repo
  alone (the pre-existing default, unchanged); an *explicit* key --
  even an empty one -- fully replaces that default, resolving each
  named master to its own repo location, silently dropping an unknown
  name (real `config.py` only warns, never a hard error). `portage_repo
  ::RepoConfig` gains a new `masters: Vec<PathBuf>` field (Python:
  `repos[i]["masters"]`, a list of locations, same shape), resolved by
  `find_repos` in a genuine second pass over the now-complete repo list
  (a master *name* can only resolve to a location once every repo's own
  location is already known). `portage_profile::resolve_config` gains a
  new `repo_masters: &HashMap<String, Vec<PathBuf>>` parameter (keyed by
  repo name, the caller's own already-resolved chain) threaded through
  every one of its own **61** existing call sites (36 in this crate's
  own tests, 24 in `portage-repo`'s, 1 real -- scripted the same way
  `resolve_pretend_graph`'s own new parameter was added, then hand-fixed
  the handful of multi-line call sites the script mangled), replacing
  the previous hardcoded "every overlay masters main alone" fallback
  with each overlay's own real, resolved chain (falling back to that
  exact same default when a repo name isn't a key in the map at all --
  every pre-existing call site, including all 61 of the above, keeps
  passing an empty map and keeps getting byte-identical results). The
  actual stacking logic itself only needed a small, genuinely new
  change: `package.mask` sources are now built from *every* declared
  master's own `package.mask`, in order, not just the main repo's --
  simplified from real `MaskManager.py`'s own per-master `stack_lists`
  (which stacks each master separately against the repo's own lines,
  then concatenates every one of those per-master results, so an
  unmatched `-atom` removal warning can be attributed to the specific
  master that should have supplied it) to one flat multi-source
  `stack_mask_lines` call over every master's lines followed by the
  repo's own -- produces the identical final masked-atom set for the
  common case, diverging only from real portage's own warning-
  attribution mechanics this pilot doesn't reproduce at all regardless.
  New fixture repo `independentoverlay` (`repos.conf`'s own `masters =
  overlay`, explicitly *not* the main repo) proves both directions with
  two packages that exist only there: one masked only by the main
  repo's own `package.mask` (must **not** apply -- main isn't a declared
  master) and one masked only by the `overlay` repo's own `package.mask`
  (must apply -- `overlay` is). Rust and Python byte-identical,
  confirmed both via the shared pytest contract suite and a direct
  manual diff against both built binaries.

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
  holds *and the installed version is still a real matched candidate at
  all*, it's returned immediately, without ever searching for a newer
  one. (The italicized qualifier is a later correction, not part of the
  original claim here: a follow-up below found that for a directly-named
  top-level atom specifically, `avoid_update` alone isn't sufficient --
  real portage's own `selective` gap means the installed version often
  isn't even a candidate to begin with, so this early return is skipped
  entirely in that case. `emerge cat/pkg` with no other flags does NOT
  offer to upgrade *because it's already installed and would otherwise
  stay that way*; it can still end up offering one for a different, less
  obvious reason -- see `--noreplace`/`--selective` below.) This was a
  genuine, discovered inaccuracy
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

  **`--noreplace`/`-n` and `--selective`: real portage's own `selective`
  gap, closing a genuine correctness bug the `--update` slice above
  didn't catch.** Found by comparing this pilot's own output against
  the real, installed system `emerge` on a real package
  (`sys-apps/portage`) and tracing real portage's own decision live,
  via monkeypatched instrumentation of the actual installed
  `_emerge.depgraph` module -- not read from source alone. Real
  portage's `avoid_update` shortcut above (`!update`) turns out to NOT
  be sufficient on its own for a **directly-requested (top-level)
  atom**: real `_wrapped_select_pkg_highest_available_imp`'s own
  per-candidate loop computes `want_reinstall = reinstall or empty or
  (found_available_arg and not selective)`, and `if want_reinstall and
  matched_packages: continue` -- for a candidate found via an atom on
  the command line, this skips ever re-adding the already-installed
  `Package` object as a candidate at all whenever real `myparams[
  "selective"]` is absent, so `avoid_update`'s own later shortcut (`if
  avoid_update: ... return pkg`) finds nothing installed to return and
  falls through to picking the best *available* (ebuild) candidate
  instead -- even when its version is identical to what's installed.
  The net real effect, confirmed live: a bare `emerge <atom>` with no
  other flags, on a directly-named atom, always searches for a newer
  version exactly as `--update` would, and even when nothing newer
  exists, still reports a bare reinstall (real `[ebuild R] cat/pkg-ver`,
  no parenthetical reason at all) rather than treating the identical
  installed version as satisfying -- `--noreplace`/`--selective`
  restore the "nothing to do" result, confirmed against the real system
  both ways. `selective` mirrors real `create_depgraph_params.py`'s own
  `myparams["selective"] = True` condition, computed from whichever of
  its eight real trigger flags this pilot implements: `--update`,
  `--newuse`, `--changed-use` (real portage's own `-U` rewrites to
  `--reinstall=changed-use` before `create_depgraph_params` ever runs,
  and `--reinstall` is itself constrained to that one literal choice in
  real portage -- so `--changed-use` alone covers this pilot's whole
  share of that real condition, no separate `--reinstall` flag needed),
  `--changed-deps`, `--changed-slot`, plus the two flags whose *entire*
  real effect is exactly this: `--noreplace`/`-n` (a plain boolean,
  bundle-compatible) and `--selective[=y|n]` (the identical meaning, a
  real optional value instead -- `n` explicitly cancels `selective`
  even if another flag already set it, matching real
  `create_depgraph_params.py`'s own unconditional `if myopts.get(
  "--selective") == "n": pop`, checked last). Real `--newrepo` (forces
  reinstall specifically on an installed-vs-current repo mismatch, and
  separately contributes to `selective`) is a documented, narrower
  scope cut: this pilot has no vdb `REPOSITORY` reader (confirmed
  absent from the real system during this same investigation -- the
  real vdb file is even lowercase `repository`, unlike every other
  metadata key). A dependency atom (not top-level) is never affected at
  all -- real `found_available_arg` is only ever set for an
  argument-derived candidate, matching real `_want_installed_pkg`'s own
  `return not arg` fallback for everything else. Threading this
  required a genuinely new `resolve_pretend` parameter,
  `is_top_level`, since the function previously had no way to tell a
  directly-requested atom apart from a dependency reached by
  recursion -- reused this pilot's own pre-existing `depth == 0` from
  `resolve_pretend_graph`'s BFS, the identical equivalence
  `--with-test-deps` already established between real "argument" and
  `depth == 0`. `PretendOutcome::Reinstall` gained a new, genuinely
  reasonless shape (`changed_flags` empty, `deps_changed`/`slot_changed`
  both false) for this specific trigger -- real portage prints no
  `(reinstall for ...)` parenthetical at all here, so `reinstall_reason`/
  `_reinstall_reason` now return `None`/an option rather than asserting
  at least one reason is always present, and the caller omits the
  parenthetical entirely for that case. This was the largest blast-radius
  fix in this pilot's own history: roughly two dozen pre-existing pinned
  tests whose own point was something else entirely (`--deep`,
  `--changed-deps`, `--changed-slot`, `--newuse`, package.mask, multiple
  top-level atoms) happened to lean on the old, incorrect "bare
  top-level atom stays AlreadyInstalled by default" behavior for their
  expected output -- each now passes `--noreplace` explicitly to isolate
  what it actually tests, noted inline in each case, rather than being
  silently right for the wrong reason.

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
  (real `Atom.evaluate_conditionals`, not anything `match_from_list`
  itself does) -- **now closed**, see the dedicated `opt=`/`opt?`
  paragraph further below for the follow-up.
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

  **`opt=`/`!opt=`/`opt?`/`!opt?` conditional USE-deps** (PMS 8.3.4),
  closing the one gap the USE-dep enforcement paragraph above explicitly
  named as "a wholly different mechanism". Grounded against real
  `Atom.evaluate_conditionals` (`lib/portage/dep/__init__.py:1387`,
  confirmed by reading it directly for its own truth table) and its own
  real integration point inside `use_reduce` itself
  (`__init__.py:1045-1046`: `if not matchall and hasattr(token,
  "evaluate_conditionals"): token = token.evaluate_conditionals(uselist)`
  -- called on *every* dependency-string atom token, the same `uselist`
  already threaded through for `flag? ( ... )` *group* conditionals).
  This pilot's own `use_reduce_flat` (`portage-use-reduce`) deliberately
  stays atom-grammar-agnostic (see that crate's own module doc comment on
  the tokenizing/atom-parsing split), so the equivalent step lives one
  layer up instead: `portage-repo`'s own `enqueue_flat_deps` (shared by
  the normal-deps queueing and the `--with-test-deps` follow-up, so
  they can't drift apart) now evaluates each flattened token's own
  conditional use-deps against the *owning* package's own
  already-computed effective USE -- the exact same set already being
  passed as `use_reduce_flat`'s own `uselist` for that identical
  dependency string -- before the token is ever queued or classified as
  a blocker. Ported as a new `portage_dep::evaluate_use_dep_conditionals`
  (the truth table itself, exhaustively unit-tested for all four
  operators plus default-preservation) and `evaluate_atom_conditionals`
  (applies it to a whole atom string, surgically rewriting just the
  `[...]` bracket via the same `atom_regex` capture range `parse_atom`
  itself uses, leaving everything else -- slot, `::repo`, version --
  byte-for-byte untouched, and leaving a plain or non-conditional
  use-dep atom completely unrewritten, not just semantically
  equivalent). The Python side needed no reimplementation at all: it
  calls the real `portage.dep.Atom` directly (`from portage.dep import
  Atom`, confirmed already true for this whole file), so
  `dep_atom.evaluate_conditionals(parent_use)` *is* the real mechanism,
  not a port of it. One real one-directional subtlety, faithfully
  ported: `opt?`/`!opt?` only ever *add* a constraint when their own
  condition holds -- when it doesn't, the token is dropped entirely
  (no constraint at all), never rewritten to the opposite unconditional
  form, unlike `opt=`/`!opt=` which always produce a concrete `flag`/
  `-flag` either way. `dev-libs/useeqparentonpkg`/`useeqparentoffpkg`
  (identical RDEPEND `dev-libs/useeqchildpkg[eqflag=]`, differing only
  in their own `IUSE="+eqflag"` vs `IUSE="eqflag"` default) prove both
  halves of `opt=`'s own truth table end to end: the `on` variant's
  dependency resolves normally (evaluates to `[eqflag]`, matching the
  child's own default-on flag); the `off` variant's identical use-dep
  evaluates to `[-eqflag]` instead, mismatching the child and reporting
  it as an unresolvable dependency -- the same "report, don't fail"
  outcome an ordinary rejected USE-dep already had.

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

  **REQUIRED_USE severity, corrected: collect every violation, don't
  abort the walk on the first one.** The paragraph above's own "Ported
  as a `Result::Err` returned straight out of the BFS loop" is no longer
  quite accurate -- found while re-grounding the same severity claim by
  reading real `depgraph.py`'s own `_add_pkg` end to end (~line 3600):
  on a violation, it sets `_dynamic_config._required_use_unsatisfied =
  True` and `return`s `0` -- it does **not** raise, and does **not**
  stop the rest of the graph walk. Real portage keeps resolving every
  other reachable branch (other top-level atoms, other dependencies),
  collecting every violation into `_unsatisfied_deps_for_display`, and
  only fails the whole run at the very end, once nothing more is left to
  attempt. This pilot's own previous "return `Err` immediately, right
  there in the BFS loop" shortcut meant a second, wholly independent
  top-level atom passed on the same command line -- one with its own,
  unrelated REQUIRED_USE violation -- would never even be attempted,
  let alone reported, once the first one failed: a real completeness gap
  a user would notice fixing one violation only to hit a second one on
  the next run, when real portage would have reported both up front.
  Fixed by collecting violation messages into a `Vec<String>`
  (`required_use_violations`) instead of returning immediately, letting
  the BFS `continue` past a violating candidate exactly like a
  dependency's own `NoVisibleCandidate` already does (report, don't
  recurse further into it), and checking the collected list once, at the
  very end of `resolve_pretend_graph`, right before its own success
  return -- joining every message with `\n` if any were collected,
  matching the "single global severity, whole-run scope" the original
  paragraph above already got right (only the *timing* was wrong, not
  the ultimate outcome). A genuinely *invalid* REQUIRED_USE (referencing
  a flag that isn't even valid IUSE) stays immediately fatal, deliberately
  -- real `check_required_use` raises for that case from outside the
  explicit "not satisfied" branch the delayed collection lives in, so
  this remains the one case this pilot's own architecture can't delay.
  New fixture `dev-libs/requiredusebadpkg2` (a second, independent
  REQUIRED_USE violation, `"baz? ( qux )"`, unrelated to the original
  `dev-libs/requiredusebadpkg`'s own `"foo? ( bar )"`) proves both
  violations now surface together when both atoms are requested in one
  call, in argument order, instead of the second one going silently
  unattempted.

  **`--autounmask`, a deliberately narrow v1.** Scoped starting from the
  backlog item's own label ("the `--autounmask*` family"), which turned
  out considerably bigger than expected on closer reading: real
  `--autounmask` tracks *why* each candidate was rejected
  (`_get_masking_status`, distinguishing keyword-mask from package.mask
  from USE-conditional mask), builds dependency-chain comments
  (`_get_dep_chain_as_comment`), and picks specific suggested-atom syntax
  (`=`/`>=`, with/without slot) based on whether the suggested version is
  the latest -- none of that machinery exists in this pilot at all, and
  this pilot's own `is_visible` returned a plain `bool` with no reason
  tracking whatsoever. On top of that, real `--autounmask-keep-keywords`
  (found while reading `create_depgraph_params.py`'s own default-
  resolution logic) defaults to **suppressing** keyword suggestions --
  the "simplest, most common" sub-case isn't even what a bare
  `--autounmask` invocation does by default. Scoped down, twice, to a v1
  that: (1) adds a single new `keyword_masked_only` check (candidate
  would be `is_visible` except its own KEYWORDS aren't accepted --
  package.mask/license/properties/restrict all still have to pass) and a
  `suggested_keyword` helper (the first non-`-`-prefixed KEYWORDS token,
  a real simplification of `_get_masking_status`'s own unstable-vs-
  different-arch-vs-`**` distinction); (2) appends a pilot-specific
  (not real-portage-formatted) suggestion line to a top-level atom's own
  fatal `NoVisibleCandidate` message when the best keyword-masked-only
  candidate exists; (3) ports real `create_depgraph_params.py`'s own
  `autounmask`/`autounmask_keep_keywords` default-resolution logic
  faithfully for the one sub-flag this pilot actually reads (with
  `--autounmask-use`/`-license` never consulted at all, confirmed this
  simplifies to exactly the same real outcome, not a shortcut around
  it): `autounmask` defaults enabled, off only via explicit
  `--autounmask=n`; `autounmask_keep_keywords` defaults *suppressed*
  when `--autounmask` itself was never given, but defaults *not
  suppressed* once `--autounmask` itself was explicitly given (any
  value) -- real portage's own "asking for autounmask implies wanting
  its keyword suggestions too, but the ambient default doesn't"
  asymmetry. Deliberately still out: package.mask/license/USE
  suggestions, suggestions for a *dependency's* own `NoVisibleCandidate`,
  and any actual mutation (`--autounmask-write`) -- report only. New
  fixture `dev-libs/autounmaskkeywordpkg` (`KEYWORDS="~amd64"`, no
  `package.accept_keywords` entry) confirmed live across all five
  meaningfully distinct flag combinations (nothing given; `=n`; bare;
  `--autounmask-keep-keywords=n` alone; both explicit and contradicting)
  -- Rust and Python byte-identical on every one.

  **`IUSE`'s own `+`/`-` default markers, closing a real, comprehensive
  gap this pilot's own REQUIRED_USE reporting (paragraph above) helped
  surface.** Found the same way the `selective` gap above was: comparing
  this pilot's own output against the real, installed system `emerge` on
  a real package (`media-video/ffmpeg`) -- a bare `--noreplace --newuse
  media-video/ffmpeg` reported `REQUIRED_USE not satisfied`, aborting the
  whole run, for a USE combination hand-verified (and confirmed via a
  standalone harness feeding the exact real, live-captured inputs
  straight to `check_required_use` in isolation -- the algorithm itself
  was correct) to be fully satisfied by real portage's own resolved USE.
  The actual bug: `effective_use_flags` never once consulted a package's
  own `IUSE` string for its `+`/`-` default markers at all -- confirmed
  by grepping every place `portage-repo` touches an IUSE token
  (`trim_start_matches(['+', '-'])`, four call sites): all of them strip
  the marker to get the bare flag name and then discard it, never
  branching on which one was there. Real `ffmpeg-8.1.2`'s own IUSE
  declares `+gpl`/`+dav1d`/`+drm`/`+gnutls`/`+libass`/`+truetype`/`+xml`/
  `+zlib` (among others) -- every one of them silently defaulted to
  *disabled* by this pilot instead of real portage's own *enabled*,
  which is what actually violated the (otherwise satisfied)
  `REQUIRED_USE`. Grounded precisely against real `config.py`'s own
  `_setup_pkg_iuse` (`lib/portage/package/ebuild/config.py`, ~line
  1878): `+flag` contributes a bare `flag` (enable) token, `-flag`
  contributes itself unchanged (disable), a markerless `flag`
  contributes nothing at all -- stored under `self.configdict[
  "pkginternal"]["USE"]`, a real, *named* `USE_ORDER` component (real
  default `"env:pkg:conf:defaults:pkginternal:features:repo:env.d"`).
  Confirmed via `config.py`'s own `self.uvlist` construction (`for x in
  self["USE_ORDER"].split(":"): ...; self.uvlist.reverse()`) that
  `pkginternal` (position 5 of 8) is applied well *before* `defaults`
  (profile), `conf` (`make.conf`), and `pkg` (`package.use`) in real
  incremental precedence -- i.e. all three of those real sources can
  still override an IUSE default; it only wins when none of them
  mentions the flag at all. Ported as simply the seed `effective_use_flags`'s
  own `use_flags` now starts from (a new `iuse` parameter, threaded
  through all four of its real call sites), with `base` (this pilot's
  own already-flattened profile+`make.conf` result) unioned on top --
  `base` can only ever *add* a flag here, never force one off that IUSE
  defaulted on, since it's a plain enabled-name set with no "explicitly
  disabled by a lower layer" information surviving that far. A
  documented, narrower scope cut, not a new kind of imprecision: it's
  the exact same information loss this function's own pre-existing
  `package.use.mask`/`.force` handling (see the "flat global
  accumulation" paragraph in this function's own doc comment,
  `portage-repo`) already accepted for the global tier -- and it leaves
  the dominant real-world case (an ebuild author sets a sensible IUSE
  default, nothing else ever mentions the flag) fully correct, which is
  exactly what was broken. New fixture `dev-libs/iusedefaultpkg`
  (`IUSE="+enableddefault -disableddefault plainflag"`,
  `REQUIRED_USE="enableddefault !disableddefault"`) resolves successfully
  only once the fix is in place -- under the old behavior it would have
  hit the exact same spurious-abort failure mode `ffmpeg` did -- and its
  own `package.use` entry (`plainflag`, a flag with no IUSE default at
  all) proves package.use still layers normally on top, not just that
  IUSE defaults exist in isolation. (A second, related finding from the
  same investigation -- a *different* real package's REQUIRED_USE
  referencing a profile-injected implicit USE flag this pilot doesn't
  model at all -- is closed by the next paragraph.)

  **Implicit IUSE: `PORTAGE_ARCHLIST`/`use.mask`/`use.force`/`build`/
  `bootstrap`, closing the second finding deferred above.** Resolving
  `media-video/ffmpeg`'s full dependency graph (not just the package in
  isolation) hit a *different* real failure one level down: real
  `media-libs/mesa-26.1.6`'s own `REQUIRED_USE` references `x86`, which
  `mesa`'s own `IUSE` never declares at all -- `REQUIRED_USE for
  media-libs/mesa-26.1.6 is invalid: USE flag 'x86' is not in IUSE`.
  Real `check_required_use` doesn't validate a referenced flag against a
  package's own literal `IUSE` string -- it's called with
  `pkg.iuse.is_valid_flag` (`lib/_emerge/depgraph.py`), backed by real
  `config.py`'s own `_calc_iuse_effective()`/`_get_implicit_iuse()`
  (~line 2338): every package's *effective* IUSE additionally includes
  `PORTAGE_ARCHLIST` (`profiles/arch.list`, stacked across the whole
  profile chain with the same `-entry` removal semantics `package.mask`
  uses -- confirmed by reading `config.py`'s own `grabfile` +
  `stack_lists(archlist, incremental=1)` call, ~line 962), the profile's
  own `ARCH`, every masked/forced flag (`use.mask ∪ use.force`), and the
  literal `build`/`bootstrap` flags used by `bootstrap.sh`. `x86` is a
  real, valid entry in real Gentoo's own `arch.list` even on an `amd64`
  profile -- just not the *active* arch, so it stays disabled, and
  `mesa`'s `REQUIRED_USE` (which only *references* `x86`, doesn't
  require it enabled) is satisfied once the flag is merely recognized as
  valid. This pilot's own `iuse_set` (`portage-repo`'s
  `resolve_pretend_graph`, the single call site feeding
  `check_required_use`) was built purely from a package's own literal
  `IUSE` metadata, with no implicit-IUSE concept at all. Fixed by adding
  a new `portage_profile::Config::archlist` field (read the exact same
  chain-stacking way `use.mask`/`use.force` already are, reusing the
  existing `stack_mask_lines` helper unmodified) and unioning
  `archlist ∪ use_mask ∪ use_force ∪ {"build", "bootstrap"}` into the
  `iuse_set` right before the `check_required_use` call -- ported in
  lockstep to the Python reference (which calls the *real*
  `portage.dep.check_required_use` directly, so this union is the only
  change needed there). Deliberately out of scope: `USE_EXPAND_HIDDEN`-
  derived regex-pattern implicit flags (`elibc_.*`/`kernel_.*`/
  `userland_.*`) -- a bigger, separate feature this pilot doesn't
  otherwise model at all (no `ELIBC`/`KERNEL`/`USERLAND` support). New
  fixture `dev-libs/archiuseimplicitpkg` (`IUSE=""`,
  `REQUIRED_USE="!x86"`, with a new `profiles/base/arch.list` declaring
  `amd64`/`x86`/`arm64`) mirrors `mesa`'s exact shape and was confirmed,
  by temporarily reverting the fix, to reproduce the identical
  `"USE flag 'x86' is not in IUSE"` failure -- and confirmed live that
  `mesa` itself now resolves cleanly against the real, installed system.

  **Global `use.force`/`use.mask` must win over `package.use`.** Found
  by reading real `config.py`'s own `regenerate()` end to end while
  scoping a broader "real per-source `USE_ORDER` precedence" slice:
  `myflags.update(self.useforce)` followed by
  `myflags.difference_update(self.usemask)` (~line 3024) runs as the
  literal *last* step of the incremental USE walk, strictly *after* the
  `pkg` (`package.use`) tier -- and `setcpv()` confirms
  `self.useforce`/`self.usemask` are themselves
  `getUseForce(pkg)`/`getUseMask(pkg)`: *global* `use.force`/`use.mask`
  combined with the atom-scoped `package.use.force`/`.force` this pilot
  already applies last. This pilot previously folded global
  `use_force`/`use_mask` into `base` early, inside
  `portage_profile::resolve_config` (alongside `defaults`/`conf`), well
  *before* `package.use` ever ran in `effective_use_flags` -- so a
  `package.use` entry could incorrectly override a global force/mask
  decision real portage never lets it override. Fixed by no longer
  folding `use_force`/`use_mask` into `use_flags` in `resolve_config`
  (they stay exposed as their own `Config` fields, unchanged for other
  consumers like `--newuse`'s `forced_flags`) and applying them in
  `effective_use_flags`'s existing final force/mask block instead,
  alongside the already-correctly-positioned atom-scoped
  `package_use_force`/`package_use_mask` -- force-add first, then
  force-remove, so a flag in both ends up masked, not forced, exactly
  like real portage. New fixture `dev-libs/globalprecedencepkg`
  (`IUSE="globalforceflag globalmaskflag"`, its own `package.use` entry
  `-globalforceflag globalmaskflag` -- an attempted inversion of both)
  resolves to `USE="globalforceflag -globalmaskflag"`: the profile's own
  `use.force`/`use.mask` win on both flags regardless of what
  `package.use` tried.

  **A profile-level `-flag` must genuinely cancel an IUSE `+default`.**
  The second, larger half of the same "real per-source `USE_ORDER`
  precedence" slice: real `regenerate()` runs *one continuous*
  incremental walk across the whole reversed `uvlist`
  (`pkginternal` -> `defaults` -> `conf` -> `pkg` -> ...), so a genuine
  `-flag` token in a profile's own `make.defaults` or `make.conf` really
  does cancel an earlier `pkginternal` `+flag` -- exactly like any other
  incremental variable. The IUSE-defaults slice earlier in this README
  documented a narrower scope cut here: `effective_use_flags` union-ed
  the already-*flattened* `defaults`/`conf` result (`base`) on top of
  the IUSE-defaults seed, so `base` could only ever *add* a flag, never
  explicitly cancel one real portage's own walk could. Closed by
  exposing `portage_profile::Config::use_tokens` -- the *ordered raw*
  `USE=` value strings that produced `use_flags` (every profile level's
  own `make.defaults`, in chain order, then `make.conf`, then every
  `USE_EXPAND`/`USE_EXPAND_UNPREFIXED` variable's own value), not yet
  collapsed into a flat set -- and having `effective_use_flags` replay
  `use_tokens` directly via `apply_incremental` on top of the
  IUSE-defaults seed, instead of union-ing the pre-flattened
  `use_flags`. `resolve_config` keeps both in sync (same calls populate
  both); `use_flags` itself is untouched and still used elsewhere (e.g.
  `--newuse` comparisons). New fixture `dev-libs/cancelledpkg`
  (`IUSE="+cancelme"`, with a new profile-level `-cancelme` in
  `profiles/default/make.defaults`, chosen so it's a pure no-op for
  every *other* fixture) resolves to `USE="-cancelme"` -- under the old
  union-based behavior this would have stayed enabled, since a flat
  union can never see a `-flag` that was already collapsed away.

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
  scope at the time, all confirmed real, named corners: `USE_EXPAND_UNPREFIXED`
  (closed by a later follow-up below),
  IUSE-aware wildcard expansion (needs a specific package's own IUSE,
  which global config resolution has no access to, still open), and
  `USE_EXPAND_HIDDEN`/`_IMPLICIT` (originally called `emerge --info`
  display-only and out of scope -- both since closed: `_IMPLICIT` drives
  `is_valid_flag`, not just display, and `_HIDDEN` is honoured by
  `emerge -pv`'s own `USE_EXPAND` grouping -- see the dedicated
  paragraphs below). **Now stale**: `package.use`'s own `USE_EXPAND`-prefix
  shorthand (`VIDEO_CARDS: nvidia` lines) used to be listed here as a
  separate, not-yet-ported follow-up -- see the dedicated paragraph
  further below for the follow-up that closed it. `dev-libs/useexpandpkg` (`IUSE="video_cards_nvidia
  video_cards_amdgpu"`, RDEPEND gated on each) proves the expanded flag
  genuinely drives dependency recursion, not just USE display:
  `video_cards_nvidia` (declared by `profiles/base/make.defaults`) pulls
  in its dependency, `video_cards_amdgpu` (never declared anywhere)
  doesn't.

  **`USE_EXPAND_UNPREFIXED`**: closes the cut named just above, grounded
  against real `config.py`'s own companion mechanism to `USE_EXPAND`:
  the exact same variable-NAME accumulation (`apply_incremental` on the
  `USE_EXPAND_UNPREFIXED` key itself, across the profile chain and
  `make.conf`) and the exact same "last-level-wins" scalar read for each
  named variable's own value this pilot's own `USE_EXPAND` already uses
  -- the *only* difference is that the value's own tokens fold into
  `use_flags` with **no prefix at all**, not `lowercase(varname)_`.
  This is a real, load-bearing mechanism, not an edge case: real
  Gentoo's own `profiles/arch/amd64/make.defaults` sets
  `USE_EXPAND_UNPREFIXED="ARCH"`, which is literally how `amd64`/`x86`/
  `arm64`/etc. exist as ordinary USE flags at all (there is no other
  mechanism that defines them). The fixture's own `profiles/arch/amd64/
  make.defaults` (which already declared `ARCH="amd64"`, feeding
  `ACCEPT_KEYWORDS` via `${ARCH}` substitution, since the very first
  profile-chain slice) now also declares `USE_EXPAND_UNPREFIXED="ARCH"`,
  completing a mirror of the real tree its own comment already claimed
  to be -- verified this doesn't collide with any existing fixture
  package's own `IUSE` first (none declares `amd64` as a flag). New
  fixture package `dev-libs/archusepkg` (`IUSE="amd64 riscv"`, RDEPEND
  gated on each) proves the unprefixed flag genuinely drives dependency
  recursion the same way `useexpandpkg` above already proves for the
  prefixed case: `amd64` (now a real global USE flag) pulls in its
  dependency, `riscv` (never set by anything) doesn't.

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
  the time. (A *later* slice -- the `-pv` `USE_EXPAND` grouping below --
  did add `PYTHON_TARGETS` to the fixture profile's `USE_EXPAND`, so
  that this same fixture's `-pv` line groups under `PYTHON_TARGETS="…"`;
  that has no effect on the shorthand itself.)

  **`emerge --pretend -v`: `USE_EXPAND` grouping (+ `USE_EXPAND_HIDDEN`).**
  The `-v` USE line used to be a flat `USE="video_cards_nvidia
  -video_cards_amdgpu …"`. Real `output.py::_display_use` /
  `map_to_use_expand` / `output_helpers.py::_create_use_string` instead
  split the IUSE-declared flags into the plain `USE` group plus one
  `VAR="…"` group per `USE_EXPAND` variable whose `lowercase(name)_`
  prefixes the flag (prefix stripped), omitting any `USE_EXPAND_HIDDEN`
  group (`remove_hidden`) and any group that came out empty (the
  `if ret:` guard -- so a package whose flags are *all* `USE_EXPAND`
  flags shows only `VIDEO_CARDS="…"` and no `USE=""`). New
  `portage_repo::build_use_expand_display` ports exactly that, carried
  on a new `GraphEntry::use_expand_display` field (the raw flat
  `use_flags_display` stays for `--json`'s own `use_flags` map -- more
  useful programmatically, and real `--json` has no USE display to
  match). `portage-profile` grew `Config::use_expand_hidden` (a new
  incremental, `USE_EXPAND_HIDDEN`), display-only -- never consulted by
  `iuse_effective`/visibility/resolution. Deliberately *not* ported at
  the time (separate cuts, each closed by a later slice): real portage's
  ANSI colorization; its installed-vs-new `*`/`%` diff markers (closed by
  the next slice); the enabled-first within-group ordering (closed by the
  "`emerge --pretend -v`: enabled-first USE order + `--alphabetical`"
  slice below -- until then this kept the pilot's own bare-name sort).
  Fixture
  profile's `USE_EXPAND` gained `PYTHON_TARGETS` (so
  `dev-libs/packageuseexpandpkg` groups) and `CPU_FLAGS_X86` with
  `USE_EXPAND_HIDDEN="CPU_FLAGS_X86"`; new `dev-libs/hiddenexpandpkg`
  (`IUSE="cpu_flags_x86_sse2 cpu_flags_x86_avx"`, `sse2` really enabled)
  shows *no* `-pv` USE line at all. `dev-libs/useexpandpkg` prints
  `VIDEO_CARDS="nvidia -amdgpu"`, `dev-libs/wildexpandpkg` `LINGUAS="de
  -en"`.

  **`emerge --pretend -v`: installed-vs-new USE markers.** The rest of
  real `output_helpers.py::_create_use_string`. For an entry that
  replaces an installed one (`Upgrade`/`Downgrade`/`Reinstall` -- real
  `pkg_info.previous_pkg is not None`, `is_new` false), each flag is
  diffed against the installed version's own vdb-recorded `USE`/`IUSE`
  and gets a suffix: `flag%*` (enabled, brand-new IUSE flag), `flag*`
  (enabled, was off), `-flag%` (disabled, brand-new IUSE flag), `-flag*`
  (disabled, was on) -- and an *unchanged* flag is dropped from the line
  entirely **[superseded by the "`all_flags`" slice below -- `emerge -pv`
  always shows every flag]**.
  `build_use_expand_display` grew an `installed: Option<&InstalledUseState>`
  parameter (the call site fills it from `read_vdb_flag_set` for the
  installed version, `None` for a `New`/`AlreadyInstalled` entry -> every
  flag shown plain, unchanged). Named as this slice's boundary when it
  was scoped, still cut then: ANSI color, the `( … )` forced/masked wrap
  (closed by the next slice), and the `(-flag%)` "removed from IUSE"
  line (closed by the "`all_flags`" slice below). New
  `dev-libs/upgradeusepkg` (installed 1.0 `IUSE="+keep change
  drop"` / `USE="keep change"`, 2.0 ebuild `IUSE="+keep -change +added"`)
  prints `USE="added%* keep -change* (-drop%)"`; `dev-libs/reinstallpkg`'s
  own `-v` line goes from `USE="foo"` to `USE="foo*"`.

  **`emerge --pretend -v`: the `( … )` forced/masked wrap.** Real
  `_display_use` builds `self.forced_flags = pkg.use.force |
  pkg.use.mask` and `_create_use_string` wraps any such flag's rendered
  token in `( … )` -- it's not under the user's control. New
  `portage_repo::forced_or_masked_flags` computes that set for a
  candidate from the exact same `use.force`/`use.mask` +
  `package.use.force`/`.mask` (+ stable variants when stable) layering
  `effective_use_flags` already applies (reusing
  `specificity_ordered_flags`, so a more-specific `-flag` cancels a
  less-specific force/mask), intersected with the candidate's IUSE.
  `build_use_expand_display` grew a `forced: &HashSet<String>` parameter;
  besides the `( )` wrap it also suppresses the trailing `%` on a
  `-flag%` (real: `if flag not in iuse_forced: flag_str += "%"` -- a
  masked brand-new IUSE flag renders `(-flag)`, not `(-flag%)`). The
  existing `dev-libs/pkgusemaskforcepkg` fixture (`forceflag`
  force-enabled, `maskflag` masked, `specflag` masked-then-unmasked by a
  more-specific atom) now prints `USE="(forceflag) (-maskflag)
  -specflag"` -- `specflag` stays unwrapped, proving the wrap tracks the
  *resolved* force/mask set, not the raw entries. Still cut: ANSI color,
  and the `--all-flags` "removed from IUSE" line **(closed by the
  "`all_flags`" slice below)**.

  **`emerge --pretend -v`: enabled-first USE order + `--alphabetical`.**
  Until this slice `build_use_expand_display` rendered each `USE=`/`VAR=`
  group's flags fully bare-name-sorted -- but real
  `output_helpers.py::_create_use_string` joins `" ".join(enabled +
  disabled)`: the enabled flags first, then the disabled ones, each
  alphabetical within its half. `--alphabetical` (real `main.py`'s own
  plain boolean, `conf.alphabetical`) is the *only* thing that collapses
  the two back into one interleaved list -- `_create_use_string` then
  aliases `disabled = enabled` and joins just `enabled`. `build_use_
  expand_display` now does the enabled-first split (a stable
  `sort_by_key(|(en, _)| !*en)` over the already-name-sorted flags);
  `pretend.rs::use_suffix` grew an `alphabetical` parameter (threaded
  through `print_entry_line`/`print_tree`) that, when set, re-sorts each
  group's already-rendered tokens by their bare flag name (new
  `use_flag_sort_key`: strip a leading `(`/`-` and trailing `)`/`*`/`%`).
  `--json`'s own `use_flags` map is untouched -- it's the pilot's
  structured representation, and real `--json` has no USE display to
  match. Two pinned fixtures flipped: `dev-libs/iusedefaultpkg` now
  `USE="enableddefault plainflag -disableddefault"` (was fully
  alphabetical), `dev-libs/useexpandpkg` `VIDEO_CARDS="nvidia -amdgpu"`.
  Still cut (unchanged): ANSI color and real portage's *natural*
  within-group sort (`_alnum_sort_key` -- this pilot's plain lexicographic
  only differs on e.g. `python3_9` vs `python3_12`).

  **`emerge --pretend -v`: `all_flags` -- the diff shows every flag.**
  Real `_DisplayConfig` sets `verbosity = 3` whenever `--verbose` is
  given (`output_helpers.py:180-187`), so `all_flags = (verbosity == 3)`
  is *always* true for `emerge -pv`. That changes the
  `Upgrade`/`Downgrade`/`Reinstall` USE diff: `_create_use_string` shows
  *every* flag, not just the changed ones -- an unchanged enabled flag
  renders `flag` (plain, was dropped from the line before), an unchanged
  disabled flag renders `-flag`, and a flag the new ebuild dropped from
  IUSE renders `(-flag%)` / `(-flag%*)` (real `removed_iuse`, rendered
  after the enabled and disabled groups). `render_flag` gained a
  three-state `FlagState` (`Enabled`/`Disabled`/`Removed`);
  `build_use_expand_display` now also walks `old_iuse \ cur_iuse` for the
  removed tokens and ranks the three states 0/1/2 for the within-group
  order. An `Upgrade` whose USE didn't change now shows the full
  `USE="…"` line too (real `_create_use_string` returns it non-empty
  under `all_flags`). The `upgradeusepkg` fixture's own `-v` line goes
  from `USE="added%* -change*"` to `USE="added%* keep -change* (-drop%)"`
  -- `keep` unchanged-on, `(-drop%)` gone from IUSE. `reinst_flags` (the
  extra per-flag reinstall force) is still not modelled; it only widens
  what `all_flags` already shows. ANSI color stays the sole remaining
  `_create_use_string` cut.

  **`emerge --pretend`: real `PkgAttrDisplay` bracket layout + `[old-ver]`
  column (increment 1 of the `-pv` real-`output.py` layout + ANSI-color
  buildout, confirmed with the user before implementing).** Until this
  slice the pilot kept a deliberately compact bracket (`[ebuild  N]`,
  `[ebuild  U] cat/pkg-2.0 (upgrade from 1.0)`, `[ebuild  r] cat/pkg-1.0
  (reinstall for changed dependencies)`) -- readable, but visibly not
  real `emerge`. The user chose (via `AskUserQuestion`) to adopt real
  portage's actual layout rather than just paint the compact one. This
  increment lands the **structure**, no color yet: `attr_display_field`
  (new, `pretend.rs` + `_attr_display_field`, `emerge_pretend_reference.py`)
  ports real `PkgAttrDisplay.__str__` (`output_helpers.py:603-650`) --
  the fixed-width status field `[I][N/r][S/R][f/F/g][U][D]` (+ a 7th mask
  column that this slice gated on `-v`; **corrected 2026-08-30**, see
  "the bracket mask column is present at plain `-p` too" below -- real
  `include_mask_str` = `verbosity > 1` and default `emerge -p` verbosity
  is 2, so the column is always there bar `--quiet`), one column
  per attribute, a literal space where absent. `[ebuild  N]` becomes
  `[ebuild  N     ]` (both `-p` and `-pv`, post-correction);  an in-slot
  upgrade is `[ebuild     U  ] cat/pkg-2.0 [1.0]` (real `_set_no_columns`
  `f"[{type} {attr}] {indent}{pkg_str} {oldbest}"` -- `oldbest =
  blue("[from]")` from `convert_myoldbest`, replacing the `(upgrade from
  X)` prose); a downgrade adds `D` (`[ebuild     UD ]` post-correction); a plain reinstall
  is `[ebuild   R    ]` with **no** inline reason (real `_get_installed_best`
  sets `attr.replace` -- the `R` -- only when the exact cpv is already
  installed, and `emerge -pv` genuinely shows no "why" for a reinstall;
  the pilot's `(reinstall for …)` prose and `reinstall_reason` helper are
  dropped -- a `--changed-use` reinstall still shows its USE diff in the
  `USE="…"` section, `--changed-deps`/`--changed-slot` reasons are
  invisible in real `-pv` too). `_set_no_columns`' trailing ` {oldbest}`
  is faithful: a New/Reinstall line with no `oldbest` really does end in a
  space. `columns_line` takes the same field. `_reinstall_reason` deleted
  both sides; `use_suffix` drops to a 1-space prefix (the join space now
  comes from the always-present `oldbest` slot); `root_suffix` returns a
  bare `"to /"`. ~247 pinned `[ebuild …]` contract assertions re-pinned
  (589 pretend + 831 total green). **Deferred within this increment**
  (both **shipped 2026-08-29** -- see "`emerge -pv`: `:slot`/`::repo` on
  the bracket cpv" below): the other-slot version list for a new-slot
  install (`myoldbest = installed_versions`) and verbosity-3
  `:slot`/`::repo` on the cpv. **Follow-up increments**:
  colour primitive + `--color=y|n` gating, then USE-flag colours, then
  counters/cleanup/autounmask/columns-tree colour.

  **`emerge --pretend --color y|n`: the ANSI colour primitive + gating +
  bracket-line colours (increment 2 of the same buildout).** New
  `portuale/src/color.rs` ports the slice of `lib/portage/output.py` the
  pretend renderer needs: the `rgb_ansi_colors[i]` -> `ansi_codes[i]`
  table (`output.py:68-92` -- `ansi_codes` is `[30m, 30;01m, 31m,
  31;01m, …]`; `green` is `\x1b[32;01m`, `darkgreen` `\x1b[32m`, and so
  on), `colorize()` (a `codes` key wraps directly, a `_styles` key
  resolves to its colour-name first, always `+ codes["reset"]`
  (`\x1b[39;49;00m`)), the `_styles` entries the renderer reaches
  (`PKG_MERGE*`, `BAD`, `WARN`), and `nc_len()` (visible width with ANSI
  stripped). `resolve_havecolor` ports real `actions.py:2816-2828` +
  `util.no_color`: off, then on unless `NO_COLOR` is set or `NOCOLOR` is
  `yes`/`true`; an explicit `--color y|n` (real `main.py:421`'s
  `choices=("y","n")`, a required value) overrides everything; otherwise
  also off when `TERM=dumb` or stdout isn't a tty. The contract suite
  captures stdout through a pipe, so `havecolor` is false there and every
  existing pinned assertion stays plain; new `--color=y` cases (which win
  over the gate) pin the exact escape sequences.

  The bracket line is coloured per real `output.py`: the type word and
  `pkg.cp` both via `Display.pkgprint` -- `PKG_MERGE_WORLD` (green) for a
  directly-requested / world-file package, `PKG_MERGE_SYSTEM` /
  `PKG_MERGE` (darkgreen) otherwise, `PKG_BINARY_MERGE_WORLD` (fuchsia) /
  `PKG_BINARY_MERGE` (purple) for a binary; `system` wins over `world`,
  exactly as real. `check_system_world` is narrowed to what this pilot
  has: `world` = a favorite (a directly-requested target -- no
  `--oneshot` here, so a favorite is always world-bound) or a
  `var/lib/portage/world` atom match; `system` = a `@system`
  (`config.system_packages`) atom match (slot-qualified `@system` atoms
  match version-only -- a colour-only miss). `PkgAttrDisplay.__str__`'s
  own per-letter colours land too (`green("N")`, `yellow("R")`,
  `turquoise("U")`, `blue("D")`, `colorize("WARN", "I")`, the `#`/`*`
  mask `BAD` and `~` `WARN`), plus `blue("[old-ver]")` and
  `darkgreen("to <root>")`. `columns_line` measures padding with
  `nc_len` so a coloured `--columns` line aligns identically to a plain
  one. New `color.rs` unit tests (escape codes, `nc_len`, the palette,
  the gate); a dedicated `--color=y` pinned-output contract test + 7
  `CASES` entries. **Follow-up increments**: USE-flag colours
  (`build_use_expand_display`), then counters/cleanup/autounmask colour;
  blocker-line colour rides along with the deferred real blocker layout.

  **`emerge --pretend -v --color y`: the `USE="…"` flag colours
  (increment 3 of the buildout).** Real `_create_use_string`
  (`output_helpers.py:262-334`) colours each flag by its diff state:
  `red(flag)` for a plain enabled flag, `blue("-"+flag)` for a plain
  disabled one, `yellow` for a flag newly in IUSE (`flag%*` / `-flag%`),
  `green` for one whose polarity flipped (`flag*` / `-flag*`), `yellow`
  again for a `removed_iuse` `(-flag%)` -- and only the `flag`/`-flag`
  *core* is wrapped, never the `*`/`%` markers or the `( )` forced/removed
  wrap (real `yellow(flag) + "%*"` appends the markers *after* the
  `colorize` call). Since the marker suffix and sign fully determine the
  colour, this pilot applies it as a render-time pass over the
  already-rendered tokens (`pretend.rs::colorize_use_token` /
  `_colorize_use_token`) rather than threading colour back into
  `build_use_expand_display` (which runs at resolve time, before
  `--color` is known) -- the same "post-hoc token-shape parse" the
  `--alphabetical` re-sort already uses, and colour is applied *after*
  that sort so the sort key still sees plain tokens. One documented
  imperfection, unreachable by any fixture: a forced *disabled* flag
  newly in IUSE on an Upgrade renders `(-flag)` (the pilot's own
  `render_flag` drops the `%` for forced flags) and is coloured `blue`
  here where real portage would `yellow` it. A dedicated pinned contract
  test (the New red/blue case + the Upgrade `added%*`/`keep`/`-change*`/
  `(-drop%)` case) + 2 `CASES` entries. **Still open**: counters-line /
  `-pc`/`-pC`/`-pP` / autounmask colour (increment 4); blocker-line
  colour rides along with the deferred real blocker layout.

  **`emerge --pretend --color y`: the counters line + cleanup-action
  colour (increment 4 of the buildout).** Real `_PackageCounters.__str__`
  colours only two spots on the `Total:` line: `colorize("WARN",
  "interactive")` (just the word) and `bad(f" (N unsatisfied)")` after
  `Fetch Restriction:` -- both ported. The standalone cleanup actions
  (`-pC`/`-pc`/`-pP`) get real `_emerge/unmerge.py::_unmerge_display` +
  `action_depclean` colour: `darkgreen(">>> These are the packages that
  would be unmerged:")`, each `selected:` version `colorize("UNMERGE_WARN",
  …)` (red) and each `protected:`/`omitted:` version `colorize("GOOD",
  …)` (green), the `!!! … is part of your system profile.` `colorize("BAD",
  …)` and its `!!! Unmerging it may be damaging…` follow-up
  `colorize("WARN", …)`, the `Package … is going to be unmerged,` /
  `but still listed in the following package sets:` pair
  `colorize("WARN", …)`, and the `>>> 'Selected'` / `>>> 'Protected'` /
  `'omitted'` legend words `UNMERGE_WARN` / `GOOD`. The `-pc` advisory
  block matches real `action_depclean`: every line is `colorize("WARN",
  " * ")` (yellow) + text, and each backtick-wrapped command inside the
  text is `good("`…`")` (green). `--color` is now resolved once, early in
  `run`, so the standalone-action dispatch and the resolve-graph path
  share one `Colorizer`. Real `show_parents` (`-pc`/`-pP --verbose`) has
  **no** colour -- left plain, faithfully. New `_styles`: `UNMERGE_WARN`,
  `INFORM`, `MERGE_LIST_PROGRESS`; new `bold` code. A dedicated pinned
  contract test (`-pC` selected-red / legend / system-warning + the
  counters `interactive` word) + 3 `CASES`. **Update 2026-08-30**: the
  real `--autounmask` block itself (not just its colour) shipped for the
  keyword kind -- see "`emerge --pretend --autounmask`: real keyword
  *resolution*" below; its header is `colorize("BAD", …)` and the change
  line `colorize("INFORM", …)`, matching real `_display_autounmask`.
  Blocker-line colour also shipped (increment 5). This completes the
  `-pv` layout + colour buildout bar the USE half of the autounmask
  block (increment 2).

  **`emerge --pretend -v`: the `[ebuild N ~]` bracket-mask marker.** Real
  `output.py::gen_mask_str` (this slice gated it on `-v`; **corrected
  2026-08-30**, see "the bracket mask column is present at plain `-p`
  too" below -- `include_mask_str` = `verbosity > 1` and default `emerge
  -p` verbosity is 2) gives the bracket a one-character column right after
  the `N`/`U`/`D`/`r` code letter, for a package that's being installed
  *despite* not being visible via the global `ACCEPT_KEYWORDS` alone:
  `#` if it's hard-masked somewhere but was `package.unmask`'d anyway
  (`isHardMasked`, checked first -- and it deliberately ignores
  `package.unmask`), `~` if visible only via a `~<our-arch>` testing
  keyword (`get_keyword_mask` "unstable"), `*` if visible only via `**`
  or a different arch's keyword ("missing"). New
  `portage_repo::keyword_mask_marker` ports that: hard-mask via the
  provenance `mask_entry` this pilot already computes; then
  `keywords_accepted` against the *global* `ACCEPT_KEYWORDS` alone (empty
  `package.accept_keywords`) to decide "needs help at all"; then a
  `~<arch>`-in-`ACCEPT_KEYWORDS` check off the candidate's own `KEYWORDS`
  to split `~` from `*` (a deliberate narrowing of real
  `getRawMissingKeywords`, sufficient for single-arch). Carried on a new
  `GraphEntry::keyword_mask: Option<char>`; `pretend.rs`'s new
  `mask_suffix` appends it (` ~`) inside the compact bracket (`-v` only
  as this slice shipped it; the later `attr_display_field` rework folded
  it into the 7th fixed-width column, present at plain `-p` too).
  Existing fixtures: `dev-libs/bareacceptkeywordspkg` (`~amd64`) ->
  `[ebuild  N ~]`, `dev-libs/tildestarkeywordpkg` (`~arm64` via `~*`) ->
  `[ebuild  N *]`, `dev-libs/maskedandunmaskedpkg` -> `[ebuild  N #]`.

  **`emerge --pretend`: the `[ebuild NS]` new-slot marker (+ a
  slot-aware-matching correctness fix).** Real
  `output.py::_get_installed_best` sets `attr_display.new_slot` (the `S`
  bracket column, next to `N`) when the resolved candidate's own
  category/package *is* installed but `not myinslotlist` -- nothing in
  the candidate's own slot (`vardb.match(pkg.slot_atom)`, main slot
  only, sub-slot ignored). Grounding this turned up that `resolve_pretend`
  answered "is this candidate already installed?" against *all* installed
  versions regardless of slot: `emerge -p dev-libs/foo:1` with only
  `foo:0` installed wrongly returned `[ebuild  U] foo-2.0 (upgrade from
  1.0)` (both Rust and the Python oracle agreed -- both wrong; real
  portage: `[ebuild  NS] foo-2.0`, a `New` into a fresh slot with no
  "from"). The fix filters the installed-version set to the resolved
  candidate's own main slot at every "already installed" decision point
  in `resolve_pretend` (`--exclude` keep, the `!update` selective
  shortcut, the Reinstall/AlreadyInstalled branch, and the
  Upgrade/Downgrade/New branch); `dependency_avoid_update_candidate`'s
  own `avoid_update` matching stays version-only across slots, a
  documented residual. New `GraphEntry::new_slot: bool` (Python: stashed
  on the `provenance` dict like `keyword_mask`), set in
  `resolve_pretend_graph` for a `New` entry whose cp has any installed
  candidate; `pretend.rs` renders `S` right after the `N` letter
  unconditionally (not `-v`-gated, unlike the mask column), and `--json`
  carries `"new_slot"` on every `new` entry. New fixture
  `dev-libs/newslotpkg` (`-1.0` SLOT 0 installed, `-2.0` SLOT 1 not):
  `:1` (or the bare atom, non-selective) -> `[ebuild  NS]
  dev-libs/newslotpkg-2.0`; `:0` stays an in-slot outcome.

  **`emerge --pretend`: the `[ebuild I..]` interactive bracket column.**
  Real `output.py:833`: `if "interactive" in pkg.properties and
  pkg.operation == "merge": pkg_info.attr_display.interactive = True`,
  and `PkgAttrDisplay.__str__` renders `I` *before* the `N`/`r` code
  letter. `pkg.properties` is `PROPERTIES` after real USE-conditional
  evaluation against the candidate's own effective USE
  (`_PackageMetadataWrapper.__getitem__`, gated on `"?" in v` -- the
  same "resolve USE only when it could matter" shortcut this pilot
  already applies to `LICENSE`/`PROPERTIES`/`RESTRICT` masking). New
  `portage_repo::evaluated_metadata_tokens` (Rust: `use_reduce_flat`
  with the candidate's `use_flags_if_conditional` USE; Python: real
  `use_reduce(..., flat=True)`) returns the evaluated token set;
  `resolve_pretend_graph` sets `GraphEntry::interactive` (Python:
  stashed on `provenance` like `keyword_mask`/`new_slot`) for a
  merge-bound entry (`New`/`Upgrade`/`Downgrade`/`Reinstall` -- the only
  outcomes `resolved_slots` indexes, so real portage's `pkg.operation ==
  "merge"` needs no separate check) whose evaluated `PROPERTIES` contains
  `interactive`. `pretend.rs` prepends `I` to the code letter in every
  merge arm (`[ebuild  IN]`, `[ebuild  IU]`, `[ebuild  ID]`, `[ebuild
  Ir]`, plus `[ebuild  INS]`), unconditional like the `S` column;
  `--json` carries `"interactive"` on every merge-bound entry. New
  fixtures: `dev-libs/interactivemergepkg` (`PROPERTIES="interactive"`)
  -> `[ebuild  IN]`; `dev-libs/interactivecondpkg`
  (`PROPERTIES="gtk? ( interactive )"`, `gtk` off) -> plain `[ebuild
  N]`, proving the conditional gates it out; `dev-libs/
  interactiveinstalledpkg` (installed) -> `[ebuild  Ir]` on a bare
  reinstall.

  **`emerge --pretend -v`: the `Total:` counters summary line.** Real
  `output.py::display`'s own `if self.conf.verbosity == 3:
  self.print_verbose(...)` -> `writemsg_stdout(f"\n{self.counters}\n")`,
  i.e. `_PackageCounters.__str__` (`output_helpers.py`). Gated -- in
  real portage too -- on `verbosity == 3`, so `-pv` only, never plain
  `-p`. New `package_counters_summary` (`pretend.rs`, mirrored in
  `emerge_pretend_reference.py`) reduces the resolved graph's own
  outcomes into `Total: N package[s][ (A upgrade[s], B downgrade[s], C
  new, D in new slot[s], E reinstall[s], F binar{y,ies}, G interactive)]`
  plus a trailing `Conflict: N block[s]` line, faithful to real
  `__str__`'s exact pluralization (`total != 1` -> `packages`; `> 1` ->
  `s` for the rest; `binary`/`binaries`; `newslot` counts toward the
  total but renders as `in new slot`). A `New` entry with `new_slot`
  counts as `newslot` not `new` (real `output.py:763`); `binary` and
  `interactive` are additive over their merge-bound entries; an
  `--onlydeps`-suppressed top-level package isn't counted (real portage
  drops it from the merge list). Printed after the entry list for the
  flat/`--columns`/`--tree` layouts alike, with a leading blank line.
  **Cut**: the
  `Conflict:` line's own `(N unsatisfied)`/`(all satisfied)` suffix --
  this pilot resolves no blocker (report, don't enforce), so it can't
  honestly classify one. ~18 existing `-pv` pinned-output contract
  tests updated for the new trailing line. Then (once the `f`/`F`
  fetch-restrict slice built the machinery) completed with `, Size of
  downloads: N KiB` and the `Fetch Restriction:` line -- see the next
  paragraph.

  **`emerge --pretend -v`: `Size of downloads` + `Fetch Restriction:`,
  completing `_PackageCounters`.** Real `output.py:300-332`'s own
  `_calc_size` sums `counters.totalsize` from
  `db.getfetchsizes(cpv, useflags=pkg.use)` (no `only_restricted`) over
  every merge-bound entry -- the Manifest bytes of each `SRC_URI`
  distfile not already in `DISTDIR` at that size, a shared distfile
  counted once (real `myfetchlist`). Ported as
  `GraphEntry::download_files: Vec<(String, u64)>` (new
  `fetch_bytes_to_download`, sharing `flatten_src_uri` +
  `parse_manifest` with the `f`/`F` helper), summed with a
  filename-`HashSet` dedup in `package_counters_summary`, formatted by
  `localized_size` (real `portage.localization.localized_size`:
  `ceil(bytes/1024)` KiB, always KiB -- this pilot drops real portage's
  `LC_NUMERIC` thousands grouping of the KiB count, only observable
  above 999 KiB and locale-dependent). The `Fetch Restriction: N
  package[s][ (M unsatisfied)]` line comes straight from the
  `GraphEntry::fetch_restrict` / `fetch_restrict_satisfied` counts.
  Every `-pv` `Total:` line now ends `, Size of downloads: 0 KiB` for a
  no-`SRC_URI` package (real portage always shows it); ~18 more pinned
  tests updated. `--json` is unchanged (`download_files` is a
  display-time detail). Binary candidates contribute 0 (real
  `_calc_size` runs for them too, but this pilot has no remote-binpkg
  fetch -- a local `PKGDIR` binary is always already present). This
  closes `_PackageCounters.__str__`.

  **`emerge --pretend`: the `f`/`F` fetch-restrict bracket column.**
  Real `output.py:633`: `if not pkg.built and pkg.operation == "merge"
  and "fetch" in pkg.restrict: attr_display.fetch_restrict = True`, then
  `if not portdb.getfetchsizes(cpv, useflags=pkg_info.use,
  only_restricted=True): attr_display.fetch_restrict_satisfied = True`.
  `PkgAttrDisplay.__str__` renders it right after the `S`/`R` column:
  green `f` (satisfied -- every distfile already in `DISTDIR`), red `F`
  (some missing -- `emerge` won't auto-download a `RESTRICT=fetch`
  package, you fetch them by hand). At the time this slice shipped it
  completed `PkgAttrDisplay`'s bracket bar `g` (remote binary); `g` has
  since shipped too (see "`emerge -pv --getbinpkg`" below). The `RESTRICT`
  check reuses `evaluated_metadata_tokens` (built for the `interactive`
  slice); `fetch_restrict_files_all_present` (new, `portage-repo`, which
  gained a `portage-fetch` dependency) flattens the candidate's own
  `SRC_URI` against its effective USE (`portage_fetch::flatten_src_uri`,
  the `useflags=pkg_info.use` real portage passes) and checks each file
  against `DISTDIR` (present + `Manifest` `DIST` size match -- an
  unparsable `SRC_URI` or missing `Manifest` entry counts as `F`, the
  loud choice). `resolve_pretend_graph` gained a `distdir` parameter
  (env `DISTDIR`, real `make.globals` default
  `/var/cache/distfiles`); new `GraphEntry::fetch_restrict` /
  `fetch_restrict_satisfied`; `--json` carries both. The Python
  reference grew its own small `_flatten_src_uri` /
  `_manifest_dist_sizes` (a bespoke `SRC_URI` parser mirroring
  `portage-fetch`, not real `use_reduce` -- same "two independent
  implementations" discipline). New fixtures
  `dev-libs/fetchrestrictsatisfiedpkg` / `fetchrestrictmissingpkg` (both
  `RESTRICT="fetch"`) + a committed `PORTING/fixtures/distfiles/`
  (holding only the first's distfile at its `Manifest` size), wired into
  the test `fixture_env`'s `DISTDIR`. (The `, Size of downloads` /
  `Fetch Restriction:` parts of the `-pv` `Total:` line landed in the
  next slice, reusing this machinery.)

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

  **`--emptytree`/`-e`: reinstall the whole deep dependency tree.**
  Grounded against real `create_depgraph_params.py:176-179`: `--emptytree`
  sets `myparams["empty"] = True`, `myparams["deep"] = True`, and
  `myparams.pop("selective", None)`. Real portage then stops selecting
  installed packages as merge-list candidates (`depgraph.py:7889`), so
  every atom in the (now mandatory-deep) tree resolves to a merge. This
  pilot's candidate pool is *already* tree-only -- it consults the vdb
  only to *classify* an outcome -- so `empty` threads through
  `resolve_pretend` as three small changes: it forces `deep` on
  (`Deep::Unlimited`), clears `selective` locally, and turns every "the
  resolved best candidate is already installed at that exact version"
  result -- top-level *or* a dependency reached by the deep walk -- into a
  bare `Reinstall` (real `output.py`: `attr_display.replace` is still set
  from `vardb.cpv_exists`, so `[ebuild   R   ]`, no `[oldver]`, no
  reason -- exactly the pilot's own reasonless `[ebuild R]`). The net
  effect matches real `emerge -e`: `emerge -p --emptytree dev-libs/deeppkg`
  shows `deeppkg` + `deeppkg2` as `[ebuild   R   ]` (both installed) and
  `newpkg` as `[ebuild  N    ]` (not), where a plain `emerge -p
  dev-libs/deeppkg` shows only `deeppkg`'s "nothing to do" line and never
  walks the chain. `-e` alone reinstalls what's installed; `-e -u`
  additionally upgrades a dependency where a newer version exists (same
  `avoid_update` split real portage has). `--emptytree` is a plain
  boolean (real `main.py`'s own `options` list, short alias `e`,
  `main.py:58`), so a bundled `-pe`/`-pev` decomposes the same way every
  other bundled boolean does. Deliberately does not reach `--root-deps`
  running-root build entries (its own resolver path, an exotic
  combination). Threading the new `empty` param touched ~50
  `resolve_pretend`/`resolve_pretend_graph` call sites (all positional,
  the same churn the function's own doc comment already laments for a
  bundled-options refactor). New Rust unit tests
  (`emptytree_forces_an_installed_atom_to_a_bare_reinstall`,
  `emptytree_reinstalls_the_whole_deep_dependency_tree`, …), a dedicated
  pinned contract test, and 7 `CASES`; mirrored in
  `emerge_pretend_reference.py`. **Motivation** (from the request):
  byte-for-byte comparison against real portage and debugging resolution.

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
  BFS happened to resolve first. `source` mirrors the plain-text loop's
  own bracket word (`"ebuild"`/`"binary"`, real `RootConfig.py`'s own
  `pkg_tree_map`-driven `type_name`) -- included since binary-package
  support (`--usepkg`/`--usepkgonly`) was added so a JSON consumer
  doesn't have to assume it's always `"ebuild"` (confirmed with the user
  directly, choosing this over omitting the field entirely, back when it
  genuinely always was). `entry_to_json` originally hardcoded the
  literal `"ebuild"` regardless of the entry's own actual source, a real
  bug left over from before binary-package support existed at all that
  only surfaced once a binary candidate could actually resolve --
  fixed to read the entry's own `source`/`candidate_source` instead.
  Output is deliberately unaffected by `--onlydeps`
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

`--usepkg`/`--usepkgonly`/`--binpkg-respect-use` add a second candidate
source alongside ebuilds: prebuilt binary packages, read straight from
`PKGDIR`'s own `Packages` index file (real `bintree.py`'s own format --
`KEY: value` lines, blank-line-separated blocks, first block a global
header) rather than a real xpak/gpkg archive parser, since the index
alone already carries every field candidate-listing and dependency
recursion need (`CPV`, `IUSE`, baked-in `USE`, `KEYWORDS`, `SLOT`,
`*DEPEND`, `EAPI`, ...). Mirrors two real, non-obvious asymmetries from
`depgraph.py`'s own `_dynamic_depgraph_config.__init__` and
`create_depgraph_params.py:47-55`: `--usepkgonly` excludes ebuild
candidates entirely (no fallback), while `--usepkg` alone just makes
binaries additionally eligible alongside ebuilds; and
`--binpkg-respect-use` (comparing a binary's own baked-in `USE` against
what would currently be selected over its `IUSE`, rejecting mismatches)
defaults *on* under `--usepkg` but defaults *off* under `--usepkgonly`,
since there's no ebuild left to fall back to. Binary candidates get a
deliberately low `repo_priority` (`i32::MIN`/`-inf`) so the existing
`vercmp`-then-`repo_priority` tie-break naturally prefers an
identical-version ebuild, matching real portage's own `dbs` list order
(ebuild checked before binary). `dev-libs/binaryonlypkg` (in
`fixtures/pkgdir/Packages` only, no ebuild anywhere) proves
`--usepkg`/`--usepkgonly` eligibility; `dev-libs/binaryusemismatchpkg`
(binary `USE:` empty, `IUSE="foo"`, but the fixture profile's own global
USE would select `foo`) proves the `--binpkg-respect-use` rejection and
its default asymmetry: `--usepkg` alone falls back to the identical-
version ebuild, `--usepkgonly` accepts the mismatched binary since
there's nowhere else to fall back to. The `[ebuild N]`/`[binary N]`
bracket word itself mirrors real `RootConfig.py`'s own
`pkg_tree_map`-driven `type_name` display. At the time this slice
shipped, `--getbinpkg`/`--getbinpkgonly` (remote binhosts) were out of
scope -- local `PKGDIR` only; the `--pretend` half of `--getbinpkg` has
since shipped (see "`emerge -pv --getbinpkg`" below), still stopping
short of an actual remote download.

`--usepkg-exclude`/`--usepkg-include`: a direct follow-on, grounded
against real `main.py` ("a space separated list of package names or
slot atoms", the identical grammar `--exclude`/`-X` already uses) and
real `depgraph.py`'s own per-candidate binary-eligibility check
(`in_usepkg_exclude = have_usepkg_exclude and usepkg_exclude.
findAtomForPackage(pkg, ...)`; `in_usepkg_include = not
have_usepkg_include or usepkg_include.findAtomForPackage(pkg, ...)`;
`if in_usepkg_exclude or not in_usepkg_include: break`) -- confirmed by
reading it during the original binary-package slice's own research, but
deliberately not acted on until now. A matching `--usepkg-exclude` atom,
or a non-empty `--usepkg-include` list a candidate doesn't match, drops
that binary candidate from the pool entirely, before it's ever
considered alongside ebuilds -- reusing the exact same
`matches_config_entry` two-tier (plain atom or `*`-wildcard) matcher
`--exclude`/`package.mask`/`.unmask` already share.
`dev-libs/binaryonlypkg` (the existing binary-only fixture) proves it:
`--usepkg-exclude dev-libs/binaryonlypkg` makes its only candidate
disappear entirely (`"there are no ebuilds to satisfy"`, since there's
no ebuild to fall back to either); `--usepkg-include` with a
non-matching atom does the same, while a matching one leaves it
eligible. `--rebuilt-binaries` was deliberately left out of this slice:
it needs comparing a binary's own baked-in dependency versions against
the current best plus a `--rebuilt-binaries-timestamp` cutoff, a
meaningfully fuzzier scope than a straightforward include/exclude list.

`--rebuilt-binaries`/`--rebuilt-binaries-timestamp`: the deferred half
above, closed as its own follow-on once properly grounded in real
`depgraph.py` (lines ~8394-8429) -- a new independent reinstall trigger
alongside `--newuse`/`--changed-use`/`--changed-deps`/`--changed-slot`,
comparing a binary candidate's own `BUILD_TIME` against the vdb's own
recorded `BUILD_TIME` for an already-installed, same-version package
("replace installed packages with binary packages that have been
rebuilt", real `main.py`'s own help text -- the common real-world case
being a same-version binary rebuilt against updated dependencies, e.g.
a toolchain/ABI bump, not a version change at all). Real code's own
"skip the check if a newer *source* candidate exists" branch has no
equivalent here: this pilot only ever reaches the check once the best
*visible* candidate already equals what's installed, so nothing newer
can exist by construction. `--rebuilt-binaries-timestamp`, when given,
narrows the comparison from "any difference triggers a reinstall"
(real code's own "don't care ... this is for closely tracking a
binhost" default) to "only a *newer* binary at or above this cutoff"
-- a real, deliberate asymmetry in real portage itself, ported exactly.
The real default-resolution is a second non-obvious asymmetry, this
one from `create_depgraph_params.py` (lines 185-193): `--rebuilt-
binaries` auto-enables with no explicit flag at all whenever
`--usepkgonly`, bare `--deep` (no explicit number), and `--update` are
ALL given together -- confirmed live, including that a *bounded*
`--deep 3` does NOT count as the real "deep is True" bare form and so
correctly does NOT auto-enable. `dev-libs/rebuiltbinarypkg` (installed
at `1.0` with `BUILD_TIME=1000`, a binary candidate at the same version
with `BUILD_TIME=2000`) proves it end to end, including the timestamp
cutoff and the auto-enable default, live-verified with `--selective` to
avoid the unrelated "bare top-level atom always reinstalls" behavior
muddying the baseline.

A real bug fix, found by grounding against real `output.py`'s own
`PkgAttrDisplay` logic (around line 750): before this slice, any version
change for an already-installed package was unconditionally labeled
`Upgrade`, even when the resolved candidate is actually *older* than
what's installed -- real portage distinguishes this with its own
`downgrade` attr, set precisely when the resolved cpv isn't `best()` of
itself plus the installed in-slot version (typically because a newer
version got masked or removed from the tree since the older one was
merged). `PretendOutcome::Downgrade` now exists as its own variant
(`vercmp(to, from) < 0` gates it, right where `Upgrade` used to be
constructed unconditionally), printing `(downgrade from X)` instead of
`(upgrade from X)`, with its own dedicated `D` bracket letter --
deliberately a single letter, not real portage's own stacked `U`+`D`
columns, matching this pilot's established one-letter-per-outcome
scheme. `dev-libs/downgradepkg` (installed at `2.0`, only `1.0` visible
in the tree) proves it live -- and does so even *without* `--update`,
since the installed `2.0` has no visible candidate of its own to satisfy
real `avoid_update`'s own shortcut (see `resolve_pretend`'s own doc
comment), so resolution falls through to the ordinary best-visible-
candidate path unchanged, exactly matching real portage's own comment
about enabling "upgrade or downgrade to a version with visible KEYWORDS
when the installed version is masked."

`--tree`/`-t` and `--unordered-display`: indents each entry under
whichever other entry's own dependency string reached it. Grounded
against real `output_helpers.py`'s own `_tree_display`/
`_ordered_tree_display`/`_unordered_tree_display` -- but, unlike almost
every other slice in this series, explicitly **not** a faithful port of
it: real `_ordered_tree_display` walks a genuine topologically-*
scheduled* merge order (`mylist`) and a real bidirectional digraph
(`parent_nodes`/`child_nodes`) to decide, for each node, exactly which
already-placed node to nest it under, including cycle-avoiding
parent-chasing logic when a fresh top-level branch needs to attach
somewhere -- machinery this pilot has no equivalent of at all, since
there's no merge scheduler (that's task #55's own "real merge/install"
boundary). A deliberate, pilot-specific simplification instead: the
only edges this pilot has are `GraphEntry::required_by` (already "every
distinct owner, sorted"), inverted here into a `children` map (owner
key -> the entries it pulled in), walked from the top-level/requested
entries as roots in their own already-argv-ordered `entries` sequence.
A node already rendered once anywhere in the tree (diamond dependencies
included) is never rendered or recursed into again -- real
`_unordered_tree_display`'s own `seen_nodes` behavior, ported exactly,
and what keeps the recursion from looping forever on a genuine
dependency cycle too, for free. `--unordered-display` (real man page:
"does NOT sort the tree in merging order", only ever meaningful
together with `--tree` -- given alone it's accepted but does nothing,
matching real portage's own `_tree_display`-never-called-otherwise
gating) chooses the child order at each level: `entries`' own natural
BFS discovery order when given -- genuinely "not sorted", using real
already-existing data, no invented bookkeeping -- versus alphabetical-
by-`(category, package)` by default, this pilot's own deterministic
stand-in for real portage's genuine merge-order sort. Both display
modes (flat and tree) now share one `print_entry_line` per-outcome
implementation (indent-parameterized) rather than duplicating the
bracket/reason logic twice. `dev-libs/diamond` (already-established:
`shared-a`/`shared-b` both RDEPEND on `common`) proves the
once-only-rendering rule live -- `common` nests under `shared-a`
only, not repeated under `shared-b`; a new `dev-libs/treeorderpkg`
(RDEPEND deliberately `"dev-libs/ztreechild dev-libs/atreechild"`,
reverse-alphabetical) proves the ordered/unordered distinction itself,
since every pre-existing fixture's own RDEPEND happened to already be
alphabetical. An entry never reached from any root at all (shouldn't
normally happen) still prints, unindented, after the tree rather than
being silently dropped -- this pilot's own "never silently lose
information" invariant, already established for slot conflicts and
unresolvable dependencies.

**Bug fix (2026-08-27): a multi-slot dependency lost its `required_by`
owner on every slot but the first.** The `required_by` merge post-pass
in `resolve_pretend_graph` used a *destructive* `required_by_map.remove(
&(category, package))` -- but `entries` can hold more than one entry per
`(category, package)`, one per resolved slot (`dev-libs/multislotparent`
pulls in `multislotpkg:0` **and** `:1`). The first slot's entry consumed
the owners; every later one was left with `required_by: []`. Visible two
ways: `--tree` dropped the second slot to its flush-left "never reached"
safety net instead of nesting it under the parent, and `--json` reported
`"required_by": []` for it. Fixed to a non-destructive `.get(...)` (the
Python reference already did the equivalent non-destructive dict lookup,
so this was a real Rust-vs-Python divergence -- Python and real portage
were correct). New `required_by_is_set_on_every_slot_of_a_multi_slot_
dependency` unit test + `--tree`/`--json` contract assertions on
`dev-libs/multislotparent`.

`--json`'s own state-change trace: each entry now carries a
`"provenance"` object (`{"mask_entry", "unmask_entry", "keyword_entry"}`)
recording which `package.mask`/`.unmask`/`package.accept_keywords`
config entries, if any, were actually load-bearing for that candidate to
be visible at all -- this pilot's own feature, not a port of any real
emerge output (see `--json`'s own module doc comment for why `--json`
exists in the first place). `mask_entry` is set even when a matching
`unmask_entry` goes on to cancel it, so the trace shows the mask was
there, not just that it didn't end up mattering; `keyword_entry` names
the *specific* `package.accept_keywords` entry needed, found by walking
matching entries in the same least-to-most-specific order
`specificity_ordered_flags` already applies them in and reporting the
first one whose own addition actually flips visibility from false to
true -- not merely the most specific matching entry, which might not
have been the one that mattered. All three fields are `null` (not
omitted) when nothing special was needed. `dev-libs/maskedandunmaskedpkg`
(already-established: masked then unmasked by identical entries) proves
the mask/unmask half live; `dev-libs/wildcardkeywordpkg`
(already-established: `~amd64`-only, visible only via a
`*/wildcardkeywordpkg ~amd64` `package.accept_keywords` entry) proves the
keyword half. Deliberately duplicates a small, stable chunk of
`is_visible`'s own body (`mask_entry`/`unmask_entry`) rather than
threading a reason out of its own hot per-candidate filtering loop --
the same precedent `keyword_masked_only` (the `--autounmask` keyword-
suggestion feature) already set. Computed once per finally-chosen
candidate, not for every candidate considered; `AlreadyInstalled`/
`NoVisibleCandidate` entries never pick a fresh repo/`PKGDIR` candidate
to trace at all, so their own `provenance` is always all-`null`, same
scope cut as `slot`/`use_flags_display`.

> **Superseded 2026-08-30** by "`emerge --pretend --autounmask`: real
> keyword *resolution*" below: the pilot no longer *suggests* a keyword
> change and fails — it applies the implicit `=cpv ~arch` change,
> resolves the graph, and prints the real `The following keyword changes
> are necessary to proceed:` block. The `!!! note:` text and the
> `"no_visible_candidate"` + `"keyword_suggestion"` `--json` shape
> described in this paragraph no longer apply when `--autounmask` is
> given (they still do for the *default*, keyword-keeping behavior).

`--autounmask`'s own keyword-suggestion sub-feature, extended to a
*dependency's* own `NoVisibleCandidate`: this pilot's own v1 (task #51)
only ever suggested something for a top-level atom's own **fatal**
`NoVisibleCandidate` (the one that aborts the whole call) -- a
dependency's own `NoVisibleCandidate` (reported, not fatal -- the graph
still resolves) got no suggestion at all, explicitly called out as
deliberately out of scope in `resolve_pretend_graph`'s own doc comment.
Closes that gap: a `GraphEntry`'s new `keyword_suggestion` field
(`Option<(version, keyword)>`) is computed the same way the top-level
case's own message already was, via a new shared `suggested_keyword_
candidate` helper factored out of what was previously separate inline
logic at both call sites -- unlike `is_visible`/`keyword_masked_only`'s
own deliberate duplication (which trade off genuinely different
questions), these two calls wanted the exact same "best near-miss"
computation, so sharing it was the right call. The plain-text loop
prints it as an extra `!!! note: ...` line right after the existing
`!!! no visible ebuild for dependency "..."` one (same wording as the
top-level case's own message, sharing the fixture:
`dev-libs/autounmaskkeywordpkg`, `~amd64`-masked with no
`package.accept_keywords` help), gated on `--autounmask` exactly like
the top-level case already was. `--json` gets the mirror-image field:
`"keyword_suggestion"` (`{"version", "keyword"}` or `null`) appears only
on a `"no_visible_candidate"` entry, in the very slot `"source"`/
`"provenance"` occupy on every other outcome (those two are absent
there instead -- there's nothing visible to name a source or trace
provenance for). A new `dev-libs/autounmaskdepconsumer` fixture (RDEPEND
on the existing keyword-masked-only package) proves it live, both with
and without `--autounmask`, and in both plain-text and `--json` form.

> **Superseded 2026-08-30** by "`emerge --pretend`: real
> `--autounmask-use` USE *resolution*" below (increment 2): the pilot no
> longer *suggests* a USE flip and reports the dependency unresolvable —
> it applies the flip, resolves the graph, and prints the real `The
> following USE changes are necessary to proceed:` block. The
> `use_suggestion` / `parent_use_suggestion` `--json` fields and the
> `!!! note:` text described in the next three paragraphs only apply now
> under `--autounmask-use=n`.

**`--autounmask-use`, the plain-dependency-atom half.** Real
`create_depgraph_params.py`'s own `--autounmask` family also covers the
far more common real-world case the KEYWORDS-only v1 above deliberately
narrowed away: a candidate that's otherwise the best match except a
parent's plain `pkg[flag]`/`pkg[-flag]` dependency atom doesn't match
its current USE state. Grounded against real `_wrapped_select_pkg_
highest_available_imp` (`lib/_emerge/depgraph.py:8093-8158`, the
`autounmask_level.allow_use_changes` branch) and the real engine behind
it, `_pkg_use_enabled` (`:7657-7785`): a direct, deterministic
one-shot flag flip built straight from the failing atom's own USE-dep,
*not* a search over flag combinations, refused outright if the needed
flag is `package.use.mask`/`.force`'d (masked/forced IUSE can never be
adjusted). Ported as a new sibling to `keyword_masked_only`/
`suggested_keyword_candidate`: `use_masked_only` (candidate is
`is_visible`, including KEYWORDS this time — unlike `keyword_masked_
only`, which explicitly skips that check — but its own USE-deps don't
match), `suggested_use_flip` (the flag-flip computation itself, refusing
the *whole* suggestion rather than a partial one if any needed flag
isn't genuinely IUSE-declared or turns out unfixable), and `flag_is_
settable` — this last one a genuinely reusable trick worth naming: rather
than re-deriving `use.mask`/`.force`/`package.use.mask`/`.force`
matching logic a second time, it recomputes `effective_use_flags` with a
synthetic, exact-version `package.use` entry appended and checks whether
the result actually reflects the desired state, piggy-backing on already-
correct, already-tested logic instead of duplicating it. A first attempt
built that synthetic entry from the fully slot/repo-qualified
`candidate_str` itself (matching real `match_from_list`'s own atom-vs-
candidate-string convention everywhere *else* in this codebase) —
silently always returned "not settable" until caught by a failing new
test, root-caused to `match_from_list` needing a real *atom pattern* on
the left, not a candidate string used as both sides at once; fixed by
using a plain `=category/package-version` atom instead. `GraphEntry`
gains a new `use_suggestion: Option<(version, Vec<(flag, enabled)>)>`
field, computed and surfaced at exactly the same two call sites as
`keyword_suggestion` (a top-level atom's own fatal message, and a
dependency's own `NoVisibleCandidate` entry), with its own real
`package.use`-suggestion message syntax
(`=category/package-version flag -flag`). Real `autounmask_use` has no
"suppressed unless `--autounmask` was explicitly given" asymmetry the
way `autounmask_keep_keywords` does — `autounmask_suggest_use` is on by
default whenever `autounmask` itself is (which itself defaults on), only
suppressed by an explicit `--autounmask-use=n` — confirmed by reading
real `create_depgraph_params.py`'s own `myparams["autounmask_keep_use"]
= True if autounmask_use == "n" else False` directly. Deliberately still
out (confirmed with the user before implementing, given real portage's
own considerably bigger machinery here — cross-parent conflict
detection, a full backtrack-restart when a USE change cascades into
other packages' own dependencies, no equivalent of which exists anywhere
in this pilot): the real `binpkg_respect_use == "y"` interaction (a rare
corner case: an *explicit* `--binpkg-respect-use=y` forcing `autounmask_
use` to `"n"`, not reproduced since this pilot's own `binpkg_respect_use`
is already a resolved bool with no way to distinguish "explicit y" from
the "auto" default by the time it's available), and the separate
`opt?`/REQUIRED_USE-conditional mechanism (real `_show_unsatisfied_dep`
flipping the *requesting parent's own* flag rather than the candidate's
— a different code path, covered in its own dedicated section below).
Reuses existing fixtures rather than adding new ones:
`dev-libs/useflagpkg[-foo]` (top-level) and `dev-libs/usedeprejectedpkg`
→ `dev-libs/useflagpkg[-foo]` (dependency-level), both already
established for real USE-dep enforcement itself, now also proving the
suggestion — Rust and Python byte-identical, confirmed both via the
shared pytest contract suite and a direct manual diff against both
built binaries.

**`--autounmask-use`, the `opt?`/REQUIRED_USE-conditional half.** Real
`_show_unsatisfied_dep` (`lib/_emerge/depgraph.py:6756-6846`) has a
second, architecturally distinct mechanism from the plain-atom flip
above: when a dependency atom's own use-dep was originally conditional
on the *requesting parent's* own USE state (`opt?`/`!opt?`/`opt=`/
`!opt=`), the fix isn't a change to the candidate at all — it's a
change to the *parent's* own USE. This pilot's own conditional-use-dep
evaluation (`enqueue_flat_deps`/`_enqueue_flat_deps`, using the real
`Atom.evaluate_conditionals`/`portage_dep::evaluate_atom_conditionals`
already ported for `opt=`/`opt?` support) happens eagerly, at
dependency-queueing time, using the parent's own *current* USE — and
until this slice, the original conditional atom text was discarded
immediately afterward, so there was nothing left to reconsider once a
dependency turned out unsatisfiable.

**Data-flow change, confirmed with the user before implementing**: the
BFS queue item (Rust's own `QueueItem` type alias; Python's own bare
tuple) grows a fourth field, the atom's own *unevaluated* text —
`Some`/non-`None` only when `evaluate_atom_conditionals`/
`evaluate_conditionals` actually rewrote something (real `Atom.
unevaluated_atom`, which the Python side gets for free by checking
`is not` identity against the evaluated result — real
`evaluate_conditionals` is a documented no-op, returning `self`
unchanged, whenever nothing conditional is present at all; only a
genuine rewrite ever constructs a new `Atom` with its own
`unevaluated_atom` pointing back at the original). Threaded through
every queue-push site: `enqueue_flat_deps`'s own normal-deps queueing,
the top-level atom seed (always `None` — no parent to ever flip a flag
on), and the `AlreadyInstalled`/`--deep` recursion path (also always
`None` there — a real, pre-existing, unrelated gap this slice didn't
introduce: that path never evaluates conditional use-deps against its
own USE at all, in either language).

**The suggestion itself**: a new `suggested_parent_use_candidate`/
`_suggested_parent_use_candidate`, attempted only when a dependency's
own `NoVisibleCandidate` carries an unevaluated atom. `conditional_
flags`/`_conditional_flags` reads the *unevaluated* atom's own
conditional use-dep flags (real `Atom.use.conditional`'s own
`.enabled`/`.disabled`/`.equal`/`.not_equal` frozensets); the parent's
own current resolved candidate, IUSE, and effective USE come from
`parent_use_state`/`_parent_use_state`, looked up via the parent's own
already-built graph entry (always present by the time any of its own
dependencies are dequeued — BFS processes a package's own entry before
ever enqueueing its dependencies). Every involved flag must be real,
valid IUSE on the parent and not `package.use.mask`/`.force`'d there
(`flag_is_settable`/`_flag_is_settable`, the exact same helper Part A
already built, reused as-is — its own logic never assumed anything
child-specific). All involved flags are toggled together into one
hypothetical parent USE state (matching real `target_use`'s own
"flip everything involved at once" shape), the atom is re-evaluated
against it, and the result must actually become satisfiable
(`atom_currently_satisfiable`/`_atom_currently_satisfiable`, the same
helper the pre-existing `AlreadyInstalled` recursion path already uses
to skip unsatisfiable disjunctions) — and must not newly violate the
parent's own `REQUIRED_USE` (mirrors real `_show_unsatisfied_dep`'s own
`collect_use_changes and not required_use_warning` gate: a change that
was *already* `REQUIRED_USE`-violating before isn't disqualified by it,
only one that flips from satisfied to violated is).

**Deliberately narrower than real `Atom.violated_conditionals`** (~150
lines of per-token-operator partitioning this pilot doesn't reproduce
in either language): instead of determining exactly *which* conditional
use-deps were violated, this toggles *every* flag the unevaluated
atom's own conditional use-deps reference, together, in one
hypothetical. Matches real portage's own behavior for the common case
(an atom whose conditional use-deps are the only USE-deps present, all
referencing flags that need to move the same direction to fix it) but
diverges from it for more exotic mixed cases (concrete *and*
conditional use-deps on the same atom, or independent conditional flags
where only a subset actually needs flipping) — confirmed with the user
before implementing. The suggestion attaches to the *dependency's* own
entry (`parent_use_suggestion`, a new field alongside `use_suggestion`
— both can be `Some`/non-`None` at once, and often are: they're
genuinely independent, alternative fixes for the same mismatch) rather
than the parent's own entry the way real `missing_use_reasons.append
((myparent, ...))` does — a pragmatic simplification, since this
pilot's own entry model has no per-parent "reasons" list to attach it
to instead, and the dependency's own entry is where the "no visible
ebuild for dependency" note already lives.

Proven against the existing `dev-libs/useeqparentonpkg`/
`useeqparentoffpkg` → `dev-libs/useeqchildpkg` fixtures (already
established for `opt=` support itself, PMS 8.3.4): `useeqparentoffpkg`'s
own `IUSE="eqflag"` (no `+`, defaults off) makes its own RDEPEND
`dev-libs/useeqchildpkg[eqflag=]` evaluate to `[-eqflag]`, mismatching
the child's own default-on `eqflag` — both suggestions fire at once
(flip the child's `eqflag` off, *or* flip the parent's `eqflag` on;
either genuinely resolves the mismatch), gated on the same
`autounmask_suggest_use` flag as Part A, and suppressed together by the
same explicit `--autounmask-use=n`. Rust and Python byte-identical,
confirmed both via the shared pytest contract suite (including a new
dedicated `--json` test asserting both suggestion fields at once) and a
direct manual diff of stdout/stderr/exit-code/`--json` against both
built binaries.

`--columns`: real `output.py`'s own `_set_root_columns`, a purely
display-layer alternate rendering of the same New/Upgrade/Downgrade/
Reinstall entries the default bracket format already shows -- no new
resolution logic, just a different layout, and mutually exclusive with
`--tree` (real `actions.py`: `"can't specify both of \"--tree\" and
\"--columns\"."`, checked once parsing finishes -- this pilot reports it
via its own established CLI-usage-error convention, exit 2 on stderr,
rather than real portage's own literal exit 1 on stdout, same deviation
every other CLI-usage error in this pilot already makes). The
`"[{bracket}  {code}]"` segment is untouched -- only what comes after it
changes: bare `category/package` (no version) padded out to
`columnwidth - 60`, then `[version]` right-padded to `columnwidth - 30`,
then an `Upgrade`/`Downgrade`'s own old version in brackets (`Reinstall`
gets no such column -- it has no "old" version distinct from the new
one, and, unlike the non-columns format, `--columns` mode has no room
for a `(reinstall for ...)` reason at all, matching real portage's own
`_set_root_columns` exactly, which never surfaces one either). `--v`'s
own `USE="..."` suffix still applies, appended after everything else,
same as always. `columnwidth` itself defaults to 130, overridable via
the `COLUMNWIDTH` environment variable exactly like real portage (an
unparsable value warns and falls back to the default rather than
erroring) -- except the warning text itself: real portage's own message
echoes the raw exception's `str()`, which Rust's `ParseIntError` and
Python's `ValueError` never render identically, so this pilot uses one
fixed, pilot-authored line instead, the same "never leak a language-
specific parse-error string into pinned output" precedent `--deep`'s own
invalid-value handling already set.

`--newrepo`: a 5th independent, freely-combinable `Reinstall` trigger,
alongside `--newuse`/`--changed-use`, `--changed-deps`, `--changed-slot`,
and `--rebuilt-binaries` -- same architecture, one new vdb read
(`new_repo_changed`, reusing the already-generic `read_vdb_string`
helper for the vdb's own `repository` file) plus one new plain boolean
CLI flag (real `main.py`'s own `options` list -- no value at all, unlike
`--changed-slot`/`--rebuilt-binaries`, which are real `true_y_or_n`).
Fires when the installed package's own vdb-recorded `repository` differs
from the repo that currently provides that exact version -- a straight
string compare against the already-resolved candidate's own `repo_name`
at each of `resolve_pretend`'s two call sites, no md5-cache re-read
needed at all (unlike `slot_changed`'s own re-lookup). A vdb entry with
no `repository` file at all -- real portage predates this tracking, or a
hand-installed/synthetic entry -- is treated as real
`portage.versions._unknown_repo` (`"__unknown__"`) exactly, per real
`depgraph.py`'s own comparison, which has no tolerant "missing data
means unchanged" fallback the way `--changed-slot`/`--changed-deps` do:
an unrecorded repo is a real, distinct value, and it either equals the
current repo or it doesn't. `--newrepo` is also one of real
`create_depgraph_params.py`'s own `selective` triggers (confirmed by
reading it), so it now joins `update`/`newuse`/`changed_use`/
`changed_deps`/`changed_slot`/`noreplace` in this pilot's own `selective`
default-resolution OR-list too. Three fixtures prove it: `newrepopkg`
(vdb `repository=oldrepo`, current provider `testrepo` -- fires),
`samerepopkg` (vdb `repository=testrepo`, matching -- doesn't fire), and
the pre-existing `samepkg` (no `repository` file at all -- fires via the
`"__unknown__"` sentinel, a real, sometimes-surprising consequence of
that missing-tolerant-fallback design worth demonstrating explicitly).

`--buildpkgonly`/`-B`: real `depgraph.py`'s own resolution-time
validation (`lib/_emerge/depgraph.py:5706-5717`), not a display tweak --
`--buildpkgonly` only ever builds a binary package without merging it,
so every dependency of a package that needs building must already be
satisfied by something *already installed*; if a dependency itself also
needs building, real portage refuses to resolve at all. Implemented as
one more field on `resolve_pretend_graph`'s own return value
(`buildpkgonly_deps_unsatisfied`): once the whole graph is known, collect
every entry that would newly merge (`New`/`Upgrade`/`Downgrade`/
`Reinstall` -- anything but `AlreadyInstalled`/`NoVisibleCandidate`),
then check whether any of *those* entries has a `required_by` owner
that's *also* in that same set -- exactly real `digraph.hasallzeros()`'s
own check, expressed against this pilot's own `required_by` edges
instead of a real `digraph`. When it fires, `pretend.rs` prints the
resolved merge list first (matching real `display_problems()`'s own
`_show_merge_list()`-then-error ordering) and *then* the real error text
verbatim (`"--buildpkgonly requires all dependencies to be merged."` /
`"Cannot merge requested packages. Merge deps and try again."`, both to
stderr) and exits `1`. Two fixtures prove it: `dualdep` (`New`, both
`DEPEND` and `RDEPEND` on `newpkg`, itself `New` -- fires) and the new
`buildpkgonlysatisfied` (`New`, `RDEPEND` on the already-installed
`samepkg` -- doesn't fire).

**v1 scope cuts**: no real "build the `.tbz2`/skip the merge, run
`clean`" *execution* at all -- this pilot's `emerge` binary is still a
pure `--pretend`-only dependency-resolution tool with no real merge
orchestration of its own (`ebuild <file> merge`/`package` are the real,
separate execution surfaces task #54/#55 built; `--buildpkgonly` doesn't
change what either of those does, since it's an `emerge`-level flag).
No `ignore_priority` distinction (real `DepPrioritySatisfiedRange.
ignore_medium` ignores soft/optional edges when deciding whether the
graph is "clean" -- this pilot treats every `required_by` edge as hard).
No `--fetchonly` interaction (real portage only runs this check when
`--fetchonly` is *not* also given -- this pilot has no `--fetchonly` at
all yet, so the interaction can't arise). No `--quickpkg-direct`
sibling check, and no `_start_resolution_display`'s own cosmetic
"packages that would be *built*" spinner text (this pilot has no spinner
concept at all).

### Real ebuild phase execution (task #54): the first slice

A genuinely different kind of slice from everything above: not a new
`emerge --pretend` flag, but the first working piece of the *next major
phase* this pilot's own `PORTING/PROMPT-next.md` had investigated but
never started in code -- running real ebuild phase functions
(`pkg_setup`, `src_unpack`, `src_prepare`, `src_configure`, `src_compile`,
`src_test`, `src_install`) and landing real files under a real `${D}`,
via `ebuild <file> install` (`portuale/src/ebuild.rs`, previously a pure
dry-run stub).

**Bash-execution backend**: an embedded [`brush`](https://github.com/reubeno/brush)
shell (`brush_core::Shell`), pinned by exact commit to the fork
`vivo75/brush` (branch `fix/pipeline-function-stage-deadlock`). Two real
fixes: the brace-less function-definition form `name() [[ ... ]]` (used
60 times by `bin/eapi.sh`) **merged upstream** as
[reubeno/brush#1274](https://github.com/reubeno/brush/pull/1274)
(`18851e7`, 2026-08-20); the pipeline function-stage deadlock fix is
**fork-only**, open upstream as
[reubeno/brush#1276](https://github.com/reubeno/brush/pull/1276) — the
one reason the pin isn't just upstream `main`. Full tracking record and
the re-pin checklist live in **`PORTING/BRUSH_FORK.md`**. A deliberate,
accepted departure from this pilot's own near-zero-dependencies
discipline elsewhere -- the alternative (shelling out to the system's
real bash) was rejected earlier for tension with the "runs on even the
most minimal Linux system" hard goal.

**What's real, what's Rust**: `portuale/src/ebuild_phases.rs` computes
the environment `doebuild_environment()` would (`CATEGORY`/`PN`/`PV`/
`PR`/`PVR`/`P`/`PF`, the real `${PORTAGE_TMPDIR}/portage/${CATEGORY}/
${PF}` directory layout) and drives the same per-command phase
sequencing real `doebuild()` does (`actionmap_deps`,
`lib/portage/package/ebuild/doebuild.py:871-884` -- `ebuild file.ebuild
install` runs the whole `pretend → setup → unpack → prepare → configure
→ compile → test → install` prerequisite chain, not just the one named
phase, confirmed by reading it). Everything else is real, unmodified
bash: `bin/ebuild.sh` (sourced directly, which itself sources
`bin/phase-functions.sh`/`bin/phase-helpers.sh`/
`bin/isolated-functions.sh`/`bin/bashrc-functions.sh`/
`bin/save-ebuild-env.sh`, and the ebuild file itself) drives each real
`__ebuild_main <phase>` call. Real EAPI-default phase functions
(`default_src_install` etc.) are themselves ordinary bash functions in
`phase-functions.sh`/`phase-helpers.sh` -- ported here for free by
sourcing those files, not reimplemented -- so even a fixture ebuild that
defines *no* phase functions at all still gets real `unpack`/`econf`/
`emake`/`emake install`-driven behavior automatically. Real
`insinto`/`doins` (themselves real bash wrapping a real `doins.py`
subprocess) really do write real files.

**A fresh embedded shell per phase, not one shared across a whole
invocation**: confirmed necessary the hard way -- real `bin/ebuild.sh`'s
own tail makes `EBUILD_PHASE` (among other variables) `readonly`, so a
*second* phase in one shared shell can't `export EBUILD_PHASE=<next>` at
all. A fresh shell per phase mirrors what real `doebuild()` itself does
(a fresh `bin/ebuild.sh` *process* per phase, via `spawnebuild()`) far
more literally than sharing one shell ever would have; real
`PORTAGE_BUILDDIR`-relative resume markers (`.pretended`/`.setuped`/
`.unpacked`/etc., written by `__dyn_*` themselves) are what make
re-"running" an already-done prerequisite phase from a fresh shell
cheap, exactly like real portage's own separate `spawnebuild()` calls
rely on -- not a mechanism invented for this pilot. Also confirmed the
hard way: the embedding tokio runtime **must** be multi-threaded
(`rt-multi-thread`) -- a single-threaded one deadlocks partway through a
real multi-phase run, consistent with `brush-core`'s own `Cargo.toml`
requiring that exact feature.

**v1 scope cuts** (see `ebuild_phases.rs`'s own module doc comment for
the full list): only the `actionmap_deps`-chained phases run for real
(`pretend` through `install`) -- `merge`/`qmerge`/`unmerge`/`package`
and friends still fall through to the pre-existing dry-run stub
unchanged, since real merge/vdb/`CONTENTS` machinery is task #55's own,
separately-scoped, much bigger piece (`dblink.merge()`/`treewalk()`/
`mergeme()` in `lib/portage/dbapi/vartree.py`, ~6500 lines). No
sandboxing. No fetch/unpack of a real `SRC_URI` (`${S}` is pre-created
empty rather than populated by a real `unpack`). `EAPI` is read directly
from the ebuild's own text via the real PMS 7.3.1 rule (`parse_eapi`),
since `ebuild <file> <command>` operates on an arbitrary standalone
ebuild file, not necessarily one indexed in a configured repo.

### Real merge/filesystem mutation (task #55): the first slice, plus real `pkg_preinst`/`pkg_postinst` hooks

The natural next step after task #54: `ebuild <file> merge`
(`portuale/src/ebuild_merge.rs`) now really copies `${D}` into `${ROOT}`
and writes a real vdb entry, instead of falling through to the dry-run
stub. Runs the real `install` phase chain first (task #54's own
`ebuild_phases::run_commands`), then really runs `pkg_preinst`
(`ebuild_phases::run_single_phase`, not `run_commands` -- real
`dblink.treewalk()` invokes `pkg_preinst`/`pkg_postinst` directly,
`EbuildPhase(phase="preinst"/"postinst")`, not through `doebuild()`'s own
`actionmap_deps` chain the way `pretend`..`install` are), then walks
`${D}` and, for every regular file, directory, and symlink found, really
merges it into `${ROOT}` -- matching real `dblink.mergeme()`'s own
`_format_contents_line` format exactly: `dir <path>`, `obj <path> <md5>
<mtime>`, `sym <path> -> <target> <mtime>`. Writes that `CONTENTS` text,
plus `CATEGORY`/`SLOT`/`repository`/`COUNTER`, into a real
`${ROOT}/var/db/pkg/<category>/<pf>/` directory -- the same
one-value-per-file vdb layout this pilot's own fixtures and
`portage_repo`'s own vdb readers (`installed_candidates`,
`read_vdb_string`, etc.) already use, so a package merged this way is
immediately visible to every other slice in this pilot (`emerge
--pretend`'s own `AlreadyInstalled`/`Reinstall` detection, `--deselect`,
and so on) -- then really runs `pkg_postinst`, only once the vdb entry is
fully written, matching real `treewalk()`'s own relative ordering.
`bin/phase-functions.sh`'s own `__ebuild_main` already accepts
`preinst`/`postinst` as literal phase arguments directly, and silently
no-ops when the ebuild defines neither function at all -- no new
bash-side gap, this is exactly the same real, unmodified toolchain task
#54 already drives.

The vdb write is real and atomic, not a direct write: `write_vdb_entry`
builds the whole entry (`CATEGORY`/`SLOT`/`repository`/`CONTENTS`/
`COUNTER`) in a `-MERGING-<pf>`-prefixed temporary sibling directory
(real `lib/portage/const.py`'s own `MERGING_IDENTIFIER`) under the same
`<category>` directory, then `std::fs::rename`s it into place -- the
same same-filesystem atomicity guarantee real `dblink.merge()`'s own
`dbtmpdir`-then-`_movefile()` approach relies on, so a crash mid-write
leaves at most a harmless leftover temp directory, never a half-written
*final* vdb entry. `COUNTER` is a real, monotonically-increasing global
merge counter too: `next_counter` reads/increments/writes
`${ROOT}/var/cache/edb/counter` (real `vardbapi.counter_tick_core()`'s
own mechanism -- a missing or corrupt file is treated as `-1`, so the
very first merge anywhere gets `COUNTER=0`).

`SLOT` is read directly from the ebuild's own text (a literal
`SLOT=...` assignment, scanned anywhere in the file -- unlike `EAPI`,
real PMS doesn't restrict where `SLOT` may appear), the same
direct-text-parsing shortcut `parse_eapi` already established.
`repository` is resolved by walking up from the ebuild's own package
directory looking for a `profiles/repo_name` file (real portage's own
mechanism for naming a repo), falling back to the same `"__unknown__"`
sentinel `portage_repo::new_repo_changed` already uses when none is
found.

**v1 scope cuts** (see `ebuild_merge.rs`'s own module doc comment for
the full list): no `CONFIG_PROTECT`/collision-protect/preserve-libs. No
`env_update()`/`ldconfig` triggering. Directory-entry merge order is
sorted by filename for deterministic tests, rather than real
`os.listdir()`'s own arbitrary/OS-dependent order (`CONTENTS` line order
has no real semantic meaning portage itself relies on).

### Real package removal: `unmerge` (task #55's own natural complement)

`ebuild <file> unmerge` (`portuale/src/ebuild_unmerge.rs`) really
removes a package `merge` previously installed, instead of falling
through to the dry-run stub -- without this, `merge` alone could never be
exercised through a real install/reinstall/removal cycle; every merge
would just accumulate vdb entries and files forever. Mirrors real
`dblink.unmerge()` plus the top-level `unmerge()` function's own
success-gated `dblink.delete()` call: really runs `pkg_prerm`
(`ebuild_phases::run_single_phase`, the same non-`actionmap_deps`
mechanism `pkg_preinst`/`pkg_postinst` already use), really deletes every
file/dir/symlink the vdb entry's own `CONTENTS` lists from `${ROOT}` --
in real `_unmerge_pkgfiles()`'s own reverse-sorted order (`mykeys.sort();
mykeys.reverse()`), deepest paths first, so a directory always empties
out before its own removal is attempted -- really runs `pkg_postrm`, and
only then, on success, removes the vdb entry itself (real
`dblink.delete()`'s own `shutil.rmtree()` plus a best-effort `rmdir` of
the parent `<category>` directory if it's now empty).

A locally-modified file is protected on removal too, via real
`_unmerge_pkgfiles()`'s own actual mechanism: an `obj`/`sym` entry whose
live, on-disk mtime no longer matches what `CONTENTS` recorded at merge
time is left in place instead of deleted (real `!mtime` skip -- broader
than `CONFIG_PROTECT` alone, since it applies to every unmerge
regardless of path, and it's what actually protects a CONFIG_PROTECT'd
file on removal too, since its own recorded mtime reflects the
`._cfgNNNN_`-diverted write, never the real file a user edited).

**v1 scope cuts** (see `ebuild_unmerge.rs`'s own module doc comment for
the full list): no `bsd_chflags` handling -- confirmed dead code on
Linux (`lib/portage/__init__.py:311` sets it to `None` unconditionally
on non-BSD), not a real gap. Coarser failure tolerance: a genuine I/O
error (not "already gone" or "directory not empty", both tolerated) is
a hard failure here, rather than real `_unmerge_pkgfiles()`'s own
per-file failure counter that keeps going regardless. (`unmerge-orphans`
and `INFOPATH` handling have since shipped -- see their own sections
below; so has the "others in this slot" reverse-dependency check.)

### `unmerge`'s own `others_in_slot` reverse-dependency check: an in-place upgrade doesn't delete files the new version still owns

The last item on `ebuild_unmerge.rs`'s own gap list is real now: real
`_unmerge_pkgfiles()`'s own `is_owned` check (`vartree.py:2893-2916`,
via `dblink.isowner()`, itself `bool(self._match_contents(filename))`).
Without this, `merge`-ing a new version of a package already installed
in the same `SLOT` (an in-place upgrade -- real portage's own
"install new, then remove old" merge-list order) and then `unmerge`-ing
the *old* vdb entry would delete every file the old version's own
`CONTENTS` lists, including ones the just-installed new version also
owns, since this pilot's `unmerge` had no concept of "another installed
package might still need this" at all before this slice.

`run_unmerge` now computes real `others_in_slot` before doing any
deletion: every other installed version of the same `category`/`PN` in
the same `SLOT` as the package being unmerged, read directly from vdb
`SLOT` files the same way `ebuild_merge`'s own blocker-exclusion slice
already does (`portage_repo::installed_versions` +
`ebuild_merge::read_installed_slot`, the latter promoted from private to
`pub(crate)` for this). `remove_contents` then checks, for every
`CONTENTS` entry and *before* the existing `!mtime` check (matching real
`_unmerge_pkgfiles()`'s own ordering exactly), whether any
`others_in_slot` member's own real `CONTENTS` also claims that same path
(`ebuild_merge::owns_path_pf`, likewise promoted to `pub(crate)`,
already built for blocker exclusion's own `CONTENTS`-ownership check) --
if so, the entry is left alone entirely (real `"replaced"` skip),
regardless of node type. Real weak vs. strong node-type distinctions in
the "symlink orphan" refinement (bug #326685) aren't reproduced -- see
`ebuild_unmerge.rs`'s own module doc comment for why that narrower
sub-case is out of v1 scope.

New fixtures `dev-libs/othersinslotpkg-1.0`/`-2.0`, both `SLOT="0"`,
both installing a real shared file (`shared.txt`) plus a version-unique
one (`only-in-v1.txt`/`only-in-v2.txt`). Proven via a real, end-to-end
test: merge both versions, unmerge the *old* one -- `shared.txt`
survives (2.0 still owns it) while `only-in-v1.txt` is deleted normally
(no other owner); unmerge the *remaining* 2.0 entry too, and
`shared.txt` finally goes, proving the skip isn't unconditional. No
Python mirror needed -- like every other real-execution (non-dry-run)
slice in this pilot, this is Rust-only; the shared pytest contract suite
doesn't touch real `merge`/`unmerge` at all.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
BIN=PORTING/rust/target/release/portuale

$BIN ebuild PORTING/fixtures/repo/dev-libs/othersinslotpkg/othersinslotpkg-1.0.ebuild merge
$BIN ebuild PORTING/fixtures/repo/dev-libs/othersinslotpkg/othersinslotpkg-2.0.ebuild merge
ls "$ROOT"/usr/share/othersinslotpkg/
# only-in-v1.txt  only-in-v2.txt  shared.txt

$BIN ebuild PORTING/fixtures/repo/dev-libs/othersinslotpkg/othersinslotpkg-1.0.ebuild unmerge
ls "$ROOT"/usr/share/othersinslotpkg/
# only-in-v2.txt  shared.txt -- only-in-v1.txt is gone, shared.txt survives
cat "$ROOT"/usr/share/othersinslotpkg/shared.txt
# shared, from 2.0

$BIN ebuild PORTING/fixtures/repo/dev-libs/othersinslotpkg/othersinslotpkg-2.0.ebuild unmerge
ls "$ROOT"/usr/share/
# ls: cannot access '.../usr/share/': No such file or directory -- no owner left, so it's really gone now
```

### Real `CONFIG_PROTECT`: a locally-edited config file survives an upgrade

`ebuild <file> merge` now really implements `CONFIG_PROTECT` for `obj`
(regular file) entries, closing what was previously merge's own biggest
documented gap: real `ConfigProtect.isprotected()` path matching
(`is_protected` -- longest-prefix match against `CONFIG_PROTECT` minus
`CONFIG_PROTECT_MASK`, both env-var-sourced at the CLI boundary the same
way `PORTAGE_TMPDIR` already is, defaulting to real `make.globals`'s own
`CONFIG_PROTECT="/etc"`/`CONFIG_PROTECT_MASK="/etc/env.d"`), the real
MD5-comparison rename-instead-of-overwrite decision (real
`dblink._protect()`: a protected file whose real on-disk content differs
from what's about to be merged is diverted to the next
`._cfgNNNN_<name>` sibling -- real `new_protect_filename()` -- instead of
being overwritten), and real `vardbapi._conf_mem_file` persistence
(`<root>/var/lib/portage/config`, a real, persisted "which update has
already been offered for this path" memory, so re-merging an
already-protected update applies it directly instead of spawning a fresh
`._cfgNNNN_` file every time). `CONTENTS` still always records the
package's own logical path with the *new* content's own MD5 -- never the
`._cfgNNNN_` variant a protected write may have actually landed at --
exactly matching real `dblink.mergeme()`'s own behavior (the vdb
considers this package the owner of the logical path either way, real
content notwithstanding). `unmerge`'s own real `!mtime` staleness check
(above) is what protects the same file symmetrically on removal.

Real `movefile()` also explicitly preserves the source's own mtime onto
the merged destination; `std::fs::copy` doesn't (a fresh copy gets its
own "now" mtime), which would otherwise silently break both this
MD5-based comparison's own correctness *and* `unmerge`'s `!mtime` check
-- fixed by adding a small `filetime` dependency (no stable `std::fs`
mtime setter exists, symlinks included) and explicitly setting it after
every merged `obj`/`sym` write.

**v1 scope cuts as of this slice** (see `ebuild_merge.rs`'s own module doc
comment for the full, current list -- every gap this paragraph originally
listed here has since shipped, see the sections below): an already-
offered, unmodified-since update is applied directly here; real portage
instead leaves the destination completely untouched in that case (while
still recording the merge in `CONTENTS`) -- the one remaining, deliberate
v1 simplification in this area.

### `CONFIG_PROTECT` for symlinks, `--noconfmem`, and `new_protect_filename`'s own file-reuse logic

Three gaps the previous section's own "v1 scope cuts" originally
documented are now closed, finishing off real `dblink._protect()`/
`new_protect_filename()` (`lib/portage/util/__init__.py:1803`) parity.
**Symlink CONFIG_PROTECT**: a `sym` entry under a protected path whose
real on-disk target string differs from the one about to be merged is now
diverted too (real bug #485598: the *target string*'s own MD5 is what's
compared, not file content) -- mirrors the `obj` case exactly, just
hashing the target string's bytes instead of reading file content (the
comparison is type-independent of the live destination's own on-disk
type since a later slice -- see "CONFIG_PROTECT: a type-changing update
is real-protected too" below). **`NOCONFMEM`**: real `--noconfmem`
(`lib/_emerge/actions.py:2790`) is an `emerge`-only CLI flag with no real
`bin/ebuild` equivalent at all (confirmed against `bin/ebuild`'s own
six-option `argparse` list), so this pilot reads the `NOCONFMEM` env var
directly instead -- the same "env var, not full config resolution"
shortcut `CONFIG_PROTECT` itself already uses. Real `vartree.py:4949`'s
own `cfgfiledict["IGNORE"]`: forces every already-offered,
unmodified-since update to be re-protected instead of applied directly,
regardless of memory. **`new_protect_filename` file reuse**: real
`new_protect_filename()` no longer always allocates a fresh
`._cfgNNNN_<name>` number -- it now reuses the *last* one when that
file's own content (or, if it's itself a symlink, its own target string)
already matches the pending update, exactly like real portage. This is
what keeps `NOCONFMEM` from spawning a visibly *new* `._cfgNNNN_` file on
a repeat merge of unchanged content -- its real, visible effect is that
the logical path stays protected (left alone) instead of being
overwritten directly, not that a fresh numbered file appears each time.

### CONFIG_PROTECT: a type-changing update is real-protected too

Real `dblink._protect()`'s own destination-side computation
(`vartree.py:5434-5480`/`5831-5901`) is fully type-independent now: it
was already the case that its own `dest_md5`/`dest_link` comparison is
computed from the *live destination's own lstat'd on-disk type*,
regardless of what type the incoming source itself is -- previously,
this pilot's `merge_tree` only ever protect-compared an `obj` entry
against an `obj` (regular file) dest, and a `sym` entry only against a
`sym` dest, silently overwriting a type-changing update (a symlink
replacing a previously-installed regular file at the same path, or vice
versa) instead of diverting it. `protect_decision`, a new function
shared by `merge_tree`'s `obj`/`sym` branches, closes this: it lstats
the live destination once, computes `dest_md5` (content MD5, if it's a
regular file) or `dest_link` (target string, if it's a symlink)
accordingly, and compares against the incoming source's own MD5 either
way -- a type mismatch simply lands in a different hash domain
(content-MD5 vs. target-string-MD5), so it practically never matches and
is correctly diverted into a fresh `._cfgNNNN_` sibling, exactly like a
real content/target change would be. Real `force` (`dest_link !=
src_link` on a type mismatch) is deliberately not threaded through, for
the same reason `new_protect_filename`'s own doc comment already gives
for the general case: it only ever changes behavior when the
destination doesn't exist yet, and every call site here -- like every
one before it -- only reaches `new_protect_filename` after confirming
the destination exists. Verified directly (a `sym` source landing on a
regular-file dest is protected; the mirror-image `obj`-source-on-`sym`-
dest case too) and live against the compiled binary, in both directions
(`CONFIG_PROTECT=/usr/share/mergepkg`, merging `dev-libs/mergepkg` over
a manually-placed admin file/symlink at the destination path):

```sh
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
export CONFIG_PROTECT=/usr/share/mergepkg
mkdir -p "${ROOT}"/usr/share/mergepkg
echo "the admin's own regular file" > "${ROOT}"/usr/share/mergepkg/hello-link.txt

PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild merge

cat "${ROOT}"/usr/share/mergepkg/hello-link.txt
# the admin's own regular file -- untouched
readlink "${ROOT}"/usr/share/mergepkg/._cfg0000_hello-link.txt
# hello.txt -- the package's own symlink landed in a ._cfg0000_ sibling
unset CONFIG_PROTECT
```

### CONFIG_PROTECT: `_installed_instance`/`FEATURES=config-protect-if-modified`

Real `_installed_instance`/`protect_if_modified` (`vartree.py:4409-4418`/
`5849-5866`) are real now too, closing the last CONFIG_PROTECT gap this
area's own module doc comment used to list. `installed_instance_pf`
picks the *previous* same-slot installed instance a merge is upgrading
over -- the one with the highest real `COUNTER` among every other
currently-installed version in this exact `category/package/slot`,
reusing the same real per-package `COUNTER` file this pilot already
writes on every merge (`next_counter`), rather than needing any new
persistence. `owned_node_value_pf` (the value-returning sibling of the
already-existing `owned_node_type_pf`) consults that instance's own real
`CONTENTS` for whatever it recorded at a given path -- an `obj`'s own
content MD5, or a `sym`'s own target string.

Two distinct real behaviors, both gated on that instance having actually
recorded the path at all (real `k = self._installed_instance.
_match_contents(dest_real)`):

- A path it recorded that's now missing entirely from the live
  filesystem (the admin deleted or renamed it) always force-diverts into
  a fresh `._cfgNNNN_` sibling -- real bug #523684, prompting the admin
  instead of silently re-creating a path they deliberately removed. This
  is the one case in this whole area where `new_protect_filename` is
  reached with a destination that doesn't exist at all.
- With real `FEATURES=config-protect-if-modified` on (real `make.
  globals`'s own default -- confirmed by reading `cnf/make.globals:79`
  directly, the same category of previously-undiscovered default-
  `FEATURES` mismatch the `protect-owned`/`unmerge-orphans` fix found
  earlier; `MergeOptions::protect_if_modified` now defaults `true` to
  match), a live destination that still matches *exactly* what that
  previous instance installed -- the admin never touched it since --
  has the new version's own content applied directly, even though it
  differs from what the *old* version installed. This is what tells
  "this file's own default content changed between package versions"
  apart from "the admin hand-edited it locally", which the plain
  `src_md5 == dest_md5` comparison alone can't distinguish.

Verified directly (unmodified-since-installed content applies directly;
locally-modified content still protects; a deleted path force-diverts)
and live end to end, reusing the existing `dev-libs/othersinslotpkg`
fixture pair purely as a convenient same-slot upgrade whose own
`shared.txt` genuinely differs in content between `1.0`/`2.0`:

```sh
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
export CONFIG_PROTECT=/usr/share/othersinslotpkg
V1=PORTING/fixtures/repo/dev-libs/othersinslotpkg/othersinslotpkg-1.0.ebuild
V2=PORTING/fixtures/repo/dev-libs/othersinslotpkg/othersinslotpkg-2.0.ebuild

PORTING/rust/target/release/portuale ebuild "$V1" merge
PORTING/rust/target/release/portuale ebuild "$V2" merge
cat "${ROOT}"/usr/share/othersinslotpkg/shared.txt
# shared, from 2.0 -- applied directly, no ._cfgNNNN_ sibling at all,
# since it was never modified since 1.0 installed it.
unset CONFIG_PROTECT
```

### CONFIG_PROTECT: "confmem rejected this update" -- an already-offered, unmodified-since update now really leaves the live file untouched

The one remaining v1 simplification this whole area's own module doc
comment used to document is closed: real `_protect()`'s own `move_me`/
`protected` return values (`vartree.py:5831-5901`), traced line by line
against the exact real caller (`mergeme()`'s own `obj`/`sym` branches,
`vartree.py:5468-5481`/`5547`/`5749`) to confirm the real gate precisely.
`protect_decision` used to conflate "which path to write to" with
"whether to write at all"; it now returns `(write_dest, moveme)`, and
`merge_tree`'s own `obj`/`sym` branches only perform the actual copy/
symlink-write (and its mtime stamp) `if moveme`, matching real
`mergeme()`'s own `if moveme:` gate around `movefile()`.

Real `moveme` is `false` in exactly the case this pilot's own doc
comment already named: an update whose exact `src_md5` was already
offered for this path (`cfgfiledict` remembers it from an earlier merge)
and `NOCONFMEM` is unset -- real `move_me = protected = bool(cfgfiledict
["IGNORE"])` with `IGNORE == 0` (`vartree.py:5877`), real `mergeme()`'s
own `zing = "---"`, "confmem rejected this update". Before this slice,
this pilot applied the update directly instead, silently overwriting
whatever the admin had locally edited into that already-offered file --
a real, observable divergence from real portage now closed. Real
`cfgfiledict` is deliberately left untouched in this one branch too:
reaching it requires `src_md5 == cfgfiledict.get(dest_real)[0]` already
(the very definition of "already offered"), so real `vartree.py:5888-
5895`'s own trailing `if move_me: cfgfiledict[...] = [src_md5] elif
dest_md5 == cfgfiledict.get(...)[0]: del cfgfiledict[...]` hits neither
branch (`move_me` is `False`, and `dest_md5 != src_md5` was already
established by the earlier `src_md5 == dest_md5` check having failed).

CONTENTS itself is unaffected either way: it already recorded the
*source's* own MD5/mtime unconditionally (real `mymtime = mystat.
st_mtime_ns`, set before the real `if moveme:` gate and never touched
when it's skipped) -- this package still logically claims ownership of
the *new* content in `CONTENTS`, even though the live file on disk stays
whatever the admin left it as.

Verified by correcting an existing test that had pinned the old,
incorrect behavior (`merge_tree_remembers_an_already_offered_update_
and_leaves_the_live_file_untouched`, renamed from its own former "...
_and_stops_re_protecting_it") to assert the real one instead: after a
third merge of an already-offered update, the live file still holds the
admin's own local edits, no second `._cfgNNNN_` sibling is spawned, and
the returned `CONTENTS` text still records the new source's own MD5 for
that path. Live-verified against the compiled binary too, reusing the
existing `dev-libs/configpkg` fixture:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
export CONFIG_PROTECT=/etc
BIN=PORTING/rust/target/release/portuale
PKG=PORTING/fixtures/repo/dev-libs/configpkg/configpkg-1.0.ebuild

"$BIN" ebuild "$PKG" merge
echo "user's own edits" > "${ROOT}"/etc/configpkg.conf
"$BIN" ebuild "$PKG" merge   # not yet offered -> diverts to ._cfg0000_
ls -a "${ROOT}"/etc/ | grep cfg
# ._cfg0000_configpkg.conf
"$BIN" ebuild "$PKG" merge   # already offered -> "confmem rejected"
ls -a "${ROOT}"/etc/ | grep cfg
# ._cfg0000_configpkg.conf -- still just the one, no ._cfg0001_
cat "${ROOT}"/etc/configpkg.conf
# user's own edits -- the admin's own live edits, untouched
unset CONFIG_PROTECT
```

### `FEATURES=collision-protect`: a merge that would overwrite another package's file aborts

CONFIG_PROTECT's own sibling real merge-track feature: real
`dblink._collision_protect` (`lib/portage/dbapi/vartree.py:3836`) is now
real too. Before `pkg_preinst` ever runs (matching real `merge()`'s own
exact ordering -- confirmed by reading it, the real abort happens
strictly before the real `EbuildPhase(phase="preinst")` block, not
after), `find_collisions` walks the real install image (`${D}`) the
same way `merge_tree` does, read-only, checking every real file/symlink
entry (never directories -- real `_collision_protect` only ever checks
`file_list`/`symlink_list`) against the real, on-disk destination:

- Real PMS 13.4's own symlink-over-directory ban is checked
  **unconditionally**, regardless of `FEATURES` -- a symlink this
  package would install landing exactly where an existing directory
  already sits always aborts the merge.
- An ordinary collision (the destination already exists, isn't owned by
  an older installed version of this exact package in the same slot --
  the one this merge is about to replace -- and isn't `CONFIG_PROTECT`'d,
  which diverts instead of colliding) only aborts when real
  `FEATURES=collision-protect` itself is set (read once via
  `std::env::var("FEATURES")` at the `ebuild.rs` CLI boundary, the same
  "env var, not full config resolution" shortcut every other real
  setting there already uses) -- matching real portage's own default:
  without it, the merge proceeds and silently overwrites the file, same
  as before this slice.

`find_owners` (real `vardbapi._owners.get_owners()`, narrowed to a
fresh scan of every installed package's own real `CONTENTS` rather than
a persistent reverse index -- acceptable for a real, but not
performance-critical, error-reporting path only reached when a merge is
about to abort anyway) names which other real installed package(s)
actually claim each colliding path in the abort message.

Deliberately not attempted (see `ebuild_merge.rs`'s own module doc
comment for the full list): `preserve-libs` exclusion (a collision
against a library real portage is about to unregister and hand over is
a real, separately-scoped subsystem this pilot doesn't implement
anywhere yet), blocker exclusion (real `mypkglist = others_in_slot +
blockers` -- blockers are a real, broad gap this pilot doesn't attempt
anywhere else either), and `FEATURES=protect-owned` (a separate real
feature: abort only when an owner was actually identified, regardless
of `collision-protect`).

Proven via three new, real, end-to-end tests in `ebuild_merge.rs`
against two new fixture packages (`dev-libs/collisionpkg-a`, the
already-installed half; `dev-libs/collisionpkg-c`, an unrelated package
that collides with it on an ordinary file) plus a third
(`dev-libs/collisionpkg-b`, which collides via a symlink over
`collisionpkg-a`'s own real directory): collision-protect off merges
over the collision as before; collision-protect on aborts and names
`collisionpkg-a` as the real owning package, leaving the file
byte-for-byte untouched; the symlink-over-directory case aborts
unconditionally regardless of `collision_protect`. Live-verified
against the compiled binary first, with fresh `ROOT`s per scenario (an
earlier attempt that reused one `ROOT` across sequential merges without
unmerging in between produced a stale, misleading result -- a leftover
vdb entry from an earlier merge made a later package look like it
already "owned" a path it didn't actually still own on disk; not a bug
in the collision logic itself, a reminder that this pilot's own
`unmerge` is what real portage relies on to keep vdb ownership in sync,
not just overwriting files in place).

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/collisionpkg-a/collisionpkg-a-1.0.ebuild merge
FEATURES="collision-protect" PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/collisionpkg-c/collisionpkg-c-1.0.ebuild merge
# ebuild: This package will overwrite one or more files that may belong to other packages:
# dev-libs/collisionpkg-a-1.0:
#         /usr/share/collisiontest/shared.txt
# Package 'dev-libs/collisionpkg-c-1.0' NOT merged due to file collisions.
cat "${ROOT}"/usr/share/collisiontest/shared.txt
# hello from collisionpkg-a
```

### `--debug`: real `PORTAGE_DEBUG` plumbing (task #56)

Real `emerge --debug` (`lib/_emerge/main.py:1234-1239`) does two things:
sets `PORTAGE_DEBUG=1` in the environment, and bumps Python's own
`logging` level to `DEBUG`. Grounding both against this pilot's own
real-execution surface (`ebuild_phases.rs`/`ebuild_merge.rs`) found they
split into two unrelated features: the logging-level bump has *zero*
effect on `doebuild.py`/`vartree.py` (they route messages through
`writemsg_level()`, gated by `PORTAGE_VERBOSE`, not the logging level at
all) -- the real content there lives in `lib/_emerge/depgraph.py`'s own
~60 `logging.DEBUG`-level dependency-resolution trace calls, a much
bigger, separately-scoped port into `pretend.rs`'s own resolution logic,
not attempted here. `PORTAGE_DEBUG=1` itself, though, is exactly what
real `bin/ebuild.sh:479` and friends (`phase-functions.sh`,
`phase-helpers.sh`, `misc-functions.sh`) check to `set -x` -- real bash
xtrace of every phase command as it runs -- and that part *is* real
execution this pilot already drives.

`ebuild.rs`'s option-parsing loop now captures `--debug` instead of
silently discarding it like every other real `Kind::Boolean` option
still does, threading a `debug: bool` through
`ebuild_phases::run_commands`/`run_single_phase` and
`ebuild_merge::run_merge` down to `run_one_phase`'s own environment-setup
block, which now always explicitly `export`s `PORTAGE_DEBUG=1` or `=0`
(never leaving it unset, so the real bash guard's behavior doesn't
depend on whatever happened to be in the host environment already).
Real, unmodified `bin/ebuild.sh` does the rest -- brush-core already has
full `xtrace` support (`set -x`'s own `namedoptions.rs` mapping to
`options.print_commands_and_arguments`, real `trace_command` call
sites), so nothing needed fixing on the shell-backend side, this was
pure plumbing.

Proven via a new `dev-libs/debugpkg` fixture whose `src_install` writes
the `PORTAGE_DEBUG` value it actually observed to a file under `${T}`,
run once with `debug: true` and once with `debug: false`, asserting `"1"`
and `"0"` respectively (`ebuild_phases::tests::
debug_flag_exports_real_portage_debug`) -- a marker-file proof rather
than one that captures the real `set -x` trace text itself (which would
need redirecting the whole test process's stdout/stderr, a much heavier
mechanism for the same underlying claim: that the export reaches the
phase, real `bin/ebuild.sh`'s own already-real guard does the rest).
`emerge`'s own `--debug` deliberately stays unchanged, still routed to
`pretend.rs`'s `report_option()` "not implemented" bucket -- `emerge`
never calls `ebuild_phases`/`ebuild_merge` at all (it's still pure
dry-run/`--pretend`), so there's no real phase-execution path there yet
to make `PORTAGE_DEBUG`/xtrace observable.

### Real `ebuild <file> package`/binpkg building (task #54's own natural sibling)

`ebuild <file> package` mirrors real `doebuild()`'s own `"package"`
action: `actionmap_deps["package"] == ["install"]` (the same
prerequisite-chain idea as `merge`), so it first runs the real `install`
phase chain, then really invokes `bin/misc-functions.sh`'s own
`__dyn_package` (real, unmodified bash). Grounding this against
`lib/_emerge/MiscFunctionsProcess.py`/`doebuild.py` found `misc-
functions.sh` is spawned as a *separate script invocation*, not a
`bin/ebuild.sh` phase (`bin/phase-functions.sh`'s own case statement has
no `"package"` branch at all) -- `ebuild_phases.rs` gained a new
`run_misc_function`/`run_misc_functions` primitive alongside the
existing `run_one_phase`, sharing a new `phase_setup_script()` helper
extracted from what used to be `run_one_phase`'s own inline `format!`
block, plus an `extra_env` hook for command-specific exports
(`PKGDIR`/`PORTAGE_BINPKG_TMPFILE`/`BINPKG_FORMAT`/
`PORTAGE_COMPRESSION_COMMAND`/`PORTAGE_PYTHONPATH`) that `run_one_phase`
itself doesn't need.

`__dyn_package` itself is real, unmodified bash: it tars `${D}` into
`PORTAGE_BINPKG_TMPFILE`, then shells out to the real, unmodified
`bin/xpak-helper.py recompose` to append XPAK metadata read straight out
of `${PORTAGE_BUILDDIR}/build-info` (already populated for free as a
side effect of the `install` chain's own case branch -- no
reimplementation needed on either side). `PORTAGE_BINPKG_TMPFILE` is the
real, final destination path itself (`lib/_emerge/EbuildPhase.py:210-249`:
`os.path.join(PKGDIR, CATEGORY, PF) + ".tbz2"` for the real default
`BINPKG_FORMAT="xpak"`) -- no separate atomic-rename step for the binpkg
file the way this pilot's own vdb write uses one.

`portage_repo`'s own pre-existing binary-package reader (task #53/#63)
never parses a `.tbz2`/XPAK file's own content at all -- only
`<PKGDIR>/Packages`, a plain-text index -- so the new `ebuild_package.rs`
module also writes/updates a real entry there (`write_packages_index_entry`:
creates a minimal `TIMESTAMP`-only header on first write, replaces an
existing block for the same `CPV` on a rebuild, otherwise preserves every
other entry untouched), sourcing `SLOT`/`KEYWORDS`/`IUSE`/`LICENSE`/
`PROPERTIES`/`RESTRICT`/the `*DEPEND` family from the ebuild's own repo's
real `metadata/md5-cache` entry via `portage_repo::read_md5_cache` --
the exact same source `emerge --pretend`'s own dependency resolution
already trusts -- when the ebuild's own containing repo can be found by
walking up for a `profiles/repo_name` file, closing the loop: a package
built this way is immediately visible to `emerge --pretend --usepkg`. A
real `BUILD_TIME` (the wall-clock time the package finished building) is
written into both `build-info` and the `Packages` entry, matching real
portage's own use of it for `--rebuilt-binaries` comparisons.

Real vdb `COUNTER` is deliberately left untouched by this code -- grepped
`doebuild.py` for `counter_tick`/`COUNTER` and found zero hits, confirming
real `doebuild()`'s own `"package"` action never touches it at all (it's
a real install/merge-time-only concept, and `ebuild <file> package` never
merges anything).

**v1 scope cuts as of this section's own original slice** (see
`ebuild_package.rs`'s own module doc comment for the current, full
list): `BINPKG_FORMAT` was always `"xpak"` here -- the newer `"gpkg"`
format has since shipped, see this file's own "Real `ebuild <file>
package`: the `gpkg` binary-package format" section below. Real
`PORTAGE_COMPRESSION_COMMAND` resolution has since shipped -- see this
file's own "Real `PORTAGE_COMPRESSION_COMMAND` resolution" section
below; that paragraph's original claim (hardcoded `"bzip2 -c"`) is now
stale. `USE` is always empty in the `Packages` entry, matching this
pilot's own phase
environment (nothing was actually built with any USE flags enabled, so
an empty set is the honest value). No `BUILD_ID`/`packdebug`/
`splitdebug`/RPM (`__dyn_rpm`) support, no `PKGDIR`-index locking (this
pilot's own CLI is never invoked concurrently against the same
`PKGDIR`), and no real `bindbapi.inject()` equivalent (no long-lived
in-memory binary-package database here, only ever re-reading `Packages`
fresh each invocation).

Proven via a new `dev-libs/packagepkg` fixture (deliberately new rather
than retrofitted onto the pre-existing `mergepkg` fixture, to avoid
making `mergepkg` newly visible to unrelated `emerge --pretend` repo-wide
scans) and a real end-to-end test
(`ebuild_package::tests::real_package_builds_a_real_xpak_tbz2_and_a_real_packages_entry`):
runs `run_package`, asserts the produced `.tbz2` contains the real
`"XPAKPACK"`/`"XPAKSTOP"` magic bytes, then asserts `portage_repo`'s own
unmodified `list_binary_candidates`/`read_binary_metadata` correctly see
the freshly-built package with the right version/slot/keywords/`RDEPEND`
-- proving the metadata really flowed end-to-end from the ebuild's
md5-cache entry into a `Packages` index this pilot's own binary-package
reader already trusts.

### Real `emerge --buildpkgonly` execution: `emerge`'s own first real, non-dry-run action

Every prior slice in this pilot kept `emerge` itself 100% dry-run --
`--pretend` was a hard requirement, and even `--buildpkgonly` only ever
changed what the dry-run *report* said (see the write-up above). This
slice makes `emerge --buildpkgonly <atom>`, given *without* `--pretend`,
actually build a real binary package for every entry the resolution
graph says needs one -- the one real, non-dry-run action `emerge` itself
now implements. `--pretend --buildpkgonly` together still stays a pure
dry-run report, unchanged: real `--pretend` always suppresses every real
action, no matter what else is requested alongside it, and this pilot
now mirrors that precisely rather than only supporting dry-run at all.

The gate `resolve_pretend_graph`'s own `buildpkgonly_deps_unsatisfied`
check already computes (see the write-up above) turns out to do double
duty here: real `--buildpkgonly` refuses to resolve at all when a
package that needs building depends on another package that *also*
needs building, which means that once the gate passes, nothing in the
needs-building set depends on anything else in it -- there is no
cross-package build ordering to compute at all. `emerge_build.rs`'s new
`run_buildpkgonly` just walks the resolved entries in order and calls
`ebuild_package::run_package` (task #105-#109 -- the exact same
machinery `ebuild <file> package` already uses) for each one, since
`GraphEntry` doesn't carry the winning candidate's own repo location (a
deliberate omission -- see its own doc comment); `locate_candidate`
re-derives it via `portage_repo::list_candidates`, the same repo/version
lookup `resolve_pretend_graph` already did internally to pick the
winning version in the first place.

Building this surfaced a real, worth-recording finding, checked
empirically rather than assumed: this pilot's environment setup never
populates `A`/`AA` from `SRC_URI` at all, so a real ebuild with a
nonempty `SRC_URI` does *not* fail at `unpack` the way "no fetch
machinery" first suggested it would -- EAPI 0's own default `src_unpack`
(`unpack ${A}`) just runs with nothing to unpack and silently
*succeeds*. Real portage's own `SRC_URI`-vs-`DISTDIR` check happens in a
separate pre-phase step inside `doebuild()` itself, before the ebuild's
own phases ever run at all -- a mechanism this pilot has no equivalent
of. Left unguarded, this would silently produce a real, valid-looking
but functionally *empty* binary package instead of erroring, which is
worse than a loud failure. `run_buildpkgonly` therefore checks the
winning candidate's own md5-cache `SRC_URI` field and refuses outright
(exit 1, `"has a real SRC_URI, but this pilot has no real fetch/unpack
machinery ... refusing rather than silently building an empty
package"`) rather than letting that happen. Real fetch + Manifest
verification stays a separately-scoped, not-yet-attempted follow-up.

**v1 scope cuts as of this section's own original slice** (see
`emerge_build.rs`'s own module doc comment for the current, full list):
a `CandidateSource::Binary` entry (would only come from `--usepkg`) is
skipped outright, nothing to build, still true. Real `--keep-going` has
since shipped -- see this file's own "`emerge --buildpkgonly
--keep-going`" section below; this paragraph's original "no partial-
graph continuation" claim is now stale. `--debug` isn't threaded
through this path yet (real `emerge --debug` still routes to the
pre-existing "not implemented" bucket, unchanged).

Proven via `dev-libs/packagepkg` (real end-to-end build, same fixture
task #105-#109 already established) and a new `dev-libs/fetchpkg`
fixture (a real, nonempty `SRC_URI`, proving the refusal fires and
nothing gets built), both as Rust unit tests
(`emerge_build::tests::real_buildpkgonly_builds_a_real_binary_package_end_to_end`/
`real_buildpkgonly_refuses_a_real_src_uri_instead_of_building_an_empty_package`)
and as black-box CLI tests against the compiled `emerge` binary
(`test_portuale.py`'s own
`test_emerge_buildpkgonly_without_pretend_really_builds_a_binary_package`/
`test_emerge_buildpkgonly_with_pretend_stays_dry_run`/
`test_emerge_buildpkgonly_refuses_a_real_src_uri_with_no_manifest_entry`)
-- this path has no Python reference mirror at all (unlike every
dry-run-only feature so far): the Python side gained the identical
CLI-gate/message-text changes so the shared dry-run contract tests
still match byte-for-byte, but it has no real ebuild-execution
machinery to mirror the actual building with, consistent with every
other real-execution slice in this pilot staying Rust-only.

### Real `ebuild <file> package`: the `gpkg` binary-package format

`ebuild <file> package` (and `emerge --buildpkgonly`) built an `xpak`
`.tbz2` unconditionally -- `BINPKG_FORMAT` was hardcoded. The `$PKGDIR`
directory-scan buildout added a *reader* for the newer `gpkg`
(`.gpkg.tar`) format (`binpkg::read_gpkg_metadata`, see that section);
this slice closes the loop with the *writer*, so a package this pilot
builds can round-trip through its own reader.

The mechanism is the same "drive real, unmodified bash + a real,
unmodified helper" one the `xpak` path already uses. Real
`bin/misc-functions.sh __dyn_package` (already invoked, unmodified) has
an `elif [[ "${BINPKG_FORMAT}" == "gpkg" ]]` branch that shells out to
real, unmodified `bin/gpkg-helper.py compress "${PF}"
"${PORTAGE_BINPKG_TMPFILE}" "${PORTAGE_BUILDDIR}/build-info" "${D}"` --
real `portage.gpkg.gpkg()._generate_metadata_from_dir()` +
`.compress()`, no reimplementation. All `ebuild_package.rs` does is:
take a `BINPKG_FORMAT` (`PackageOptions::binpkg_format`, env-var-sourced
at the `ebuild.rs`/`pretend.rs` CLI boundary exactly like
`BINPKG_COMPRESS` already is; `"xpak"` or `"gpkg"`, anything else is
`Err("Unknown BINPKG_FORMAT …")` -- real `__dyn_package`'s own `die`);
name the output `<cat>/<pf>.gpkg.tar` instead of `.tbz2`; export
`BINPKG_FORMAT=gpkg` (plus `BINPKG_COMPRESS`/`BINPKG_COMPRESS_FLAGS[_
<NAME>]`/`PORTAGE_BZIP2_COMMAND`, because real `gpkg-helper.py` builds
its own `portage.settings` inside the subprocess and reads the
compressor from it -- real `gpkg._get_binary_cmd` -- rather than from
the `PORTAGE_COMPRESSION_COMMAND` the `xpak` tar-pipe uses, so the build
would otherwise depend on the host's own `make.conf`); and write a
`PATH: <cat>/<pf>.gpkg.tar` field into the `Packages` entry (real
portage records `PATH` for every `gpkg` -- unlike a plain `.tbz2`, a
`.gpkg.tar` isn't derivable from the `CPV`).

The produced `.gpkg.tar` is a genuine real-portage `gpkg` container: an
outer tar of `<basename>/gpkg-1` (the format marker), the compressed
`<basename>/metadata.tar.<comp>`, the compressed
`<basename>/image.tar.<comp>`, and a `<basename>/Manifest`. **v1 cuts**
(see `ebuild_package.rs`'s own doc comment): no `gpkg` *signing*
(`FEATURES=binpkg-signing`/`binpkg-request-signature` -- the same "this
pilot has no crypto" cut the reader's `Manifest`/`.sig` verification
already documents), no `BUILD_ID` in the basename.

Proven end-to-end both ways:
`ebuild_package::tests::real_package_with_gpkg_format_builds_a_real_gpkg_tar_this_pilots_reader_round_trips`
builds the `.gpkg.tar` (bzip2), then reads it back with this pilot's
*own* `binpkg::read_gpkg_metadata` and asserts `SLOT`/`CATEGORY`/`PF`/
`RDEPEND` survived the round trip, plus `BinaryIndex::from_pkgdir` and
`binpkg::scan_pkgdir` both see it; the black-box
`test_emerge_buildpkgonly_with_binpkg_format_gpkg_builds_a_real_gpkg_tar`
runs `emerge --buildpkgonly` with `BINPKG_FORMAT=gpkg BINPKG_COMPRESS=
gzip` and asserts the real container members + the `Packages` `PATH`
field. Rust-only, like every real-execution slice.

### Real `SRC_URI` fetch: `emerge --buildpkgonly`/`ebuild <file> install` now really download real distfiles

The previous slice's own "known, documented gap" -- no fetch/unpack
machinery at all, a real `SRC_URI` silently produced an empty install
image -- is now real: a real, unmodified `wget` subprocess (real
`make.globals`'s own default `FETCHCOMMAND` template, invoked verbatim)
downloads each file `SRC_URI` names into a real `DISTDIR`, verified
against the real, unmodified `Manifest` file's own `BLAKE2B`/`SHA512`
digests (`MANIFEST2_HASH_DEFAULTS`) before `unpack` ever runs. A new
crate, `portage-fetch`, implements the non-network half as pure,
100%-offline-testable logic: a recursive-descent `SRC_URI` parser
supporting the real grammar (arrow-rename, `flag?`/`!flag?`
USE-conditional groups, nested -- real `SRC_URI` has no `||` any-of
groups at all, PMS 8.2.6.5, so none are implemented), a `Manifest`
`DIST`-line parser, and digest verification via the real, standard
`blake2`/`sha2` crates (not reimplemented from scratch, same precedent
`ebuild_merge.rs`'s own real MD5 `CONTENTS` digest already set). A new
module, `portuale/src/fetch.rs`, adds the one network-touching piece:
spawning `wget` and orchestrating the "already verified, skip" vs
"fetch then verify" decision. `ebuild_phases.rs`'s own `run_commands`
now runs this once per invocation, right before the phase loop,
whenever the real prerequisite chain includes `unpack` -- matching real
`pkg_pretend`'s own PMS-mandated position *before* fetching -- and
exports the real `A`/`AA` variables (and `DISTDIR` itself, unconditionally)
into every phase, exactly like real `doebuild()`'s own environment.

A real, load-bearing bug this surfaced, found only by testing against a
real, live package rather than trusting the design on paper: the first
working version never actually `export`ed `DISTDIR` itself into the
phase environment (only used it internally to decide where to fetch
to) -- real `unpack` (a `bin/ebuild-helpers/` script, unmodified) reads
`${DISTDIR}` itself to resolve `${A}`'s own filenames, so a real,
successfully fetched-and-verified distfile still failed with `"either
does not exist or is not a regular file"` until this was fixed.

**Live-verified against the real system** (not just fixtures): running
`emerge --buildpkgonly` against real packages from this machine's own
live Gentoo tree confirmed, for real: `app-arch/unzip`'s first
`SRC_URI` entry was genuinely downloaded from `sourceforge.net` over
real HTTPS and its real SHA-512 digest matched the tree's own real
`Manifest` exactly; `sys-apps/which`/`app-arch/unzip`'s own *second*
`SRC_URI` entry (a real `mirror://gnu/...`/`mirror://debian/...` URI)
correctly hit the documented `mirror://` gap below with a clear,
specific error rather than silently misbehaving; after manually
pre-fetching that one `mirror://`-gated file by hand into the real
system `DISTDIR`, `app-arch/unzip` went on to really fetch, really
verify, really unpack, and produce a real, genuine `.tbz2` binary
package end-to-end.

**v1 scope cuts** (see `portage-fetch`'s own and `fetch.rs`'s own
module doc comments for the full lists): no `mirror://` resolution at
all (a real, live, documented gap confirmed above) -- real
`thirdpartymirrors`/`GENTOO_MIRRORS` config resolution is a
separately-scoped follow-up. No resume support (real `RESUMECOMMAND`'s
own `-c`/retry behavior) -- a failed download is removed and retried
from scratch. No GPG verification (`FEATURES=verify-sig`). No
`FEATURES=distlocks` (this pilot's own single-invocation-at-a-time CLI
usage never races a concurrent fetch of the same file). A file with no
`Manifest` entry at all is refused outright (unverifiable content is
worse than a loud failure) rather than fetched-but-unverified.

Proven via `portage-fetch`'s own 14 unit tests (`Manifest` parsing,
`SRC_URI` flattening including nested/negated conditionals and arrow-
rename, and digest verification against real, independently-confirmed
BLAKE2b-512/SHA-512 test vectors), `fetch.rs`'s own 5 unit tests
(including two genuine `wget` subprocess downloads over real loopback
HTTP -- this system's own `wget` build has no `file://` support at all,
confirmed empirically, so a tiny real HTTP server stands in for a truly
external one, fully offline and deterministic), and a new
`dev-libs/verifiedfetchpkg` fixture whose real, checked-in `Manifest`
matches a pre-seeded `DISTDIR` payload -- exercising the full real
`SRC_URI` grammar (arrow-rename plus a `test?` group that must stay
excluded from `A` but still appear in `AA`) through the real CLI with
no network access at all, both as a Rust unit test
(`ebuild_phases::tests::
install_computes_real_a_and_aa_from_a_verified_distfile_with_no_network`)
and a black-box one
(`test_ebuild_install_really_fetches_via_the_already_verified_skip_path`).

### Bug fix: `avoid_update`'s own dependency-atom shortcut was requiring visibility it never should have

Found by live-testing `emerge --buildpkgonly` against this machine's own
real Gentoo tree (not a fixture): `sys-fs/fuse`'s own real
`sys-libs/liburing:=[abi_x86_64(-)?,...]` dependency was installed at
2.14, whose real `KEYWORDS` had since gone `~amd64`-only (no longer
accepted under this system's own default `ACCEPT_KEYWORDS="amd64"`).
This pilot's `resolve_pretend` printed a spurious `[ebuild D]
sys-libs/liburing-2.9 (downgrade from 2.14)` -- and, worse, recursed
into `liburing-2.9`'s own dependency chain as if it needed real work,
inflating the "would rebuild" set with ~20 unrelated packages
(`python`, `openssl`, `perl`, `meson`, ...) that real portage never
touches at all. Real `emerge --pretend --buildpkgonly
=sys-fs/fuse-3.18.2` prints exactly one line
(`[ebuild R] sys-fs/fuse-3.18.2`); this pilot's own `--buildpkgonly`
gate (task #100-#104) then correctly refused to proceed given that
inflated set -- the gate itself was never wrong, the resolution
feeding it was.

Root cause, found by reading real `_select_pkg_highest_available_imp`
(`lib/_emerge/depgraph.py` ~8440) directly: for a **dependency** atom
(`parent is not None`), real portage returns the installed package
immediately whenever nothing wants an update (`not self.
_want_update_pkg(parent, inst_pkg)`) -- with **no visibility check at
all**. That's a genuinely different, *earlier* real code path than the
**top-level** atom's own `avoid_update` block a few lines later, which
*does* require visibility (`self._pkg_visibility_check(pkg, ...)`).
This pilot's own `resolve_pretend` had only ever ported the top-level
version of that shortcut, applying its visibility requirement
uniformly to dependency atoms too -- silently wrong for any dependency
installed at a version the tree no longer keyword-accepts, a routine
occurrence on a `~amd64`-tracking system syncing against a
still-mostly-stable tree.

The real ebuild case that actually surfaces this (`liburing:=[...]`) also
carries a USE-dep on the atom itself -- real portage checks that against
the installed package's own real vdb `USE`/`IUSE` for this same early
return, not the current tree's, so the fix does too
(`use_deps_satisfied`, reused as-is from the existing tree-USE call
site, just fed vdb data instead). Fixed in both `resolve_pretend`
(single-atom) and implicitly `resolve_pretend_graph` (which calls it
for every dependency atom in the BFS) -- no separate graph-level change
needed. A first attempt only checked this early enough when the atom
had no USE-deps at all (falling back to the old, visibility-gated path
otherwise) -- caught immediately by re-testing against the real
`liburing:=[...]` atom itself, which still showed the spurious
downgrade; the real fix needed the vdb-USE-dep check moved *before* the
tree's own visibility/USE-dep filtering can bail out with
`NoVisibleCandidate` at all, not just added on top of it afterward.

Proven via three new fixture pairs, each as both a Rust unit test and a
Rust-vs-Python contract test: `keywordmaskedpkg`/`needskeywordmasked`
(the base case -- installed 2.0, `~amd64`-only, only 1.0 stable-visible;
kept as-is when reached as a dependency, still a real downgrade when
requested directly as a **top-level** atom, proving the two real code
paths stay genuinely distinct) and
`keywordmaskedusepkg`/`needskeywordmaskeduse` (the same situation plus a
real USE-dep on the atom, checked against real vdb `USE` -- the actual
`liburing:=[...]`-shaped case).

### Real eclass `inherit()` support: `PORTAGE_ECLASS_LOCATIONS`

The single biggest real blocker to "a real ebuild actually builds",
found by running `emerge --buildpkgonly` against real packages
(`app-editors/nano`, `app-arch/xz-utils`, `sys-fs/fuse`) after the
`avoid_update` fix above: real `bin/ebuild.sh`'s own `inherit()`
function (unmodified bash) walks a real bash array,
`PORTAGE_ECLASS_LOCATIONS`, looking for `<location>/eclass/
<name>.eclass` for every eclass an ebuild's own top-level `inherit
...` line names -- and this pilot never populated that variable at
all, so `inherit()` `die`d immediately for literally any eclass,
before this fix. Nearly every real-world ebuild inherits *something*
(`nano` needed `verify-sig`; `xz-utils` needed `dot-a`/`flag-o-matic`/
`libtool`/`multilib`/`multilib-minimal`/`preserve-libs`/
`toolchain-funcs`; `fuse` needed `flag-o-matic`/`meson-multilib`/
`toolchain-funcs`/`udev`/`python-any-r1`), so this alone made
real compilation of almost any real package impossible.

Real `doebuild.py` sets this variable from `repo.eclass_db.
eclass_locations_string` -- a real, `shlex`-quoted, priority-ordered
list of the ebuild's own repo *plus every one of its master repos*
(real masters-chain resolution, `config.py:1256-1266`/`eclass_cache.
py:177-179`: `eclass_locations = [master.location for master in repo.
masters] + [repo.location]` unless already present, exported
`reversed()` -- the ebuild's own containing repo searched first, its
masters after, in real declared order). `eclass_locations_value`
(`ebuild_phases.rs`) originally implemented only the narrower half of
this -- the ebuild's own containing repo alone, no masters chain at
all -- a real, separately-scoped gap for an overlay ebuild that
inherits a main-repo-only eclass without redeclaring it locally. The
masters chain is real now too: `RepoConfig::masters` (already resolved
elsewhere -- `ebuild_merge::blocked_installed_packages`/`pretend.rs`
already consult it for profile/USE config stacking) is looked up via
`portage_repo::find_repos(config_root)`, matched against the ebuild's
own containing repo by location; `config_root` itself is threaded all
the way down from each real caller's own CLI-boundary `PORTAGE_
CONFIGROOT` resolution (`ebuild.rs`'s `merge_options.config_root`,
`MergeOptions`/`UnmergeOptions`/`PackageOptions`'s own `config_root`
field, mirroring `MergeOptions::config_root`'s own established
"explicit field, deliberately-inert `Default`" shape) through the
whole phase-execution call chain (`run_commands`/`run_single_phase` ->
... -> `phase_env_vars`). Any resolution failure (missing `repos.conf`,
the containing repo not listed in it, etc.) degrades to the original
v1 behavior -- the same graceful-degrade precedent `blocked_installed_
packages` established. Every real eclass this pilot had live-verified
before this fix happened to live in the *same* repo as the ebuild that
inherits it (`nano`'s/`xz-utils`'s/`fuse`'s own eclasses are all real
files under the real `gentoo` main repo's own `eclass/` directory,
that repo's own real `metadata/layout.conf` declaring `masters =`
empty), so the gap was real but never yet hit live -- now closed
directly, with a real, end-to-end fixture (an overlay ebuild inheriting
an eclass that only exists in its own master repo) proving it.
Exported unconditionally (like `DISTDIR`), not just for phases that
happen to need it, matching real `doebuild()`'s own environment.

```sh
CFGROOT="$(mktemp -d)"
MAIN="${CFGROOT}/repos/main"
OVERLAY="${CFGROOT}/repos/overlay"
mkdir -p "${MAIN}"/profiles "${MAIN}"/eclass "${OVERLAY}"/profiles \
    "${OVERLAY}"/dev-libs/overlaypkg
echo main > "${MAIN}"/profiles/repo_name
echo overlay > "${OVERLAY}"/profiles/repo_name
cat > "${MAIN}"/eclass/mastershared.eclass <<'ECLASS'
mastershared_hello() {
	echo "hello from mastershared.eclass"
}
ECLASS
cat > "${OVERLAY}"/dev-libs/overlaypkg/overlaypkg-1.0.ebuild <<'EBUILD'
EAPI=8
DESCRIPTION="cross-repo masters-chain eclass inherit"
SLOT="0"
KEYWORDS="amd64"
inherit mastershared
src_install() {
	mastershared_hello > "${T}/eclass-marker.txt" || die
}
EBUILD
mkdir -p "${CFGROOT}"/etc/portage
cat > "${CFGROOT}"/etc/portage/repos.conf <<EOF
[DEFAULT]
main-repo = main

[main]
location = ${MAIN}

[overlay]
location = ${OVERLAY}
masters = main
EOF

export PORTAGE_CONFIGROOT="$CFGROOT"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    "${OVERLAY}/dev-libs/overlaypkg/overlaypkg-1.0.ebuild" install
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/overlaypkg-1.0/temp/eclass-marker.txt
# hello from mastershared.eclass -- found via the overlay's own masters
# chain, even though the eclass itself never lived in the overlay repo.
unset PORTAGE_CONFIGROOT
```

**A new, separate gap surfaced while live-verifying this -- since
fixed upstream**: after `inherit()` itself stopped `die`ing,
`app-arch/xz-utils` and `sys-fs/fuse` (but not `app-editors/nano`)
hung indefinitely during real phase execution. Bisected live (a
scratch fixture repo, copying in real eclasses one at a time under a
timeout) to the `multilib` eclass family specifically: `flag-o-matic`
+ `toolchain-funcs` alone (the pair `nano` doesn't need but both
hanging packages do) complete fine; adding `multilib-minimal` (which
pulls in `multilib-build` -> `multibuild`/`multilib`) reproduces the
hang.

Root-caused down to a real bug in the pinned `brush` fork itself, not
this pilot's own code, confirmed with a minimal, portage-free repro
against the standalone `brush` binary (nothing eclass- or
ebuild-related):

```sh
big() { for i in $(seq 1 5000); do echo "padding line ${i}"; done; }
big | cat > out.txt   # hung forever before the fix
```

`brush-core/src/interp.rs`'s `spawn_pipeline_processes` spawns each
pipeline stage in a loop, `.await`ing `execute_in_pipeline` before
moving to the next stage. For external processes and builtins run in
an owned (non-last-stage) shell, that's fine -- `execute_via_builtin_
in_owned_shell` wraps the builtin in `tokio::task::spawn_blocking`, so
the `.await` resolves as soon as the task is *spawned*, not when it
*finishes*. But `commands.rs`'s `execute_via_function` had no such
wrapping: it ran a shell function's body inline, so `spawn_pipeline_
processes`'s loop genuinely blocked until the function fully returned
-- meaning the *next* pipeline stage (the one that would actually
drain the function's stdout pipe) was never even spawned yet. A
function that writes more to stdout than the OS pipe buffer holds
(~64KiB on Linux) before returning then blocks on that `write()`
forever. This is exactly what real `bin/phase-functions.sh`'s
`__save_ebuild_env | __filter_readonly_variables` hits during the
post-phase "save the env" step (`bin/phase-functions.sh`'s own
`pretend|setup` case arm) -- both sides are shell functions, and
`__save_ebuild_env` dumps every function and variable in scope. The
tiny `pilotcheck.eclass` fixture stays under 64KiB so it never
triggered this; the multilib family (dozens of functions,
`toolchain-funcs.eclass` alone is ~1300 lines) easily does.

Fixed in the pinned `vivo75/brush` fork (`brush-core/src/commands.rs`),
and submitted upstream as
[reubeno/brush#1276](https://github.com/reubeno/brush/pull/1276) (open,
no review yet -- this is the one fork-only fix keeping the pin off
upstream `main`; see `PORTING/BRUSH_FORK.md`). The fix splits
`execute_via_function` the same way `execute_via_builtin`
already was: an owned-shell path that spawns the function's body as a
background task (`tokio::task::spawn_blocking` + `rt.block_on`,
mirroring `execute_via_builtin_in_owned_shell` exactly) so pipeline
spawning can proceed to the next stage immediately, and an unchanged
parent-shell path (used only for a pipeline's own last stage) that
still awaits inline. Verified: the `big | cat` repro above now
completes instantly; real `xz-utils`/`fuse` `pretend` now exit `0`
instead of hanging; the fork's own 2,174-case bash-compatibility test
suite (`cargo test -p brush-shell --test brush-compat-tests`) passes
identically before and after (1795 succeeded / 0 failed / 379 known-
to-fail / 28 skipped in both cases) -- confirmed by rebuilding from a
clean `cargo clean` on both sides of the fix, not just trusting a
cached binary. A new regression case (`brush-shell/tests/cases/
compat/pipeline.yaml`'s own "Function stage writing more than a pipe
buffer before the next stage is spawned") reproduces the original
hang under the suite's own 15s per-test timeout.

Proven via a new `dev-libs/eclasspkg` fixture with a real (if fixture-
only) `eclass/pilotcheck.eclass` defining one real function,
`pilotcheck_hello` -- `src_install` calls it directly, proving the
eclass's own *content*, not just its own presence, is real and usable
after `inherit()` finds it. Both as a Rust unit test
(`ebuild_phases::tests::
install_really_inherits_a_real_eclass_and_calls_its_own_function`, plus
two narrower ones directly exercising `eclass_locations_value`) and a
black-box one against the compiled `ebuild` binary
(`test_ebuild_install_really_inherits_a_real_eclass`).

### `ebuild --shell bash|brush`: a second, real shell execution backend

Every phase, hook, and `bin/misc-functions.sh` `__dyn_*` call this
pilot runs (`ebuild_phases::run_one_phase`/`run_misc_functions`) had
exactly one real execution backend until now: an embedded
`brush_core::Shell` (see this module's own doc comment for why brush
at all). `--shell bash|brush` (default `brush`, matching the pre-
existing behavior unchanged) adds a second, genuinely different real
backend: a plain `bash <bin_dir>/ebuild.sh <phase>` subprocess --
matching real portage's own `_doebuild_spawn()` invocation shape
almost exactly (`lib/portage/package/ebuild/doebuild.py`'s own `cmd =
"{ebuild.sh} {phase}"`, spawned via `portage.process.spawn()`; real
`bin/ebuild.sh:153`'s own `EBUILD_SH_ARGS="$*"` picks `<phase>` up
from the subprocess's own positional args, which its own tail
(`bin/ebuild.sh:830-843`) then really uses to call `__ebuild_main
${EBUILD_SH_ARGS}` and `exit` -- the exact real mechanism the brush
backend's own doc comment explains it deliberately avoids, since a
bare `exit` inside an *embedded* shell would kill the whole hosting
Rust process; a real subprocess has no such problem, so `--shell bash`
uses that real mechanism directly instead of brush's own "source, then
separately `invoke_function`" two-step). `bin/misc-functions.sh` gets
the same treatment (`bash misc-functions.sh __dyn_<mydo>`, matching
real `doebuild.py`'s own `misc_sh` invocation shape exactly).

Both backends are built from the exact same `phase_env_vars` (name,
value) pairs -- `--shell brush` formats them into `export NAME=value`
bash source text first (`phase_setup_script`); `--shell bash` passes
them directly as real subprocess environment variables
(`std::process::Command::envs`), with no shell-quoting step -- and so
no `$`/backtick-expansion risk -- at all, arguably simpler and safer
than the brush path's own `{value:?}` Rust-Debug escaping.

A pilot-only flag, not a real `bin/ebuild` option at all -- checked
directly in `ebuild.rs`'s own CLI-parsing loop (the same "special-
cased outside the real-options table" treatment `--help`/`-h` already
get), deliberately not added to `ebuild_options::OPTIONS` (a
transcription of real bin/ebuild's own argparse setup). Threaded down
through every real-execution call site this pilot has (`ebuild_
phases::run_commands`/`run_single_phase`/`run_misc_function`,
`ebuild_merge::MergeOptions`, `ebuild_package::PackageOptions`,
`ebuild_unmerge::run_unmerge`) -- `emerge --buildpkgonly`'s own real
build path (`emerge_build.rs` -> `ebuild_package::run_package`)
inherits it too via `PackageOptions`, though `emerge`'s own CLI
doesn't expose a `--shell` flag of its own yet (matching `--debug`'s
own pre-existing, identical non-wiring at that exact call site).

Proven identical, not just "also exits 0": a new Rust unit test
(`ebuild_phases::tests::
install_lands_a_real_file_under_a_real_d_via_real_bash`) runs the same
`dev-libs/phasepkg` fixture `install_lands_a_real_file_under_a_real_d`
already covers via brush, asserting the exact same real file lands
with the exact same content; a black-box pytest test
(`test_ebuild_shell_bash_produces_the_same_real_result_as_the_brush_
default`) does the same via the compiled binary, running both `--shell
brush` and `--shell bash` against the identical fixture and comparing
results directly.

### Real `mirror://` resolution: `profiles/thirdpartymirrors` + `GENTOO_MIRRORS`

The fetch slice's own documented gap -- a `mirror://<name>/<path>`
`SRC_URI` token was treated as an ordinary, unfetchable URI -- is now
real. `portage_fetch::resolve_mirror_candidates` looks `<name>` up in
the ebuild's own repo's real `profiles/thirdpartymirrors` file (a real
`grabdict()`-format file, `<name> <url1> [<url2> ...]` per line --
`parse_thirdpartymirrors` replicates real `grabdict`'s own per-token
`#`-comment truncation and "a name with zero URLs is skipped" default
exactly), expanding to `<mirror_root>/<path>` for every root under that
name (real `.rstrip("/") + "/" + path` string-built exactly).
`portage_fetch::gentoo_mirror_fallback` adds real portage's *second*
mirror mechanism on top: even a plain (non-`mirror://`) URI gets a
`GENTOO_MIRRORS`-root + `/distfiles/<filename>` fallback candidate
appended, real `async_mirror_url`'s own flat-layout path (this pilot
never negotiates a live, per-mirror `layout.conf` the way real portage
can -- confirmed by reading `MirrorLayoutConfig.get_best_supported_
layout`'s own fallback, this is exactly what real portage itself falls
back to whenever a mirror's `layout.conf` isn't reachable, and is what
every well-known `GENTOO_MIRRORS` entry actually uses).
`portuale/src/fetch.rs`'s own `fetch_src_uri` tries every candidate in
order (`mirror://`-expanded/literal-URI candidates first, `GENTOO_
MIRRORS` fallback last -- a real, deliberate deviation from real
portage's own more elaborate interleaving, documented in
`portage_fetch`'s own module doc comment; every candidate is still
real-digest-verified regardless of order, so this only affects which
mirror is attempted first, never correctness), stopping at the first
one that both fetches *and* verifies.

Deliberately not attempted (see `portage_fetch`'s own "KNOWN,
DOCUMENTED GAPS" for the current, full list): live `layout.conf`
negotiation, real candidate-list shuffling (`random.shuffle`, pure
load-balancing, not correctness), and `RESTRICT=mirror`/`primaryuri`
interactions. (Real `custommirrors` has since shipped -- see this
file's own "Real `custommirrors`: an admin-configured
`/etc/portage/mirrors` file" section below; this paragraph's original
claim is now stale.)

Live-verified against the real system: `app-arch/unzip-6.0_p31`'s own
real `SRC_URI` (`https://downloads.sourceforge.net/infozip/${MY_P}.
tar.gz mirror://debian/pool/main/u/${PN}/...debian.tar.xz`) -- the
exact package/entry this whole gap was originally found on -- now
really fetches *both* files, including the `mirror://debian/...` one,
resolved through the real `gentoo` main repo's own `profiles/
thirdpartymirrors` `debian` entry against a real Debian mirror.
`GENTOO_MIRRORS` itself is read once via `std::env::var` at exactly one
call site (`ebuild_phases::fetch_sources`'s own `FetchOptions`
construction), not inside `fetch_src_uri` -- `FetchOptions.gentoo_
mirrors` is an explicit field precisely so tests can set it to `vec![]`
(no real fallback attempted) or a scratch local server, matching this
pilot's own established "explicit parameter, not an ambient env read
inside library code" reasoning for anything a parallel test might need
to vary.

Proven via two new, real, end-to-end integration tests in
`portuale/src/fetch.rs` (a real local HTTP server, a real `wget`
subprocess, real digest verification -- no mocking): one builds a
scratch repo checkout (`profiles/repo_name` + `profiles/
thirdpartymirrors`) and fetches a real `mirror://testmirror/...` URI
through it; the other points `FetchOptions.gentoo_mirrors` at that same
local server while the literal `SRC_URI` itself is a real, immediately-
refused address (`127.0.0.1:1` -- deliberately *not* a black-holed
address like `192.0.2.1`, which would make real `wget -t 3 -T 60`
actually hang for its full multi-minute retry budget instead of failing
fast), proving the fallback path alone. Plus seven new, pure, offline
unit tests in `portage-fetch` for `parse_thirdpartymirrors`/
`resolve_mirror_candidates`/`gentoo_mirror_fallback` individually.

### `preserve-libs` collision exclusion: a merge can take over a registered preserved lib without aborting

`FEATURES=collision-protect`'s own documented gap is real now, for the
"consult and exclude" half: real `dblink._collision_protect`'s own
`plib_inodes`/`plib_collisions` handling (`lib/portage/dbapi/
vartree.py:3860-3985`). A colliding path whose real, on-disk `(st_dev,
st_ino)` matches a path the real `preserved_libs_registry` JSON already
lists for some other package is excluded from ordinary collision
reporting **unconditionally** -- real `_plib_registry` is constructed
unconditionally in `vardbapi.__init__`, never gated by
`FEATURES=preserve-libs` itself (that flag only gates the separate
*registration* side, not consulted here at all -- see below), so this
exclusion applies the same whether or not `FEATURES=collision-protect`
is set. After a successful merge, real `merge()`'s own post-copy step
(`:5095-5159`) is mirrored too: `unregister_preserved_libs` drops the
taken-over paths from the registry (removing the owning `cp:slot` entry
entirely once its own path list empties) and from the previous owner's
own real vdb `CONTENTS` (real `removeFromContents`), skipped only when
the previous owner *is* the package that was just merged.

The registry itself (`<root>/var/lib/portage/preserved_libs_registry`)
is a narrow, fixed-shape JSON document (real `PreservedLibsRegistry.
store()`'s own `{"cp:slot": [cpv, counter, [paths...]]}`,
`json.dumps(indent="\t", sort_keys=True)`) -- read and written with a
small hand-rolled parser/writer rather than a new `serde_json`
dependency, matching this pilot's own "small, format-specific parser
over a generic dependency" precedent (`--json` output's own hand-rolled
emitter, `SRC_URI`'s recursive-descent grammar, `grabdict`-format
`thirdpartymirrors`).

At the time this slice shipped, the preserve-libs *registration*/
detection side itself (real `_find_libs_to_preserve`/`LinkageMap`,
`scanelf`-based ELF `NEEDED`/soname introspection) and real `NEEDED`/
`LinkageMap` bookkeeping in `unregister_preserved_libs` were both
deliberately not yet attempted -- this slice only ever consulted and
unregistered a registry some other, unimplemented mechanism (or a
hand-seeded fixture, for testing) already populated. Both have since
shipped: the full registration/detection computation across "`preserve-
libs` registration: a real post-install `NEEDED.ELF.2`" and "the full
`LinkageMap`/`findConsumers`/decision computation" below, and the real
`NEEDED`-line stripping in `remove_from_contents` itself in "preserve-
libs: real `NEEDED.ELF.2` pruning on `remove_from_contents`" further
below. (As already documented in the `collision-protect` section above,
blocker exclusion and `FEATURES=protect-owned` have since shipped for
that feature too, though not for this one.)

Proven via five new Rust unit tests in `ebuild_merge.rs`: a JSON
round-trip test for the hand-rolled registry parser/writer; a
"missing/corrupt file degrades to an empty registry" test (real
`load()`'s own tolerance); an inode-map test confirming a registry path
that no longer exists on disk is silently skipped (real
`_lstat_inode_map`'s own `except OSError` -> `continue`); a sanity
baseline proving the new `dev-libs/preservepkg-old`/`dev-libs/
preservepkg-new` fixture pair is a genuine ordinary collision-protect
abort with **no** registry entry present (this pilot's own "a fixture
must actually distinguish the new behavior" rule); and the main
end-to-end proof -- with `preservepkg-old`'s own already-merged file
hand-registered in a seeded `preserved_libs_registry`,
`preservepkg-new` colliding on that exact path merges successfully even
with `collision_protect: true`, takes over the file's real content, and
afterwards neither `preservepkg-old`'s own vdb `CONTENTS` nor the
registry still list the path.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/preservepkg-old/preservepkg-old-1.0.ebuild merge
mkdir -p "${ROOT}/var/lib/portage"
cat > "${ROOT}/var/lib/portage/preserved_libs_registry" <<'EOF'
{
	"dev-libs/preservepkg-old:0": [
		"dev-libs/preservepkg-old-1.0",
		"0",
		[
			"/usr/lib/preservedtest/libfoo.so.1"
		]
	]
}
EOF
FEATURES="collision-protect" PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/preservepkg-new/preservepkg-new-1.0.ebuild merge
echo "exit: $?"
# exit: 0 -- no collision-protect abort, even though the destination
# path was already claimed by another package's own vdb CONTENTS
cat "${ROOT}"/usr/lib/preservedtest/libfoo.so.1
# new library content
cat "${ROOT}"/var/db/pkg/dev-libs/preservepkg-old-1.0/CONTENTS
# no longer lists /usr/lib/preservedtest/libfoo.so.1
cat "${ROOT}"/var/lib/portage/preserved_libs_registry
# {
# }
```

### `preserve-libs` registration: a real post-install `NEEDED.ELF.2` (first step, not the full subsystem)

The *registration* side of `preserve-libs` (`_find_libs_to_preserve`/
`_linkmap_rebuild`, ELF `NEEDED`/soname introspection via a real,
~954-line `LinkageMapELF`) is still the biggest single item in the open
backlog -- a genuinely new subsystem (ELF parsing plus a persistent
linkage graph), not attempted here. But its own real prerequisite turned
out narrower and better-scoped than "write an ELF parser in Rust":
`NEEDED.ELF.2`, the file the real linkage map is actually built from, is
generated by real, unmodified `bin/misc-functions.sh`'s own real
`scanelf`-driven `install_qa_check` (`bin/misc-functions.sh:164-221`,
`app-misc/pax-utils`'s own `scanelf` binary) -- no new ELF-parsing code
needed at all, the same "reuse real, unmodified `bin/*.sh`" precedent
this whole pilot already relies on everywhere else.

The real catch, found while grounding this: `install_qa_check` (plus
`install_symlink_html_docs`/`install_hooks`) is real `_post_phase_cmds
["install"]` (`EbuildPhase.py:424`/`442-461`) -- an **unconditional**
step real portage runs after *every* real `install` phase completes,
gated on no `FEATURES` flag at all, that this pilot's own install chain
never ran at all before this slice. `ebuild_phases::run_commands_async`
now runs all three (as one combined `bin/misc-functions.sh` invocation,
matching real `MISC_FUNCTIONS_ARGS`'s own unquoted `for x in
${MISC_FUNCTIONS_ARGS}` re-splitting -- three names joined into one
string here is exactly equivalent to three separate real argv entries)
right after `install` finishes, `EBUILD_PHASE` staying `"install"` for
the call, matching real portage's own `settings` reuse. `write_vdb_entry`
then copies the resulting `build-info/NEEDED.ELF.2` (if `scanelf`
actually found anything) into the real vdb entry, mirroring one file of
real `dblink.merge()`'s own broader `treewalk()` behavior (`vartree.py:
4912-4913`: *every* `build-info` file gets copied into the vdb wholesale
-- real `IUSE`/`KEYWORDS`/`EAPI`/etc. metadata this pilot's own vdb
entries still don't carry at all, a separate, broader, not-yet-attempted
gap deliberately left alone here).

**A real, previously-undiscovered bug this surfaced**: real EAPI 8's own
`___eapi_has_strict_keepdir` makes `bin/install-qa-check.d/95empty-dirs`
unconditionally strip any genuinely empty directory from the install
image (real PMS: ebuilds must not install empty directories; the real
ebuild-author fix is `keepdir`, not bare `dodir`). Two existing fixtures
(`envupdatepkg`, `collisionpkg-a`) used bare `dodir` on a directory
nothing was ever installed into -- accidentally relying on this pilot
never having run this real check before. Both switched to `keepdir`,
the real fix a real ebuild author would make; not a workaround.

Verified directly (a bare `dodir`'d empty directory is gone after
`install`; a `keepdir`'d one survives) and end to end (`NEEDED.ELF.2`
lands in the real vdb entry for a package installing a real,
dynamically-linked ELF binary):

```sh
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/elfpkg/elfpkg-1.0.ebuild merge
cat "${ROOT}"/var/db/pkg/dev-libs/elfpkg-1.0/NEEDED.ELF.2
# X86_64;/usr/bin/true;;;libc.so.6
```

### `preserve-libs` registration: the full `LinkageMap`/`findConsumers`/decision computation (still not wired into a real merge, confirmed with the user before implementing)

A second, deliberately narrow step toward `preserve-libs` registration:
`needed_elf.rs` (new module) ports real `NeededEntry`
(`lib/portage/util/_dyn_libs/NeededEntry.py`) -- the data model for one
parsed `NEEDED.ELF.2` line (`arch;filename;soname;rpaths;needed`,
semicolon-delimited, an optional 6th `multilib_category` field, any
further fields silently ignored). `soname` is a plain, possibly-empty
`String`, not `Option`, matching real Python exactly: real `scanelf`
genuinely reports an empty soname for some real libraries (e.g. musl's
own `libc.so`, which has no `DT_SONAME` at all -- precisely why real
`bin/misc-functions.sh` deliberately never invokes `scanelf -q`, which
would otherwise omit such libraries entirely). `rpaths`'s own real
`"  -  "` sentinel (two spaces, a dash, two spaces) -- the placeholder
that same `-q`-avoidance forces real portage to handle itself -- means
"no rpath at all" and is recognized as such. A malformed line (fewer
than 5 fields) parses to `None`; `parse_file` skips it and keeps going,
the same tolerance real `LinkageMap.rebuild()` itself already has.

A third narrow step, added in the same slice: `read_all_needed_entries`
ports real `LinkageMap.rebuild()`'s own *initial data-gathering loop*
(`LinkageMapELF.py:218-231`) -- for every real installed package (real
`dbapi.cpv_all()`, walked the same way `ebuild_merge::find_owners`
already walks every installed package's own vdb directory), its own
real vdb-stored `NEEDED.ELF.2`, parsed via `NeededEntry::parse_file`.
Degrades gracefully to an empty entry list for a package with no such
file at all (real `aux_get` itself already tolerates a missing aux file
the same way, returning `""`) -- a package is still included, with an
empty list, matching real `rebuild()`'s own unconditional per-cpv walk.

A fourth step, in the same slice: `rebuild` ports real `LinkageMap.
rebuild()`'s own *remaining* indexing logic (`LinkageMapELF.py:325-469`,
everything after the initial data-gathering loop) -- the real soname
providers/consumers map, deliberately excluding the one branch inside
real `rebuild()` that isn't `NEEDED.ELF.2`-driven at all (the live-
`scanelf`-for-orphaned-preserved-libs fallback, `LinkageMapELF.py:
233-324` -- the one real spot a raw ELF header read matters, still
correctly left for whenever preserve-libs registration actually needs
it). Per real entry: the real multilib category (its own field, or
`approx_multilib_category`'s own static-table fallback, real
`_approx_multilib_categories`); real `normalize_path`'d filename; real
`$ORIGIN`/`${ORIGIN}` runpath expansion (`os.path.dirname` of the
object's own filename, real dynamic-linker semantics) followed by
`normalize_path` again. Then real "implicit runpath" inference for
bundled libraries (`LinkageMapELF.py:380-410`): within the *same* owner
package's own entries, a needed soname provided by another entry from
that same owner gets its provider's own directory added to the
consumer's own runpaths when it isn't already there -- accounting for a
package's own internal library resolution without requiring an explicit
rpath. Finally, real per-object indexing keyed by `ObjKey` (a real
`(dev, ino)` pair when the object still exists on disk, `os.stat`-
followed-symlinks -- collapsing hardlink/symlink aliases of the same
real file into one entry, every recorded filename kept as an
`alt_paths` entry, matching real `_obj_key`'s own dedup-by-inode
semantics; falls back to the literal path string for an object that no
longer exists, narrower than real `os.path.realpath`'s own symlink-
resolving fallback, a deliberate simplification for a case that should
be rare -- an entry read moments after real `scanelf` itself confirmed
the object's existence, gone by the time this runs).

A fifth and final computational step, after re-reading `findConsumers()`/
`_find_libs_to_preserve()` in full to confirm the real scope before
committing to it: `getlibpaths`, `find_consumers`, and `find_libs_to_
preserve` port the rest of the real subsystem. `getlibpaths`
(`lib/portage/util/__init__.py:1945-1963`) is the real default dynamic-
linker search path -- `LD_LIBRARY_PATH` (an explicit parameter here, not
an ambient env read), every line of the real `/etc/ld.so.conf`, then
the real `/usr/lib`/`/lib` defaults. Its own `/etc/ld.so.conf.d/*.conf`
`include`-directive expansion is deliberately not reproduced, the same
v1 cut `env_update.rs`'s own module doc comment already documents and
confirmed with the user for the *other* real `/etc/ld.so.conf` reader in
this pilot.

`find_consumers` ports real `LinkageMap.findConsumers()`
(`LinkageMapELF.py:817-960`), narrowed to the one real calling
convention `_find_libs_to_preserve` itself actually uses (`obj` always a
path string; `exclude_providers` always exactly one real callable, not a
general collection). Real "shadowed by another version" detection first
(a same-directory soname symlink pointing somewhere else entirely means
no consumers at all -- the real binutils-`CHOST`-symlink bug context);
then real `exclude_providers`/`greedy` consumer-satisfaction filtering
(a consumer already reachable via some *other*, non-excluded provider of
the same soname is dropped from the result -- it wouldn't actually
break); finally, only consumers that can actually *reach* the queried
object (its own directory in their own runpath or the real default lib
path) are returned.

`find_libs_to_preserve` ports real `dblink._find_libs_to_preserve()`
(`vartree.py:3491-3595`), narrowed to its own pure computation -- the
real gating conditions (`FEATURES=preserve-libs` on, a real
`_installed_instance`, etc.) are left as the *caller's* own
responsibility, since there is no real caller yet. A minimal `LibGraph`
(real `portage.util.digraph`, narrowed to exactly the three operations
`_find_libs_to_preserve` uses: `add(node, parent)`, `root_nodes()`,
`child_nodes()`) builds a real dependency graph from `find_consumers`
results -- an edge from each provider to each of its own real consumers,
skipping a consumer that's itself being removed and isn't also a
provider. Walking from every real "root" consumer (nothing depends on
it, and it isn't itself a provider) finds every provider transitively
reachable -- those are the real preserve candidates. For each, real
hardlink/soname-symlink classification (`stat.S_ISREG` via
`symlink_metadata`, matching real `os.lstat`) decides what to actually
preserve, skipping a candidate the *new* package already replaces both
the real file *and* the soname symlink for -- that "does the new
package already cover it" check is folded into a caller-supplied
closure rather than a separate `unmerge: bool` parameter, since real
`not unmerge and self.isowner(f)` collapses to `false` for every real
unmerge-only caller anyway.

**Confirmed scope, before implementing, each of these five times**: even
with the pure computation now complete, this slice does *not* wire `find
_libs_to_preserve`'s own output into a real merge/unmerge's actual
control flow -- calling it at the right point in `ebuild_merge::
merge_after_install`, writing results into the real `preserved_libs_
registry.json` via the already-existing `write_plib_registry`, and
making `ebuild_unmerge::remove_contents` skip deleting a preserved path
are all real, separate control-flow integration work across two already-
tested files, left for a following slice rather than risking the
already-shipped preserve-libs *consult/unregister* side. This module
still has no real caller (`#[allow(dead_code)]`, documented in its own
module doc comment) -- the same "narrow, additive, no wiring until the
next slice needs it" shape this pilot used for explicit `masters =`
parsing landing before eclass masters-chain search ever consumed it.
That control-flow wiring has since shipped -- see "`preserve-libs`
registration: wired into a real unmerge's control flow" below.

Verified directly against hand-crafted lines/entries (`NeededEntry`
parsing: a real soname/multiple rpaths/multiple needed entries, the
`"  -  "` rpath sentinel, the optional multilib-category field both
present and empty, extra fields beyond the sixth ignored, a malformed
line rejected; `read_all_needed_entries`: multiple installed packages
some with a real `NEEDED.ELF.2` and some without, a missing
`var/db/pkg` degrading to an empty result; `rebuild`: a simple
provider/consumer pair indexed correctly, the approximate-multilib-
category fallback, `$ORIGIN` expansion, implicit same-owner runpath
inference *not* crossing package boundaries, real inode-based dedup of
two recorded paths for the same real file with both kept as
`alt_paths`, and the real path-string fallback for a since-vanished
object; `getlibpaths`: real `/etc/ld.so.conf` reading with comments and
an explicit `LD_LIBRARY_PATH`, degrading gracefully when missing;
`find_consumers`: a consumer found via the real default lib path, a
provider whose own directory is unreachable correctly excluded, a
consumer already satisfied by a non-excluded provider correctly
dropped, a shadowed object returning no consumers at all, and a real
`KeyError`-equivalent for an unknown object; `find_libs_to_preserve`: a
lib still needed by a real surviving consumer is preserved, a lib the
new package fully replaces -- both the real file and its own real
soname symlink -- is not, and a lib with no real consumers at all is
never preserved) and end to end against a real, live `scanelf`-
generated `NEEDED.ELF.2` for `rebuild` specifically (the same
`dev-libs/elfpkg` fixture, parsed, collected, and indexed through the
full real chain -- `NeededEntry::parse_file` -> `read_all_needed_
entries` -> `rebuild` -- after a real `run_merge`, confirming the real
installed binary's own real `DT_NEEDED` entries land as real consumers).

### `preserve-libs` registration: wired into a real unmerge's control flow

The control-flow integration explicitly deferred at the end of the
previous section is real now: `preserve_libs_on_unmerge` (new,
`ebuild_merge.rs`) ports real `dblink._prune_plib_registry()`
(`lib/portage/dbapi/vartree.py:2228-2314`), called from real
`unmerge()` right before `_unmerge_pkgfiles()` runs (`vartree.py:2493`/
`2529`, confirmed by reading the real call site, not just the method
itself), narrowed to the one real shape this pilot's own standalone
`ebuild <file> unmerge` always reaches: `unmerge_with_replacement=
False`, since this pilot's own `merge`/`unmerge` are always separate,
independent CLI invocations, never a combined depgraph-driven upgrade
transaction passing `preserve_paths` between them (real `instance_owns_
files and not unmerge_with_replacement` collapses to just `instance_
owns_files`). It rebuilds the system-wide `LinkageMap` from every real
installed package's own vdb-stored `NEEDED.ELF.2` (the package being
unmerged hasn't left the vdb yet, so its own data is still really part
of the map, matching real `exclude_pkgs=None` in this exact shape),
calls `find_libs_to_preserve` with the real `unmerge=True` semantics,
then unconditionally unregisters this package's own prior registry
entry before registering it as the new keeper of anything actually
preserved -- real `plib_registry.unregister`/`.register`, ported as
one `register_preserved_libs` function since real `unregister(cpv,
slot, counter)` **is** `register(cpv, slot, counter, [])` verbatim.
Real `cps = cpv_getkey(cpv) + ":" + slot` (`category/pn:slot`, no
version); empty `paths` removes the `cps` entry only if it still
records the *same* `cpv`/`counter` (never blindly erasing a different
package's own entry sharing that key); non-empty `paths` unconditionally
overwrites, matching real `_normalize_counter`'s own plain whitespace-
trim (not integer parsing).

`ebuild_unmerge::run_unmerge` calls it right before `remove_contents`,
threading the returned preserved-path set into a new `remove_contents`
parameter that filters them out of the parsed `CONTENTS` entries
immediately -- real "remove the preserved files from our contents so
that they won't be unmerged" (`vartree.py:2293-2294`). Persisting a
separately-rewritten `CONTENTS` file was judged unnecessary for this
pilot's own observable-behavior fidelity: the whole vdb entry directory
gets deleted wholesale moments later regardless, so filtering the
in-memory entries before the deletion loop already matches real
portage's own observable behavior (the file survives on `ROOT`) without
a wasted disk write real portage's own architecture needs but this
pilot's doesn't.

Verified end to end against two real, `gcc`-compiled packages (`dev-
libs/libpreservetest`/`dev-libs/consumepreservetest` fixtures): the
consumer package independently rebuilds a throwaway same-sonamed copy
of the library purely to link against at its own build time (baking in
a real `DT_NEEDED: libpreservetest.so.1`), while the library package
separately builds and installs the "real" instance -- mirroring how a
real cross-package Gentoo build already has a dependency's shared
library installed on the real system when a consumer links against it.
Merging the library, then the consumer, then unmerging the library
proves the full real chain: `/usr/lib/libpreservetest.so.1` survives on
disk, the library's own vdb entry is still removed like any other
unmerge, and the real `preserved_libs_registry` records the preserved
path under `dev-libs/libpreservetest-1.0`'s own real `category/pn:slot`
key -- confirmed both via a `#[test]` and by directly running the
compiled `portuale` CLI binary through the same merge/merge/unmerge
sequence. Smaller, hand-crafted unit tests separately cover `register_
preserved_libs`'s own unregister-only-on-matching-cpv-and-counter and
unconditional-overwrite-with-paths semantics, and `preserve_libs_on_
unmerge`'s own empty-`CONTENTS` short-circuit.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
BIN=PORTING/rust/target/release/portuale
"$BIN" ebuild PORTING/fixtures/repo/dev-libs/libpreservetest/libpreservetest-1.0.ebuild merge
"$BIN" ebuild PORTING/fixtures/repo/dev-libs/consumepreservetest/consumepreservetest-1.0.ebuild merge
"$BIN" ebuild PORTING/fixtures/repo/dev-libs/libpreservetest/libpreservetest-1.0.ebuild unmerge
ls "${ROOT}"/usr/lib/
# libpreservetest.so.1 -- still there, preserved, even though its own
# package was just unmerged
ls "${ROOT}"/var/db/pkg/dev-libs/
# consumepreservetest-1.0 -- libpreservetest-1.0's own vdb entry is gone,
# same as any other unmerge
cat "${ROOT}"/var/lib/portage/preserved_libs_registry
# {
# 	"dev-libs/libpreservetest:0": [
# 		"dev-libs/libpreservetest-1.0",
# 		"0",
# 		[
# 			"/usr/lib/libpreservetest.so.1"
# 		]
# 	]
# }
```

### preserve-libs: real `NEEDED.ELF.2` pruning on `remove_from_contents`

The last documented preserve-libs gap in this area is closed: real
`vardbapi.removeFromContents()`'s own "Also remove corresponding NEEDED
lines, so that they do no corrupt LinkageMap data for preserve-libs"
step (`vartree.py:1279-1310`) is real now. `remove_from_contents` (real
`merge()`'s own post-copy collision-exclusion step, `unregister_
preserved_libs`'s only caller -- the *unmerge*-time preserve-libs path,
`preserve_libs_on_unmerge`, filters its own package's in-memory
`CONTENTS` directly instead, since the whole vdb entry gets deleted
moments later regardless, see that function's own doc comment) now
tracks real `removed` (whether any `CONTENTS` line was actually
dropped, matching real `if removed:`) and the *surviving* `CONTENTS`
paths. When something was removed and this package's own `NEEDED.ELF.2`
exists at all (real `if new_needed is not None:` -- a package that
never had one is left alone, no file conjured into existence), every
entry whose own `filename` no longer appears among the surviving paths
is dropped; every other entry survives untouched. This pilot's own
`CONTENTS`/`NEEDED.ELF.2` convention already stores both `ROOT`-relative
(no literal `ROOT` prefix baked in), so the comparison needs no
`os.path.join(root, ...)` step the real Python side requires.

New `NeededEntry::to_needed_line` (`needed_elf.rs`) ports real
`NeededEntry.__str__` (`NeededEntry.py:67-87`) -- "format this entry for
writing to a NEEDED.ELF.2 file", the rewrite-side sibling of the
existing `parse`/`parse_file` read side, with two real, intentional
asymmetries from what `scanelf` itself would have written: an empty
`runpaths` serializes as a plain empty string, never the `"  -  "`
sentinel `scanelf` emits; and the 6th (`multilib_category`) field is
*always* written, even when `None` (as `""`), unlike the original file
which may omit it entirely for pre-multilib-category data.

Also fixed in the same slice: this module's own top-of-file "KNOWN,
DOCUMENTED GAPS" comment had drifted stale across the last several
preserve-libs slices, still claiming the registration/detection side
was entirely unattempted and the CONFIG_PROTECT "confmem rejected"
simplification was still open -- both corrected in place to point at
the sections that actually shipped them.

Proven via three new, hand-crafted Rust unit tests in `ebuild_merge.rs`
(a matching entry is pruned while an unrelated one survives; no
`NEEDED.ELF.2` is created for a package that never had one; the file is
left completely untouched, byte for byte, when nothing was actually
removed from `CONTENTS`) plus two round-trip tests for `to_needed_line`
in `needed_elf.rs` itself (parse -> serialize -> parse yields the same
entry; the real rewrite format's own two asymmetries from the `scanelf`
read format). Live-verified against the compiled binary too, reusing
the existing `dev-libs/preservepkg-old`/`dev-libs/preservepkg-new`
collision-exclusion fixture pair with a hand-seeded `NEEDED.ELF.2`
(since neither fixture installs a real ELF binary of its own):

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
BIN=PORTING/rust/target/release/portuale
FIX=PORTING/fixtures/repo/dev-libs

"$BIN" ebuild "$FIX/preservepkg-old/preservepkg-old-1.0.ebuild" merge
VDB="${ROOT}/var/db/pkg/dev-libs/preservepkg-old-1.0"
cat > "${VDB}/NEEDED.ELF.2" <<'EOF'
X86_64;/usr/lib/preservedtest/libfoo.so.1;libfoo.so.1;;
X86_64;/usr/lib/preservedtest/unrelated.so;libunrelated.so;;
EOF
echo "unrelated file" > "${ROOT}/usr/lib/preservedtest/unrelated.so"
echo "obj /usr/lib/preservedtest/unrelated.so abc123 100" >> "${VDB}/CONTENTS"
mkdir -p "${ROOT}/var/lib/portage"
cat > "${ROOT}/var/lib/portage/preserved_libs_registry" <<'EOF'
{
	"dev-libs/preservepkg-old:0": [
		"dev-libs/preservepkg-old-1.0",
		"0",
		[
			"/usr/lib/preservedtest/libfoo.so.1"
		]
	]
}
EOF
FEATURES="collision-protect" "$BIN" ebuild \
    "$FIX/preservepkg-new/preservepkg-new-1.0.ebuild" merge
cat "${VDB}/NEEDED.ELF.2"
# X86_64;/usr/lib/preservedtest/unrelated.so;libunrelated.so;;;
# -- the taken-over libfoo.so.1 entry is gone, the unrelated one survives
```

### `env_update()`/`ldconfig` triggering: a merge regenerates `/etc/profile.env`/`/etc/csh.env`/`/etc/ld.so.conf` and runs real `ldconfig`

The last item on `ebuild_merge.rs`'s own gap list from the "Real merge/
filesystem mutation" section above is real now: real `env_update()`
(`lib/portage/util/env_update.py`). New `portuale/src/env_update.rs`,
wired into `ebuild_merge.rs::run_merge` right after `pkg_postinst` --
matching real `merge()`'s own exact ordering (`lib/portage/dbapi/
vartree.py:5198-5209`), including running unconditionally even when
`postinst` itself failed ("It's stupid to bail out here, so keep going
regardless of phase return code"), gated only on whether the merge
actually installed anything (real `if contents:`).

It parses real `/etc/env.d/*` files (the real numeric-prefix filename
filter, real cumulative `SPACE_SEPARATED`/`COLON_SEPARATED` variable
handling for the two real hardcoded default sets --
`CONFIG_PROTECT`/`CONFIG_PROTECT_MASK`; `PATH`/`LDPATH`/`MANPATH`/etc.)
and regenerates all four real output files: `/etc/ld.so.conf`,
`/etc/profile.env` (bash `export`), `/etc/csh.env` (tcsh `setenv`), and
the real systemd `/etc/environment.d/10-gentoo-env.conf`. It then
really runs `ldconfig` -- specifically the *target `ROOT`'s own*
`<ROOT>/sbin/ldconfig` (real `env_update()`'s own `else` branch when no
`CHOST`/`CBUILD` cross-compile is configured, which this pilot never
is), invoked exactly as real portage does: `ldconfig -X -r <ROOT>`,
`cwd="/"` -- a real, unmodified subprocess, the same "real subprocess,
accepted dependency" pattern already established for `wget` in the
fetch slice.

**v1 scope, confirmed with the user before implementing** (see
`env_update.rs`'s own module doc comment for the full list): the
biggest cut is real `ldconfig`-triggering's own persistent, cross-
process mtime cache (real `portage.mtimedb["ldpath"]`, which lets a
long-lived real portage session skip `ldconfig` on a merge that didn't
touch any lib directory). This pilot's own CLI is a fresh process per
command, so there's no such cache to persist -- every merge is instead
treated as a first run: any candidate lib dir (`LDPATH` entries from
env.d, an existing `usr/lib*`/`lib*` directory, excluding `libexec`)
found on disk *after* the merge triggers `ldconfig`, exactly matching
real portage's own genuine first-run behavior (empty `prev_mtimes`).
The only real divergence is a *repeat* merge into the same `ROOT` that
didn't touch any lib dir, where real portage would skip `ldconfig` and
this pilot re-runs it anyway -- never wrong, just occasionally extra,
cheap, idempotent invocations. Also cut: real `getlibpaths()`'s own
`/etc/ld.so.conf.d/*.conf` `include`-directive parsing (a rare,
admin-configured mechanism no fixture needs); real `EPREFIX`/bfd-linker
`/usr/etc/ld.so.conf` (no `EPREFIX` concept anywhere in this pilot);
env.d-declared extra `SPACE_SEPARATED`/`COLON_SEPARATED` keys (a rare
escape hatch); real `getconfig()`'s own shlex-based parser (env.d files
are parsed with the same simple per-line `KEY="value"` shortcut
`ebuild_merge::parse_slot` already takes for `SLOT`); and always
rewriting `/etc/ld.so.conf` rather than only when its content actually
changed (moot here since it isn't vdb-tracked and this pilot's own
`ldconfig`-triggering decision doesn't depend on that comparison at
all).

Proven via seven pure, offline unit tests in `env_update.rs` itself
(env.d line parsing, the filename filter, candidate-lib-dir detection
including the real `libexec` exclusion, the four generated files, and
`ldconfig` invocation vs. a present-but-non-executable one correctly
staying a no-op) plus two new real, end-to-end tests in
`ebuild_merge.rs` against a new fixture, `dev-libs/envupdatepkg` (which
installs its own `/etc/env.d/50-envupdatetest` and a `/usr/lib/
envupdatetest` directory): one proves the four generated files really
reflect the merge's own just-installed env.d entry; the other seeds a
fake, marker-writing executable at `<ROOT>/sbin/ldconfig` before
merging and proves it's really invoked as a subprocess with the real
`-X -r <ROOT>` arguments -- the same "prove it with a marker file"
style already used for `pkg_preinst`/`pkg_postinst` ordering elsewhere
in this file. (An earlier fixture draft used a `cat > file <<-'EOF'`
heredoc to write the env.d file from `src_install` -- silently produced
an *empty* `${D}` under brush, caught by live-verifying against the
built binary before trusting the Rust test suite alone; switched to
plain `echo`/`>>` redirection, the same style every other fixture in
this pilot already uses, which works correctly.)

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/envupdatepkg/envupdatepkg-1.0.ebuild merge
cat "${ROOT}"/etc/ld.so.conf
# /usr/lib/envupdatetest (plus the autogenerated header)
cat "${ROOT}"/etc/profile.env
# export ENVUPDATETEST_VAR='hello from envupdatetest'
cat "${ROOT}"/etc/csh.env
# setenv ENVUPDATETEST_VAR 'hello from envupdatetest'

# Seed a fake, marker-writing ldconfig into a fresh ROOT to prove the
# real subprocess invocation:
export ROOT="$(mktemp -d)"
mkdir -p "${ROOT}/sbin"
printf '#!/bin/sh\necho "$@" > "$3/ldconfig-was-invoked"\n' > "${ROOT}/sbin/ldconfig"
chmod +x "${ROOT}/sbin/ldconfig"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/envupdatepkg/envupdatepkg-1.0.ebuild merge
cat "${ROOT}"/ldconfig-was-invoked
# -X -r /tmp/tmp.XXXXXXXXXX
```

### `FEATURES=protect-owned`: a merge aborts only when the colliding file has a known owner

`collision-protect`'s own sibling real merge-track feature, and the
last item queued from this pilot's own "next slice" backlog: real
`dblink.merge()`'s own *separate* abort condition alongside
`collision-protect` (`lib/portage/dbapi/vartree.py:4770-4838`). Python
operator precedence makes the real check `collision_protect or
(protect_owned and owners)` -- `collision-protect` aborts on any
ordinary collision unconditionally, but `protect-owned` alone only
aborts when `find_owners` (the same real `vardbapi._owners.
get_owners()`-alike already built for `collision-protect`'s own abort
message) actually identifies an owning package for at least one
collision. Real portage's own "None of the installed packages claim
the file(s)" case -- a stray file already on disk with no owning vdb
entry at all -- does **not** abort under `protect-owned` alone, the
one behavioral difference from `collision-protect` this feature exists
for. `MergeOptions.protect_owned` is read from `FEATURES` at the
`ebuild.rs` CLI boundary, the same env-var-not-full-config-resolution
shortcut `collision_protect` already uses -- **unlike** `collision_protect`,
though: a later slice found real `protect-owned` is actually one of real
`make.globals`'s own default `FEATURES` tokens (`cnf/make.globals:77-84`),
so `MergeOptions::default()`'s own `protect_owned` is now `true`, not
`false` -- this section's own original claim (an implied
`collision_protect`-style default-false) is now stale; see this file's
own "`FEATURES=distlocks`" section below for the fuller writeup of this
discovery and the two other flags it also applied to.

No new machinery needed beyond reusing what `collision-protect`
already built -- `find_owners` is called once more in `run_merge`
itself (alongside `collision_message`'s own, separate call for the
abort text) specifically to decide whether `protect_owned` should abort
at all, matching real portage's own `get_owners()` likewise only
running when `collision_protect or protect_owned or symlink_collisions`
might need it.

Proven via two new real, end-to-end tests reusing the existing
`dev-libs/collisionpkg-a`/`-c` fixtures: `protect-owned` alone aborts
and names `collisionpkg-a` as the owner, identical to `collision-
protect`'s own abort message; a second test places a stray, unowned
file directly on disk (no merge, no vdb entry) at the same destination
path and proves `protect-owned` alone does *not* abort, merging over it
instead -- the fixture-pair reuse plus the stray-file variant is what
actually distinguishes this feature's own behavior from
`collision-protect`'s, not just "also aborts."

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/collisionpkg-a/collisionpkg-a-1.0.ebuild merge
# FEATURES="protect-owned" is explicit here for clarity, but is now the
# real default anyway -- omitting it entirely aborts the same way.
FEATURES="protect-owned" PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/collisionpkg-c/collisionpkg-c-1.0.ebuild merge
# ebuild: This package will overwrite one or more files that may belong to other packages:
# dev-libs/collisionpkg-a-1.0:
#         /usr/share/collisiontest/shared.txt
# Package 'dev-libs/collisionpkg-c-1.0' NOT merged due to file collisions.

# A stray, unowned file at the same path -- no merge, no vdb entry:
export ROOT="$(mktemp -d)"
mkdir -p "${ROOT}/usr/share/collisiontest"
echo "a stray, unowned file" > "${ROOT}/usr/share/collisiontest/shared.txt"
FEATURES="protect-owned" PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/collisionpkg-c/collisionpkg-c-1.0.ebuild merge
cat "${ROOT}"/usr/share/collisiontest/shared.txt
# hello from collisionpkg-c -- no abort, since no owner was ever identified
```

### Real blocker exclusion: `mypkglist = others_in_slot + blockers`

The last item on `ebuild_merge.rs`'s own gap list from the collision-
detection work above is real now: real `dblink.merge()`'s own
`mypkglist = others_in_slot + blockers` (`others_in_slot` is already
this pilot's own `own_versions`). Real `dblink._blockers` is never
computed by `dblink` itself -- it's injected by the real depgraph
resolver, which already knows the full dependency graph by the time a
merge runs. This pilot's own `ebuild <file> merge` has no depgraph at
all (a standalone, single-ebuild real-execution path, unlike `emerge
--pretend`), so closing this gap meant something genuinely new: bringing
real `repos.conf`/profile/USE config resolution into the real-execution
path for the first time ever (previously entirely env-var-driven, no
`portage_repo::find_repos`/`portage_profile::resolve_config` call
anywhere in it). Confirmed with the user before implementing, given the
size -- a narrower "literal blocker-atom text scan, no USE-conditional
evaluation" alternative was on the table and explicitly declined in
favor of doing this correctly.

New `blocked_installed_packages`: resolves the merging package's own
repo (`ebuild_phases::repo_root_for`, already used by the `package`/
binpkg-building slice) and real md5-cache metadata, resolves real
config the same way `pretend.rs` does (including this session's own
`masters =` work), computes the merging package's own effective USE
(`portage_repo::effective_use_flags`, made `pub` for this), flattens
its own real `DEPEND`+`RDEPEND`+`BDEPEND`+`PDEPEND`+`IDEPEND`
(`portage_use_reduce::use_reduce_flat`) against it, and matches every
blocker atom found against every real installed package
(`portage_dep::match_from_list`, which -- real, already-verified
behavior elsewhere in this pilot -- ignores an atom's own blocker
marker when matching, so the atom string is passed through as-is,
`!`/`!!` prefix included). Weak and strong blockers aren't
distinguished, matching real `mypkglist`'s own construction. Degrades
gracefully to an empty blocked set on any resolution failure (missing
`repos.conf`, unreadable md5-cache, etc.) -- a collision that would
have been excluded just gets reported as an ordinary one instead,
never a false negative in the direction that could silently corrupt a
real merge.

A real safety issue surfaced and was fixed before this ever ran against
real fixtures: this pilot's own dev/test machine has a real, populated
`/etc/portage/repos.conf` (a real Gentoo system), so naively reading
`PORTAGE_CONFIGROOT` via an ambient env-var default (real portage's own
default is `/` when unset) would have made every existing test that
never touches this new feature at all silently start reading real host
config the moment it ran on this machine. Fixed by making `config_root`
an explicit new `MergeOptions` field instead -- the same "explicit
parameter, not an ambient env read inside library code" reasoning
`portage_fetch::FetchOptions::gentoo_mirrors` already established, but
load-bearing here for a genuinely different, more serious reason.
`MergeOptions::default()`'s own value is a deliberately impossible path
(`/dev/null/...` -- `/dev/null` is a real character device, never a
directory, so nothing can ever exist under it), guaranteeing
`find_repos` always fails cleanly for every one of this pilot's own
~30 pre-existing merge tests unless a test opts in explicitly; only
`ebuild.rs`'s own real CLI boundary reads the real env var, matching
real portage's own actual default behavior for real usage.

New fixtures `dev-libs/mergeblockerpkg` (`RDEPEND="!dev-libs/
mergeblockedbypkg"`) and `dev-libs/mergeblockedbypkg`, both installing
the same file -- proven via two new tests: with `MergeOptions::
default()`'s own inert `config_root`, the collision is an ordinary
`collision-protect` abort (the fixture pair's own "genuinely collides"
baseline); with `config_root` pointed at the real `PORTING/fixtures`
tree (a real `repos.conf` of its own), the real blocker atom excludes
it and `mergeblockerpkg` takes over the file even with `collision_
protect: true`.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergeblockedbypkg/mergeblockedbypkg-1.0.ebuild merge

# Baseline: config resolution unavailable (a deliberately empty/
# nonexistent PORTAGE_CONFIGROOT) -- an ordinary collision-protect abort
PORTAGE_CONFIGROOT="/tmp/definitely-empty-configroot-$$" \
FEATURES="collision-protect" PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergeblockerpkg/mergeblockerpkg-1.0.ebuild merge
# ebuild: This package will overwrite one or more files that may belong to other packages:
# dev-libs/mergeblockedbypkg-1.0:
#         /usr/share/mergeblockertest/shared.txt
# Package 'dev-libs/mergeblockerpkg-1.0' NOT merged due to file collisions.  (exit 1)

# With real config resolution: mergeblockerpkg's own real RDEPEND
# blocks mergeblockedbypkg, so the collision is excluded
PORTAGE_CONFIGROOT="$(realpath PORTING/fixtures)" \
FEATURES="collision-protect" PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergeblockerpkg/mergeblockerpkg-1.0.ebuild merge
cat "${ROOT}"/usr/share/mergeblockertest/shared.txt
# hello from mergeblockerpkg -- no abort, the blocked package's file was simply taken over
```

### `ebuild_merge.rs`/`ebuild_unmerge.rs`: real FIFO/device node `CONTENTS` support (`fif`/`dev`)

`merge_tree`'s own last documented gap ("real `mergeme()` handles these
too, but no fixture this pilot has needs them") is closed: real
`mergeme()`'s own `else:` branch ("we are merging a fifo or device
node", `vartree.py:5787-5811`) is real now. Neither node type is ever
`_protect()`'d (that branch doesn't call it at all, unlike `obj`/`sym`),
and a node is only actually created when the live destination doesn't
already exist yet (real `if mydmode is None:` -- an existing node at
that path is left completely alone). The real `fif`/`dev` `CONTENTS`
line is written unconditionally either way, with no digest/mtime/target
field at all (real `_format_contents_line(node_type=..., abs_path=
myrealdest)` only).

Real `movefile()` has no dedicated fifo/device-node logic of its own --
an ordinary same-filesystem `os.rename()` just works for a special file
too, since `rename(2)` doesn't care what type of file it's moving (real
`movefile()`'s own "we don't yet handle special, so we need to fall
back to /bin/mv" comment only fires on a genuine cross-device `EXDEV`
failure). This pilot's own merge step never moves `${D}` content at all
though -- every other branch copies/recreates instead, so `${D}` itself
stays intact -- so new `create_special_node` recreates a fresh node at
the destination via real `mkfifo(3)`/`mknod(3)` instead, matching the
source's own real type, permission bits, and (for a device node) real
major/minor (`st_rdev`); an explicit `chmod` afterward closes the one
real gap `mkfifo(3)`/`mknod(3)` themselves have that `std::fs::copy`
doesn't (both apply the process's own umask to the given mode, unlike
`std::fs::copy`'s automatic exact permission-bit preservation for a
regular file).

Real `mknod(2)` genuinely requires root/`CAP_MKNOD` for a real (nonzero
major:minor) character or block device on a real Linux system -- an
unrelated real kernel carve-out exists for `mknod(path, S_IFCHR, 0)`
specifically (`dev_t == 0`, the overlayfs "whiteout" convention, never a
usable real device), confirmed empirically while building this slice's
own tests (a naive first attempt at a permission-failure test happened
to trip this exact carve-out by using an arbitrary regular file, whose
own `rdev` defaults to `0`, as the stand-in device source -- silently
succeeding instead of demonstrating the real permission wall it was
meant to prove). `create_special_node` surfaces a real permission
failure as an ordinary `Result::Err`, not a panic.

The unmerge side needed no functional change at all: real
`_unmerge_pkgfiles()`'s own `"fif"`/`"dev"` branches
(`vartree.py:3062-3068`) never call `unlink()` in the first place --
both just report a real `"---"` status, portage's own conservative
"leave a device/fifo node in place" behavior, unlike `obj`/`sym`/`dir`.
`ebuild_unmerge.rs`'s own catch-all for these two node types already
happened to do nothing, matching real behavior by coincidence rather
than by design -- its own doc comment (previously citing the wrong
reason, "merge_tree doesn't create these either") is corrected in place
to cite the real one.

Verified end to end with a new `dev-libs/fifopkg` fixture (a real FIFO,
created via real `mkfifo(1)` in `src_install` -- unlike a device node,
this needs no special privilege at all, so it's the one of the two real
node types this pilot can actually exercise live): merging creates a
real FIFO and records a real `fif` line; re-merging over something else
already planted at that same path leaves it completely untouched;
unmerging leaves the FIFO in place while still removing the vdb entry
as normal. Device-node creation itself is verified only via a narrower,
hand-crafted unit test proving `create_special_node` fails cleanly
without root (using real `/dev/null` as `src`, so its own real, nonzero
`rdev` genuinely exercises the real privileged path rather than the
`dev_t == 0` carve-out above) -- not reproducible as a real, live
end-to-end merge in this unprivileged dev/test environment.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
BIN=PORTING/rust/target/release/portuale
PKG=PORTING/fixtures/repo/dev-libs/fifopkg/fifopkg-1.0.ebuild

"$BIN" ebuild "$PKG" merge
ls -la "${ROOT}"/usr/lib/fifopkg/
# prw-r--r-- ... myfifo  -- a real FIFO, not a regular file
cat "${ROOT}"/var/db/pkg/dev-libs/fifopkg-1.0/CONTENTS
# fif /usr/lib/fifopkg/myfifo  -- no digest/mtime field at all

"$BIN" ebuild "$PKG" unmerge
ls -la "${ROOT}"/usr/lib/fifopkg/
# the FIFO is still there -- real portage never unlinks a fif/dev entry
```

### `emerge --pretend --root-deps`: real `ESYSROOT`-vs-`ROOT` dependency resolution

The last genuinely open item in the dry-run backlog's own "Open backlog"
section (`PROMPT-next.md`) is real now, at a deliberately narrowed v1
scope confirmed with the user before implementing (see below). Real
`depgraph.py`'s own `depend_root` selection (~`depgraph.py:4209-4225`)
resolves `DEPEND` (and, for a `BDEPEND`-capable EAPI -- this pilot's own
5+ floor always is -- also the root `BDEPEND` folds into) against
`ESYSROOT`, not the target `ROOT` -- and real `ESYSROOT`'s own default
(`LocationsManager.py:406-411`) is the **real build machine's own `/`**
whenever `SYSROOT` is left unset, which is true for every single fixture
test this pilot has ever run (`ROOT` always a `mktemp -d`, `SYSROOT`
never set). Porting this literally would mean every existing `--deep`/
`--with-bdeps` test's own `DEPEND`/`BDEPEND` resolution would start
silently consulting whichever real host machine happens to run the test
suite -- fundamentally in tension with this pilot's own hard goal of a
deterministic shared pytest contract suite (`PROMPT.md`'s own "Rust must
be measurably faster... shared pytest contract suite as executable
behavioral spec" framing implicitly assumes deterministic tests to
measure and compare against in the first place).

Resolved by scoping this as new, **additive, opt-in-only** machinery
that changes nothing about any pre-existing call site or test: real
`--root-deps` (`emerge --pretend --root-deps`) computes a real running
root's own satisfiability for `DEPEND`/`BDEPEND` atoms and drops any
that are already satisfied there from the queue entirely (real "no
separate graph node needed for an already-satisfied dep") -- new
`portage_repo::running_root_satisfies_atom` (a plain vdb existence
check, `installed_candidates` + `match_from_list`, deliberately generic
on which root it's pointed at -- exactly like `installed_versions`
elsewhere in this crate) and `root_deps_satisfied_atoms` (flattens the
package's own `DEPEND`+`BDEPEND` alone, using the exact same
`atom_currently_satisfiable` closure the caller's own combined flatten
already used, so branch choices are always identical), threaded through
both real dep-walk sites (`enqueue_dependencies`'s `AlreadyInstalled`
path, and the main New/Upgrade/Reinstall flatten) via a new
`root_deps_running_root: Option<&Path>` parameter on
`resolve_pretend_graph` -- `None` for every one of this pilot's own
~30 pre-existing call sites/tests (a strict no-op, verified by the full
`cargo test` suite passing unchanged before and after). Only
`pretend.rs`'s own real CLI boundary ever resolves this to real `/` by
default (`running_root_from_env`, matching real portage's own actual
default), gated on the new `--root-deps` flag (bare, `=True`, or
`=rdeps` all just enable this pilot's one real behavior -- the "fold
DEPEND into RDEPEND" vs. "ignore DEPEND for non-BDEPEND-EAPI packages"
distinction real portage's own two explicit values carry isn't
observable in this pilot's own single-root graph architecture anyway,
so it's deliberately not reproduced). A new, pilot-specific
`PORTAGE_RUNNING_ROOT` environment variable (real portage has no
equivalent override at all) lets a test point this at a fixture's own
fake vdb tree instead, the same "explicit override for tests, real
default at the CLI boundary" pattern `MergeOptions::config_root`
already established.

Running-root satisfiability now feeds into the disjunctive (`||`)
branch-selection closure too: a `DEPEND`/`BDEPEND` `||` group with no
branch visible in the fixture tree resolves correctly when some branch
*is* running-root-satisfied, in both real dep-walk sites (the main
New/Upgrade/Reinstall flatten and `enqueue_dependencies`'s own
`--deep`/`AlreadyInstalled` recursion). Since this pilot's own single-
unified-graph architecture merges all five dep keys into one combined
string before ever flattening at all, the running-root-aware closure
can't tell which key a given atom came from, so an `RDEPEND`/`PDEPEND`/
`IDEPEND` `||` group gets the same permissive check too -- harmless in
practice (those atoms almost always resolve via ordinary tree
visibility already; a running-root coincidence only ever widens
acceptance, never narrows it). New fixture `dev-libs/rootdepsorpkg`
(`BDEPEND="|| ( dev-libs/rootdepsnonexistent dev-libs/rootdepsprovider
)"`, neither branch with an ebuild in the fixture tree) proves it, in
both Rust and the Python reference mirror, plus a new dedicated pytest
contract test.

~~**KNOWN, DOCUMENTED SCOPE CUT (still open)**: real portage's fuller
behavior of recursively pulling in and building a *new* package against
the running root when it's *not* already there is still not
attempted.~~ Partially shipped 2026-08-26 -- see "`emerge --pretend
--root-deps`: recursively building a new package against the running
root" below for the real, deliberately non-recursive first increment,
and that section's own doc comment for exactly what's still left.

New fixture `dev-libs/rootdepspkg` (`BDEPEND="dev-libs/rootdepsprovider"`,
no ebuild for `rootdepsprovider` anywhere in the fixture repo tree at
all) plus a hand-seeded vdb-only entry (`rootdepsprovider-1.0`, no
ebuild, just `SLOT`/`CATEGORY` files) under `PORTING/fixtures/var/db/pkg`
itself -- reused directly as the running root in tests, since ordinary
dependency resolution never consults a root's own vdb at all, only the
ebuild repo tree, so this is a valid, real proof the new running-root
check (not some other pre-existing mechanism) is what excludes it.
Proven via Rust unit tests (`running_root_satisfies_atom` directly, plus
two end-to-end `resolve_pretend_graph` tests with/without
`root_deps_running_root`), mirrored exactly in
`emerge_pretend_reference.py` (`_running_root_satisfies_atom`,
`_root_deps_satisfied_atoms`, the same threading through both dep-walk
sites), and a new dedicated pytest contract test
(`test_root_deps_matches_between_implementations`) asserting byte-for-
byte Rust/Python parity in both the without-`--root-deps` (a reported,
non-fatal `NoVisibleCandidate` dependency entry) and with-`--root-deps`
(no such entry) cases.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"

# Without --root-deps: rootdepsprovider has no ebuild anywhere in the
# fixture repo tree, so it's reported as an unresolvable dependency
# (not fatal -- it's a dependency, not the top-level atom).
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTING/rust/target/release/portuale \
    emerge --pretend dev-libs/rootdepspkg
# [ebuild  N] dev-libs/rootdepspkg-1.0
# !!! no visible ebuild for dependency "dev-libs/rootdepsprovider"

# With --root-deps, pointed (via the pilot-specific PORTAGE_RUNNING_ROOT
# override) at a running root where rootdepsprovider genuinely is
# installed: no more unresolved-dependency report.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rootdepspkg
# [ebuild  N] dev-libs/rootdepspkg-1.0

# Disjunctive branch-selection feed-in: rootdepsorpkg's own BDEPEND is
# "|| ( rootdepsnonexistent rootdepsprovider )" -- neither branch has an
# ebuild anywhere in the fixture repo tree, so without --root-deps
# *both* branches are reported (this pilot's own pre-existing "leave an
# unresolved || group's branches all in flat_deps" fallback).
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTING/rust/target/release/portuale \
    emerge --pretend dev-libs/rootdepsorpkg
# [ebuild  N] dev-libs/rootdepsorpkg-1.0
# !!! no visible ebuild for dependency "dev-libs/rootdepsnonexistent"
# !!! no visible ebuild for dependency "dev-libs/rootdepsprovider"

# With --root-deps: the closure now selects the running-root-satisfied
# branch specifically, so neither is reported at all.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rootdepsorpkg
# [ebuild  N] dev-libs/rootdepsorpkg-1.0
```

### `emerge --pretend --root-deps`: recursively building a new package against the running root

The scope check paid off before any code was written: re-reading real
`depgraph.py:4207-4271` directly (not just the summary the previous
slice's own "still open" note carried) surfaced that the real gap is
bigger than that note suggested -- real `BDEPEND` *always* targets the
running root, completely independent of whether `--root-deps` is even
passed, and real `DEPEND`'s own target root is EAPI-conditional
(`ESYSROOT` for a `BDEPEND`-capable EAPI, the running root otherwise).
Confirmed with the user directly before implementing anything: build
toward the real, full multi-root shape, one confirmed increment at a
time -- the same "narrow first" rhythm the five-part preserve-libs
registration buildout already used successfully, not a single giant
change to a ~10,600-line file.

This first increment: real `DEPEND`/`BDEPEND` atoms that
`unsatisfied_root_deps_atoms` (new, the complement of the already-
shipped `root_deps_satisfied_atoms`) reports as *not* satisfied by the
running root are now resolved there directly -- reusing `resolve_pretend`
wholesale, just pointed at the running root instead of the target `ROOT`
(`resolve_root_deps_build_entry`, new). A genuine `New`/`Upgrade`/
`Reinstall`/`Downgrade` outcome becomes its own real `GraphEntry`, a new
`targets_running_root: bool` field distinguishing it from every ordinary
`ROOT`-targeted entry (`false` for all ~30 pre-existing call sites/
tests, unchanged). Deduplicated separately from the main graph's own
`resolved_slots`/`other_outcomes` (a `(category, package)` set of its
own) -- a package can validly need building into *both* roots at once
(an ordinary `RDEPEND` into `ROOT`, some other package's own `BDEPEND`
into the running root), which must never collide into one shared dedup
key. Wired into both real dep-walk sites, same as every other
`--root-deps` mechanism in this area: the main New/Upgrade/Reinstall
loop, and `enqueue_dependencies`'s own `--deep`/`AlreadyInstalled`
recursion.

A real, non-obvious bug surfaced and got fixed in the same slice: an
unsatisfied `DEPEND`/`BDEPEND` atom used to simply fall through into the
ordinary `flat_deps` queue (silently resolved against `ROOT` instead,
since nothing previously excluded it) -- invisible in every existing
fixture, since `rootdepsprovider`/`rootdepsnonexistent` (the pre-existing
`rootdepspkg`/`rootdepsorpkg` fixtures) were both deliberately
tree-invisible, so they never reached that fallback with a resolvable
candidate at all. The new `dev-libs/rootdepsbuildpkg` fixture (a real,
tree-visible `BDEPEND` target, `dev-libs/rootdepsbuildtool`) exposed it
immediately: the atom was resolving *twice*, once as the new
`targets_running_root` entry and once more via the old `ROOT`-targeted
fallback. Fixed by excluding the full `unsatisfied_root_deps_atoms` set
from `flat_deps` too, not just `root_deps_satisfied_atoms`'s own already-
satisfied subset -- consistent with this pilot's own established
`--root-deps` simplification (real `DEPEND`/`BDEPEND` never targets
`ROOT`/`ESYSROOT` at all under it, matching `root_deps_satisfied_atoms`'s
own pre-existing DEPEND-and-BDEPEND-treated-uniformly precedent).

**Deliberately not recursive, confirmed with the user as this slice's
own scope boundary**: the new entry's *own* further dependencies aren't
walked. A faithful, fully recursive version means either threading a
genuinely separate, root-aware queue through the entire existing single-
root BFS (the real architectural work `PROMPT-next.md`'s own backlog
already flagged as bigger and riskier than a typical slice), or
recursively invoking `resolve_pretend_graph` itself per atom -- which
introduces a real cycle-safety hazard this slice deliberately doesn't
take on: two packages whose own `BDEPEND`s point at each other (an
unremarkable pattern for bootstrap-style build tools), neither yet
satisfied by the running root, would recurse with no cross-call memory
of "already resolving this atom," right up to a real stack overflow --
solvable, but only with its own careful, separately-scoped design and
testing. Left for a follow-up slice, the same "narrow first, recurse
later" shape preserve-libs registration already used across five slices
before its own control-flow wiring landed.

Verified with two new Rust unit tests in `portage-repo` (a real build
entry appears, `targets_running_root: true`, `required_by` naming the
requesting package -- and the fix above, proven by asserting the atom
appears *exactly once* in the resolved graph, not twice), mirrored
exactly in `emerge_pretend_reference.py` (`_unsatisfied_root_deps_atoms`,
`_resolve_root_deps_build_entry`, the same 13th tuple field and the same
double-exclusion fix threaded through both dep-walk sites -- including
three more exhaustive-tuple-unpack sites the extra field's own end-to-end
plumbing touched: the `required_by`-merge post-pass, `resolve_blockers`,
and `--json` serialization), and a new dedicated pytest contract test
(`test_root_deps_recursive_build_entry_matches_between_implementations`)
proving byte-for-byte Rust/Python parity for the new fixture in both
`--root-deps` modes.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rootdepsbuildpkg
# [ebuild  N] dev-libs/rootdepsbuildpkg-1.0
# [ebuild  N] dev-libs/rootdepsbuildtool-1.0 to /home/.../PORTING/fixtures
# -- rootdepsbuildtool is a real, separate graph entry (targets_running_
# root: true internally); its own " to <running root>" marker was added
# in the follow-up slice below.
```

### `emerge --pretend --root-deps`: the running-root build entry is marked in `--pretend`/`--json`/`--tree` output

The immediately-preceding slice's own last remaining follow-up: a
`targets_running_root` build entry was resolved and displayed
identically to any ordinary `ROOT`-targeted entry, so nothing in the
output told you it installs somewhere else. Real portage annotates
exactly this -- `lib/_emerge/resolver/output.py:841-862` appends
`darkgreen("to " + pkg.root)` to any entry whose own
`pkg.root_config.settings["ROOT"] != "/"`.

New `root_suffix` (`pretend.rs`, mirrored in `emerge_pretend_reference.
py`) ports that suffix, deliberately narrower than real portage's own
gate: this pilot annotates *only* the running-root build entries, never
every entry merged under a non-`/` `ROOT`. Porting the real gate
literally would make every one of this pilot's ~30 fixture tests emit
its own non-deterministic `mktemp -d` `ROOT` path on every line,
breaking the shared contract suite's determinism -- the same tension
the parent `--root-deps` slice resolved by scoping its behavior as
strictly opt-in machinery. The marker text is the running root exactly
as resolved (`running_root_from_env`): `to /` at the real CLI default
(matching real portage's own common case), or whatever
`PORTAGE_RUNNING_ROOT` a test points it at. `--json` grows a matching
`"builds_against_running_root"` field (the running-root path string for
such an entry, `null` for every ordinary one -- same `null`-not-absent
shape as the existing `"slot"` field); `--tree` mode carries the marker
through the indent unchanged.

Implementing this surfaced a real, pre-existing Rust/Python divergence
from the parent slice, invisible until now because only flat plain-text
output (which never renders `required_by`) was contract-tested for the
`rootdepsbuildpkg` fixture: the Python reference's own `required_by`
post-pass unconditionally *replaced* every entry's `required_by` with
`sorted(required_by_map.get(key, ()))`, wiping the `[owner]` that
`_resolve_root_deps_build_entry` sets at construction (the build entry
is added outside the normal flat-deps queue, so `required_by_map` has
no key for it). The Rust side's own post-pass already guards this
correctly (`if let Some(owners) = required_by_map.remove(...)` -- only
touches entries the map actually has). Fixed on the Python side to
match, which is what makes the new `--json`/`--tree` parity assertions
pass (a build entry now correctly lists its requesting package as an
owner on both sides, and `--tree` nests it under that package).

Verified with a new dedicated contract test
(`test_root_deps_build_entry_output_marks_the_running_root`, pinned to
`PORTAGE_RUNNING_ROOT=/` for a deterministic `to /`) asserting
byte-for-byte Rust/Python parity across plain, `--json`, and `--tree`
output, plus an update to
`test_root_deps_recursive_build_entry_matches_between_implementations`
(its own "both print an identical plain-text line" claim is now stale --
the `--root-deps` case is visually distinguished from the fallback).

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"

# Default running root (/): real portage's own common case, "to /".
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rootdepsbuildpkg
# [ebuild  N] dev-libs/rootdepsbuildpkg-1.0
# [ebuild  N] dev-libs/rootdepsbuildtool-1.0 to /

# --json: a "builds_against_running_root" field, null for the ordinary entry.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps --json \
    dev-libs/rootdepsbuildpkg
# ...{"package":"rootdepsbuildpkg",...,"builds_against_running_root":null,...}
# ...{"package":"rootdepsbuildtool",...,"builds_against_running_root":"/",...}

# --tree: the marker survives the indent.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps --tree \
    dev-libs/rootdepsbuildpkg
# [ebuild  N] dev-libs/rootdepsbuildpkg-1.0
# [ebuild  N]   dev-libs/rootdepsbuildtool-1.0 to /
```

### `emerge --pretend --root-deps`: the running-root build walk is recursive

The last open piece of `--root-deps`'s own build-entry story. The first
increment resolved a single unsatisfied `DEPEND`/`BDEPEND` atom against
the running root and stopped -- that entry's *own* dependencies were
never walked, a deliberate cut because a faithful recursion has a real
cycle-safety hazard (two bootstrap build tools `BDEPEND`ing each other
would recurse forever). This slice takes it on.

`resolve_root_deps_build_entries` (new, replacing the non-recursive
`resolve_root_deps_build_entry`) resolves the atom against the running
root exactly as before, and then walks the resolved package's *own*
`DEPEND` + `BDEPEND` + **`RDEPEND`** against the running root too,
recursively. `RDEPEND` is the deliberately-broader half, confirmed with
the user: real `depgraph.py:4207-4271`'s own `_add_pkg_deps` `deps`
tuple resolves all three of those keys against `pkg.root`, and a package
pulled in as a build tool has `pkg.root == running_root` -- so its
runtime deps must be present *there* as well, not under the target
`ROOT`. `unsatisfied_root_deps_atoms` grew a `dep_keys` parameter
(`["DEPEND", "BDEPEND"]` at the two ordinary dep-walk sites, `["DEPEND",
"BDEPEND", "RDEPEND"]` for the recursion); a new
`resolved_version_meta_and_use` re-looks-up the resolved candidate's own
md5-cache + effective USE (the same `list_candidates` ->
highest-`repo_priority` -> `read_md5_cache` pattern `slot_changed`/
`deps_changed` already use), so each recursed package's conditional deps
flatten against *its own* USE, not its requester's.

**Cycle safety**: the already-existing `root_deps_build_seen` set (a
`(category, package)` set threaded through the whole graph resolution) is
now both the cross-package dedup key *and* the cycle guard -- a package
is inserted *before* its own dependencies are walked, so a mutual
`BDEPEND` (`rdrcyca` <-> `rdrcycb`) terminates cleanly with each node
appearing exactly once. One `required_by` edge is lost at whichever
point a cycle is cut (real portage's own bidirectional digraph keeps
both); a bounded, documented imprecision, the same best-effort
`required_by` tracking already has elsewhere. Each entry's `required_by`
now names its *immediate* requester, not the original top-level atom, so
`--tree` nests the recursion correctly.

**Unbuildable build deps are now reported** (confirmed with the user):
before this slice, a `--root-deps` build dependency that was neither
installed on the running root nor buildable from the tree was silently
swallowed. Now `resolve_root_deps_build_entries` produces a real
`NoVisibleCandidate` entry for it (`targets_running_root: true`), so the
renderer emits its own non-fatal `!!! no visible ebuild for dependency`
note exactly as it would without `--root-deps` -- closing a real
inconsistency where `--root-deps` used to *hide* an unresolvable build
dep.

New fixtures under `dev-libs/rdr*`: `rdrapp` -> `rdrtool` ->
(`rdrtooldep` via `BDEPEND`, `rdrlib` via `RDEPEND`); a mutual-`BDEPEND`
cycle (`rdrcyc` -> `rdrcyca` <-> `rdrcycb`); and `rdrmiss` -> a build
tool whose own `BDEPEND` (`rdrnothere`) has no ebuild anywhere. Verified
with three Rust unit tests in `portage-repo` plus three dedicated pytest
contract tests asserting byte-for-byte Rust/Python parity for all three
scenarios across plain, `--json`, and `--tree` output, mirrored in
`emerge_pretend_reference.py` (`_resolve_root_deps_build_entries`,
`_resolved_version_meta_and_use`, the `dep_keys` parameter, the
`root_deps_unsatisfied` list-not-set determinism fix threaded through
both dep-walk sites).

Still open, a separately-scoped follow-up: `IDEPEND` of a running-root
build entry (real portage resolves `IDEPEND` against the running root
too; `PDEPEND` correctly stays a target-`ROOT` concern and is not
walked here), and the full multi-root graph architecture this pilot
still approximates edge by edge rather than carrying a `root` per
dependency.

Running it:

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"

# Recursion through BDEPEND (rdrtooldep) and RDEPEND (rdrlib):
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rdrapp
# [ebuild  N] dev-libs/rdrapp-1.0
# [ebuild  N] dev-libs/rdrtool-1.0 to /
# [ebuild  N] dev-libs/rdrtooldep-1.0 to /
# [ebuild  N] dev-libs/rdrlib-1.0 to /

# --tree nests each entry under its immediate requester:
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps --tree \
    dev-libs/rdrapp
# [ebuild  N] dev-libs/rdrapp-1.0
# [ebuild  N]   dev-libs/rdrtool-1.0 to /
# [ebuild  N]     dev-libs/rdrlib-1.0 to /
# [ebuild  N]     dev-libs/rdrtooldep-1.0 to /

# A mutual BDEPEND cycle terminates, each node once:
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rdrcyc
# [ebuild  N] dev-libs/rdrcyc-1.0
# [ebuild  N] dev-libs/rdrcyca-1.0 to /
# [ebuild  N] dev-libs/rdrcycb-1.0 to /

# An unbuildable build dep is surfaced, not swallowed (exit 0 -- it's a dep):
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rdrmiss
# [ebuild  N] dev-libs/rdrmiss-1.0
# [ebuild  N] dev-libs/rdrmisstool-1.0 to /
# !!! no visible ebuild for dependency "dev-libs/rdrnothere"
```

### `emerge --pretend --root-deps`: `IDEPEND` of a running-root build entry

A one-key follow-up to the recursion slice above. Real
`depgraph.py:4247-4252`'s own `deps` tuple resolves `edepend["IDEPEND"]`
against `self._frozen_config._running_root.root` **always** -- not
EAPI-conditional like `DEPEND`, not `--root-deps`-gated. So a package
pulled in as a running-root build entry has its own `IDEPEND` (the
install-time helpers its `pkg_preinst`/`pkg_postinst` need) resolved
against the running root too. `resolve_root_deps_build_entries`'s
recursive dep-key set went from `DEPEND` + `BDEPEND` + `RDEPEND` to
`+ IDEPEND` (one tuple entry, `unsatisfied_root_deps_atoms`'s existing
`dep_keys` parameter). `PDEPEND` stays out -- real portage keeps it a
target-`ROOT` concern.

New `dev-libs/rdri*` fixtures (`rdriapp` -> `rdritool`, whose own
`IDEPEND` is `rdrilib`). One Rust unit test + one dedicated pytest
contract test, mirrored in `emerge_pretend_reference.py`.

Follow-up, now landed (see the next section): a *top-level* package's
own `IDEPEND` also resolves against the running root under `--root-deps`.

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/rdriapp
# [ebuild  N] dev-libs/rdriapp-1.0
# [ebuild  N] dev-libs/rdritool-1.0 to /
# [ebuild  N] dev-libs/rdrilib-1.0 to /   -- rdritool's own IDEPEND
```

### `emerge --pretend --root-deps`: a top-level package's own `IDEPEND` vs the running root

The preceding slice walked `IDEPEND` against the running root only for
packages *recursed into* as running-root build entries. But real
`depgraph.py:4247-4252` resolves `edepend["IDEPEND"]` against
`self._frozen_config._running_root.root` for **every** package it adds,
top-level requests included -- `IDEPEND` is install-time tooling and is
never a target-`ROOT` concern. This slice closes that last gap.

`root_deps_satisfied_atoms` gained the same `dep_keys` parameter its
complement `unsatisfied_root_deps_atoms` already had, and both ordinary
dep-walk sites (the main New/Upgrade/Reinstall flatten and
`enqueue_dependencies`'s own `--deep`/`AlreadyInstalled` recursion) now
pass `["DEPEND", "BDEPEND", "IDEPEND"]` to *both* functions -- the
satisfied and unsatisfied subsets must stay in lockstep, since an atom
in neither would fall through to the ordinary `flat_deps` queue and be
wrongly resolved against `ROOT`. `DEPEND`/`BDEPEND` keep the pilot's
established `--root-deps`-gated simplification (real portage's
`ESYSROOT`-vs-`ROOT` split); `IDEPEND` rides the same gate here, so in
the pilot a top-level `IDEPEND` still only reaches the running root when
`--root-deps` is passed -- real portage does it unconditionally, a
documented residual of this pilot's opt-in `root_deps_running_root`
plumbing rather than a per-dependency `root`.

New `dev-libs/topidepapp` fixture (`IDEPEND` on `dev-libs/topideplib`,
no other deps). One Rust unit test asserting `topideplib` flips from an
ordinary entry to `targets_running_root: true` exactly when the running
root is supplied, plus one dedicated pytest contract test proving
byte-for-byte Rust/Python parity with and without `--root-deps`,
mirrored in `emerge_pretend_reference.py` (`_root_deps_satisfied_atoms`'s
`dep_keys` parameter, both call sites).

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"

# Without --root-deps: topideplib is an ordinary ROOT-targeted entry.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend \
    dev-libs/topidepapp
# [ebuild  N] dev-libs/topidepapp-1.0
# [ebuild  N] dev-libs/topideplib-1.0

# With --root-deps: the top-level package's own IDEPEND goes to the running root.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="/" \
    PORTING/rust/target/release/portuale emerge --pretend --root-deps \
    dev-libs/topidepapp
# [ebuild  N] dev-libs/topidepapp-1.0
# [ebuild  N] dev-libs/topideplib-1.0 to /
```

### Real `ebuild <file> qmerge`

`qmerge` is now real too, real `doebuild()`'s own `mydo == "qmerge"`
branch (`lib/portage/package/ebuild/doebuild.py:1562-1591`): skips the
`install` phase entirely and goes straight to `merge()`'s own body,
assuming a prior real `install` (or `merge`, which itself runs `install`
first) already populated `${D}`. Real portage gates this on a real,
on-disk marker, `${PORTAGE_BUILDDIR}/.installed` -- and real, unmodified
`bin/phase-functions.sh`'s own `__dyn_install` already creates that
marker unconditionally on a successful `src_install`
(`phase-functions.sh:653`), so this pilot needed to write no new
marker-writing code at all: a real `ebuild <file> install` run via this
pilot's own binary already leaves `.installed` behind as a natural
byproduct of real phase execution, confirmed empirically. Missing the
marker is real portage's own ordinary "forgot a step" mistake, not a
crash: `writemsg(...); return 1`, ported here as the exact same message
text. Implemented as a refactor, not new merge logic: `ebuild_merge.rs`'s
own `run_merge` (the `install`-then-merge path) and the new `run_qmerge`
(skip straight to merge) now both call a shared `merge_after_install`
helper -- real `merge()`'s own body, collision detection through
`pkg_postinst`/`env_update()`, unchanged from what `run_merge` already
did.

```sh
cd PORTING/rust && cargo build --release && cd ../..
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"

# Without a prior install: real doebuild()'s own ordinary "forgot a
# step" message, exit 1 -- not a crash.
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild qmerge
# ebuild: mydo=qmerge, but the install phase has not been run

# install, then qmerge -- no install phase re-run, straight to merge().
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild install qmerge
cat "${ROOT}"/usr/share/mergepkg/hello.txt
# hello from mergepkg
ls "${ROOT}"/var/db/pkg/dev-libs/mergepkg-1.0/
# CATEGORY  CONTENTS  COUNTER  SLOT  repository
```

### `unmerge`'s own "symlink orphan" refinement (bug #326685)

Real `_unmerge_pkgfiles()`'s own bug-#326685 handling
(`vartree.py:2895-2926` + `_unmerge_dirs()`, `:3209-3332`) is real now,
on top of the `others_in_slot`/`"replaced"` skip (see "What this proves"
above). When a live symlink-to-directory this package's own `CONTENTS`
recorded as `sym` (or `dir`) is `is_owned` by another still-installed
same-slot instance that itself now records that *exact path* as a
literal `dir` entry (the directory it pointed to got "promoted" to a
real directory across an upgrade), the ordinary `is_owned` skip already
leaves the symlink itself untouched -- but real portage goes further: it
defers a decision on the symlink's own *target* directory to a second
pass over this package's own literal `dir` entries. If that target
directory is itself one of them, and actually gets removed during this
same unmerge (nothing else needs it as a real directory either), the
now-truly-orphaned symlink is deleted too -- and its own freshly-emptied
parent directory, which could only have failed to `rmdir` earlier
because the symlink was still occupying it, gets a recursive revisit
(real bug #640058) and is removed as well. `remove_dirs`
(`ebuild_unmerge.rs`) ports this as a real LIFO-stack second pass
(`remove_contents` defers this package's own `dir` entries into it
instead of removing them inline), verified for both directions:
`remove_contents_leaves_an_orphaned_symlink_alone_while_its_target_is_still_needed`
(the target directory is never part of this package's own removal at
all -- symlink and target both survive) and
`remove_contents_deletes_an_orphaned_symlink_once_its_target_directory_empties_and_revisits_the_freed_parent`
(the target directory *is* removed here, so the symlink is deleted too,
and its own now-empty parent is revisited and removed -- exercising bug
#640058's own recursive-parent-revisit end to end).

A real, confirmed finding surfaced while tracing this: real
`_unmerge_protected_symlinks()` (`vartree.py:3114-3207`, real portage's
own separate function for whatever `protected_symlinks` entries *don't*
get resolved by `_unmerge_dirs()`) is **not** ported here, deliberately.
Its own first loop re-checks the exact same `others_in_slot`/`isowner`
condition that was already required to populate `protected_symlinks` in
the first place -- since that fact can't change between the two passes
within one real `unmerge()` call, its own early `return` fires
unconditionally, making the real system-wide `get_owners()`-gated
delete-or-elog-warn logic after it genuinely unreachable dead code in
current portage. Confirmed by tracing the exact call graph directly, not
a simplification -- there's no real behavior there to be unfaithful to.
The real elog warning text for symlinks that do survive
(`vartree.py:3085-3103`) also isn't reproduced: this module has no
message-printing output anywhere else either, only the behavioral effect
(the symlink is left in place).

### `unmerge`: real `FEATURES=unmerge-orphans`

Real `_unmerge_pkgfiles()`'s own `unmerge_orphans` handling
(`vartree.py:2934-2950`) is real now too. Despite the name, this isn't
untracked-orphan scanning -- for a non-`CONFIG_PROTECT`'d `obj`/`sym`
entry (excluding a symlink whose live target itself resolves to a
directory, real comment: "Don't unlink symlinks to directories here
since that can remove /lib and /usr/lib symlinks"), it bypasses the
ordinary `!mtime` staleness check entirely and deletes the entry
unconditionally, even if locally modified. `UnmergeOptions` (new: this
pilot's own `run_unmerge` had only two loose parameters,
`debug`/`shell`, before this slice -- now five, so it gets the same
struct treatment `MergeOptions` already established) reuses
`ebuild_merge::is_protected` (promoted to `pub(crate)` for this) for
the real `ConfigProtect.isprotected()` check, and mirrors
`MergeOptions`'s own `config_protect`/`config_protect_mask` fields and
env-var-sourced CLI-boundary defaults exactly.

```sh
export CONFIG_PROTECT=/etc
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild merge
echo "hand-modified content" > "${ROOT}"/usr/share/mergepkg/hello.txt

# Real default (unmerge-orphans is a real make.globals default FEATURES
# token, see this file's own "FEATURES=distlocks" section below): the
# locally-modified file is deleted anyway.
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild unmerge
test -e "${ROOT}"/usr/share/mergepkg/hello.txt && echo "still there" || echo "gone"
# gone

# Re-merge, modify again, unmerge with FEATURES="-unmerge-orphans" (real
# make.conf's own opt-out syntax -- this pilot's own simplified env-var
# check treats *any* explicit FEATURES value as a literal membership
# list rather than a +/- delta against the real default set, so setting
# FEATURES at all to anything other than the literal token
# "unmerge-orphans" already reads as off here): survives this time.
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild merge
echo "hand-modified content" > "${ROOT}"/usr/share/mergepkg/hello.txt
export FEATURES=some-other-token
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild unmerge
cat "${ROOT}"/usr/share/mergepkg/hello.txt
# hand-modified content
unset FEATURES
unset FEATURES
```

### `unmerge`: real `INFOPATH` cleanup

Real `_unmerge_dirs()`'s own `INFOPATH` cleanup (`vartree.py:3226-3251`)
is fully real now, both triggers. A directory literally named `"info"`
(real comment: "since it might have been in INFOPATH previously even
though it may not be there now") whose only remaining content is a
subset of `{"dir", "dir.old"}` (real `_infodir_cleanup`, GNU
`install-info`'s own auto-generated index files, which live outside any
package's own tracked `CONTENTS`) has those removed first -- otherwise a
stray leftover index would keep such a directory from ever emptying out
and being removed at all. The other real trigger, `inode_key in
infodirs_inodes` -- an `INFOPATH`/`INFODIR` env-var-driven inode match
covering an info directory that isn't literally named `"info"` -- is
threaded through too: `env_update::info_dirs_inodes` collates real
`INFOPATH`/`INFODIR` values from `/etc/env.d/*` the same way
`env_update::run_env_update` collates every other real
`COLON_SEPARATED` key, and `run_unmerge` computes that set once per
unmerge and threads it down through `remove_contents`/`remove_dirs`
into `cleanup_info_dir`. Verified directly against `cleanup_info_dir`
(the lone-index-file case, both `dir` and `dir.old` together, a real
remaining file that correctly blocks cleanup entirely, a
same-named-but-not-`"info"` directory that's correctly ignored, and now
an inode-match hit on a directory *not* named `"info"`), against
`env_update::info_dirs_inodes` directly (real multi-entry
`INFOPATH`/`INFODIR` resolution, a candidate that doesn't actually
exist, and a missing `/etc/env.d` altogether), and end to end via
`remove_contents`
(`remove_contents_removes_an_info_directory_blocked_only_by_a_leftover_index_file`
for the `basename == "info"` half,
`remove_contents_removes_a_real_env_d_sourced_infopath_directory_not_named_info`
for the `infodirs_inodes` half).

```sh
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild merge

# A real install-info leftover index, untracked by CONTENTS, in a
# directory that is NOT literally named "info".
echo -n "" > "${ROOT}"/usr/share/mergepkg/dir

# A real /etc/env.d entry declaring this directory via INFOPATH, the
# same way env_update() collates every other real COLON_SEPARATED key.
mkdir -p "${ROOT}"/etc/env.d
echo 'INFOPATH="/usr/share/mergepkg"' > "${ROOT}"/etc/env.d/50-fixture

PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild unmerge
test -e "${ROOT}"/usr/share/mergepkg && echo "still there" || echo "gone"
# gone -- the inode-match trigger fired even though the directory isn't
# named "info", so the leftover index no longer blocks its removal.
```

### `unmerge`: real `stale_confmem` cleanup

Real `_unmerge_pkgfiles()`'s own `stale_confmem` cleanup
(`vartree.py:2747`/`2931-2932`/`3106-3109`) is real now too.
`cfgfiledict` -- the real `_conf_mem_file` "already offered this MD5"
memory `ebuild_merge` writes on merge (`read_cfgfiledict`/
`write_cfgfiledict`, promoted `pub(crate)` for this slice) -- is read
once up front, the same way the real function reads it before its own
per-file removal loop starts. Any path `remove_contents` actually
removes -- one not `is_owned` by another same-slot instance -- that
`cfgfiledict` still remembers is collected into `stale_confmem` and
dropped from the persisted memory afterward: real ordering exactly,
since `elif relative_path in cfgfiledict: stale_confmem.append(...)` is
the same `if is_owned` check the "replaced" skip already uses. Without
this, a long-gone package's own previously-offered CONFIG_PROTECT
update would keep sitting in `_conf_mem_file` for a path nothing
installs anymore, ready to wrongly satisfy a real future merge's own
"already offered" check for some unrelated package that happens to
write the same path. Verified directly against `remove_contents`
(collects a removed, not-owned path's own stale entry; does *not*
collect one still `is_owned` by another same-slot instance) and end to
end via `run_unmerge`
(`real_unmerge_drops_a_stale_conf_mem_entry_but_keeps_an_unrelated_one`).

```sh
export ROOT="$(mktemp -d)"
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild merge

# A real _conf_mem_file: one entry for a path this package actually
# owns and is about to remove, one entry for an unrelated path.
mkdir -p "${ROOT}"/var/lib/portage
cat > "${ROOT}"/var/lib/portage/config <<CONF
/usr/share/mergepkg/hello.txt deadbeef
/etc/unrelated.conf cafebabe
CONF

PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild unmerge
cat "${ROOT}"/var/lib/portage/config
# /etc/unrelated.conf cafebabe
# -- the removed package's own now-stale entry is gone, the unrelated
# one survives untouched.
```

### Standalone `ebuild <file> config`/`info`

Real `doebuild()`'s own early-return branch for a handful of commands
(`lib/portage/package/ebuild/doebuild.py:1326-1351`, "running them out
of the sandbox -- and stop now") is real now, for the two of them a real
admin/user actually invokes directly by name: `config`/`info`. No
`install` chain, no merge/vdb step at all -- just the real, single
`pkg_config`/`pkg_info` phase function, run directly against the ebuild
file via `run_single_phase`, the exact same machinery
`preinst`/`postinst`/`prerm`/`postrm` already use internally (real,
unmodified `bin/phase-functions.sh`'s own `__ebuild_main` already
accepts `config`/`info` as literal phase arguments) -- so this slice
needed no new phase-execution machinery at all, purely CLI routing
(`ebuild_phases::is_real_standalone_phase_command`). `prerm`/`postrm`
joined them as standalone commands too, in a later slice -- see this
file's own "Standalone `ebuild <file> prerm`/`postrm`" section below.
`preinst`/`postinst` (real too, but only reachable internally, as part
of `merge` -- a real ordering constraint, `dblink.treewalk()` invokes
them directly around the actual file-copy step, that `prerm`/`postrm`
don't share with `unmerge`) and `pretend` (already part of the
`actionmap_deps` chain) still aren't reachable as their own top-level
command.

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/standalonephasepkg/standalonephasepkg-1.0.ebuild config info
ls "${PORTAGE_TMPDIR}"/portage/dev-libs/standalonephasepkg-1.0/temp/ | grep pkg-
# pkg-config-ran
# pkg-info-ran

# Still a dry-run stub, unlike config/info now:
PORTING/rust/target/release/portuale ebuild foo-1.0.ebuild clean
# ebuild (pilot stub): dry-run only, no phase execution yet ...
```

### Standalone `ebuild <file> prerm`/`postrm`

`prerm`/`postrm` join `config`/`info` as standalone commands, real for
the same reason and via the same `run_single_phase` machinery (see the
section above). Unlike `preinst`/`postinst` -- which stay internal-only,
tied to `merge`'s own real file-copy ordering (`dblink.treewalk()`
invokes them directly around it, a constraint no standalone invocation
could reproduce) -- `prerm`/`postrm` have no equivalent constraint tying
them to `unmerge`'s own file-removal step: real portage itself allows
invoking them completely standalone (e.g. to test a `pkg_prerm`/
`pkg_postrm` function without actually removing the package). So
`unmerge`'s own internal use (`ebuild_unmerge::run_unmerge`) and this
new standalone path are simply two independent, real ways to reach the
same real phase functions -- no new phase-execution machinery needed
here either, purely a CLI-routing addition.

```sh
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/standalonephasepkg/standalonephasepkg-1.0.ebuild prerm postrm
ls "${PORTAGE_TMPDIR}"/portage/dev-libs/standalonephasepkg-1.0/temp/ | grep pkg-
# pkg-postrm-ran
# pkg-prerm-ran
```

### Real `PORTAGE_COMPRESSION_COMMAND` resolution

Real `doebuild.py:697-750`'s own `PORTAGE_COMPRESSION_COMMAND`
resolution is real now, replacing the previous hardcoded `"bzip2 -c"`.
Looks up `BINPKG_COMPRESS` (real `make.globals`'s own default is
`"zstd"` -- **not** `"bzip2"`; this pilot's own previous hardcoded
value predated noticing real portage's own default had changed) in the
real `_compressors` table (all six real entries: `bzip2`/`gzip`/`lz4`/
`lzip`/`lzop`/`xz`/`zstd`), substitutes `{JOBS}` (real host CPU count)
and `${PORTAGE_BZIP2_COMMAND}`/`${BINPKG_COMPRESS_FLAGS}` (narrowed to a
plain `${VAR}` substitution, not a full shell `varexpand` -- none of the
six real templates or realistic flag values need anything beyond that),
and confirms the resolved binary is real-`PATH`-findable (real
`find_binary()`). An unknown `BINPKG_COMPRESS` name or a compressor
whose binary isn't actually installed leaves `PORTAGE_COMPRESSION_
COMMAND` unset entirely -- matching real behavior exactly, real,
unmodified `bin/misc-functions.sh` then hits its own real `[[ -z
"${PORTAGE_COMPRESSION_COMMAND}" ]] && die "PORTAGE_COMPRESSION_COMMAND
is unset"` guard naturally, rather than this pilot fabricating a
fallback. `BINPKG_COMPRESS_FLAGS_<NAME>` (the real per-compressor
override) is resolved once, at the `ebuild.rs`/`pretend.rs` CLI
boundary (both real entry points into `PackageOptions`), falling back to
plain `BINPKG_COMPRESS_FLAGS` when unset -- `ebuild_package.rs` itself
never needs to know about the override naming convention at all.

Since this pilot's own `portage_repo` binary-package reader never parses
a `.tbz2`/XPAK file's own content (only `Packages`), the tar body's
actual compression codec was always cosmetic to this pilot's own
reading path -- verified by checking the real magic bytes at the start
of the produced `.tbz2` directly (`28 b5 2f fd` for real zstd, `fd 37 7a
58 5a 00` for real xz, both matching real `compression_probe.py`'s own
`_compression_re`), not just that the file exists.

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export PKGDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/packagepkg/packagepkg-1.0.ebuild install package
xxd "${PKGDIR}"/dev-libs/packagepkg-1.0.tbz2 | head -1
# 00000000: 28b5 2ffd ...   <- real zstd magic bytes, the new default

export PKGDIR="$(mktemp -d)"
export BINPKG_COMPRESS=xz
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/packagepkg/packagepkg-1.0.ebuild install package
xxd "${PKGDIR}"/dev-libs/packagepkg-1.0.tbz2 | head -1
# 00000000: fd37 7a58 5a00 ...   <- real xz magic bytes

export PKGDIR="$(mktemp -d)"
export BINPKG_COMPRESS=made-up-codec
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/packagepkg/packagepkg-1.0.ebuild install package
# * ERROR: dev-libs/packagepkg-1.0:: failed (package phase):
# *   PORTAGE_COMPRESSION_COMMAND is unset
unset BINPKG_COMPRESS
```

### Real `custommirrors`: an admin-configured `/etc/portage/mirrors` file

Real `fetch.py:984-985`'s own `custommirrors` (`grabdict(os.path.join(
PORTAGE_CONFIGROOT, "etc/portage/mirrors"))`) is real now, closing the
one gap the previous `mirror://` resolution slice's own doc comment
explicitly flagged. Real `fetch.py:1143-1149`'s own "Try user-defined
mirrors first" ordering: for a `mirror://<name>/<path>` token,
`custommirrors`'s own roots for `<name>` (if any) are listed *before*
`profiles/thirdpartymirrors`'s own roots for the same name --
`portage_fetch::resolve_mirror_candidates` now takes both maps and
expands `custommirrors` first, real `cmirr.rstrip("/") + "/" + path`
string-built identically to the `thirdpartymirrors` half. Reuses
`parse_thirdpartymirrors` directly for the actual file parsing (the
exact same real `grabdict()` line format, just pointed at a different
real file) rather than a second, duplicate parser.

`FetchOptions` gained a `config_root` field (real `PORTAGE_CONFIGROOT`),
mirroring `ebuild_merge::MergeOptions::config_root`'s own doc comment
and its own deliberately-impossible-path `Default` exactly -- this
pilot's own dev/test machine is a real Gentoo system, so a naive
real-`/`-style default here would make every test that doesn't
override this field read real host config. Fixed a real bug surfaced by
that exact sentinel path during this slice's own implementation:
`${config_root}/etc/portage/mirrors` fails with `ENOTDIR` (an
*ancestor* path component, `/dev/null`, isn't a directory) when
`config_root` is the sentinel, not the `NotFound` `parse_thirdpartymirrors`
itself already tolerated -- an early version of this code propagated
that as a raw I/O error instead of degrading gracefully to "no
`custommirrors`", the same graceful-degrade precedent `ebuild_merge::
blocked_installed_packages`'s own `find_repos(config_root).ok()?`
already established for this exact pattern. Now a regression test
(`fetch_src_uri_degrades_gracefully_when_config_root_is_the_default_
sentinel`) locks that fix in.

Real `custommirrors["local"]`'s own *separate* meaning -- a real
filesystem-path/local-network fast-path lookup tried before any remote
fetch at all (real `fetch.py:1017-1029`'s own `fsmirrors`/
`local_mirrors` split) -- is not reproduced; nor is real `grabdict(...,
recursive=1)`'s own directory-form (`/etc/portage/mirrors/` as a
directory of drop-in files), the same narrowing `profiles/
thirdpartymirrors` itself already has. Proven via a new, real,
end-to-end integration test in `portuale/src/fetch.rs`
(`fetch_src_uri_resolves_a_real_mirror_uri_via_custommirrors`: a real
local HTTP server, a real `${config_root}/etc/portage/mirrors` file, a
real `wget` subprocess, real digest verification -- no `profiles/
thirdpartymirrors` entry for the name at all, proving `custommirrors`
is consulted independently) plus a new, pure, offline unit test in
`portage-fetch` (`resolve_mirror_candidates_tries_custommirrors_
before_thirdpartymirrors`) proving the real ordering directly.

### `FEATURES=distlocks`, and a real default-`FEATURES` correction to `protect-owned`/`unmerge-orphans`

Real `lib/portage/locks.py`'s own `lockfile(mypath, wantnewlockfile=1)`
(called at real `fetch.py:1315-1330`, unlocked at `:2032-2033`) is real
now: a real, blocking `flock(2)` exclusive lock on a real, separate
`.{basename}.portage_lockfile` sibling of the distfile, held for the
*entire* per-file fetch-and-verify sequence (not just the actual
download), guarding against two concurrent portage processes racing the
same file. `DistfileLock` releases it simply by closing the lock file's
own fd when the guard drops -- POSIX guarantees all of a process's own
`flock` locks on an fd release when that fd closes, the same real effect
real `unlockfile()`'s own explicit `LOCK_UN` has. Real `unlinkfile=0`
(this pilot's own default too): the lockfile persists on disk after
release, just unlocked, ready for reuse. No std equivalent exists for
`flock(2)` at all, so `libc` (already a transitive dependency of
`tokio`/`brush-core`, declared directly here now for its own direct use)
joins this pilot's own small set of deliberate, documented dependency
waivers.

Verified genuinely live, not just "returns `Ok`": a real, blocking,
cross-thread test (`distfile_lock_blocks_a_second_acquire_until_
released`) holds the lock on one thread, spawns a second thread that
tries to acquire the same lock, confirms it does *not* complete within
200ms while the first lock is held, then drops the first lock and
confirms the second completes within 5s -- genuine, real OS-level
blocking, not a mocked assertion. Confirmed live against the compiled
binary too: a real fetch leaves a real `.<filename>.portage_lockfile`
sibling behind in `DISTDIR`, correctly unlocked and reusable.

While researching real `distlocks`'s own actual default, a genuinely
significant, previously-undiscovered finding surfaced: real
`cnf/make.globals`'s own default `FEATURES` list (lines 77-84) --
`assume-digests binpkg-docompress binpkg-dostrip binpkg-logs
binpkg-multi-instance buildpkg-live compress-index config-protect-
if-modified distlocks ebuild-locks fixlafiles ipc-sandbox merge-sync
merge-wait multilib-strict network-sandbox news parallel-fetch
pkgdir-index-trusted pid-sandbox preserve-libs protect-owned
qa-unresolved-soname-deps sandbox strict unknown-features-warn
unmerge-logs unmerge-orphans userfetch userpriv usersandbox usersync`
-- includes not just `distlocks` but also `protect-owned` and
`unmerge-orphans`, **both already shipped in earlier slices with
`Default: false`**, each documented at the time with the same
now-disproven claim `collision_protect` genuinely has ("real `FEATURES`
itself isn't in `FEATURES` by default"). That claim was simply wrong for
these two tokens. Fixed as part of this slice: `MergeOptions::
protect_owned` and `UnmergeOptions::unmerge_orphans` now both default to
`true`, matching real portage's own actual out-of-the-box behavior --
confirmed live (an ordinary file collision with an identifiable owner
now aborts by real default; a locally-modified file is now deleted on
unmerge by real default) and locked in by two new tests
(`ordinary_collision_aborts_by_real_default_via_protect_owned`,
`real_unmerge_deletes_a_locally_modified_file_by_real_default`). One
pre-existing test's own premise needed correcting to match: what used to
be `ordinary_collision_is_merged_over_when_collision_protect_is_off`
(implying "collision-protect off" alone was real portage's own default
behavior) is now `ordinary_collision_is_merged_over_with_both_
collision_protect_and_protect_owned_off`, explicitly setting
`protect_owned: false` rather than relying on a default that no longer
means what the name implied.

This pilot's own env-var read for all three flags still only checks
whether the literal `FEATURES` value, when set at all, contains the
exact token -- it doesn't *accumulate* onto the real default set the way
real portage's own `+`/`-`-prefixed `make.conf` `FEATURES` merging does,
so setting `FEATURES` to any other, unrelated token still reads as
`false` here for all three, unlike real portage (which would keep them
enabled unless explicitly removed with a leading `-`). A pre-existing
simplification this fix doesn't attempt to also resolve.

While correcting this, also found (and fixed) that this same backlog's
own "`FEATURES=verify-sig`" entry was mis-scoped from the start: real
signature verification is a `gpkg` (the newer GPG-signed binary package
format) and repo-sync concept, not a `SRC_URI`/distfile-fetch one at
all -- confirmed by grepping `fetch.py` directly and finding zero hits
for either term. Removed from the backlog rather than left to mislead a
future "scope the next slice" round.

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export DISTDIR="$(mktemp -d)"
echo "hello from verifiedfetchpkg" > "${DISTDIR}/verifiedfetchpkg-1.0.tar.gz"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/verifiedfetchpkg/verifiedfetchpkg-1.0.ebuild install
ls -a "${DISTDIR}"
# .  ..  .verifiedfetchpkg-1.0.tar.gz.portage_lockfile  verifiedfetchpkg-1.0.tar.gz
```

### `RESTRICT=mirror`: no `GENTOO_MIRRORS` flat-layout fallback

Real `fetch.py`'s own `restrict_mirror = "mirror" in restrict or
"nomirror" in restrict` (`:880`) and `file_restrict_mirror`
(`:1117-1127`): when a package restricts mirroring, real portage does
**not** append the public `GENTOO_MIRRORS` list to that file's candidate
locations. This pilot's `gentoo_mirror_fallback` step (the flat
`<mirror>/distfiles/<filename>` expansion) is now gated on
`!options.restrict_mirror` -- a `mirror://` URI's own
`custommirrors`/`thirdpartymirrors` expansion and any explicit `SRC_URI`
URI are still tried (real portage keeps `local_mirrors` in
`location_lists` regardless).

`FetchOptions` gained a `restrict_mirror` field;
`ebuild_phases::fetch_sources` sets it from the ebuild's own md5-cache
`RESTRICT` field via `restrict_mirror_from_restrict`, which
USE-conditional-evaluates the raw value (real `_PackageMetadataWrapper`'s
own `use_reduce` pass, same as `PROPERTIES`/`LICENSE`) against this
pilot's always-empty fetch-side USE set -- so a `foo? ( mirror )` group
drops and only an unconditional `mirror`/`nomirror` counts.

v1 scope (**superseded 2026-08-30** -- see "`mirror+`/`fetch+` `SRC_URI`
prefixes" below): real portage's own `mirror+`/`fetch+` `SRC_URI` prefix
was not handled here. Three new tests: the public fallback is skipped (a
near-clone of the existing `…falls_back_to_gentoo_mirrors…` test
asserting failure instead of success), a `mirror://` custommirror still
resolves under `restrict_mirror`, and `restrict_mirror_from_restrict`'s
own conditional-evaluation cases. Rust-only (real fetch, no `--pretend`
mirror).

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"; export DISTDIR="$(mktemp -d)"

# dev-libs/restrictmirrorpkg has RESTRICT="mirror" and its distfile
# pre-verified in DISTDIR -> the skip-fetch path, install succeeds
printf 'hello from restrictmirrorpkg\n' > "${DISTDIR}/restrictmirrorpkg-1.0.tar.gz"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/restrictmirrorpkg/restrictmirrorpkg-1.0.ebuild install
# ... exits 0

# remove it: the fetch is attempted, and ONLY the (unreachable) primary
# SRC_URI is tried -- no GENTOO_MIRRORS fallback line, because
# RESTRICT=mirror bars it
rm "${DISTDIR}/restrictmirrorpkg-1.0.tar.gz"
GENTOO_MIRRORS="https://distfiles.gentoo.org" PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/restrictmirrorpkg/restrictmirrorpkg-1.0.ebuild install
# ebuild: restrictmirrorpkg-1.0.tar.gz: every candidate failed:
# wget failed to fetch "https://example.invalid/payload.bin" (exit status: 4)
# ... exits 1  (a non-restricted package would also have tried
#     https://distfiles.gentoo.org/distfiles/restrictmirrorpkg-1.0.tar.gz)
```

### `mirror+`/`fetch+` `SRC_URI` prefixes (`override_mirror`/`override_fetch`)

Real `fetch.py:1103-1106` (a portage extension, not PMS): a `SRC_URI`
URI token may carry a `mirror+` or `fetch+` prefix. Real portage strips
it (`myuri = myuri.partition("+")[2]`) and sets `override_mirror =
myuri.startswith("mirror+")` / `override_fetch = override_mirror or
myuri.startswith("fetch+")`. Those feed `file_restrict_mirror =
(restrict_fetch or restrict_mirror) and not override_mirror`
(`:1117-1119`) and `if (restrict_fetch and not override_fetch)`
(`:1167`): `mirror+` re-permits the public flat-layout mirror list for
that file even under `RESTRICT=mirror`, and either prefix exempts the URI
from `RESTRICT=fetch`.

`portage_fetch::flatten_src_uri` now strips the prefix in the parser (so
the recorded `uri` and its derived `filename` are both clean) and
records `SrcUriEntry::override_mirror`/`override_fetch` (`mirror+` sets
both, matching real `override_fetch = override_mirror or ...`).
`portuale/src/fetch.rs` checks `entry.override_mirror` per entry:
`public_mirrors_barred = options.restrict_mirror && !entry.override_mirror`
now gates the `gentoo_mirror_fallback` append (and the "no working
candidate" error wording). `override_fetch` became live 2026-08-30 with
`RESTRICT=fetch` modelling -- see "`RESTRICT=fetch`" below; the prefix
is stripped so the URL stays valid regardless.

Rust-only (real fetch, no `--pretend` mirror). Tests:
`portage_fetch` gains three parser tests (`mirror+` sets both overrides
and strips the prefix; `fetch+` sets only `override_fetch`; a plain URI
has neither), and `portuale/src/fetch.rs` gains
`fetch_src_uri_mirror_prefix_re_permits_the_gentoo_mirrors_fallback_under_restrict_mirror`
-- the exact `RESTRICT=mirror` + unreachable-primary-URI setup that fails
in `…restrict_mirror_skips_the_gentoo_mirrors_fallback`, but with a
`mirror+` prefix on the SRC_URI so the mirror server is tried and rescues
the fetch.

### `RESTRICT=fetch`: the plain `SRC_URI` URI is never downloaded

Real `fetch.py:1061` (`restrict_fetch = "fetch" in restrict`) +
`:1166-1174` (`if (restrict_fetch and not override_fetch) …: continue`):
a *plain* (non-`mirror://`) `SRC_URI` URI is not a fetchable candidate
under `RESTRICT=fetch`, and `file_restrict_mirror = (restrict_fetch or
restrict_mirror) …` bars the public `GENTOO_MIRRORS` fallback too. So a
fetch-restricted package fetches OK only from an already-verified
`DISTDIR` copy (the user placed it by hand — the normal case), a
`custommirrors` entry, or a `mirror://`-named mirror. A `fetch+`/`mirror+`
`SRC_URI` prefix (`override_fetch`) re-permits the URI.

`FetchOptions` gained a `restrict_fetch` field, set by
`ebuild_phases::fetch_sources` from the ebuild's own `RESTRICT`
md5-cache field via the new `restrict_fetch_from_restrict`
(USE-conditional-evaluated against the empty fetch-side USE set, same as
`restrict_mirror_from_restrict` — the two now share a `restrict_has_
token` helper). `fetch_src_uri` `retain`s the plain URI out of the
candidate list when `restrict_fetch && !override_fetch && !uri.starts_
with("mirror://")`, and folds `restrict_fetch` into `public_mirrors_
barred`. **v1 cut:** this pilot does **not** run the ebuild's own
`pkg_nofetch` phase for a missing file (real `spawn_nofetch` — custom
"download it from … and place it in `DISTDIR`" instructions);
`fetch_src_uri` fails with a generic pointer instead.

New `dev-libs/fetchrestrictpkg` fixture (`RESTRICT="fetch"`,
`https://example.invalid/…` SRC_URI, real Manifest digests, a
`src_install`). `ebuild <file> install` with the distfile **absent**
exits 1 with the `RESTRICT=fetch bars downloading it …` message and
never contacts `example.invalid`; with the distfile **present +
verified**, it installs via the already-verified skip path. Rust-only
(real fetch). 4 unit tests (`fetch.rs` ×3, `ebuild_phases.rs` ×1) + 1
black-box test.

### `license_groups` read from each repo's `profiles/`, not the profile chain (real-tree finding)

Found by running `portuale` against a **real Gentoo tree** in a
container for the first time: `emerge --pretend <anything>` failed with
`there are no ebuilds to satisfy …` for *every* package. Root cause:
`resolve_config` read `license_groups` from each *profile-chain
directory* (`<repo>/profiles/base/`, `.../amd64/`, …), but real gentoo
puts the file at `<repo>/profiles/license_groups` — the repo `profiles/`
base, which is **not** a profile-chain level. So `@FREE` (the profile's
`ACCEPT_LICENSE`) expanded to nothing, and the default `* -@FREE`-style
filter rejected every ebuild on a license check.

Real `LicenseManager._read_license_groups` (`LicenseManager.py:47`)
iterates `LocationsManager.profile_locations` (`LocationsManager.py:432`),
which is exactly `[<main_repo>/profiles] + [<overlay>/profiles …]` — the
`profiles/` directory of the main repo and each overlay, never the
individual chain levels. `resolve_config` (both sides) now reads
`<repo>/profiles/license_groups` for every configured repo (main first,
then overlays) instead of `<chain-level>/license_groups`. This also
makes an overlay's own `license_groups` unconditional (real behavior) —
the earlier "only reachable via a `reponame:path` cross-repo `parent`"
framing (see the profile-chain section above) is superseded; that
mechanism is still tested for profile *directories*, just not for
`license_groups`.

The two fixture `license_groups` files moved from
`repo/profiles/base/` → `repo/profiles/` and `overlay/profiles/
crossrepo-parent/` → `overlay/profiles/`. A new `portage-profile` unit
test drops a stray `license_groups` into a chain dir and asserts it's
ignored. After the fix, `portuale --pretend` resolves real packages
(`sys-apps/coreutils`, `app-misc/tmux` → `dev-libs/libevent`, …)
matching real `emerge --pretend`'s package set. Both sides; 1 `CASES` +
1 pinned test relabeled, 1 new unit test.

### `emerge --buildpkgonly --keep-going`

Real `--keep-going` (real `main.py`'s own `y_or_n` option, narrowed by
this pilot's own CLI transcription, `emerge_options::BOOLEAN_OPTIONS`,
to the bare/`y` form only -- already recognized on the command line
before this slice, just silently ignored) is real now, for
`--buildpkgonly` without `--pretend`: without it, `run_buildpkgonly`
still stops at the *first* build failure, its own long-established
default; with it, every remaining entry is still attempted regardless
of an earlier failure, and all failures are collected into a single
combined error at the end.

This pilot's own version is genuinely simpler than real portage's own
general `--keep-going`: real `Scheduler.py` must also skip every
*dependent* of a failed package (tracked via real mergelist
recalculation against `_mtimedb`), since a real merge list can have
real ordering dependencies between entries. `--buildpkgonly`'s own real
depgraph gate (`GraphResult::buildpkgonly_deps_unsatisfied`, already
checked in `pretend.rs` before `run_buildpkgonly` is ever called at
all) guarantees the opposite here: every entry it resolves already has
every real dependency satisfied by something *already installed*, so
no entry in this pilot's own build list can ever depend on another one
in it. A failure therefore has nothing downstream left to invalidate --
`--keep-going` here reduces to "attempt every entry regardless, report
every failure at the end," with none of real portage's own mergelist
machinery needed to make that safe.

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_CONFIGROOT="$(pwd)/PORTING/fixtures"
export ROOT="$(pwd)/PORTING/fixtures"
export PORTAGE_TMPDIR="$(mktemp -d)"
export PKGDIR="$(mktemp -d)"

# Without --keep-going: stops at fetchpkg (no Manifest entry), never
# even attempts packagepkg.
PORTING/rust/target/release/portuale emerge --buildpkgonly \
    dev-libs/fetchpkg dev-libs/packagepkg
ls "${PKGDIR}/dev-libs" 2>&1
# ls: cannot access '.../dev-libs': No such file or directory

# With --keep-going: fetchpkg still fails, but packagepkg gets built
# anyway.
export PKGDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale emerge --buildpkgonly --keep-going \
    dev-libs/fetchpkg dev-libs/packagepkg
# emerge: 1 package(s) failed to build (--keep-going):
# dev-libs/fetchpkg-1.0: fetchpkg-1.0.tar.gz: no Manifest entry, ...
ls "${PKGDIR}/dev-libs"
# packagepkg-1.0.tbz2
```

### `USE_EXPAND_IMPLICIT`: `elibc_*`/`kernel_*`/... are valid implicit IUSE

Real `config.py::_calc_iuse_effective` (the EAPI 5+ `IUSE_EFFECTIVE`
definition): a package's own `pkg.iuse.is_valid_flag` domain includes,
on top of its declared `IUSE`, every flag derived from a
`USE_EXPAND_IMPLICIT` variable -- real Gentoo's own
`profiles/base/make.defaults` sets `USE_EXPAND_IMPLICIT="ARCH ELIBC
KERNEL USERLAND"`, so `elibc_glibc`, `kernel_linux`, `userland_GNU`,
etc. count as valid IUSE for *every* package even when unlisted, and a
USE-dep like `dev-libs/foo[elibc_glibc]` matches a `foo` that never
declares `elibc_glibc`. Before this slice the pilot checked a USE-dep's
`.required` flags against a candidate's *declared* `IUSE` alone, so such
a dep spuriously failed and its target went invisible.

New `portage_profile::Config::iuse_effective` computes the real formula:
`IUSE_IMPLICIT` values, plus every `USE_EXPAND_VALUES_<v>` value for each
`USE_EXPAND_UNPREFIXED` var `v` also in `USE_EXPAND_IMPLICIT`
(unprefixed), plus `lowercase(v)_<value>` for each `USE_EXPAND` var `v`
also in `USE_EXPAND_IMPLICIT`. `USE_EXPAND_IMPLICIT`/`IUSE_IMPLICIT` are
read as real INCREMENTALS (like `USE_EXPAND`); `USE_EXPAND_VALUES_*` as
plain scalars (not in real portage's own INCREMENTALS list either). A
new `valid_iuse(declared, config)` helper unions `iuse_effective` in at
each `use_deps_satisfied` call site, matching real `is_valid_flag` --
deliberately *not* inside `candidate_iuse_and_use`, whose result also
feeds `--newuse`'s own IUSE-*presence* diff (an implicit flag there
would read as "newly added to IUSE" and spuriously trigger a reinstall).
`implicit_iuse_set` (the `REQUIRED_USE`/parent-USE-state path) also gains
`iuse_effective`, so a `REQUIRED_USE` referencing `elibc_*` is
recognized the same way one referencing `x86` (via `archlist`) already
was -- **this closes the "`USE_EXPAND_HIDDEN`-derived ... `elibc_.*`/
`kernel_.*`/`userland_.*` ... a bigger, separate feature" cut named in
the implicit-IUSE bullet above** (via the EAPI 5+ `USE_EXPAND_IMPLICIT`
path, with explicit `USE_EXPAND_VALUES` rather than the pre-EAPI-5 regex
form).

`USE_EXPAND_HIDDEN` stays unimplemented and is genuinely a non-gap: for
EAPI 5+ it is a pure `emerge --info`/`-pv` USE-*grouping* display
concern (the pre-EAPI-5 `_get_implicit_iuse` is the only place it fed
`is_valid_flag`), and this pilot's `-pv` shows a flat declared-IUSE list
with no `USE_EXPAND` grouping to hide from. The two stale
"`USE_EXPAND_HIDDEN`/`_IMPLICIT` are display-only" comments in
`portage-profile` (they conflated the two) are corrected in place.

New fixtures: `profiles/base/make.defaults` gains
`USE_EXPAND_IMPLICIT="ELIBC"` + `USE_EXPAND_VALUES_ELIBC="glibc musl"` +
`ELIBC="glibc"`; `dev-libs/implicitiusepkg` RDEPENDs
`implicitiuseprov[elibc_glibc]` (resolves -- valid + enabled) and
`dev-libs/implicitiusepkgmusl` RDEPENDs `implicitiuseprov[elibc_musl]`
(unsatisfiable -- valid but not enabled, proving the slice widened the
*valid* domain, not the *enabled* one). Two Rust unit tests, two
parametrized contract cases, and a dedicated pinned-output contract test,
mirrored in `emerge_pretend_reference.py` (`_valid_iuse`,
`iuse_effective` in `resolve_config`, the `dep_keys` threading through
`_process_config_lines`/`_process_make_conf_file`).

Deliberately still a documented cut: an *installed* package's USE-dep
check (`dependency_avoid_update_candidate`) uses the raw vdb `IUSE`,
since real portage uses that package's own *vdb-recorded*
`IUSE_EFFECTIVE`, which this pilot doesn't persist; and IUSE-aware `_*`
wildcard expansion (`linguas_*`), which needs a specific package's own
`IUSE`.

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"

PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend dev-libs/implicitiusepkg
# [ebuild  N] dev-libs/implicitiusepkg-1.0
# [ebuild  N] dev-libs/implicitiuseprov-1.0
#   -- implicitiuseprov[elibc_glibc] resolves, though implicitiuseprov
#      never lists elibc_glibc in its own IUSE

PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend dev-libs/implicitiusepkgmusl
# [ebuild  N] dev-libs/implicitiusepkgmusl-1.0
# !!! no visible ebuild for dependency "dev-libs/implicitiuseprov"
#   -- elibc_musl is valid implicit IUSE but not enabled (ELIBC="glibc")
```

### `USE_EXPAND` `_*` wildcard: enable every matching flag in a package's own IUSE

The last `USE_EXPAND` corner. Real `config.py`'s `setcpv` (~line 2242):
once `package.use` has been folded, a `k_*` flag still in the USE set --
from `USE="linguas_*"`, from `LINGUAS="*"` being prefix-folded, or from a
`package.use` `LINGUAS: *` shorthand -- means "enable every `k_<x>` flag
declared in **this candidate's own `IUSE`** that isn't masked". It is
inherently per-package (the IUSE-blind global config layer can't do it),
so this lands in `portage_repo::effective_use_flags`, right after
`package.use` and before the `use.mask`/`.force` steps: for each
`_*`-suffixed token, every `IUSE` flag sharing its `k_` prefix is added,
then the existing `use.mask` steps drop any that are masked (real
portage's own `x not in usemask` guard), and finally every `_*`-suffixed
pseudo-flag is stripped from the result (real portage strips them from
`PORTAGE_USE`).

Deliberately **not** guarded on `k` actually being a declared
`USE_EXPAND` variable name (real portage checks `use_expand_split`): a
`_*`-suffixed token in this pilot's USE set only ever originates from
`USE_EXPAND` folding or `package.use`'s own `USE_EXPAND` shorthand
anyway. This was the "IUSE-aware `_*` wildcard expansion (`linguas_*` --
needs a specific package's own `IUSE`)" cut named in `portage-profile`'s
module doc; that note is corrected.

New fixtures: `profiles/base/make.defaults` adds `LINGUAS` to
`USE_EXPAND`; `etc/portage/package.use` gets `dev-libs/wildexpandpkg
linguas_*`; `profiles/base/package.use.mask` gets `dev-libs/wildexpandpkg
linguas_en`; `dev-libs/wildexpandpkg` (`IUSE="linguas_en linguas_de"`,
`RDEPEND="linguas_de? ( dev-libs/wildexpanddep ) linguas_en? (
dev-libs/wildexpandmasked )"`) -- so `linguas_de` is wildcard-enabled and
pulls `wildexpanddep`, while `linguas_en` stays masked off and
`wildexpandmasked` (which needn't exist) is never referenced. Two Rust
unit tests, one parametrized contract case, one dedicated pinned-output
contract test; mirrored in `emerge_pretend_reference.py`.

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend -v dev-libs/wildexpandpkg
# [ebuild  N] dev-libs/wildexpandpkg-1.0  USE="linguas_de -linguas_en"
# [ebuild  N] dev-libs/wildexpanddep-1.0
#   -- linguas_* expanded to the two declared linguas_* IUSE flags;
#      linguas_en stayed masked, so only linguas_de's RDEPEND clause fired
```

### `metadata/layout.conf`: `masters =` middle tier, `repo-name`, and the `profile-formats` parent-colon gate

Nothing read `<repo>/metadata/layout.conf` before -- `RepoConfig`'s
canonical repo name was its `repos.conf` `[section]` name, `masters`
came only from `repos.conf`, and a profile `parent` line's cross-repo
`reponame:path`/`:path` syntax was always expanded. This slice ports
three real `layout.conf` keys.

**`masters =` as a middle tier.** Real `config.py:237-245`/`484-490`:
an explicit `repos.conf` `masters =` wins; else the repo's own
`layout.conf` `masters =` (an empty one is a real "no masters",
distinct from the key being absent); else the implicit default (the
main repo alone). `find_repos` now resolves all three tiers -- new
`parse_layout_conf` (a section-less `key = value` reader), a
`repos_conf_masters` capture in the first pass, and a `layout.conf`
pass between them.

**`repo-name`.** Real `config.py:500-505`: `layout.conf`'s `repo-name`
overrides the repo's name, applied onto `RepoConfig::name` after the
`profiles/repo_name` resolution below. (An earlier version of this
paragraph said the pilot used the `repos.conf` section name as
canonical and didn't model `profiles/repo_name` -- both since closed,
see the next section.)

**`profile-formats` gate.** Real `_config/LocationsManager.py:47`/`259`:
`_allow_parent_colon = frozenset(["portage-2"])` -- a profile `parent`
line's `:` cross-repo syntax is only expanded for a profile node whose
own repo declares `profile-formats = portage-2` in `layout.conf`.
`allow_parent_colon` *defaults* `True` and is only overridden when the
node intersects a known repo, so a node outside any repo keeps the
permissive default and a node inside a repo is gated. `resolve_config`
reads each repo's `layout.conf` directly (`repo_profile_formats`, a
tiny dedicated reader -- `portage-profile` can't depend on
`portage-repo`) and threads the allowed-repo-name set through
`resolve_profile_chain` -> `visit_profile` -> `expand_parent_colon`.
Real portage's EAPI-conditional `profile-formats` *default* when the
key is absent (`portage-1`/`portage-1-compat`) is not modeled -- absent
simply means "no `portage-2`".

`PORTING/fixtures/repo/metadata/layout.conf` gains `profile-formats =
portage-2` (its `profiles/default/parent`'s own `overlay:crossrepo-parent`
line -- shipped by an earlier slice -- keeps working). New overlay
`layoutmasteroverlay` (repos.conf section, **no** `masters` key) has a
`layout.conf` declaring `masters = overlay` + `repo-name = layoutrenamed`;
`dev-libs/layoutmasterpkg` exists only there and is masked only by the
`overlay` repo's own `profiles/package.mask` -- so it resolves to "no
ebuilds", proving the `layout.conf` masters tier feeds `package.mask`
stacking and the overlay loads under its `layout.conf` name. Rust unit
tests for `find_repos` (all three keys) and for the negative gate (a
`:` parent in a non-`portage-2` repo is left literal), a parametrized
contract case, and a dedicated contract test; mirrored in
`emerge_pretend_reference.py`.

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend dev-libs/layoutmasterpkg
# emerge: there are no ebuilds to satisfy "dev-libs/layoutmasterpkg".
#   -- layoutmasteroverlay masters `overlay` via its own layout.conf, so
#      overlay's package.mask entry for layoutmasterpkg applies
```

### `profiles/repo_name` as the canonical name, `aliases`, and the section-name mismatch drop

The follow-up to the `layout.conf` `repo-name` slice above (confirmed
with the user to port faithfully rather than deviate). Real
`_read_repo_name` (`config.py:670-688`): a repo's canonical name comes
from `<location>/profiles/repo_name` (its first line, trimmed) when
present, and only falls back to the `repos.conf` `[section]` name when
the file is absent. `find_repos` now reads that file; the name
precedence is `layout.conf` `repo-name` > `profiles/repo_name` > section.

`aliases` (real `config.py:216-224`/`492-499`): a repo's own
`layout.conf` `aliases =` (first) plus its `repos.conf` `aliases =`
(appended), stored on `RepoConfig::aliases`. This pilot acts on aliases
in exactly one place -- the mismatch escape hatch below;
`::alias`-constrained atoms and `alias:path` profile parents still use
the canonical name only (a documented cut).

The mismatch drop (real `config.py:1121-1136`): a repo whose resolved
name differs from its `repos.conf` `[section]` name is **dropped
entirely**, with a `!!! Section '<sect>' in repos.conf has name
different from repository name '<name>' set inside repository` error to
stderr -- *unless* the section name is one of that repo's aliases (the
real way to legitimately run two enabled copies of one repo under
distinct names). Ported faithfully, drop included -- not softened to a
warning. (`find_repos` is called at more than one layer per `--pretend`
run, so the error line can repeat; a pre-existing double-call, noted in
the code.)

`layoutmasteroverlay` (from the previous slice) gains `aliases =
layoutmasteroverlay` so its `repo-name = layoutrenamed` no longer
mismatches. New `repnamerepo` (`[repnamesection]`, `profiles/repo_name =
repnamefromfile`, `aliases = repnamesection`) -- `dev-libs/repnamepkg`
carries `::repnamefromfile`, not `::repnamesection`. Rust unit test for
`find_repos` (repo_name file source, alias-kept, mismatch-dropped), a
dedicated contract test for the canonical name, and a temp-tree
contract test for the drop + warning; mirrored in
`emerge_pretend_reference.py`.

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend \
    "dev-libs/repnamepkg::repnamefromfile"
# [ebuild  N    ] dev-libs/repnamepkg-1.0
#   -- resolves by the profiles/repo_name name, not the [repnamesection]
#      section name; the repo is kept only because it aliases the section
```

### `package.provided`: a package portage is told to treat as already installed

Real `config.py:970-1027` builds `pprovideddict` from every profile
level's own `package.provided` file (chain order) plus the user-level
`/etc/portage/profile/package.provided`, stacked with the same
`stack_lists(incremental=1)` `-atom` removal `package.mask`/`packages`
already use. Each line is a bare `cat/pkg-version` CPV. This pilot's
`portage_profile::Config` gains a flat `package_provided: Vec<String>`
(real cp-keys it into a dict, but `match_from_list` already filters by cp,
so the flat list is equivalent -- and it keeps `portage-profile`
dependency-free of `portage-dep`/`portage-versions`).

Two consumers in `resolve_pretend_graph`'s own BFS loop, right after the
blocker check, before any resolution:

- a **dependency** atom whose `cat/pkg` is listed and whose constraint one
  of that cp's provided CPVs satisfies is silently dropped from the walk
  -- no entry, no `required_by` edge (real `dep_check.py:1052` removes it
  from the deplist entirely);
- a **directly-requested** atom that matches is not resolved and is
  collected on `GraphResult::pprovided_atoms` (real
  `depgraph.py:5497-5615`'s `_pprovided_args`), which `pretend.rs` turns
  into real `depgraph.py:11192-11235`'s `WARNING: … listed in
  package.provided:` block -- to stderr, before the merge list,
  `bad("\nWARNING: ")` (red) + one `INFORM`-coloured (`darkgreen`) atom
  line per match, singular/plural phrasing on the count, exit `0`.

**Documented cuts**: the real EAPI 7+ gate (`allows_package_provided`
disallows `package.provided` for EAPI 7+) isn't ported -- this pilot
tracks no per-profile-level EAPI, consistent with its "no EAPI
parametrization within the 5+ floor" precedent (EAPI 5 -- what every
fixture profile is -- does allow it). Real portage validates each line
with `isvalidatom("=" + line)` and drops malformed ones with a warning;
this pilot carries every stacked line through and simply lets
`match_from_list` never match a malformed one. The pilot has no `SetArg`,
so the "pulled in by" ref is always `'args'` and real portage's
`@world`/`@selected` "A) B) C)" solution text is unreachable. Directory-
form `package.provided` (portage-1 `recursive`) is not read (no
`package.*` file in this pilot is).

New fixtures: `profiles/default/package.provided` lists
`dev-libs/providedpkg-1.0` + `dev-libs/providedpkg2-1.0` (both have
ebuilds in the repo), `dev-libs/needsprovided` RDEPENDs `providedpkg` +
`newpkg`. Rust unit tests in `portage-profile` (chain+user stacking with
`-atom` removal) and `portage-repo` (dep dropped / top-level recorded), a
dedicated pinned contract test, and 5 `CASES`; mirrored in
`emerge_pretend_reference.py`. **Motivation** (from the request): useful
for byte-for-byte comparison against a real system tree, where
`package.provided` is a real, common configuration (manually-built
toolchains, external kernel sources, …).

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend dev-libs/needsprovided
# [ebuild  N    ] dev-libs/needsprovided-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
#   -- needsprovided RDEPENDs dev-libs/providedpkg too, but that's in
#      package.provided, so the dep is silently dropped
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge --pretend dev-libs/providedpkg
#   (stdout empty; to stderr:)
# WARNING: A requested package will not be merged because it is listed in
# package.provided:
#
#   dev-libs/providedpkg pulled in by 'args'
```

### `emerge -pv`: `:slot`/`::repo` on the bracket cpv (and every `[old-ver]`)

Real `emerge -pv` runs at verbosity 3, which triggers
`output.py::_append_slot` + `_append_repository` on the bracket cpv and
`convert_myoldbest` on each `[old-ver]`:

- **`::repo` is always appended** (`_append_repository`, gated only on
  `quiet_repo_display`, whose default -- `--quiet-repo-display` not given
  -- is off). So `emerge -pv dev-libs/newpkg` is `[ebuild  N     ]
  dev-libs/newpkg-1.0::testrepo` (`testrepo` being this pilot's fixture
  repo).
- **`:slot` is appended** when the package's slot/sub-slot is other than
  `0/0`, or `new_slot` (`_append_slot`'s own `elif any(x.slot + "/" +
  x.sub_slot != "0/0" for x in oldbest_list + [pkg])`). `dev-libs/
  subslotpkg` (`SLOT="0/2"`) shows `subslotpkg-1.0:0/2::testrepo`;
  `dev-libs/newpkg` (`SLOT="0"`) shows no `:0`.
- **`/sub_slot` is appended** after `:slot` when the sub-slot differs.
- Every `[old-ver]` gets the same treatment with *its own*
  slot/sub_slot/repo: an `Upgrade` is `[ebuild     U  ]
  dev-libs/upgradepkg-2.0::testrepo [1.0::testrepo]`; a new-slot `New` is
  `[ebuild  NS    ] dev-libs/newslotpkg-2.0:1::testrepo [1.0:0::testrepo]`
  (real `myoldbest = installed_versions`, all slots, each with the old
  slot always shown under `new_slot`).
- Plain `emerge -p` (no `-v`) shows **none** of this -- the bare
  `cat/pkg-version` exactly as before.

`GraphEntry` gained `sub_slot`/`repo_name` (from the resolved
`Candidate`) and `oldbest: Vec<InstalledRef>` (`{version, slot, sub_slot,
repo}`), populated in `resolve_pretend_graph`: an `Upgrade`/`Downgrade`'s
own in-slot installed version(s), or every installed version for a
new-slot `New`. An installed package's repo is its vdb `repository`
file's first line, or `"__unknown__"` (real
`portage.versions._unknown_repo`) when absent -- so the fixture vdb
entries gained `repository` files (all `testrepo` except `newrepopkg` and
`samepkg`, which keep the states their own tests need). `pretend.rs`'s
own `emit`/`columns_line` gained `decorate_version` (real `_append_slot`
+ `_append_repository`), applied to the main cpv and each `[old-ver]`
only when `verbose`. ~25 `-pv` pinned contract assertions re-pinned; a
new `portage-repo` unit test (`sub_slot`/`repo_name`/`oldbest`
population) + a dedicated pinned contract test + 5 `CASES`; mirrored in
`emerge_pretend_reference.py`. **Motivation** (from the request): the
biggest remaining `emerge -pv` fidelity gap for byte-for-byte comparison
against real portage.

```sh
cd PORTING/rust && cargo build --release && cd ../..
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge -pv dev-libs/subslotconsumer
# [ebuild  N     ] dev-libs/subslotconsumer-1.0::testrepo
# [ebuild  N     ] dev-libs/subslotpkg-1.0:0/2::testrepo   <- :0/2 shown (SLOT="0/2")
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    PORTING/rust/target/release/portuale emerge -pv --update dev-libs/upgradepkg
# [ebuild     U  ] dev-libs/upgradepkg-2.0::testrepo [1.0::testrepo]
```

### `emerge -pc` / `-pP`: the `--depclean-lib-check` soname-consumer scan

Real `_calc_depclean` (`actions.py:1356-1590`) does not just trust the
dependency-graph cleanlist. After computing it, unless
`--depclean-lib-check=n` (the default `_DEPCLEAN_LIB_CHECK_DEFAULT` is
`True`), it scans every cleanlist package's `NEEDED.ELF.2`-recorded
libraries: if one is still linked against (`DT_NEEDED`) by a *surviving*
package that has no ebuild-level dependency on it, that provider is kept
installed anyway -- "In order to avoid breakage of link level
dependencies". This is what stops `emerge --depclean` from removing, say,
an old `openssl` an un-rebuilt binary still needs.

- The `#[allow(dead_code)]` `needed_elf` module (a from-scratch port of
  real `NeededEntry` + `LinkageMap.rebuild()` + `findConsumers()`, built
  in earlier preserve-libs slices but never wired to a caller) is now
  live: `pretend.rs::lib_consumer_scan` builds the linkage map from every
  installed `NEEDED.ELF.2`, and for each cleanlist package asks
  `find_consumers` (non-greedy -- so a consumer already satisfied by
  another provider of the same soname is excluded) which survivors still
  link its libs.
- A protected provider is fed back into a **second**
  `depclean_cleanlist` / `prune_cleanlist` pass as an extra reachability
  root (`lib_protected_providers`), so its own dependencies leave the
  cleanlist too (real `resolver._add_pkg` + `_complete_graph`), and
  `required_count` / `kept_parents` / the removal order all recompute
  consistently.
- Output matches real: `>>> Checking for lib consumers...` /
  `>>> Assigning files to packages...` / `>>> Adding lib providers to
  graph...` progress, and the `bad(" * ")`-prefixed WARNING with a
  per-provider `  <cpv> pulled in by:` / `    <consumer> needs <soname>`
  breakdown. `--depclean-lib-check=n` skips the scan and, with no
  package args, adds the `Depclean may break link level dependencies`
  advisory paragraph.
- Applies to `--prune` too (real `_calc_depclean` serves
  `action in ("depclean", "prune")`).

**Documented narrowings**: `find_consumers` is not clean-set aware, so
the pilot can under-report in the rare case where the only surviving
provider of a soname is itself another cleanlist member; the intermediate
`>>> Assigning files to packages...` line is printed only alongside the
WARNING (real can print it with all consumers satisfied elsewhere); and a
lib-protected provider contributes no `--verbose` reverse-dep line of its
own unless an ordinary dependency also reaches it (real labels it with
the link-level consumer). New fixture `_libcheck_root` (`dcconsumer`
links `libdclib.so.1` with no package dep on its orphan provider
`dclib`); 3 dedicated + 1 between-implementations contract test, a
`portage-repo` unit test, mirrored in `emerge_pretend_reference.py`
(which ports the same `needed_elf` subset rather than wrapping real
`LinkageMapELF`).

```sh
cd PORTING/rust && cargo build --release && cd ../..
# dcconsumer links libdclib.so.1 but nothing depends on dclib as a package
PORTING/rust/target/release/portuale emerge -pc            # keeps dclib, warns
PORTING/rust/target/release/portuale emerge -pc --depclean-lib-check=n  # removes it
```

### `emerge -pc` / `-pP`: the "dependencies could not be resolved" safety halt

Real `_calc_depclean` (`actions.py:1137-1248`) runs `unresolved_deps()`
after building the cleanlist: if any *kept* installed package has a hard
runtime dependency (`dep.priority > UnmergeDepPriority.SOFT` -- i.e.
`RDEPEND`/`PDEPEND`; `DEPEND`/`BDEPEND` are `buildtime` = SOFT and never
count) that nothing installed satisfies, it prints the `bad(" * ")`
`Dependencies could not be completely resolved due to the following
required packages not being installed:` block + the `emerge --update
--newuse --deep --with-bdeps=y @world` hint (`logging.ERROR` → stderr)
and **exits 1 without removing anything** -- "As a safety measure,
depclean will not remove any packages unless *all* required dependencies
have been resolved." Applies to `--prune` too (with the extra `use
--nodeps` trailer); `--prune --nodeps` skips `_calc_depclean` entirely so
it never halts.

`DepcleanResult` gains an `unresolved: Vec<(atom, parent_cpv)>` field,
filled by `unresolved_runtime_deps` during the reachability walk:
`use_reduce_structured` (USE-evaluated, `||`/`( )` structure kept) over
every kept package's `RDEPEND`/`PDEPEND`, flagging a plain atom that
matches no installed package. `run_depclean_pretend` /
`run_prune_pretend` check it right after the first cleanlist pass (real's
primary call site, before the lib scan) via `depclean_unresolved_halt`.

**Documented narrowings**: an atom inside a `||` group is not checked
(the any-of resolution needed to decide whether the *whole* group is
unsatisfiable is out of scope, and the reachability walk already keeps
every alternative so a partly-broken group never wrongly shrinks the
cleanlist); a libc-provider atom (real `find_libc_deps`) is never flagged
(real relies on libc genuinely being installed -- `strip_libc_deps`'s
premise -- so a fixture without an installed libc must not spuriously
halt); the real "show the unevaluated atom when it differs and vardb
matches it" readability case (`actions.py:1196`) is not reproduced. New
fixture `_unresolved_root` (`ukept` RDEPENDs a missing package); 4
contract tests + a `portage-repo` unit test; mirrored in
`emerge_pretend_reference.py`.

### `emerge -pv --getbinpkg`: remote binhost binary candidates + the `g` bracket column

The `--pretend` half of `--getbinpkg`/`-g` and `--getbinpkgonly`/`-G`
(real `main.py`, `y_or_n`). Real `emerge` adds every configured binhost's
own `Packages` index to the candidate pool alongside `$PKGDIR`
(`bintree._populate_remote`); this pilot reads each binhost's *cached*
index off disk (`<EROOT>/var/cache/edb/binhost/<host>/<path>/Packages`
for an `http(s)`/`ssh` `sync-uri`, the URI path itself for `file://`) and
resolves against it -- `--pretend` never downloads, so a binhost whose
cache is absent simply contributes nothing.

`portage-profile` gained `binrepos: Vec<BinRepo>` on `Config`, parsed by
`parse_binrepos` -- real `BinRepoConfigLoader`
(`lib/portage/binrepo/config.py:97-172`): every `[section]` in
`<config_root>/etc/portage/binrepos.conf` (`sync-uri`, optional
`priority`), then one implicit entry per whitespace-separated
`PORTAGE_BINHOST` URI not already a section's `sync-uri` (real "Convert
PORTAGE_BINHOST entries into implicit binrepos.conf ones", reversed with
an incrementing priority), sorted `(priority, name)`. Documented
narrowings: the implicit-entry name is `md5(uri)` in real portage (no md5
here -- the URI's own `host/path`, only ever a sort key); `[DEFAULT]`
interpolation, `getbinpkg-exclude`/`-include`, `fetchcommand`/
`resumecommand`, signature config, and the `location =` fallback are all
out (none affect a `--pretend` resolution).

`portage-repo`: `list_remote_binary_candidates` scans each `BinRepo`'s
`packages_dir()`, marking every candidate `remote: true` and dropping any
cpv+version the local `$PKGDIR` already carries (real `bintree.isremote`
-- once downloaded a package is no longer "remote"). `Candidate` gained
`remote`; `GraphEntry` gained `remote_binary`, flowing to real
`output.py:648`'s own `attr_display.remote_binary = pkg.remote` -- the
`g` character in the `f`/`F` bracket slot (`[binary  N g  ]`). A
remote-binary entry also gets its download `SIZE` from the index
(`read_binary_metadata_any` -- local `$PKGDIR` first, then each binrepo),
feeding both the verbosity-3 per-line ` N KiB` suffix (real
`output.py::verbose_size`) and the `Size of downloads:` counter (real
`bindbapi.getfetchsizes`). `REPO` from the index entry becomes the
`::gentoo` decoration at `-pv` (falling back to `__unknown__`); `:slot`/
`sub_slot` decoration applies to a `[binary ... g]` line like any other.
The `--getbinpkg` family folds into `--usepkg`/`--usepkgonly` for pool
eligibility (real depgraph treats a binhost package like a `$PKGDIR`
one), with `getbinpkg` additionally switching on remote-index loading, so
`--usepkg` alone still never reaches a binhost.

New fixtures: `PORTING/fixtures/etc/portage/binrepos.conf` (one
`file://` `[testbinhost]`) + `PORTING/fixtures/binhost/Packages`
(`dev-libs/remotebinpkg-1.0`, and `dev-libs/remotebinslotpkg-1.0` with
`SLOT=2/1`) -- both binhost-only, no ebuild, no `$PKGDIR` entry. 12
contract `CASES` + 2 dedicated pinned-output contract tests + 3
`portage-{profile,repo}` unit tests; mirrored in
`emerge_pretend_reference.py`. Still out of scope: an actual remote
download / `layout.conf` negotiation / `gpkg`, and `--getbinpkg` for a
real (non-`--pretend`) merge.

### `emerge -p`: the blocker line follows real `ResolverOutput._blockers`

The pilot's blocker report was pilot-shaped (`[blocks] cat/foo-1.0 hard
blocks cat/bar-2.0 ("!!cat/bar")`, printed inline right after its
owner's `[ebuild …]` line). Now it ports real `ResolverOutput._blockers`
(`lib/_emerge/resolver/output.py:75-123`):

```
[blocks B     ] dev-libs/samepkg ("dev-libs/samepkg" is hard blocking dev-libs/blockerpkg-1.0)
```

the fixed-width `[blocks B     ]` bracket (`B` + 5 spaces + a 6th mask-
column space at `-v`, real `empty_space_in_brackets`); the `!`-stripped
atom (real `dep_expand(str(atom).lstrip("!"))` — category-qualification
only, and every pilot blocker atom is already `cat/pkg[…]`); then
`("<atom>" is {hard,soft} blocking <parent cpv>)` — `hard` for a `!!`
blocker (real `blocker.atom.blocker.overlap.forbid`), `soft` for `!`.
Under `--color=y` the `blocks` word, the `B`, the atom, and the
parenthetical are each `colorize("PKG_BLOCKER", …)` — style `red`
(`\x1b[31;01m`); the teal `b` / `PKG_BLOCKER_SATISFIED` branch is
unreachable here (this pilot only ever *reports* a blocker, never
resolves one away, so `blocker.satisfied` is always false), as is real's
`(is <desc> <parents>)` alternative (`resolved` drops the `!` while
`blocker.atom` keeps it).

Blocker lines are now also **collected and printed as one group after
every package line** (real `Display.display` → `print_messages()` then
`print_blockers()`), not interleaved — before the `-v` counters line.
New fixture `dev-libs/blockerorderpkg` (`RDEPEND="!!dev-libs/samepkg
dev-libs/newpkg"` — its blocker's owner is the first graph entry, a
plain dep follows) proves the deferral. 8 contract `CASES` + 4 dedicated
pinned-output contract tests (including the exact `--color=y` ANSI
codes); mirrored in `emerge_pretend_reference.py`. This closes the
blocker half of the `-pv` layout gap; only `--autounmask` message colour
is left (its own future slice — that text is pilot-invented, not a port).

### `emerge -p`: an installed dependency's USE-dep checked against its *built* USE (bug 640318)

When a dependency atom carries a USE-dep (`cat/pkg[flag]`) and
`cat/pkg` is already installed, `dependency_avoid_update_candidate`
checks that USE-dep against the installed version's own real vdb
`USE`/`IUSE` (not the current tree's). The *valid-flag domain* for that
check now follows real `dbapi._iuse_implicit_cnstr` for a built package
on an EAPI 5+ (`iuse_effective`): the recorded `IUSE`, unioned with the
profile's `IUSE_EFFECTIVE` (`valid_iuse` — `elibc_*` etc), **and every
flag the package was actually built with** (real `_iuse_implicit_built`'s
own `flag in use` clause, [bug 640318](https://bugs.gentoo.org/640318) —
a built package's own `USE` is authoritative for what counts as a valid
flag, independent of the profile's current `IUSE_IMPLICIT` or an ebuild
that has since dropped a flag from its `IUSE`). Real `_match_use`
recomputes the domain this way rather than reading a vdb `IUSE_EFFECTIVE`
file, so the pilot not persisting one is not a gap here.

New fixture `dev-libs/builtusedivergedep` (installed 1.0 with vdb
`USE="divergedflag"` but vdb `IUSE=""`, and the current ebuild has
*dropped* `divergedflag` from its `IUSE`) + `dev-libs/needsbuiltusediverge`
(`RDEPEND="dev-libs/builtusedivergedep[divergedflag]"`). Before: the
dependency spuriously hit `!!! no visible ebuild` (nothing in the tree
can satisfy `[divergedflag]`); now the installed version satisfies it and
the dependency is kept as installed. The same atom as a *top-level*
target still fails — the avoid-update-against-vdb path is dependency-only.
2 contract `CASES` + 1 dedicated pinned test + 1 `portage-repo` unit test;
mirrored in `emerge_pretend_reference.py`.

### `$PKGDIR` directory-scan fallback + the `gpkg`/`xpak` metadata readers

Every binary-package path in this pilot so far is `<pkgdir>/Packages`-
index-driven and format-agnostic: `portage-repo` never opens a binpkg
file, so a `gpkg` (`.gpkg.tar`) listed in the index already resolves for
`--pretend` exactly like an `xpak` `.tbz2` (verified: `PATH:
…gpkg.tar` → `[binary … g]`, `::repo`, `SIZE`, recursive dep walk, all
unchanged). What the pilot **can't** do is the real
`bintree._populate_local` fallback: scan `$PKGDIR` for binpkg *files*
when there is no `Packages` index and rebuild it from each file's own
embedded metadata. That needs a real per-format metadata reader.

This is the `gpkg` half. New `portuale/src/binpkg.rs` ::
`read_gpkg_metadata` ports real `portage.gpkg.gpkg.get_metadata()` /
`unpack_metadata(want=None)` (`lib/portage/gpkg.py:838-870`): a
`.gpkg.tar` is a plain tar container with `<basename>/{gpkg-1,
metadata.tar[.<comp>], image.tar[.<comp>], Manifest}` members; the reader
unpacks the outer container, classifies the `metadata.tar[.<comp>]`
member exactly as real `_extract_filename_compression`
(`gpkg.py:2176` + `ext_list`) does, decompresses it via the same
`_compressors` decompress argv real portage uses (all seven —
`gzip`/`bzip2`/`lz4`/`lzip`/`lzop`/`xz`/`zstd`), unpacks the inner
`metadata.tar`, and returns the `metadata/<KEY>` → value map (real
`_strip_metadata_prefix`). It shells out to `tar` + the decompressor
rather than parsing the archive natively or adding a Rust
tar/compression crate — consistent with every other real-execution path
here (`wget`, `ldconfig`, `scanelf`, `bash`/`brush`, the compressors
`ebuild_package.rs` already invokes), and `tar` + these compressors are
hard Gentoo requirements anyway (real `gpkg.py` is `tarfile` + the exact
same compressor subprocesses).

**v1 cuts** (matching this pilot's own `Packages`-index reader, which
"trusts the index outright" — real `pkgdir-index-trusted`): NO `Manifest`
digest verification and NO GPG `.sig` signature check (real
`gpkg._verify_binpkg`). Those are `gpkg`'s whole point, but this pilot
has no crypto anywhere and its `--pretend` binary path has never verified
a binpkg's integrity — a separately-scoped follow-up. The `gpkg-1`
version-marker presence check is still enforced (real
`_get_inner_tarinfo`'s `InvalidBinaryPackageFormat` guard).

New fixture `PORTING/fixtures/pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar` —
a real, hand-built container (`tar` + `zstd`, real member layout). 3
`portuale` unit tests; also verified by hand against a real-world
`/var/cache/binpkgs/*.gpkg.tar` (with `.sig` members, `environment.bz2`,
a build-id basename).

**Increment 2** — the `xpak` (`.tbz2`) reader — adds
`binpkg::read_xpak_metadata`, porting real `portage.xpak.tbz2`'s own
`scan` + `getindex_mem`/`searchindex` (`lib/portage/xpak.py:395-460` /
`234-266`). An `xpak` binpkg is `[image tarball]` immediately followed by
a fully self-describing trailer — `"XPAKPACK" be32(indexsize)
be32(datasize) <index> <data> "XPAKSTOP" be32(infosize) "STOP"` — so the
reader parses it in **pure Rust** (no `tar`, no subprocess), reading only
the bounded `infosize + 8` file tail; the image tarball itself is never
touched. `<index>` is a flat run of `be32(namelen) name be32(datapos)
be32(datalen)` records into `<data>`; every metadata key is one record.
`CONTENTS` is never present in a *binary* package's own xpak (real
`xpak()` skips it — it's a merge-time artifact). Codec-agnostic (the
trailer is raw whatever compressed the tarball). New committed fixture
`PORTING/fixtures/pkgdir/dev-libs/packagepkg-1.0.tbz2` — a genuine
`.tbz2` built once by the pilot's own `ebuild <file> package` (real
`xpak-helper.py recompose` → real `xpak.py`) rather than rebuilt
per-test (the read side needs no reproducible bytes, and driving the
full brush phase chain in a unit test adds parallel-load pressure to the
suite's brush-heavy tests for no reader-coverage gain). 3 more `portuale`
unit tests (a synthetic multi-key XPAK segment, the committed real
`.tbz2`, a no-trailer rejection).

**Discovered while writing the real-`.tbz2` test**: this pilot's own
`build-info` generation is a *subset* of real portage's — it writes
`EAPI`/`SLOT`/`CATEGORY`/`PF`/`KEYWORDS`/`USE`/`DEFINED_PHASES`/
`BUILD_TIME` + the bundled `<pf>.ebuild` + `environment.bz2`, but **not**
the dependency-string files (`DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/
`IDEPEND`/`IUSE`/`LICENSE`/`PROPERTIES`/`RESTRICT`/`INHERITED`/
`IUSE_EFFECTIVE`/`SIZE`/`PROVIDES`/`REQUIRES`/…). Those come through into
the pilot's `Packages` index anyway (from `md5-cache`), so `--pretend`
binary resolution is unaffected — but a `$PKGDIR` scan of a pilot-built
`.tbz2` would see an incomplete candidate. Tracked as its own
`ebuild_package.rs` / phase-execution follow-up, orthogonal to these
readers.

**Increment 3** — the `$PKGDIR` directory scan, wired in. New
`binpkg::scan_pkgdir` walks `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}` (one
level deep — `$PKGDIR`'s real layout), reads each file with the matching
reader, and synthesizes one `Packages`-style entry per file (`CPV` from
the path, `SIZE` from the file's own byte size, `REPO` from the embedded
`repository`, `PATH`, `CPV`-sorted for a deterministic pool). `pretend.rs`
runs it once, after flag parsing, only when `--usepkg`/`--usepkgonly` is
given and `<pkgdir>/Packages` is *absent*; the result lands on the new
`portage_profile::Config::scanned_binpkgs` field. **Not written back to
`Packages`** — real portage caches it there, but that would make
`--pretend` mutate `$PKGDIR` and break contract-suite determinism, so
this pilot recomputes each run (same "recompute, don't persist" stance
as not persisting vdb `IUSE_EFFECTIVE`).

The plumbing decision (`portage-repo` is deliberately subprocess-free —
`scan_pkgdir` shells out) landed as a **refactor**: a new
`portage_repo::BinaryIndex` value (`from_pkgdir` — parse
`<pkgdir>/Packages`; `from_entries` — the scan's synthesized entries)
threads through every binary-candidate function
(`list_binary_candidates`/`read_binary_metadata`/
`list_remote_binary_candidates`/`read_binary_metadata_any`/
`rebuilt_binary_changed`) in place of a re-read `pkgdir: &Path`, so "read
the index file or scan the files" is decided exactly once
(`local_binpkg_index`, from `config`). The scan itself stays in
`portuale`. A present `Packages` is always trusted as is — no
mtime-staleness revalidation (real `FEATURES=pkgdir-index-trusted`
behavior, this pilot's long-standing stance for the index).

v1 cuts: bare `.xpak` files (real `binpkg-multi-instance`
`<pkgdir>/<cat>/<pf>/<build_id>.xpak`) are skipped — no multi-instance
concept here, and a bare `.xpak` is a different on-disk shape; a file
that fails to parse aborts the scan (rather than real portage's own
skip-and-warn — a `$PKGDIR` of unreadable binpkgs is worth surfacing).
The Python mirror scans too (`_scan_pkgdir` — `portage.xpak.tbz2` for
`.tbz2`, a hand-rolled `tarfile` + decompressor-subprocess reader for
`.gpkg.tar` that matches the Rust reader's cuts, since real
`portage.gpkg` would reject a `Manifest`-less container). New contract
test builds an ad-hoc `$PKGDIR` with no `Packages` holding both fixture
binpkgs and resolves each under `--usepkgonly`, Rust ≡ Python; a
regression test confirms the committed `PORTING/fixtures/pkgdir` (which
has a `Packages` *and* the two loose fixture files) still resolves via
the index alone. 3 more `portuale` + 1 `portage-repo` unit tests.

### `build-info` metadata generation: a merged vdb entry / built `.tbz2` carries its real dependencies

The `$PKGDIR`-scan work above surfaced this: a package the pilot *itself*
built or merged carried **no dependency metadata**. `bin/phase-functions.
sh __dyn_install` (run unmodified) writes `${PORTAGE_BUILDDIR}/build-info/
{CATEGORY,SLOT,KEYWORDS,IUSE,USE,EAPI,DEFINED_PHASES,DESCRIPTION,…}` — but
not `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/`IDEPEND`/`LICENSE`/
`PROPERTIES`/`RESTRICT`; in real portage the *Python* side fills those in.
And this pilot's `write_vdb_entry` only ever copied a hardcoded
`{CATEGORY,SLOT,repository,CONTENTS,COUNTER,NEEDED.ELF.2}` subset.

New `ebuild_phases::write_post_install_metadata` ports real
`doebuild.py::_post_src_install_write_metadata`
(`doebuild.py:2700-2782`): after a successful `src_install` (and its
post-phase `misc-functions.sh`), write those keys into `build-info`,
USE-conditional-evaluated (`use_reduce_structured` — real
`paren_enclose(use_reduce(v, uselist=use))`) against the pilot's empty
phase-side USE set (the same stance `crate::fetch` documents). Source is
the ebuild's own `metadata/md5-cache` entry (real `settings.configdict
["pkg"]`). And `write_vdb_entry` now copies **every** regular file from
`build-info` into the vdb entry (real `treewalk()`,
`vartree.py:4911-4913`) — so a pilot-merged package's vdb dir carries
`RDEPEND`/`EAPI`/`KEYWORDS`/`environment.bz2`/the `<PF>.ebuild` copy/…
like a real one, and `ebuild <file> package`'s `.tbz2` (whose XPAK is
built from `build-info` by the real `xpak-helper.py`) gets the dep
strings too.

v1 cut: real portage, for an EAPI with slot operators (every EAPI 5+),
writes the `*DEPEND` files from `evaluate_slot_operator_equal_deps`
(which binds `:=` against the resolved depgraph) rather than this loop.
This pilot does no build-time `:=` binding anywhere, so it writes the
plain `use_reduce`'d `*DEPEND` — byte-identical for an ebuild with no
`:=` operator, the bare `:=` token kept for one with. `IUSE_EFFECTIVE`
isn't written either (needs a resolved `Config` threaded through the
phase chain; the vdb `IUSE_EFFECTIVE` file is only read by an
already-narrowed check). Real-execution, Rust-only. New
`ebuild_merge`/`binpkg` tests; the committed
`fixtures/pkgdir/dev-libs/packagepkg-1.0.tbz2` was regenerated (it now
carries `RDEPEND`), which the `$PKGDIR`-scan contract test now asserts is
actually walked.

### Profile `parent` lines resolve an aliased repo name

`repos.conf` / `layout.conf` `aliases =` were parsed
(`RepoConfig::aliases`) but wired nowhere. A profile `parent` line
`<name>:some/path` now resolves `<name>` through the alias map when it
isn't a canonical repo name — real `LocationsManager._expand_parent_colon`
looks the token up via `repositories.get_location_for_name`, which is
keyed on aliases as well as canonical names. `resolve_config` gained a
`repo_aliases: &[(String, PathBuf)]` param (every repo's aliases × its
location), threaded to `expand_parent_colon`, which checks the canonical
`repos` list first then `repo_aliases` (an alias never shadows a
canonical name — real `config.py`'s alias-registration loop skips an
already-taken name).

**Not a gap, and deliberately left as-is: an atom's own `::alias`.** Real
`match_from_list` filters `::repo` with a straight `pkg.repo ==
atom.repo` name comparison (`dep/__init__.py:3201`) — no alias step — so
`emerge cat/pkg::somealias` finds nothing. The pilot already matched
this on both sides (the Python reference calls the real
`portage.dep.match_from_list`; the fixture
`dev-libs/repnamepkg::repnamesection` — an alias — is rejected by Rust
and Python identically). Adding alias resolution to the Rust
`matches_repo` would *diverge* from real portage.

New contract test builds an ad-hoc config tree whose main profile's
`parent` names an overlay by its alias (`ovl:shared`, canonical
`otherrepo`) and asserts the aliased-in `USE=aliasflag` reaches `-pv`
output, Rust ≡ Python; +1 `portage-profile` unit test (alias resolves,
and an unregistered alias gets the same "no repo named" error).

### `emerge -pv`: the `USE="…"` flag list is natural-sorted

Real `output_helpers.py::_alnum_sort_key` (`any_iuse.sort(key=
_alnum_sort_key)` in `_create_use_string`): split each flag on runs of
digits and compare the digit runs as numbers, not lexically — so
`python3_9` sorts *before* `python3_12` (`9 < 12`), not after (`"9" >
"12"`). The pilot's flat list was plain lexicographic. New
`portage_repo::alnum_sort_key` (+ an `AlnumKey` `Str`/`Num` enum,
`u128`-or-`Str`-on-overflow) applied at all three flag-sort sites — the
`display` list, the removed-from-IUSE list, and `pretend.rs`'s
`--alphabetical` combined list. New `dev-libs/naturalsortpkg` fixture
(`IUSE="+n2 +n9 +n10"`) → `USE="n2 n9 n10"`. 2 contract `CASES` + 1
pinned test + 1 `portage-repo` unit test; mirrored in
`emerge_pretend_reference.py` (`_alnum_sort_key`). Closes SCOPE_BACKLOG
Part 2.A item 2 residual (b).

### `emerge --pretend --autounmask`: real keyword *resolution* + the "necessary to proceed" block (increment 1)

Until now, `emerge --pretend --autounmask <keyword-masked-pkg>` *failed*
with a pilot-invented `there are no ebuilds to satisfy … note: … suggests
adding …` hint. Real portage doesn't do that: when `--autounmask` can
find a consistent set of changes, it *resolves the graph as if those
changes were applied*, shows the normal merge list, then prints the
`The following <X> changes are necessary to proceed:` block (real
`depgraph.py::_display_autounmask`, `:10625`) — and `emerge --pretend`
exits **0** (real `actions.py:563`). This increment ports that for the
**keyword** kind.

`resolve_pretend` grew an `autounmask_keywords` param: when set and the
normal visibility filter finds nothing, a candidate masked by `KEYWORDS`
*alone* (`keyword_masked_only` — `package.mask`/license/properties/
restrict all still have to pass) is treated as visible, the implicit
`=cpv ~arch` change real portage would apply. The resulting entry's
`[ebuild N ~]` marker already reflected it (`keyword_mask_marker` keys
off the candidate's own `~<arch>` token, not `package.accept_keywords`).
`resolve_pretend_graph` records each such resolution as an
`AutounmaskChange` (`cpv`, `token`, and a `#required by …` dep chain —
real `_get_dep_chain_as_comment`: `required by <atom> (argument)` for a
command-line target, `required by <parent cpv>::<repo>` then `required
by <parent atom> (argument)` for a dependency), on the new
`GraphResult::autounmask_keyword_changes`. `pretend.rs` prints the
block to stderr after the merge list — real `_writemsg`'s
`\nThe following <BAD>keyword changes</BAD> are necessary to proceed:\n
 (see "package.accept_keywords" …)\n`, then `format_msg` (the
`#`-prefixed dep-chain lines stay plain, the `=<cpv> <kw>` line is
`INFORM`-green). `--pretend` deliberately omits real portage's `Use
--autounmask-write …` hint (`:11084` `not pretend`). `--json` gains a
top-level `"autounmask_keyword_changes"` array.

Gating is unchanged: `--autounmask` must be *explicit* for keyword
changes (real `--autounmask-keep-keywords` defaults to keep). Without it,
a top-level keyword-masked atom is still fatal and a keyword-masked
dependency still gets the `!!! no visible ebuild` line. Both sides;
existing fixtures `dev-libs/autounmaskkeywordpkg` / `autounmaskdepconsumer`.
6 contract tests updated (fail-and-hint → resolve-and-block), 3
`portage-repo` unit tests. SCOPE_BACKLOG Part 2.G item 14 / Part 1 #17.

### `emerge --pretend`: real `--autounmask-use` USE *resolution* + the `-pv` USE line (increment 2)

The USE half. Unlike keywords, `--autounmask-use` is **on by default**
(real `create_depgraph_params` — no `--autounmask-keep-keywords`-style
asymmetry), so `emerge -p 'dev-libs/foo[-bar]'` where `bar` is
default-enabled now *resolves* with an implicit `package.use` flip
rather than failing — exactly real portage's default.

`resolve_pretend` grew an `autounmask_use` param: the USE-dep post-filter
keeps a candidate whose atom use-deps its default USE state doesn't
satisfy *if* a `package.use` flip would fix it (`suggested_use_flip` is
`Some` — flag must be in IUSE and not mask/force-blocked). The graph
layer then applies the flip to that entry's effective `use_flags`
**once**, before the `-pv` USE display, the REQUIRED_USE check and the
dependency walk — matching real `_pkg_use_enabled`. So `-pv` shows the
adjusted `USE="-bar …"` (for a `New` entry real `_create_use_string`
renders an autounmask-flipped flag exactly like any normally-set flag —
`is_new=True`, no `*`/`%` marker), and a `foo? ( … )` group keyed off
the flipped flag fires the new way.

The change is recorded on `GraphResult::autounmask_use_changes` and
printed as the `The following USE changes are necessary to proceed:`
block (`(see "package.use" …)`), after the keyword block. The left-hand
atom is `>=<cpv>` / `>=<cpv>:<slot>` / `=<cpv>` per real
`check_if_latest(check_visibility=True)` (`autounmask_use_atom_form`) —
real portage uses `>=` for USE, unlike keywords' bare `=` (bug #536392).
`AutounmaskChange` now carries the op prefix in its `atom` field (`--json`
field renamed `cpv` → `atom`); `--json` gains `autounmask_use_changes`.

`--autounmask-use=n` restores the strict "USE-dep mismatch → no visible
candidate" behavior (the `test_use_dep_enforcement_*` contract tests now
pass it to keep testing raw matching). Both sides; existing
`dev-libs/useflagpkg` / `usedeprejectedpkg` / `useeqparentoffpkg`
fixtures. ~10 contract tests updated, 3 `portage-repo` unit tests.

### `emerge --pretend`: real `--autounmask-use` `opt=` *parent* flip (increment 3)

Closes the `--autounmask-use` buildout. Real `_apply_parent_use_changes`
→ `_show_unsatisfied_dep(collect_use_changes=True)` (`depgraph.py:5820`/
`6768`): a dependency atom's use-dep was originally conditional on the
*requesting parent's* own USE (`opt?`/`opt=` forms — `cat/pkg[flag=]`
means "child's `flag` must match the parent's `flag`"), the parent's
current USE evaluates it to a concrete constraint no candidate can
satisfy, **and** a child-side `package.use` flip is impossible because
the child's own flag is `use.mask`'d/forced. Real portage then flips the
*parent's* conditional flag instead (dropping the constraint) and
re-resolves.

The pilot already *computed* `suggested_parent_use_candidate` (flip the
parent flag, re-evaluate the atom, verify it now resolves, check the
parent's REQUIRED_USE) — this increment *acts* on it. In the BFS loop,
when a dependency comes back `NoVisibleCandidate` and the parent flip
would fix it, `resolve_pretend_graph` re-evaluates the unevaluated atom
with the parent's flipped USE, re-resolves the freed dependency, records
`>=<parent-cpv> -flag` on `GraphResult::autounmask_use_changes` (printed
in the same "necessary to proceed" USE block, with the *parent's* own
one-level dep chain), and re-renders the parent entry's own `USE=` line
to match. `--autounmask-use=n` suppresses it via the shared
`autounmask_suggest_use` gate.

New fixtures: `dev-libs/parentflipchildpkg` (IUSE `feat`, `feat`
`use.mask`'d via `profiles/base/package.use.mask`) and
`dev-libs/parentflipeqpkg` (IUSE `+feat`, RDEPEND
`parentflipchildpkg[feat=]`). `emerge -p dev-libs/parentflipeqpkg` now:

```
[ebuild  N     ] dev-libs/parentflipchildpkg-1.0  USE="(-feat)"
[ebuild  N     ] dev-libs/parentflipeqpkg-1.0  USE="-feat"

The following USE changes are necessary to proceed:
 (see "package.use" in the portage(5) man page for more details)
# required by dev-libs/parentflipeqpkg-1.0::testrepo
# required by dev-libs/parentflipeqpkg (argument)
>=dev-libs/parentflipeqpkg-1.0 -feat
```

Deliberate cuts, confirmed against the "one change per blocked atom, no
real backtracking" boundary the earlier increments set: this pilot
re-resolves only the *freed dependency*, not the whole graph (a parent
`feat? ( other-dep )` sibling dep that the flip would also drop stays in
the list); the parent's own dep chain is its one-level chain (a parent
that is itself a deep dependency would need real `_get_dep_chain`'s full
walk); and a non-`New` parent (Upgrade/Reinstall) re-renders its USE
line as if `New` (no installed-diff markers). `--autounmask-license` is
the one remaining unstarted `--autounmask*` member.

Both sides; 2 contract tests + 2 `CASES`, 2 `portage-repo` unit tests.

### `emerge -pc <atoms> --deselect=n`

Real `action_depclean`: `deselect = myopts.get("--deselect") != "n"`
(default `True`). In args mode, `if deselect:` empties `required_sets
["selected"]` (`actions.py:1037-1042`) so a package named as an arg that
is *also* in `world` still gets removed (and, non-`--pretend`, dropped
from `world`). `--depclean <atoms> --deselect=n` skips that — the world
set stays a protection root, so a world member named as an arg is
**kept**.

`depclean_cleanlist` gained a `deselect: bool` param (the world seeds
are used as roots when `args.is_empty() || !deselect`, not just
`args.is_empty()`); `pretend.rs` tracks a `deselect_n` flag (`--deselect
n` / `--deselect=n`) and passes `!deselect_n` through. The same slice
fixed a related bug: `--deselect` (bare / `y`) alongside `--depclean` /
`--prune` / `--unmerge` was wrongly routing to the standalone deselect
*action* — real `main.py` only makes `--deselect` an action when
`myaction is None` (the other three set their action first), so the
dispatch is now gated on `!depclean && !prune && !unmerge`.

`emerge -pc dev-libs/dcworld` (a world member nothing needs) → removed
(`Number to remove: 1`); `-pc dev-libs/dcworld --deselect=n` → `>>> No
packages selected for removal by depclean` (`Number to remove: 0`).
3 contract `CASES` + 1 pinned test + 1 `portage-repo` unit test; both
sides.

### `emerge -pC` / `-pP`: the higher-slot set-protection refinement

Real `unmerge.py:421-441`: the "still listed in the following package
sets" warning is suppressed for a set when an installed *newer* version
of the same cp *in a different slot* also matches that set's atom —
removing the version being unmerged still leaves the set satisfied by
the other one. (Real portage's `higher_slot`: it walks the atom's
installed matches in descending order, stops at `pkg`, and takes the
first `> pkg` version whose `slot_atom` differs.)

New shared `still_listed_parents(root, installed_sets, cat, pkg,
version)` (`pretend.rs`, mirrored `_still_listed_parents`) — used by
**both** `run_unmerge_pretend` and `run_prune_pretend`, matching real
portage's single `_unmerge_display` handling every `unmerge_action`.
For each set atom that matches the selected `cat/pkg-version`, it now
also checks `installed_candidates` for a `vercmp`-newer instance in a
different slot that matches the atom (built with a `:slot/sub_slot`
suffix so a slotted set atom resolves correctly); a covered atom
doesn't make its set a "parent".

New fixture `dev-libs/dualslotpkg` installed in slot 1 (`1.0`) and slot
2 (`2.0`), with `etc/portage/sets/dualslotset` (bare
`dev-libs/dualslotpkg`, selected via `world_sets`): `emerge -pC
'dev-libs/dualslotpkg:1'` prints **no** warning (slot 2 covers the set
atom), `-pC 'dev-libs/dualslotpkg:2'` prints the warning (nothing
higher). This slice also recorded the one remaining `_unmerge_display`
cut — the "currently used Python interpreter" self-skip (real
`_dblink(cpv).isowner(portage._python_interpreter)`) — as a **non-gap**
for this pilot: its `emerge` is a Rust binary with no Python interpreter
of its own to protect. 2 contract `CASES` + 1 pinned test + 2
`pretend.rs` unit tests; both sides.

```sh
FX="$(realpath PORTING/fixtures)"
# slot 1 (1.0): slot 2 (2.0, higher) still matches the bare
# dev-libs/dualslotpkg atom in @dualslotset -> no warning
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pC 'dev-libs/dualslotpkg:1' | grep -c 'still listed'
# 0
# slot 2 (2.0): nothing installed higher -> the warning fires
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pC 'dev-libs/dualslotpkg:2'
# ...
# Package dev-libs/dualslotpkg-2.0 is going to be unmerged,
# but still listed in the following package sets:
#     dualslotset
```

### `emerge -p`: the bracket mask column is present at plain `-p` too, not only `-v` (real-tree finding)

Running `portuale` against a real Gentoo tree in the container turned up
a one-column width difference from real `emerge`: portuale printed
`[ebuild   R   ]` (13 inside the brackets) where real portage printed
`[ebuild   R    ]` (14). The pilot had gated the 7th `PkgAttrDisplay`
column — the `#`/`~`/`*`/space mask marker from `output.py::gen_mask_str`
— on `-v`, per `include_mask_str()` = `verbosity > 1`. But real portage's
**default** `emerge -p` verbosity is **2**, not 1
(`_DisplayConfig.__init__`: `"--quiet" and 1 or "--verbose" and 3 or 2`)
— so `include_mask_str()` is already true at plain `-p`, and the column
is absent only under `--quiet` (verbosity 1), which this pilot doesn't
model at all.

`attr_display_field` (`pretend.rs`, mirrored `_attr_display_field`) now
always renders the 7th column; `format_blocker_lines`' own
`empty_space_in_brackets()` pad (real `output.py:90`) is likewise always
6 spaces after `B`, not 5. A side effect worth noting: keyword/hard-mask
markers that were previously invisible without `-v` now show at plain
`-p` — `[ebuild  N    ~] dev-libs/autounmaskkeywordpkg-1.0`, `[ebuild  N
   #] dev-libs/overlaymaskedpkg-1.0`, `[ebuild  N    *]
dev-libs/livekeywordpkg-9999` — matching real `emerge -p`. Both sides;
~240 pinned-output test assertions widened by one column, 1 new
`pretend.rs` unit test, and `test_pv_bracket_mask_marker` rewritten to
assert the marker at plain `-p` too. After the fix, portuale's non-`-v`
bracket width matches real `emerge -p` on a real tree.

Not addressed here (a separate gap, its own slice below — "the `USE="…"`
line shows at plain `-p`"): at verbosity 2 real portage *also* prints
the `USE="…"` line for a **changed** flag set or a new package
(`_create_use_string` only returns "" when nothing changed *and*
`all_flags` is off), where the pilot still gates the whole `USE=`
display on `-v`.

```sh
FX="$(realpath PORTING/fixtures)"
# plain -p: the 7th (mask) column is a bare space for an ordinary pkg...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -p dev-libs/newpkg
# [ebuild  N     ] dev-libs/newpkg-1.0
# ...and a real marker for a keyword-/hard-masked one, no -v needed:
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -p --autounmask dev-libs/autounmaskkeywordpkg
# [ebuild  N    ~] dev-libs/autounmaskkeywordpkg-1.0
```

### `emerge -p`: the merge list is in real dependency-first order (real-tree finding)

The same container run that turned up the mask-column gap showed portuale
listing packages in the wrong order: `emerge --pretend app-misc/tmux`
printed `tmux` *before* its dependency `libevent`, where real `emerge`
prints `libevent` first. Real portage's `mylist` (the list
`resolver/output.py::Display` renders) is a genuine topological **merge
schedule** — its `Scheduler` installs every package only after the
packages it depends on. The pilot's BFS builds `entries` the opposite
way: a package's entry is appended *before* its dependencies are ever
queued.

`resolve_pretend_graph` (mirrored `resolve_pretend_graph` in the Python
reference) now re-sorts `entries` into dependency-first order as its last
step, via new `topological_merge_order` / `_topological_merge_order`: a
**stable** topological sort keyed off the `required_by` edges every entry
already carries — a dependency always precedes the packages that pull it
in, and two packages with no dependency relationship keep their
BFS-discovery (RDEPEND-string / argv) order. A genuine dependency cycle
(real portage's `Scheduler` breaks these with slot-operator/priority
heuristics the pilot doesn't reproduce) is left in discovery order.

The chosen model (confirmed with the user via `AskUserQuestion`, "Model
A"): `entries` is *canonically* merge-ordered — one order for the flat
`--pretend` list, `emerge --buildpkgonly`'s build loop, and the `--json`
`entries` array alike. `--json` additionally stamps each entry with an
explicit `"merge_order"` integer (its 0-based position) so a consumer
that re-sorts or filters the array keeps the schedule. `--tree` is
unaffected in structure — it re-derives its nesting from `required_by`
top-down from the roots (real portage feeds `_tree_display`
`reversed(mylist)`, an implementation detail this different algorithm
doesn't need), though a `--tree` root that depends on another root now
appears dep-first among the roots.

~240 pinned multi-entry test assertions reordered (Rust unit +
contract-suite, mechanically, both implementations verified byte-identical
throughout); new `portage-repo` unit test, new dedicated contract test +
3 `CASES`. After the fix, portuale's `--pretend` order matches real
`emerge --pretend` on a real tree.

```sh
FX="$(realpath PORTING/fixtures)"
# dev-libs/diamond -> shared-a, shared-b -> common: the shared leaf first,
# its two consumers next, the requested root last.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -p dev-libs/diamond
# [ebuild  N     ] dev-libs/common-1.0
# [ebuild  N     ] dev-libs/shared-a-1.0
# [ebuild  N     ] dev-libs/shared-b-1.0
# [ebuild  N     ] dev-libs/diamond-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -p --json dev-libs/diamond \
  | python3 -c 'import json,sys; print([(e["package"], e["merge_order"]) for e in json.load(sys.stdin)["entries"]])'
# [('common', 0), ('shared-a', 1), ('shared-b', 2), ('diamond', 3)]
```

### `emerge -p`: the `USE="…"` line shows at plain `-p`, not only `-v` (real-tree finding)

Third gap from the same container run: real `emerge` printed
`[ebuild  N     ] dev-libs/libevent-2.1.13  USE="clock-gettime ssl
-debug …"` at plain `-p`, where portuale printed only the bracket line.
Real `_DisplayConfig` sets `print_use_string = verbosity != 1` and real
default `emerge -p` verbosity is 2 — so the `USE="…"` line is **not**
`-v`-gated. It's `all_flags = verbosity == 3` that's `-v`-only, and it
controls *which* flags render: for a **`New`** package
`_create_use_string`'s `is_new` branch renders *every* IUSE flag
regardless (`red(flag)` / `blue(-flag)`), so a New entry's USE list is
identical at `-p` and `-pv`; for a `Reinstall`/`Upgrade` at plain `-p`,
only the *changed* flags render.

**Increment 1** lands the New case: `use_suffix` / `_use_suffix` no
longer gate a `New` entry's USE display on `verbose`. The content is
exactly what `-pv` already produced (grouped `USE=` + `VAR="…"` per
USE_EXPAND, enabled-first, `--alphabetical`-aware) minus the `-pv`-only
`::repo` cpv decoration and the trailing counters line. ~25 pinned `-p`
assertions gained a `USE=` suffix; a handful of `-v`-detection tests
switched their "is this verbose" probe from "`USE=` present" to
"`::repo` present".

**Increment 2** lands the `Reinstall`/`Upgrade`/`Downgrade` changed-flags-only
diff. `build_use_expand_display` / `_build_use_expand_display` grew an
`all_flags: bool` param; `render_flag` returns `Option` and yields
`None` — the flag is omitted — for an *unchanged* flag (and for any
removed-from-IUSE flag, whose `(-flag%)` list is `all_flags`-only) when
`all_flags` is off. `resolve_pretend_graph` computes both renderings
(`GraphEntry::use_expand_display` for `-pv`, `use_expand_display_p` for
`-p` — the Python reference re-renders at display time instead of
storing both), and `use_suffix` picks by verbosity. So an Upgrade with a
real USE diff prints e.g. `USE="added%* -change*"` at `-p` where `-pv`
shows `USE="added%* keep -change* (-drop%)"`. The one visible cut:
`reinst_flags` (real portage's per-flag "this flag triggered the
reinstall" force) is still unmodelled — a `--newuse`/`--changed-use`
reinstall's own trigger flags render via the change markers anyway, but
a flag `reinstall_for_flags` would have force-shown while otherwise
unchanged is omitted at `-p`.

`emerge -pv` output is unchanged by either increment. New `pretend.rs`
unit test, new dedicated contract test + 3 `CASES` total; both sides
byte-identical.

```sh
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -p dev-libs/useflagpkg
# [ebuild  N     ] dev-libs/newpkg-1.0            <- empty IUSE, no USE line
# [ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -p --update dev-libs/upgradeusepkg
# [ebuild     U  ] dev-libs/upgradeusepkg-2.0 [1.0] USE="added%* -change*"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pv --update dev-libs/upgradeusepkg
# [ebuild     U  ] dev-libs/upgradeusepkg-2.0::testrepo [1.0::testrepo] USE="added%* keep -change* (-drop%)"
```

### `emerge --getbinpkgonly <atom>` (no `--pretend`): the real remote-binpkg download + merge

The `--pretend` half of `--getbinpkg`/`--getbinpkgonly` shipped earlier
(`binrepos.conf`/`PORTAGE_BINHOST` parsing, remote binhost candidates
from each binhost's *cached* `Packages` index, the `g` bracket column,
`Size of downloads:`). This is the other half — the first non-dry-run
`emerge <atom>` action the pilot implements (alongside `--buildpkgonly`,
which builds but never merges).

`emerge --getbinpkgonly <atom>` now:

1. **Refreshes each `http(s)` binhost's live index** (real
   `bintree._populate_remote`): `wget <sync-uri>/Packages` into the same
   `<EROOT>/var/cache/edb/binhost/<host>/<path>/Packages` cache the
   resolver reads — done *before* resolution so the fresh pool is seen. A
   `file://` binhost needs no refresh. (`--pretend` still never touches
   the network.)
2. **Resolves** binary-only (`--usepkgonly`), in real topological merge
   order.
3. For each remote-binary `New` entry: **downloads** the binpkg
   (`<sync-uri>/<PATH>`) into `$PKGDIR`, **verifies its byte size**
   against the index `SIZE`, and **merges** it —
   `ebuild_merge::merge_binpkg` extracts the image (new
   `binpkg::extract_binpkg` — xpak `[image][XPAK trailer]` split, or the
   gpkg's `image.tar.<comp>` member; `tar` auto-detects the codec),
   copies it into `${ROOT}` via the same `merge_tree` a source build
   uses (CONFIG_PROTECT included), writes the vdb entry from the
   binpkg's own metadata + a freshly-generated `CONTENTS`, and runs
   `env_update()`/`ldconfig`.

`write_vdb_entry` was refactored to an `Environment`-free
`write_vdb_entry_from_dir` (a binpkg has no ebuild); `wget_fetch` is
`pub(crate)` now.

**Deliberate v1 cuts** (same "narrow the first slice, document it"
pattern as every other real-execution feature):
- **no `pkg_preinst`/`pkg_postinst`** — real portage sources the
  binpkg's saved `environment.bz2` and runs them; the pilot's phase
  runner is ebuild-file-driven. `environment.bz2` and the `<pf>.ebuild`
  aren't copied into the vdb either.
- **replace is refused** — if any version of the cp is already
  installed, the merge errors rather than orphaning the old version's
  files (real portage unmerges the replaced version afterwards).
- no collision-protect/`protect-owned` abort, no blocker exclusion, no
  preserve-libs registration.
- digest check is `SIZE`-only (no crypto — the `SHA*`/`MD5` fields are
  read but not verified, the same `Manifest`/`.sig` cut the gpkg
  metadata reader already documents); no `Packages.gz` (compressed
  remote index); no live `layout.conf` negotiation (the index `PATH` is
  trusted).

Rust-unit-tested end to end against a real fixture `.tbz2` served over
loopback HTTP: `refresh_binhost_indexes` lands the live `Packages` in
the edb cache, `download_and_verify` fetches + size-checks (and rejects a
mismatch), and `run_getbinpkgonly` produces a real vdb entry +
`${ROOT}/usr/share/packagepkg/hello.txt`. Plus `merge_binpkg` /
`extract_binpkg` unit tests for both the xpak and gpkg image paths.

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
ln -sf "$(realpath PORTING/rust/target/release/portuale)" /tmp/emerge
FX="$(realpath PORTING/fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/newpkg              # -> [ebuild  N    ] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg # -> [ebuild     U ] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/samepkg             # -> already installed

# real PkgAttrDisplay bracket layout (increment 1 of the -pv real-output.py
# layout + colour buildout): the fixed-width [I][N/r][S/R][f/F/g][U][D]
# field, and [old-ver] in place of the "(upgrade from X)" prose
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/downgradepkg
# [ebuild     UD] dev-libs/downgradepkg-1.0 [2.0]
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/changeddepspkg
# [ebuild   R   ] dev-libs/changeddepspkg-1.0     <- a plain reinstall: R, no inline reason (real -pv)
# [ebuild  N    ] dev-libs/newpkg-1.0

# ANSI colour (increment 2): --color y|n overrides the NO_COLOR/isatty
# gate. diamond is a favorite (PKG_MERGE_WORLD, green); its deps are
# plain PKG_MERGE (darkgreen); N is green, [old-ver] blue, ~ mask yellow.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --color y dev-libs/diamond | cat -v
# [^[[32;01mebuild^[[39;49;00m  ^[[32;01mN^[[39;49;00m    ] ^[[32;01mdev-libs/diamond-1.0^[[39;49;00m
# [^[[32mebuild^[[39;49;00m  ^[[32;01mN^[[39;49;00m    ] ^[[32mdev-libs/shared-a-1.0^[[39;49;00m
#   ... (deps darkgreen)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" NO_COLOR=1 /tmp/emerge --pretend --color y dev-libs/diamond | cat -v
#   -- still coloured: an explicit --color y wins over NO_COLOR

# dependency recursion: diamond dependency, deduped (see PORTING/fixtures)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/diamond
# [ebuild  N    ] dev-libs/diamond-1.0
# [ebuild  N    ] dev-libs/shared-a-1.0
# [ebuild  N    ] dev-libs/shared-b-1.0
# [ebuild  N    ] dev-libs/common-1.0

# --tree: the same diamond, indented -- common nests under shared-a
# only (first alphabetically), never repeated under shared-b
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree dev-libs/diamond
# [ebuild  N    ] dev-libs/diamond-1.0
# [ebuild  N    ]   dev-libs/shared-a-1.0
# [ebuild  N    ]     dev-libs/common-1.0
# [ebuild  N    ]   dev-libs/shared-b-1.0

# --unordered-display (only meaningful with --tree): preserves RDEPEND's
# own literal order instead of sorting alphabetically -- treeorderpkg's
# own RDEPEND deliberately lists its children reverse-alphabetically
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree dev-libs/treeorderpkg
# [ebuild  N    ] dev-libs/treeorderpkg-1.0
# [ebuild  N    ]   dev-libs/atreechild-1.0
# [ebuild  N    ]   dev-libs/ztreechild-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree --unordered-display dev-libs/treeorderpkg
# [ebuild  N    ] dev-libs/treeorderpkg-1.0
# [ebuild  N    ]   dev-libs/ztreechild-1.0
# [ebuild  N    ]   dev-libs/atreechild-1.0

# --columns: the version moves out of the inline "-1.0" suffix into its
# own right-aligned column instead (COLUMNWIDTH here trimmed to 70 for a
# readable example -- the real default is 130)
COLUMNWIDTH=70 PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --columns dev-libs/newpkg
# [ebuild  N    ] dev-libs/newpkg [1.0]
# an Upgrade's own old version appears in its own trailing column too --
# the same information the default format's own "(upgrade from X)"
# parenthetical carries, just repositioned
COLUMNWIDTH=70 PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --columns dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg [2.0] [1.0]
# --tree and --columns can't be combined, matching real portage
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree --columns dev-libs/newpkg
# emerge: can't specify both of "--tree" and "--columns".  (exit 2)

# BDEPEND/PDEPEND/IDEPEND are walked too, not just DEPEND/RDEPEND -- v1
# makes no distinction between any of the five real dependency-string
# keys (no real merge ordering exists yet for the distinction to matter)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/bdependpkg
# [ebuild  N    ] dev-libs/bdependpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# real slot-operator dependency atoms (":=" and ":1=") are resolved, not
# silently dropped -- ":1=" specifically resolves multislotpkg's SLOT=1
# version (2.0), not its SLOT=0 version (1.0)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/slotoperatorpkg
# [ebuild  N    ] dev-libs/slotoperatorpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/multislotpkg-2.0

# a real sub-slot restriction (":0/2", PMS 8.3.3, not a slot-operator) now
# actually matches -- dev-libs/subslotpkg's own SLOT is "0/2"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/subslotconsumer
# [ebuild  N    ] dev-libs/subslotconsumer-1.0
# [ebuild  N    ] dev-libs/subslotpkg-1.0

# ...and a genuine sub-slot mismatch (":0/3" against the same "0/2"
# candidate) is genuinely rejected, not just always accepted
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/subslotmismatchconsumer
# [ebuild  N    ] dev-libs/subslotmismatchconsumer-1.0
# !!! no visible ebuild for dependency "dev-libs/subslotpkg"

# real USE-dep dependency atoms are resolved AND enforced now: both
# "[bar(+)]"/"[baz(+)?]" are (+)-defaulted flags missing from their own
# target's IUSE, so both are genuinely, trivially satisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeppkg
# [ebuild  N    ] dev-libs/usedeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/multislotpkg-2.0

# USE-dep enforcement, top-level: useflagpkg's own IUSE="foo missingflag",
# "foo" enabled globally -- "[foo]" (declared, enabled) is satisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[foo]'
# [ebuild  N    ] dev-libs/useflagpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
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
# [ebuild  N    ] dev-libs/useflagpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# USE-dep enforcement, dependency level: usedeprejectedpkg's own RDEPEND
# is "dev-libs/useflagpkg[-foo]", genuinely unsatisfiable -- the parent
# still resolves, the rejected dependency is reported, not silently
# dropped or accepted (same "report, don't fail" spirit as an
# unresolvable dependency)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeprejectedpkg
# [ebuild  N    ] dev-libs/usedeprejectedpkg-1.0
# !!! no visible ebuild for dependency "dev-libs/useflagpkg"  (stderr)

# opt= conditional USE-dep (PMS 8.3.4): useeqparentonpkg's own
# IUSE="+eqflag" defaults it ON, so its RDEPEND's "[eqflag=]" evaluates
# to "[eqflag]" -- matches useeqchildpkg's own default-on eqflag
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useeqparentonpkg
# [ebuild  N    ] dev-libs/useeqparentonpkg-1.0
# [ebuild  N    ] dev-libs/useeqchildpkg-1.0
# the identical use-dep string, but useeqparentoffpkg's own IUSE="eqflag"
# (no "+") defaults it OFF -- "[eqflag=]" now evaluates to "[-eqflag]",
# which mismatches the child, so the dependency is reported unresolvable
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useeqparentoffpkg
# [ebuild  N    ] dev-libs/useeqparentoffpkg-1.0
# !!! no visible ebuild for dependency "dev-libs/useeqchildpkg"  (stderr)

# REQUIRED_USE is real and implemented: requireduseokpkg's own
# "foo? ( bar )" is genuinely satisfied (foo enabled globally, bar
# forced on by this package's own package.use entry)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requireduseokpkg
# [ebuild  N    ] dev-libs/requireduseokpkg-1.0
# requiredusebadpkg has the identical constraint but nothing forcing
# "bar" on -- genuinely violated, which aborts the WHOLE run (exit 1),
# a harsher severity than a merely unresolvable dependency
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadpkg
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"  (exit 1)
# ...and still aborts the whole run even when only reached as a
# dependency, not just as a top-level atom
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadparentpkg
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"  (exit 1)

# ...and the whole walk keeps going past the first violation: a SECOND,
# independent top-level atom's own unrelated REQUIRED_USE violation
# still gets attempted and reported too, not silently skipped once the
# first one failed -- matching real depgraph.py's own "collect every
# violation, only fail at the very end" severity, not "abort on the
# first hit"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadpkg dev-libs/requiredusebadpkg2
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"
# REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg2-1.0: "baz? ( qux )"  (exit 1)

# --autounmask: this package is masked by KEYWORDS alone ("~amd64", no
# package.accept_keywords entry) -- quiet by default (real
# --autounmask-keep-keywords defaults to suppressing keyword
# suggestions when --autounmask itself was never explicitly given)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/autounmaskkeywordpkg
# emerge: there are no ebuilds to satisfy "dev-libs/autounmaskkeywordpkg".  (exit 1)
# ...but once --autounmask is explicitly given, real portage RESOLVES the
# graph with the implicit `=cpv ~arch` change applied (real
# _display_autounmask) -- normal merge list on stdout, the "necessary to
# proceed" block on stderr, exit 0 (real actions.py:563)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask dev-libs/autounmaskkeywordpkg
# [ebuild  N    ] dev-libs/autounmaskkeywordpkg-1.0          (stdout)
#                                                            (stderr:)
# The following keyword changes are necessary to proceed:
#  (see "package.accept_keywords" in the portage(5) man page for more details)
# # required by dev-libs/autounmaskkeywordpkg (argument)
# =dev-libs/autounmaskkeywordpkg-1.0 ~amd64                  (exit 0)
# the same, now for a *dependency's* own keyword-masked-only candidate
# (dev-libs/autounmaskdepconsumer RDEPENDs on the fixture above) -- quiet
# by default (just the "no visible ebuild" line), exit 0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/autounmaskdepconsumer
# [ebuild  N    ] dev-libs/autounmaskdepconsumer-1.0
# !!! no visible ebuild for dependency "dev-libs/autounmaskkeywordpkg"  (exit 0)
# ...and once --autounmask is given, BOTH packages resolve and the block
# carries the real two-line dep chain
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask dev-libs/autounmaskdepconsumer
# [ebuild  N    ] dev-libs/autounmaskdepconsumer-1.0         (stdout)
# [ebuild  N    ] dev-libs/autounmaskkeywordpkg-1.0
#                                                            (stderr:)
# The following keyword changes are necessary to proceed:
#  (see "package.accept_keywords" in the portage(5) man page for more details)
# # required by dev-libs/autounmaskdepconsumer-1.0::testrepo
# # required by dev-libs/autounmaskdepconsumer (argument)
# =dev-libs/autounmaskkeywordpkg-1.0 ~amd64                  (exit 0)
# --json exposes the change as a top-level array
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask --json dev-libs/autounmaskdepconsumer | python3 -c 'import json,sys; print(json.load(sys.stdin)["autounmask_keyword_changes"])'
# [{'cpv': 'dev-libs/autounmaskkeywordpkg-1.0', 'token': '~amd64', 'dep_chain': ['required by dev-libs/autounmaskdepconsumer-1.0::testrepo', 'required by dev-libs/autounmaskdepconsumer (argument)']}]

# --autounmask-use: on by default (unlike the keyword kind), so
# useflagpkg's own real "foo" (globally enabled, but this atom demands
# "-foo") RESOLVES with an implicit package.use flip -- real portage's
# default. `>=<cpv>` atom form (real check_if_latest for USE, bug #536392)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend "dev-libs/useflagpkg[-foo]"
# [ebuild  N    ] dev-libs/useflagpkg-1.0                     (stdout)
#                                                             (stderr:)
# The following USE changes are necessary to proceed:
#  (see "package.use" in the portage(5) man page for more details)
# # required by dev-libs/useflagpkg[-foo] (argument)
# >=dev-libs/useflagpkg-1.0 -foo                              (exit 0)
# --autounmask-use=n restores the strict "USE-dep mismatch -> no visible
# candidate" behaviour
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask-use=n "dev-libs/useflagpkg[-foo]"
# emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".  (exit 1)
# a dependency's own USE-dep mismatch resolves the same way, with the
# two-line dep chain (dev-libs/usedeprejectedpkg RDEPENDs the atom above)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeprejectedpkg
# [ebuild  N    ] dev-libs/usedeprejectedpkg-1.0             (stdout)
# [ebuild  N    ] dev-libs/useflagpkg-1.0
#                                                            (stderr:)
# The following USE changes are necessary to proceed:
#  (see "package.use" in the portage(5) man page for more details)
# # required by dev-libs/usedeprejectedpkg-1.0::testrepo
# # required by dev-libs/usedeprejectedpkg (argument)
# >=dev-libs/useflagpkg-1.0 -foo                             (exit 0)
# --json exposes the change as a top-level array
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/useeqparentoffpkg | python3 -c 'import json,sys; print(json.load(sys.stdin)["autounmask_use_changes"])'
# [{'atom': '>=dev-libs/useeqchildpkg-1.0', 'token': '-eqflag', 'dep_chain': ['required by dev-libs/useeqparentoffpkg-1.0::testrepo', 'required by dev-libs/useeqparentoffpkg (argument)']}]

# IUSE's own "+"/"-" default markers are honored now: "+enableddefault"
# defaults on, "-disableddefault" stays off (own REQUIRED_USE requires
# exactly this), and "plainflag" (no default marker at all) is
# genuinely undecided by IUSE -- but forced on by this package's own
# package.use entry, proving IUSE defaults and package.use coexist
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/iusedefaultpkg
# [ebuild  N     ] dev-libs/iusedefaultpkg-1.0  USE="-disableddefault enableddefault plainflag"

# "x86" is never in this package's own IUSE, but IS a real, valid
# profiles/arch.list entry -- implicitly valid for REQUIRED_USE even
# though it's not the active profile's own arch (stays disabled), the
# same shape real media-libs/mesa's own REQUIRED_USE hits
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/archiuseimplicitpkg
# [ebuild  N    ] dev-libs/archiuseimplicitpkg-1.0

# Global use.force/use.mask win over a contradicting package.use entry:
# this package's own package.use entry tries to invert both flags
# ("-globalforceflag globalmaskflag"), but the profile's own use.force/
# use.mask (applied strictly after package.use, matching real
# regenerate()'s own literal-last-step ordering) win on both
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/globalprecedencepkg
# [ebuild  N     ] dev-libs/globalprecedencepkg-1.0  USE="globalforceflag -globalmaskflag"

# A profile-level "-flag" genuinely cancels an IUSE "+default": this
# package's own IUSE is "+cancelme" (defaults on), but
# profiles/default/make.defaults declares "-cancelme" -- real portage's
# own single continuous incremental walk lets it reach back and cancel
# the earlier IUSE default, not just fail to add on top of it
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/cancelledpkg
# [ebuild  N     ] dev-libs/cancelledpkg-1.0  USE="-cancelme"

# real profile/make.conf resolution: "foo" is enabled by the fixture's
# profile chain, so this package's foo?-gated dependency is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N    ] dev-libs/useflagpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# USE_EXPAND is real and implemented: profiles/base/make.defaults'
# VIDEO_CARDS="nvidia" expands into the pseudo-USE flag
# "video_cards_nvidia", which genuinely gates a dependency, not just -v
# display
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/useexpandpkg
# [ebuild  N     ] dev-libs/useexpandpkg-1.0  USE="-video_cards_amdgpu video_cards_nvidia"
# [ebuild  N    ] dev-libs/newpkg-1.0

# USE_EXPAND_UNPREFIXED is real and implemented too: profiles/arch/amd64/
# make.defaults' own ARCH="amd64" contributes the bare pseudo-USE flag
# "amd64" (no "arch_" prefix at all, unlike an ordinary USE_EXPAND
# variable) -- this is literally how "amd64" exists as a real USE flag
# in actual Gentoo, and it genuinely gates a dependency here too
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/archusepkg
# [ebuild  N     ] dev-libs/archusepkg-1.0  USE="amd64 -riscv"
# [ebuild  N    ] dev-libs/newpkg-1.0

# package.use's own USE_EXPAND-prefix shorthand is real and implemented
# too: "dev-libs/packageuseexpandpkg PYTHON_TARGETS: python3_12" in
# fixtures/etc/portage/package.use expands to
# "python_targets_python3_12", user-level package.use only
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/packageuseexpandpkg
# [ebuild  N     ] dev-libs/packageuseexpandpkg-1.0  USE="python_targets_python3_12"
# [ebuild  N    ] dev-libs/newpkg-1.0

# use.stable.force/package.use.stable.mask are real and implemented too:
# stableusepkg's own KEYWORDS="amd64" (no "~") is genuinely stable, so
# both apply -- stableforceflag forced on (pulling in a real dependency)
# and maskflag masked back off despite package.use enabling it first
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/stableusepkg
# [ebuild  N     ] dev-libs/stableusepkg-1.0  USE="-maskflag stableforceflag"
# [ebuild  N    ] dev-libs/newpkg-1.0
# unstableusepkg shares the identical IUSE/RDEPEND/package.use entry,
# but its own KEYWORDS="~amd64" is genuinely NOT stable -- neither
# applies: stableforceflag stays off, maskflag stays on
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/unstableusepkg
# [ebuild  N     ] dev-libs/unstableusepkg-1.0  USE="maskflag -stableforceflag"

# package.mask: hidden, no matching package.unmask entry
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/hardmaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/hardmaskedpkg".  (exit 1)

# package.mask + package.unmask: masked, then unmasked again -> visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/maskedandunmaskedpkg
# [ebuild  N    ] dev-libs/maskedandunmaskedpkg-1.0

# repo-level profiles/package.mask (real portage's most common real-world
# masking source, e.g. security/arch masks) hides a package the same way
# a user-level package.mask entry does
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/repomaskedpkg".  (exit 1)

# a repo-level mask, cancelled by a profile-level package.unmask entry --
# proving the three sources (repo, profile chain, user) are genuinely
# stacked together, not checked independently
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedthenprofileunmaskedpkg
# [ebuild  N    ] dev-libs/repomaskedthenprofileunmaskedpkg-1.0

# a repo-level mask, cancelled by a "-atom" line in the user-level
# package.mask -- -atom removal now spans all three sources, not just
# within the one file that contains the "-atom" line
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedthenuserremovedpkg
# [ebuild  N    ] dev-libs/repomaskedthenuserremovedpkg-1.0

# package.accept_keywords wildcard ("*/wildcardkeywordpkg ~amd64") makes an
# otherwise ~amd64-only, not-globally-accepted package visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/wildcardkeywordpkg
# [ebuild  N    ] dev-libs/wildcardkeywordpkg-1.0

# package.accept_keywords "**" accepts a package with no KEYWORDS at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/livekeywordpkg
# [ebuild  N    ] dev-libs/livekeywordpkg-9999

# package.accept_keywords negation ("-amd64") revokes a keyword the
# global ACCEPT_KEYWORDS="amd64" already granted -- for this one
# genuinely stable package specifically
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/keywordrevokedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/keywordrevokedpkg".  (exit 1)

# package.accept_keywords "*"/"~*" wildcards: accept any stable/testing
# keyword respectively, distinct from "**" -- "*" alone would NOT have
# covered the second package below, since it's testing-only (~arm64)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/starkeywordpkg
# [ebuild  N    ] dev-libs/starkeywordpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/tildestarkeywordpkg
# [ebuild  N    ] dev-libs/tildestarkeywordpkg-1.0

# package.accept_keywords bare atom: no keyword tokens at all, real
# accept_keywords_defaults still grants an implicit "~amd64" (global
# ACCEPT_KEYWORDS="amd64", "~"-prefixed) -- not a no-op
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/bareacceptkeywordspkg
# [ebuild  N    ] dev-libs/bareacceptkeywordspkg-1.0

# package.accept_keywords is now also stacked from the profile chain, not
# just /etc/portage -- this package has no user-level entry at all, only
# a profile-level one (see PORTING/fixtures/repo/profiles/arch/amd64)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/profileacceptkeywordspkg
# [ebuild  N    ] dev-libs/profileacceptkeywordspkg-1.0

# package.use ("*/packageuseenablepkg pkguseflag") enables a flag that's
# off everywhere else, pulling in its pkguseflag?-gated dependency
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packageuseenablepkg
# [ebuild  N    ] dev-libs/packageuseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# package.use ("dev-libs/packageusedisablepkg -foo") disables a flag for
# just this package, even though "foo" is on globally (contrast with
# dev-libs/useflagpkg above, whose own foo?-gated dependency IS pulled in)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packageusedisablepkg
# [ebuild  N    ] dev-libs/packageusedisablepkg-1.0

# package.use is now stacked from repo+profile too, not just
# /etc/portage -- neither of these packages has any user-level entry
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repouseenablepkg
# [ebuild  N    ] dev-libs/repouseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/profileuseenablepkg
# [ebuild  N    ] dev-libs/profileuseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# a strong (!!) blocker matching an already-installed package is reported
# (not enforced -- exit code is still 0, same as real --pretend); the
# line follows real output.py::_blockers now
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/blockerpkg
# [ebuild  N    ] dev-libs/blockerpkg-1.0
# [blocks B     ] dev-libs/samepkg ("dev-libs/samepkg" is hard blocking dev-libs/blockerpkg-1.0)

# a weak (!) blocker matching another package this same run would also
# newly merge (not just an installed one) -- "soft blocking"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/graphblockerparent
# [ebuild  N    ] dev-libs/graphblockerparent-1.0
# [ebuild  N    ] dev-libs/blockerpartnerpkg-1.0
# [ebuild  N    ] dev-libs/weakblockerpkg-1.0
# [blocks B     ] dev-libs/blockerpartnerpkg ("dev-libs/blockerpartnerpkg" is soft blocking dev-libs/weakblockerpkg-1.0)

# blocker lines print as one group AFTER every package line (real
# Display.print_blockers), not interleaved: blockerorderpkg's own blocker
# owner is the first entry, but the [blocks] line still lands last
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/blockerorderpkg
# [ebuild  N    ] dev-libs/blockerorderpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [blocks B     ] dev-libs/samepkg ("dev-libs/samepkg" is hard blocking dev-libs/blockerorderpkg-1.0)

# overlays: a package that exists only in the overlay repo (see
# PORTING/fixtures/etc/portage/repos.conf) is found
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayonlypkg
# [ebuild  N    ] dev-libs/overlayonlypkg-1.0

# "::reponame" repo constraint: the same package, constrained to the
# repo it's NOT in, correctly finds nothing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayonlypkg::testrepo
# emerge: there are no ebuilds to satisfy "dev-libs/overlayonlypkg::testrepo".  (exit 1)

# same version in both repos: the higher-priority overlay's own copy is
# the one actually used, proven by its RDEPEND (not the main repo copy's)
# pulling in dev-libs/newpkg
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaytiepkg
# [ebuild  N    ] dev-libs/overlaytiepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# overlay repos' own package.mask: overlaymaskedpkg is masked only in the
# overlay's own profiles/package.mask (a bare atom, auto-scoped to
# "::overlay" by real append_repo) -- an unconstrained atom still
# resolves via the main repo's own, unaffected copy
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedpkg
# [ebuild  N    ] dev-libs/overlaymaskedpkg-1.0

# an explicit "::overlay" atom does hit that same auto-scoped mask
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedpkg::overlay
# emerge: there are no ebuilds to satisfy "dev-libs/overlaymaskedpkg::overlay".  (exit 1)

# the overlay's own package.unmask cancels that same overlay's own
# package.mask entry (both get the identical "::overlay" auto-scoping)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedthenunmaskedpkg
# [ebuild  N    ] dev-libs/overlaymaskedthenunmaskedpkg-1.0

# repos.conf masters: the overlay has no explicit "masters =", so it
# implicitly masters the main repo -- mastermaskedpkg exists only in the
# overlay and is masked purely by the MAIN repo's own package.mask
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/mastermaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/mastermaskedpkg".  (exit 1)

# the overlay's own package.unmask still cancels a masters-inherited
# mask, since both get the identical "::overlay" auto-scoping
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/mastermaskedthenoverlayunmaskedpkg
# [ebuild  N    ] dev-libs/mastermaskedthenoverlayunmaskedpkg-1.0

# explicit repos.conf masters=: independentoverlay declares
# "masters = overlay", NOT the main repo -- independentmastermainonlypkg
# exists only there and is masked only by the MAIN repo's own
# package.mask, which does NOT apply since main isn't a declared master
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/independentmastermainonlypkg
# [ebuild  N    ] dev-libs/independentmastermainonlypkg-1.0

# independentmasteroverlaypkg (also only in independentoverlay) is
# masked only by the OVERLAY repo's own package.mask instead -- which
# DOES apply, since overlay is independentoverlay's declared master
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/independentmasteroverlaypkg
# emerge: there are no ebuilds to satisfy "dev-libs/independentmasteroverlaypkg".  (exit 1)

# overlay package.use/.force/.mask: all three now read from every repo,
# not just main -- overlayuseenablepkg exists only in the overlay, whose
# own profiles/package.use enables its own overlayuseflag
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayuseenablepkg
# [ebuild  N    ] dev-libs/overlayuseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# package.use.force: the overlay's own profiles/package.use.force forces
# a flag on that's off by IUSE default and every other source
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayuseforcepkg
# [ebuild  N    ] dev-libs/overlayuseforcepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# package.use.mask: the inverse -- IUSE="+overlaymaskflag" defaults the
# flag on, but the overlay's own profiles/package.use.mask masks it off
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayusemaskpkg
# [ebuild  N    ] dev-libs/overlayusemaskpkg-1.0

# slot conflict: slotconflictnewconsumer resolves slotconflicttarget to
# 2.0 first; slotconflictoldconsumer's own "<...-2.0" constraint rejects
# that -- reported, not enforced (exit code and the rest of the graph are
# unaffected)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/slotconflictparent
# [ebuild  N    ] dev-libs/slotconflictparent-1.0
# [ebuild  N    ] dev-libs/slotconflictnewconsumer-1.0
# [ebuild  N    ] dev-libs/slotconflictoldconsumer-1.0
# [ebuild  N    ] dev-libs/slotconflicttarget-2.0
# [slot conflict] dev-libs/slotconflicttarget:0 resolved to dev-libs/slotconflicttarget-2.0, which does not satisfy "<dev-libs/slotconflicttarget-2.0"

# NOT a conflict: two different slots of the same package coexist as
# independent entries, same as real portage allows
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/multislotparent
# [ebuild  N    ] dev-libs/multislotparent-1.0
# [ebuild  N    ] dev-libs/multislotpkg-1.0
# [ebuild  N    ] dev-libs/multislotpkg-2.0

# virtuals: virtual/texteditor is shaped like the real virtual/pager (an
# ordinary ebuild, any-of RDEPEND) -- no dedicated resolution code exists
# for it, or is needed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/virtualconsumerpkg
# [ebuild  N    ] dev-libs/virtualconsumerpkg-1.0
# [ebuild  N    ] virtual/texteditor-0
# [ebuild  N    ] dev-libs/newpkg-1.0

# real "||" semantics: neither alternative in this package's own RDEPEND
# has a visible candidate anywhere, so BOTH still get reported (the
# fallback, matching the pre-existing "never silently wrong about
# whether a dependency exists" invariant) instead of one being silently
# dropped
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/anyofunresolvable
# [ebuild  N    ] dev-libs/anyofunresolvable-1.0
# !!! no visible ebuild for dependency "dev-libs/doesnotexist-anywhere"
# !!! no visible ebuild for dependency "dev-libs/alsodoesnotexist-anywhere"

# multiple top-level atoms: a dependency shared between two REQUESTED
# packages (not just two deps of one package) dedupes the same way a
# diamond dependency always did
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/shared-a dev-libs/shared-b
# [ebuild  N    ] dev-libs/shared-a-1.0
# [ebuild  N    ] dev-libs/shared-b-1.0
# [ebuild  N    ] dev-libs/common-1.0

# a bad top-level atom aborts the whole run immediately, in argv order --
# real portage's own "there are no ebuilds to satisfy" wording (from
# lib/_emerge/depgraph.py), not enforced/reported-and-continued the way a
# dependency's own missing candidate is
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/does-not-exist dev-libs/newpkg
# emerge: there are no ebuilds to satisfy "dev-libs/does-not-exist".  (exit 1)

# a top-level atom can now carry an operator/slot, same as a dependency
# atom always could -- resolve_pretend's own matching needed no changes
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend '>=dev-libs/newpkg-1.0'
# [ebuild  N    ] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/newpkg:0
# [ebuild  N    ] dev-libs/newpkg-1.0

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
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild  N    ] dev-libs/nestedsetpkg-1.0
# [ebuild  N    ] dev-libs/innernestedsetpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# @world combines with an explicit atom in the same invocation
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/samepkg @world
# dev-libs/samepkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild  N    ] dev-libs/nestedsetpkg-1.0
# [ebuild  N    ] dev-libs/innernestedsetpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

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
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

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
# [ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"
# [ebuild  N    ] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N    ] dev-libs/useflagpkg-1.0   (no -v: no USE= at all)
# [ebuild  N    ] dev-libs/newpkg-1.0

# -v/--verbose isn't a plain boolean in real emerge -- an explicit
# following "n" disables it again, same as real insert_optional_args
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v n dev-libs/useflagpkg
# [ebuild  N    ] dev-libs/useflagpkg-1.0   (explicit "n": no USE= shown)
# [ebuild  N    ] dev-libs/newpkg-1.0

# --newuse/-N is real and implemented: reinstallpkg is installed with
# IUSE="foo" declared but an empty vdb USE file (foo was off at merge
# time); the fixture profile chain enables "foo" globally now, so
# --newuse reports a Reinstall for the changed flag -- and still recurses
# into its own RDEPEND, exactly like a New/Upgrade entry would
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/reinstallpkg
# [ebuild   R   ] dev-libs/reinstallpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# without --newuse, the exact same package stays AlreadyInstalled -- the
# USE mismatch is real, but nothing checks for it unless --newuse is given.
# --noreplace isolates this from real portage's own separate "selective"
# default for a bare top-level atom (see --noreplace/--selective below)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/reinstallpkg
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
# [ebuild   R   ] dev-libs/changedusepkg-1.0
# ...but --changed-use never even looks at IUSE presence, only at
# enablement -- and that flag's own enablement never changed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-use dev-libs/changedusepkg
# dev-libs/changedusepkg-1.0 is already installed; nothing to do
# --changed-use still catches an ENABLEMENT change on a flag shared by
# both IUSE sets, same as reinstallpkg's own --newuse example above
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-use dev-libs/reinstallpkg
# [ebuild   R   ] dev-libs/reinstallpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --update/-u is real and implemented: with `selective` restored via
# --noreplace, real emerge does NOT offer to upgrade a package just
# because a newer version exists -- upgradepkg is installed at 1.0, a
# newer 2.0 is visible in the tree, but "emerge --noreplace
# dev-libs/upgradepkg" leaves it alone (real depgraph.py's own
# avoid_update, lines 7814/8448)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/upgradepkg
# dev-libs/upgradepkg-1.0 is already installed; nothing to do
# --update (or its short alias -u) is what makes the newer version show up
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]
# --update threads through the whole dependency graph, not just a
# top-level atom: here upgradepkg is reached only as withdeps' own
# dependency, and still upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/withdeps
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# without --update AND without --noreplace/--selective, a bare top-level
# atom still finds the newer version on its own -- real portage's own
# "selective" gap (see --noreplace/--selective further down)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# --noreplace/-n and --selective are real and implemented: samepkg has no
# newer version and nothing else about it changed, yet a bare top-level
# atom still reports a plain reinstall, no reason given at all -- real
# portage's own "selective" gap (see the paragraph above)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/samepkg
# [ebuild   R   ] dev-libs/samepkg-1.0
# --noreplace (or its real synonym --selective) restores "nothing to do"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do
# --selective=n explicitly cancels selective even if another flag (here,
# --update) would otherwise have set it -- unlike the upgradepkg example
# above, samepkg has nothing newer, so --update alone still leaves it
# alone; --selective=n forces the bare reinstall anyway
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --selective=n dev-libs/samepkg
# [ebuild   R   ] dev-libs/samepkg-1.0

# --deep/-D is real and implemented: without it, real emerge never walks
# an already-installed package's own further dependencies, no matter how
# deep the graph goes -- deeppkg is installed and RDEPENDs on deeppkg2
# (also installed), which itself RDEPENDs on newpkg (New), but neither
# ever shows up here. --noreplace keeps deeppkg itself AlreadyInstalled
# (see --noreplace/--selective further down), isolating --deep's own
# gating from real portage's own separate "selective" default for a bare
# top-level atom
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# a bare --deep (unlimited depth) walks the whole already-installed
# chain -- deeppkg2 itself stays silent (already installed, not a
# top-level atom), but newpkg's own [ebuild N] line now appears
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0
# --deep=N bounds the depth: 1 level reaches deeppkg2 but not newpkg
# (identical output to no --deep at all); 2 levels reaches all the way
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep=1 dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep=2 dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0

# --emptytree/-e reinstalls the whole deep dependency tree as though
# nothing is installed (real create_depgraph_params.py: empty + deep,
# selective popped) -- useful for comparing against real portage and for
# debugging resolution
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pe dev-libs/deeppkg
# [ebuild   R   ] dev-libs/deeppkg-1.0        <- installed -> bare reinstall
# [ebuild   R   ] dev-libs/deeppkg2-1.0       <- installed dep, no longer hidden
# [ebuild  N    ] dev-libs/newpkg-1.0         <- not installed -> New

# --exclude/-X is real and implemented: without it, --update offers the
# visible upgrade normally
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]
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
# {"entries":[{"category":"dev-libs","package":"newpkg","outcome":"new","version":"1.0","slot":"0","source":"ebuild","provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},"requested":true,"required_by":[],"blockers":[]}],"slot_conflicts":[],"changed_deps_report":[]}
# a binary candidate's own "source" is "binary", not "ebuild" -- entry_to_json
# used to hardcode the literal "ebuild" regardless of the entry's actual
# source, a real bug only caught once a binary candidate could resolve at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json --usepkg dev-libs/binaryonlypkg
# {"entries":[{"category":"dev-libs","package":"binaryonlypkg","outcome":"new","version":"1.0","slot":"0","source":"binary","provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},"requested":true,"required_by":[],"blockers":[]}],"slot_conflicts":[],"changed_deps_report":[]}
# dev-libs/common is a diamond dependency (both shared-a and shared-b
# RDEPEND on it) -- required_by lists both owners, sorted
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/diamond | python3 -c 'import json,sys; print(next(e["required_by"] for e in json.load(sys.stdin)["entries"] if e["package"] == "common"))'
# [{'category': 'dev-libs', 'package': 'shared-a'}, {'category': 'dev-libs', 'package': 'shared-b'}]
# --json's own state-change trace: dev-libs/maskedandunmaskedpkg is
# matched by a package.mask entry that an identical package.unmask
# entry then cancels -- provenance records both, not just that the
# package ended up visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/maskedandunmaskedpkg | python3 -c 'import json,sys; print(json.load(sys.stdin)["entries"][0]["provenance"])'
# {'mask_entry': 'dev-libs/maskedandunmaskedpkg', 'unmask_entry': 'dev-libs/maskedandunmaskedpkg', 'keyword_entry': None}
# dev-libs/wildcardkeywordpkg is ~amd64-only, visible only via the
# "*/wildcardkeywordpkg ~amd64" package.accept_keywords entry --
# provenance names that specific entry, not just "some entry helped"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/wildcardkeywordpkg | python3 -c 'import json,sys; print(json.load(sys.stdin)["entries"][0]["provenance"])'
# {'mask_entry': None, 'unmask_entry': None, 'keyword_entry': '*/wildcardkeywordpkg'}

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
# [ebuild  N    ] dev-libs/anyoflicensepkg-1.0
# package.license unmasks an otherwise EULA-masked package for that one
# package specifically (etc/portage/package.license accepts SomeEula
# just for this atom)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packagelicensepkg
# [ebuild  N    ] dev-libs/packagelicensepkg-1.0

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
# [ebuild  N    ] dev-libs/uselicensepkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/uselicensepkgforced
# emerge: there are no ebuilds to satisfy "dev-libs/uselicensepkgforced".  (exit 1)

# PROPERTIES/ACCEPT_PROPERTIES/package.properties and RESTRICT/
# ACCEPT_RESTRICT/package.accept_restrict masking are real and
# implemented: real portage's own default (from cnf/make.globals) is
# "*", accepting everything, so a plain declared PROPERTIES is visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/propertiespkg
# [ebuild  N    ] dev-libs/propertiespkg-1.0
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
# [ebuild  N     ] dev-libs/pkgusemaskforcepkg-1.0  USE="forceflag -maskflag -specflag"

# --nodeps/-O is real and implemented: withdeps' own RDEPEND (which
# would otherwise pull in newpkg and upgradepkg -- see the plain
# recursion example above) is never even read
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --nodeps dev-libs/withdeps
# [ebuild  N    ] dev-libs/withdeps-1.0

# --nodeps still shows a resolved package's own USE display with -v --
# it's -N's own foo?-gated dependency recursion that's suppressed, not
# the package's own metadata
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -O -v dev-libs/useflagpkg
# [ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"

# --onlydeps/-o is real and implemented: the exact inverse of --nodeps --
# withdeps' own dependencies (newpkg, upgradepkg) print normally, but
# withdeps' own [ebuild N] line is suppressed. --update is added again
# just so upgradepkg's own dependency-level entry actually upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --onlydeps dev-libs/withdeps
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# --onlydeps on an already-installed atom: no dependencies were ever
# going to be walked (same as without --onlydeps), and its own "already
# installed" line is suppressed too -- so the whole run prints nothing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --onlydeps dev-libs/samepkg
# (no output)

# short-flag bundling: "-pv" decomposes into -p + -v, both real,
# implemented flags -- native argparse behavior for boolean short
# options, not something requiring emerge-specific parsing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pv dev-libs/useflagpkg
# [ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"
# [ebuild  N    ] dev-libs/newpkg-1.0

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

# a target with no matching world-file entry at all reports nothing,
# regardless of installed status
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/bar
# >>> No matching atoms found in "world" favorites file...

# an explicit-category target needs NO installed check at all -- real
# dep_expand() returns it unchanged, and action_deselect seeds
# expanded_atoms with it unconditionally -- so a world-listed but never-
# installed package is still discarded
echo "dev-libs/nevermerged" >> /tmp/deselect-demo-root/var/lib/portage/world
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/nevermerged
# >>> Would remove dev-libs/nevermerged from "world" favorites file...

# real Atom.intersects() is deliberately narrower than a real version-
# range check: an exact-version target matches an identical world entry,
# but ">=" against that same version doesn't, even though 1.0 would
# actually satisfy ">=dev-libs/versioned-1.0" under real dependency
# resolution -- the operator itself must match exactly here
echo "=dev-libs/versioned-1.0" >> /tmp/deselect-demo-root/var/lib/portage/world
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect ">=dev-libs/versioned-1.0"
# >>> No matching atoms found in "world" favorites file...
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect "=dev-libs/versioned-1.0"
# >>> Would remove =dev-libs/versioned-1.0 from "world" favorites file...

# --deselect "@name" matches the separate world_sets file by exact name
# (never expanded against its own set members) -- reported against
# "world_sets", not "world"
echo "@mytools" > /tmp/deselect-demo-root/var/lib/portage/world_sets
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect @mytools
# >>> Would remove @mytools from "world_sets" favorites file...

# a plain atom and a "@name" target discarded together are sorted into
# one combined list, not two separate "world" then "world_sets" blocks
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/foo @mytools
# >>> Would remove @mytools from "world_sets" favorites file...
# >>> Would remove dev-libs/foo from "world" favorites file...

# --with-bdeps: withbdepspkg is already installed, DEPENDs on
# builddeponlypkg, BDEPENDs on hostdeponlypkg, RDEPENDs on newpkg --
# --deep's default (--with-bdeps=y/auto) walks all three. --noreplace
# keeps withbdepspkg itself AlreadyInstalled (see --noreplace/--selective
# further down) -- without it, a bare top-level atom recurses into its
# own dependencies regardless of --deep at all (real portage's own
# "selective" gap turns it into a plain reinstall, and any New/Upgrade/
# Reinstall entry's own dependencies are always walked), so --deep's own
# gating couldn't be demonstrated otherwise
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/builddeponlypkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/hostdeponlypkg-1.0

# --with-bdeps=n: DEPEND/BDEPEND are skipped, but RDEPEND is unaffected
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep --with-bdeps n dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0

# --with-bdeps-auto n: with no explicit --with-bdeps given, changes the
# *default* from "auto" (walk all three) down to "n" -- same effect as
# --with-bdeps n above, but via the default instead of an explicit value
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep --with-bdeps-auto n dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0

# an explicit --with-bdeps always wins over --with-bdeps-auto regardless
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep --with-bdeps y --with-bdeps-auto n dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/builddeponlypkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/hostdeponlypkg-1.0

# --changed-deps: changeddepspkg's own vdb-recorded RDEPEND (samepkg)
# differs from its current ebuild's own RDEPEND (newpkg) -- reinstalls
# and recurses into the CURRENT ebuild's own dependency, not the vdb's
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/changeddepspkg
# [ebuild   R   ] dev-libs/changeddepspkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --changed-deps ignores a libc-only dependency change (strip_libc_deps):
# libcnoisepkg's own vdb RDEPEND names sys-libs/glibc, its current
# ebuild names sys-libs/musl -- both are real virtual/libc providers per
# the fixture vdb's own virtual/libc entry, so no reinstall fires
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/libcnoisepkg
# dev-libs/libcnoisepkg-1.0 is already installed; nothing to do

# --changed-deps-report: same stale RDEPEND as the --changed-deps example
# above, but reported (to stderr) instead of reinstalled -- stdout still
# shows the ordinary "already installed" line. --noreplace keeps
# changeddepspkg itself AlreadyInstalled -- --changed-deps-report is NOT
# one of real portage's own eight "selective" triggers (unlike
# --changed-deps itself), so without --noreplace a bare top-level atom
# would report a plain reinstall instead (see --noreplace/--selective
# further down), muddying this specific demonstration. The " for $FX"
# suffix below only appears because ROOT isn't "/" here, like every
# other example in this section -- real portage's own condition exactly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --changed-deps-report dev-libs/changeddepspkg
# dev-libs/changeddepspkg-1.0 is already installed; nothing to do
#
# !!! Detected ebuild dependency change(s) without revision bump:
#
#     dev-libs/changeddepspkg-1.0::testrepo for $FX
#
# NOTE: Refer to the following page for more information about dependency
#       change(s) without revision bump:
#
#           https://wiki.gentoo.org/wiki/Project:Portage/Changed_dependencies
#
#       In order to suppress reports about dependency changes, add
#       --changed-deps-report=n to the EMERGE_DEFAULT_OPTS variable in
#       '/etc/portage/make.conf'.
#
# HINT: In order to avoid problems involving changed dependencies, use the
#       --changed-deps option to automatically trigger rebuilds when changed
#       dependencies are detected. Refer to the emerge man page for more
#       information about this option.

# --changed-deps-report is silent once --changed-deps is also given --
# --changed-deps reinstalls normally, exactly as its own example above
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps-report --changed-deps dev-libs/changeddepspkg
# [ebuild   R   ] dev-libs/changeddepspkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --changed-slot: changedslotpkg's own vdb-recorded SLOT ("0") differs
# from its current ebuild's own SLOT ("0/2", an ABI-bump sub-slot change)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-slot dev-libs/changedslotpkg
# [ebuild   R   ] dev-libs/changedslotpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --changed-deps/--changed-slot are independent, freely-combinable
# reinstall triggers -- changedslotpkg's own vdb RDEPEND is *also* stale,
# so giving both prints both reasons on the same line
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps --changed-slot dev-libs/changedslotpkg
# [ebuild   R   ] dev-libs/changedslotpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --with-test-deps: withtestdeppkg's own RDEPEND is "dev-libs/newpkg
# test? ( dev-libs/testonlydep )" -- without the flag, only the
# unconditional dev-libs/newpkg is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/withtestdeppkg
# [ebuild  N    ] dev-libs/withtestdeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --with-test-deps additionally pulls in the test?-gated dep too
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --with-test-deps dev-libs/withtestdeppkg
# [ebuild  N    ] dev-libs/withtestdeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/testonlydep-1.0

# ...but only for a top-level (depth 0) atom -- withtestdepconsumer's own
# RDEPEND reaches withtestdeppkg at depth 1, so testonlydep stays absent
# even with --with-test-deps given
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --with-test-deps dev-libs/withtestdepconsumer
# [ebuild  N    ] dev-libs/withtestdepconsumer-1.0
# [ebuild  N    ] dev-libs/withtestdeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --usepkg/--usepkgonly: binaryonlypkg exists only in fixtures/pkgdir's
# own Packages index, no ebuild anywhere -- invisible without --usepkg
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/binaryonlypkg
# emerge: there are no ebuilds to satisfy "dev-libs/binaryonlypkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg dev-libs/binaryonlypkg
# [binary  N    ] dev-libs/binaryonlypkg-1.0

# --binpkg-respect-use defaults on under --usepkg: binaryusemismatchpkg's
# own binary entry has USE: (empty) but the fixture profile's own global
# USE would select "foo" over its IUSE="foo" -- mismatch, so the binary
# is rejected and the identical-version ebuild is used instead
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg dev-libs/binaryusemismatchpkg
# [ebuild  N    ] dev-libs/binaryusemismatchpkg-1.0

# ...but --binpkg-respect-use defaults OFF under --usepkgonly (no ebuild
# fallback to reject *to*) -- the same mismatched binary is now accepted
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkgonly dev-libs/binaryusemismatchpkg
# [binary  N    ] dev-libs/binaryusemismatchpkg-1.0

# --usepkg-exclude: drops a binary candidate from the pool entirely --
# binaryonlypkg has no ebuild to fall back to, so it disappears completely
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --usepkg-exclude dev-libs/binaryonlypkg dev-libs/binaryonlypkg
# emerge: there are no ebuilds to satisfy "dev-libs/binaryonlypkg".  (exit 1)
# --usepkg-include: the inverse -- once given, a binary candidate must
# match one of the listed atoms to stay eligible; a non-matching list
# rejects it exactly like --usepkg-exclude does
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --usepkg-include dev-libs/doesnotexist-anywhere dev-libs/binaryonlypkg
# emerge: there are no ebuilds to satisfy "dev-libs/binaryonlypkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --usepkg-include dev-libs/binaryonlypkg dev-libs/binaryonlypkg
# [binary  N    ] dev-libs/binaryonlypkg-1.0

# --rebuilt-binaries: rebuiltbinarypkg is installed at 1.0 (BUILD_TIME
# 1000), but the binary index's own copy at the same version has
# BUILD_TIME 2000 -- off by default (--selective avoids the unrelated
# "bare top-level atom always reinstalls" behavior muddying this)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective dev-libs/rebuiltbinarypkg
# dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do
# given explicitly, the differing BUILD_TIME triggers a reinstall
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective --rebuilt-binaries dev-libs/rebuiltbinarypkg
# [binary   R   ] dev-libs/rebuiltbinarypkg-1.0
# --rebuilt-binaries-timestamp narrows it to "newer AND at/above this
# cutoff" -- 2000 is below 3000, so no reinstall; 2000 clears 1500
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective --rebuilt-binaries --rebuilt-binaries-timestamp 3000 dev-libs/rebuiltbinarypkg
# dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective --rebuilt-binaries --rebuilt-binaries-timestamp 1500 dev-libs/rebuiltbinarypkg
# [binary   R   ] dev-libs/rebuiltbinarypkg-1.0
# the real, non-obvious default: --usepkgonly + bare --deep + --update
# together auto-enable --rebuilt-binaries with no explicit flag at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkgonly --deep --update --selective dev-libs/rebuiltbinarypkg
# [binary   R   ] dev-libs/rebuiltbinarypkg-1.0

# --getbinpkg: dev-libs/remotebinpkg exists ONLY in the binhost's own
# Packages index (fixtures/binhost/Packages, reached via
# fixtures/etc/portage/binrepos.conf) -- no ebuild, no local $PKGDIR
# entry, so --usepkg alone leaves it invisible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg dev-libs/remotebinpkg
# emerge: there are no ebuilds to satisfy "dev-libs/remotebinpkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --getbinpkg dev-libs/remotebinpkg
# [binary  N g  ] dev-libs/remotebinpkg-1.0
# -v renders the real `g` bracket column, the ::repo from the index's own
# REPO field, and the binary's SIZE as both the ` N KiB` line suffix and
# the Size of downloads: counter
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v --getbinpkg dev-libs/remotebinpkg
# [binary  N g   ] dev-libs/remotebinpkg-1.0::gentoo  USE="-rbfoo" 560 KiB
#
# Total: 1 package (1 new, 1 binary), Size of downloads: 560 KiB
# :slot/sub_slot decoration applies to a [binary ... g] line too
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v --getbinpkg dev-libs/remotebinslotpkg
# [binary  N g   ] dev-libs/remotebinslotpkg-1.0:2/1::gentoo  1024 KiB
#
# Total: 1 package (1 new, 1 binary), Size of downloads: 1024 KiB
# -G/--getbinpkgonly resolves it the same way (binary-only)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -G dev-libs/remotebinpkg
# [binary  N g  ] dev-libs/remotebinpkg-1.0

# --newrepo: newrepopkg is installed with a vdb repository file
# recording "oldrepo", but the current best candidate for this exact
# version lives in "testrepo" instead -- off by default...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective dev-libs/newrepopkg
# dev-libs/newrepopkg-1.0 is already installed; nothing to do
# ...fires once given explicitly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective --newrepo dev-libs/newrepopkg
# [ebuild   R   ] dev-libs/newrepopkg-1.0
# a vdb repository file that DOES match the current provider never
# triggers a reinstall
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective --newrepo dev-libs/samerepopkg
# dev-libs/samerepopkg-1.0 is already installed; nothing to do
# samepkg has no vdb repository file at all -- real portage's own
# "__unknown__" sentinel applies, which never matches a real repo name,
# so --newrepo fires here too even though nothing really changed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective --newrepo dev-libs/samepkg
# [ebuild   R   ] dev-libs/samepkg-1.0

# --buildpkgonly: dualdep is New, and both its DEPEND and RDEPEND on
# newpkg are also New -- real portage refuses to resolve this at all,
# since newpkg itself would also need building
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --buildpkgonly dev-libs/dualdep
# [ebuild  N    ] dev-libs/dualdep-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
#
# !!! --buildpkgonly requires all dependencies to be merged.
# !!! Cannot merge requested packages. Merge deps and try again.
# (exit 1)
# buildpkgonlysatisfied is also New, but its own RDEPEND (samepkg) is
# already installed -- nothing else needs building, so it resolves fine
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --buildpkgonly dev-libs/buildpkgonlysatisfied
# [ebuild  N    ] dev-libs/buildpkgonlysatisfied-1.0

# downgrade vs upgrade: downgradepkg is installed at 2.0, but only 1.0 is
# visible in the tree -- a genuine downgrade, distinct from an upgrade,
# and shown even without --update since the installed 2.0 has no visible
# candidate of its own to satisfy real avoid_update's own shortcut
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/downgradepkg
# [ebuild     UD] dev-libs/downgradepkg-1.0 [2.0]

# avoid_update bug fix (see "What this proves" above for the full
# writeup): keywordmaskedpkg is installed at 2.0 (~amd64-only, no
# longer ACCEPT_KEYWORDS-visible); requested directly as a TOP-LEVEL
# atom, it's still a real downgrade (real portage's own later
# avoid_update block DOES require visibility there)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/keywordmaskedpkg
# [ebuild     UD] dev-libs/keywordmaskedpkg-1.0 [2.0]
# ...but reached only as a DEPENDENCY (needskeywordmasked's own
# RDEPEND), real portage's own EARLIER avoid_update return requires no
# visibility at all -- kept exactly as installed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/needskeywordmasked
# [ebuild  N    ] dev-libs/needskeywordmasked-1.0
# same again, but with a real USE-dep on the dependency atom too
# (checked against the installed package's own real vdb USE, not the
# current tree's -- the actual real-world sys-libs/liburing:=[...] case)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/needskeywordmaskeduse
# [ebuild  N    ] dev-libs/needskeywordmaskeduse-1.0

# bug 640318: an installed dependency's USE-dep flag can be valid purely
# because the package was BUILT with it -- builtusedivergedep-1.0 has vdb
# USE="divergedflag" but its current ebuild dropped divergedflag from
# IUSE, so nothing in the tree satisfies [divergedflag]...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/builtusedivergedep[divergedflag]'
# emerge: there are no ebuilds to satisfy "dev-libs/builtusedivergedep[divergedflag]".  (exit 1)
# ...but as a DEPENDENCY it's kept as installed (real _iuse_implicit_built)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/needsbuiltusediverge
# [ebuild  N    ] dev-libs/needsbuiltusediverge-1.0
```

Try the `ebuild` stub (still a dry-run placeholder -- no real phase
execution -- but it now recognizes real ebuild syntax; see
ebuild.rs/ebuild_options.rs):

```sh
ln -sf "$(realpath PORTING/rust/target/release/portuale)" /tmp/ebuild

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

Real ebuild phase execution (task #54 -- see "What this proves" above for
the full writeup): `ebuild <file> install` runs the real `pretend` through
`install` phase sequence via an embedded `brush` shell, landing real files
under a real `${D}`. Uses `PORTING/fixtures/repo/dev-libs/phasepkg`, whose
own `src_install` calls real `insinto`/`doins`:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild install
# (real phase output, including some known-nonfatal noise -- see
# ebuild_phases.rs's own "KNOWN, DOCUMENTED GAPS" -- then:)
#  * Final size of build directory: 0 KiB
#  * Final size of installed tree:  4 KiB
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt
# hello from phasepkg
```

Real merge/filesystem mutation (task #55 -- see "What this proves" above
for the full writeup): `ebuild <file> merge` runs the same real `install`
chain, then really runs `pkg_preinst`, really copies `${D}` into a real
`${ROOT}` and writes a real vdb entry, then really runs `pkg_postinst`.
Uses `PORTING/fixtures/repo/dev-libs/mergepkg`, whose own `src_install`
calls real `insinto`/`doins`/`dosym`, and whose own `pkg_preinst`/
`pkg_postinst` each drop a marker file under `${T}` proving the real
ordering (preinst before the merge is visible, postinst only after):

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export ROOT="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild merge
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
cat "${ROOT}"/usr/share/mergepkg/hello.txt
# hello from mergepkg
readlink "${ROOT}"/usr/share/mergepkg/hello-link.txt
# hello.txt
cat "${ROOT}"/var/db/pkg/dev-libs/mergepkg-1.0/CONTENTS
# dir /usr
# dir /usr/share
# dir /usr/share/mergepkg
# sym /usr/share/mergepkg/hello-link.txt -> hello.txt <mtime>
# obj /usr/share/mergepkg/hello.txt <md5> <mtime>
ls "${PORTAGE_TMPDIR}"/portage/dev-libs/mergepkg-1.0/temp/ | grep -E "preinst|postinst"
# postinst-ran-after-merge
# preinst-ran-before-merge
cat "${ROOT}"/var/db/pkg/dev-libs/mergepkg-1.0/COUNTER
# 0 (a bare integer, no trailing newline -- the real vdb COUNTER format)
ls "${ROOT}"/var/db/pkg/dev-libs/
# mergepkg-1.0 (no leftover -MERGING-mergepkg-1.0 temp dir)
```

Real package removal: `ebuild <file> unmerge` (see "What this proves"
above for the full writeup) really deletes what `merge` just installed:

```sh
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild unmerge
# (real prerm/postrm phase output, then exit 0)
test -e "${ROOT}"/usr/share/mergepkg && echo "still there" || echo "gone"
# gone
test -e "${ROOT}"/var/db/pkg/dev-libs/mergepkg-1.0 && echo "still there" || echo "gone"
# gone
test -e "${ROOT}"/var/db/pkg/dev-libs && echo "still there" || echo "gone"
# gone (the now-empty category directory is removed too)
```

Real `CONFIG_PROTECT` (see "What this proves" above for the full
writeup): a locally-edited `/etc` file survives a merge. Uses
`PORTING/fixtures/repo/dev-libs/configpkg`, whose own `src_install`
installs a *new* `/etc/configpkg.conf`:

```sh
mkdir -p "${ROOT}"/etc
echo "admin's own edits" > "${ROOT}"/etc/configpkg.conf
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/configpkg/configpkg-1.0.ebuild merge
cat "${ROOT}"/etc/configpkg.conf
# admin's own edits          <- untouched
cat "${ROOT}"/etc/._cfg0000_configpkg.conf
# new content from configpkg  <- diverted here instead
grep configpkg "${ROOT}"/var/db/pkg/dev-libs/configpkg-1.0/CONTENTS
# obj /etc/configpkg.conf <md5-of-the-new-content> <mtime>
```

(A real host's own ambient `CONFIG_PROTECT` -- if this pilot's dev/test
machine is itself a real Gentoo system -- will override the `/etc`
default shown above via the same env-var-sourced CLI boundary; export
`CONFIG_PROTECT=/etc` explicitly first if reproducing this by hand
outside the test suite, which never inherits host env vars this way.)

Real symlink `CONFIG_PROTECT`, `NOCONFMEM`, and `new_protect_filename`'s
own file-reuse logic (see "What this proves" above for the full writeup):
a locally-repointed `/etc` symlink survives a merge exactly like a
regular file does, and `NOCONFMEM` changes the real, visible outcome of a
repeat merge. Uses `PORTING/fixtures/repo/dev-libs/configsympkg`, whose
own `src_install` installs a *new* `/etc/configsympkg.conf` symlink
pointing at `new-target`:

```sh
export CONFIG_PROTECT=/etc
mkdir -p "${ROOT}"/etc
ln -sfn admins-own-target "${ROOT}"/etc/configsympkg.conf
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/configsympkg/configsympkg-1.0.ebuild merge
readlink "${ROOT}"/etc/configsympkg.conf
# admins-own-target          <- untouched
readlink "${ROOT}"/etc/._cfg0000_configsympkg.conf
# new-target                  <- diverted here instead
grep configsympkg "${ROOT}"/var/db/pkg/dev-libs/configsympkg-1.0/CONTENTS
# sym /etc/configsympkg.conf -> new-target <mtime>
```

Re-merging `configpkg` (the regular-file example above) a second time
with content unchanged shows `NOCONFMEM`'s real, visible effect -- not a
second numbered file, but whether the logical path stays protected:

```sh
# Without NOCONFMEM: the already-offered update applies directly.
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/configpkg/configpkg-1.0.ebuild merge
cat "${ROOT}"/etc/configpkg.conf
# new content from configpkg  <- overwritten, no ._cfg0001_ spawned

# With NOCONFMEM: re-protected instead, reusing ._cfg0000_ (its content
# already matches -- new_protect_filename()'s own reuse logic) rather
# than spawning a ._cfg0001_ with identical content.
export NOCONFMEM=1
echo "admin's own edits" > "${ROOT}"/etc/configpkg.conf
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/configpkg/configpkg-1.0.ebuild merge
cat "${ROOT}"/etc/configpkg.conf
# admin's own edits           <- protected again, not overwritten
cat "${ROOT}"/etc/._cfg0000_configpkg.conf
# new content from configpkg  <- reused, no ._cfg0001_ spawned
unset NOCONFMEM
```

`--debug` (task #56 -- see "What this proves" above for the full
writeup): really exports `PORTAGE_DEBUG=1`, so real `bin/ebuild.sh`'s own
`set -x` guard fires -- real bash xtrace, not simulated. Uses
`PORTING/fixtures/repo/dev-libs/debugpkg`, whose own `src_install` writes
the `PORTAGE_DEBUG` value it actually observed to `${T}/portage-debug-value.txt`:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/debugpkg/debugpkg-1.0.ebuild install --debug
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, PLUS real bash xtrace not present without --debug, e.g.:)
# ++ local needle=--allow-extra-vars
# ++ shift
# ...
# + set +x
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/debugpkg-1.0/temp/portage-debug-value.txt
# 1
```

Without `--debug`, the same run produces zero `+`/`++`-prefixed lines and
the marker file reads `0` instead.

Real `ebuild <file> package`/binpkg building (see "What this proves"
above for the full writeup): runs the real `install` chain, then really
invokes `bin/misc-functions.sh`'s own `__dyn_package`, producing a
genuine XPAK `.tbz2` and a real `Packages` index entry. Uses
`PORTING/fixtures/repo/dev-libs/packagepkg` (`RDEPEND="dev-libs/samepkg"`,
so its own metadata round-trip is visible in the index):

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export ROOT="$(mktemp -d)"
export PKGDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/packagepkg/packagepkg-1.0.ebuild install package
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
file "${PKGDIR}"/dev-libs/packagepkg-1.0.tbz2
# ...: Gentoo binary package (XPAK)
cat "${PKGDIR}"/Packages
# TIMESTAMP: <unix time>
#
# CPV: dev-libs/packagepkg-1.0
# SLOT: 0
# KEYWORDS: amd64
# RDEPEND: dev-libs/samepkg
# BUILD_TIME: <unix time>
```

Real `emerge --buildpkgonly` execution (see "What this proves" above for
the full writeup): given *without* `--pretend`, actually builds a real
binary package. `dev-libs/packagepkg`'s own `RDEPEND` (`dev-libs/samepkg`)
already has an installed vdb entry under the fixture `ROOT`, so
`--buildpkgonly`'s own real depgraph gate has nothing to object to:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_CONFIGROOT="$(realpath PORTING/fixtures)"
export ROOT="$(realpath PORTING/fixtures)"
export PORTAGE_TMPDIR="$(mktemp -d)"
export PKGDIR="$(mktemp -d)"
ln -sf "$(realpath PORTING/rust/target/release/portuale)" /tmp/emerge
/tmp/emerge --buildpkgonly dev-libs/packagepkg
# [ebuild  N    ] dev-libs/packagepkg-1.0
# >>> Building binary for dev-libs/packagepkg-1.0...
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
file "${PKGDIR}"/dev-libs/packagepkg-1.0.tbz2
# ...: Gentoo binary package (XPAK)
cat "${PKGDIR}"/Packages
# TIMESTAMP: <unix time>
#
# CPV: dev-libs/packagepkg-1.0
# SLOT: 0
# KEYWORDS: amd64
# RDEPEND: dev-libs/samepkg
# BUILD_TIME: <unix time>

# --pretend still suppresses the real build entirely, same atom:
/tmp/emerge --pretend --buildpkgonly dev-libs/packagepkg
# [ebuild  N    ] dev-libs/packagepkg-1.0
# (no ">>> Building binary" line, no real files written)

# BINPKG_FORMAT=gpkg builds the newer GLEP 78 format instead, via real,
# unmodified bin/gpkg-helper.py (BINPKG_COMPRESS=gzip keeps it off zstd):
export PKGDIR="$(mktemp -d)"
BINPKG_FORMAT=gpkg BINPKG_COMPRESS=gzip /tmp/emerge --buildpkgonly dev-libs/packagepkg
file "${PKGDIR}"/dev-libs/packagepkg-1.0.gpkg.tar
# ...: Gentoo GLEP 78 (GPKG) binary package for "packagepkg-1.0" using gzip compression
tar -tf "${PKGDIR}"/dev-libs/packagepkg-1.0.gpkg.tar
# packagepkg-1.0/gpkg-1
# packagepkg-1.0/metadata.tar.gz
# packagepkg-1.0/image.tar.gz
# packagepkg-1.0/Manifest
grep PATH "${PKGDIR}"/Packages
# PATH: dev-libs/packagepkg-1.0.gpkg.tar

# a real, nonempty SRC_URI with no Manifest entry at all is refused
# outright, rather than fetched unverified (dev-libs/fetchpkg has one
# and nothing else -- see "What this proves" above for why):
/tmp/emerge --buildpkgonly dev-libs/fetchpkg
# [ebuild  N    ] dev-libs/fetchpkg-1.0
# >>> Building binary for dev-libs/fetchpkg-1.0...
# emerge: dev-libs/fetchpkg-1.0: fetchpkg-1.0.tar.gz: no Manifest entry,
# cannot verify -- refusing to fetch unverifiable content
# (exit 1)
```

Real `SRC_URI` fetch (see "What this proves" above for the full
writeup): `dev-libs/verifiedfetchpkg`'s own real `SRC_URI` exercises
the full real grammar (arrow-rename plus a `test?` conditional group);
pre-seeding `DISTDIR` with a correctly-digested payload (matching the
fixture's own checked-in `Manifest`) fires the real already-verified
skip-fetch path, so this example needs no live network access at all:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export DISTDIR="$(mktemp -d)"
printf 'hello from verifiedfetchpkg\n' > "${DISTDIR}"/verifiedfetchpkg-1.0.tar.gz
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/verifiedfetchpkg/verifiedfetchpkg-1.0.ebuild install
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0 -- no network access, since the
# pre-seeded file already matches the fixture's own real Manifest digests)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/verifiedfetchpkg-1.0/temp/fetch-vars.txt
# A=verifiedfetchpkg-1.0.tar.gz
# AA=verifiedfetchpkg-1.0.tar.gz verifiedfetchpkg-tests-1.0.tar.gz

# RESTRICT=fetch: the plain SRC_URI is never downloaded. With the
# distfile absent, install fails fast (no network) with a
# "place it in DISTDIR by hand" pointer:
export PORTAGE_TMPDIR="$(mktemp -d)"; export DISTDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/fetchrestrictpkg/fetchrestrictpkg-1.0.ebuild install
# ebuild: fetchrestrictpkg-1.0.tar.gz: no working candidate mirror for
#   "https://example.invalid/frp-payload.bin" (RESTRICT=fetch bars
#   downloading it -- place a verified copy in <DISTDIR> by hand ...)
# ... exit 1
# With the file placed by hand (and Manifest-verified), it installs:
printf 'fetchrestrictpkg fixture distfile\n' > "${DISTDIR}"/fetchrestrictpkg-1.0.tar.gz
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/fetchrestrictpkg/fetchrestrictpkg-1.0.ebuild install
# ... exit 0
```

Real eclass `inherit()` support (see "What this proves" above for the
full writeup): `dev-libs/eclasspkg` really `inherit`s a real (if
fixture-only) eclass and calls a real function it defines:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild install
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/eclasspkg-1.0/temp/eclass-marker.txt
# hello from pilotcheck.eclass
```

The pipeline-deadlock fix (see "What this proves" above for the full
writeup): `dev-libs/bigeclasspkg` inherits `bigfixture.eclass` (~400
functions, deliberately large enough that real `bin/phase-functions.
sh`'s own post-phase `__save_ebuild_env | __filter_readonly_variables`
pipe exceeds the OS pipe buffer) and used to hang here indefinitely
before the fix:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    PORTING/fixtures/repo/dev-libs/bigeclasspkg/bigeclasspkg-1.0.ebuild install
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0 -- promptly, not after a hang)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/bigeclasspkg-1.0/temp/bigfixture-marker.txt
# hello from bigfixture.eclass
```

`--shell bash|brush` (see "What this proves" above for the full
writeup): the same `dev-libs/phasepkg` fixture task #54's own example
already uses, run via a real `bash` subprocess instead of the default
embedded brush shell:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild --shell bash \
    PORTING/fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild install
# (real phase output, then exit 0)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt
# hello from phasepkg
```

Real `mirror://` resolution (see "What this proves" above for the full
writeup): a real `mirror://debian/...` `SRC_URI` entry on the real
system's own `gentoo` repo checkout, previously unfetchable:

```sh
cd PORTING/rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export DISTDIR="$(mktemp -d)"
PORTING/rust/target/release/portuale ebuild \
    /.gentoo/repos/gentoo/app-arch/unzip/unzip-6.0_p31.ebuild unpack
# (real phase output, then exit 0)
ls "${DISTDIR}"
# unzip60.tar.gz  unzip_6.0-31.debian.tar.xz
```
