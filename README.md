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
`package.accept_keywords` -- what's visible at all -- and, on top of that,
per-package USE overrides from `package.use`, so a single package's own
`DEPEND`/`RDEPEND` can be flattened against a USE set that differs from
every other package's, the same way real portage does it.

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
    portage-repo/                repo/metadata/vdb access + resolution + recursive
                                 dependency-graph walk for `emerge --pretend`
                                 (see lib.rs's doc comment on resolve_pretend_graph)
    versions-harness/          CLI harness over portage-versions
    atom-harness/               CLI harness over portage-dep
    use-reduce-harness/         CLI harness over portage-use-reduce
    multicall/                 the real emerge/ebuild dispatch binary; `emerge`
                                 implements --pretend + dependency recursion +
                                 real profile/make.conf config (pretend.rs),
                                 everything else (including all of `ebuild`) is
                                 still a dry-run stub
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
  deps, wildcards, build-ids, repo constraints, or slot operators -- see
  the doc comment at the top of `rust/portage-dep/src/lib.rs`); the
  Python harness explicitly rejects anything outside that subset as
  `INVALID` so both sides agree on the same input language. Includes a
  faithful port of an easy-to-miss PMS rule: a bare atom whose package name
  is followed by something version-shaped (`foo-bar-2`) is rejected as
  ambiguous, not silently accepted. (`portage-dep` later gained a
  *separate*, deliberately bounded wildcard-atom matcher --
  `*/*`/`category/*`/`*/package` only -- for package.mask/.unmask/
  .accept_keywords, without touching this grammar or contract at all.)
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
  `portage-repo` finds the main repo via `repos.conf` (INI, `[DEFAULT]
  main-repo` / `[name] location`), lists candidate versions from ebuild
  filenames in the repo, reads `metadata/md5-cache/<cat>/<pf>` (plain
  `KEY=value` text -- confirmed against a real vendored tree) for
  KEYWORDS/SLOT/DEPEND/RDEPEND *without executing any bash*, and checks
  the vdb (`<ROOT>/var/db/pkg`) for what's installed. There's still no
  slot conflicts, no blockers, no overlays, no backtracking -- all
  explicitly confirmed scope cuts before implementing, not silent
  omissions (see the doc comment at the top of
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
  `package.unmask`-ed package becoming visible again, and a `package.use`
  entry both enabling and disabling a per-package flag) against the
  fixture, `ebuild`-dispatch, and batch mode inside it, exiting nonzero
  on any failure -- including proving the fixture's `make.profile`
  symlink and multi-parent chain survive the image `COPY` and still
  resolve correctly.

Known simplification: `versions-harness`/`portage-versions` compare
version components as `i128` rather than Python's arbitrary-precision
integers. See the comment at the top of
`rust/portage-versions/src/lib.rs`.

Known scope cut: `atom-harness` ports a documented v1 subset of the real
atom grammar (see above and the doc comment in
`rust/atom-harness/src/atom.rs`) -- USE deps, wildcards, build-ids, repo
constraints, slot operators, and EAPI parametrization are all deferred.
Candidates for matching are plain `category/package-version[-rN][:slot[/subslot]]`
strings rather than full Package objects (no package-db/depgraph model
exists yet in this pilot), which mirrors a fallback path the real
`match_from_list` already supports.

`PORTING/fixtures` is a small synthetic repo (not the vendored real tree
used for benchmarking): `repos.conf`, a handful of ebuilds + matching
md5-cache entries, a fake vdb, and now a full profile chain + make.conf,
covering new-install, upgrade, already-installed, not-visible
(`~amd64`-only), nonexistent-package, the sibling-package-prefix-ambiguity
regression case, (for recursion) a basic dependency chain, a diamond
dependency, a dependency cycle, an any-of (`||`) group, an unresolvable
dependency, and a dependency listed in both DEPEND and RDEPEND, and (for
profile resolution) a three-level same-repo profile chain
(`profiles/base`, `profiles/arch/amd64`, `profiles/default` -- the latter
inheriting from both of the former, in that order) plus `make.conf`
sourcing `/etc/make.local`, and a package whose dependency is gated on a
USE flag that only a correctly-resolved profile/make.conf stack would
actually enable. `repos.conf`'s `location` is relative (resolved against
`PORTAGE_CONFIGROOT`) purely so the fixture is portable across checkouts
-- real `repos.conf` files always use absolute paths; see the comment in
`portage-repo/src/lib.rs`.

`PORTING/fixtures/etc/portage/package.mask`, `package.unmask`, and
`package.accept_keywords` exercise the mask/unmask/accept_keywords slice:
`hardmaskedpkg` (masked, never unmasked, so it stays hidden),
`maskedandunmaskedpkg` (masked, then unmasked, so it's visible again),
`samepkg` (masked and then immediately un-masked via a `-dev-libs/samepkg`
line within `package.mask` itself, proving `-atom` removal works, not just
`package.unmask`), `wildcardkeywordpkg` (only `~amd64`, made visible by a
`*/wildcardkeywordpkg ~amd64` wildcard entry), and `livekeywordpkg` (no
`KEYWORDS` at all, like a live/9999 ebuild, made visible by a `**` entry).

`PORTING/fixtures/etc/portage/package.use` exercises the per-package USE
slice: `packageuseenablepkg` (its `pkguseflag?`-gated dependency is only
pulled in because a `*/packageuseenablepkg pkguseflag` wildcard entry
enables a flag that's off everywhere else) and `packageusedisablepkg`
(its `foo?`-gated dependency is *not* pulled in, even though `foo` is
enabled globally by the fixture profile chain -- same as
`useflagpkg`'s own `foo?`-gated dependency, which *is* pulled in -- because
a `dev-libs/packageusedisablepkg -foo` entry disables it for this one
package only).

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

# real profile/make.conf resolution: "foo" is enabled by the fixture's
# profile chain, so this package's foo?-gated dependency is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N] dev-libs/useflagpkg-1.0
# [ebuild  N] dev-libs/newpkg-1.0

# package.mask: hidden, no matching package.unmask entry
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/hardmaskedpkg
# !!! no visible ebuild for "dev-libs/hardmaskedpkg"  (exit 1)

# package.mask + package.unmask: masked, then unmasked again -> visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/maskedandunmaskedpkg
# [ebuild  N] dev-libs/maskedandunmaskedpkg-1.0

# package.accept_keywords wildcard ("*/wildcardkeywordpkg ~amd64") makes an
# otherwise ~amd64-only, not-globally-accepted package visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/wildcardkeywordpkg
# [ebuild  N] dev-libs/wildcardkeywordpkg-1.0

# package.accept_keywords "**" accepts a package with no KEYWORDS at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/livekeywordpkg
# [ebuild  N] dev-libs/livekeywordpkg-9999

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

# or against the Python reference implementation directly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    python3 PORTING/python/emerge_pretend_reference.py --pretend dev-libs/newpkg
```

Try the `ebuild` stub (unchanged, still a dry-run placeholder):

```sh
ln -sf "$(realpath PORTING/rust/target/release/multicall)" /tmp/ebuild
/tmp/ebuild foo-1.0.ebuild merge
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
