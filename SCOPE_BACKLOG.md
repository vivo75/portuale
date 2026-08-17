# Scope backlog

This is **not** a Python-vs-Rust parity backlog. An inventory scan (CLI flags
actually implemented, the `BOOLEAN_OPTIONS`/`VALUE_OPTIONS`/`ACTIONS`
recognition tables, function-level architecture, JSON output fields, git
history) found zero gaps between `PORTING/python/emerge_pretend_reference.py`
and the Rust crate: every slice in this pilot is implemented in both
languages simultaneously and verified byte-for-byte identical via the shared
contract suite before being considered done (`PORTING/PROMPT.md`'s own hard
goal — "portability of change, not of source"). 548/548 contract tests pass
as of this writing, and every commit touching the Python reference in this
project's history also touched the Rust side in the same commit.

What *does* exist is real portage behavior this pilot hasn't ported to
**either** side yet — deliberate, already-documented scope cuts (see
`PORTING/README.md`'s "What this proves" narrative and the relevant Rust/
Python doc comments for each item's own grounding) or explicit `PROMPT.md`
architecture boundaries. This file inventories those, ranked so that items
other items depend on come first. Each item was verified against current
source (not just README prose, which occasionally documents a since-closed
cut for historical narrative reasons) before being listed here.

## Tier 0 — foundational (unlocks multiple later items)

### 1. Sub-slot modeling
`Candidate`/vdb reading currently keep only the main `SLOT` component;
`SLOT="0/5"`-style sub-slots are discarded wherever slots are read
(`list_candidates`, vdb `SLOT` file reads). Real portage tracks
`(slot, sub_slot)` as a pair. Blocks item 5 (`--changed-slot`) and any
future real slot-operator (`:=`) rebuild-trigger semantics (currently
`:=`/`:*` atoms just mean "no slot restriction," matching real
`match_from_list` but not real *rebuild* tracking).

### 2. Structured (non-flat) `use_reduce`
`portage_use_reduce::use_reduce_flat` deliberately discards `||`-group
*structure* pilot-wide (see that crate's own module doc comment) — the one
exception is the bespoke `LicenseNode`/`parse_license_tree` parser built
specifically for `LICENSE` masking, which doesn't generalize to `DEPEND`/
`RDEPEND` structure. This is why `resolve_pretend_graph` resolves *every*
alternative of an any-of dependency group rather than picking one, why
`--changed-deps` compares flattened atom *sets* rather than real
`use_reduce`'s structured trees (a documented, narrower approximation — see
`deps_changed`'s own doc comment, `portage-repo/src/lib.rs`), and blocks
real `subset=` semantics needed for item 6 (`--with-test-deps`). A
genuinely bigger, separately-scoped undertaking, not a small fix.

### 3. `repos.conf` `masters` (layout.conf repo inheritance)
An overlay can declare another repo as its own `masters` in `layout.conf`,
inheriting/stacking that repo's profiles/`package.mask`/`license_groups`.
Currently unimplemented — overlays only widen *which ebuilds are
candidates*, nothing about how they're evaluated once found (see the
overlays paragraph, `README.md`). Offered as a slice candidate multiple
times without being picked. Foundational for item 7 (overlay repos' own
masking) and item 10 (cross-repo profile parents).

### 4. Per-level/per-source config precedence (real `USE_ORDER`)
Every one of `package.mask`/`package.use.mask`/`.force`/
`package.accept_keywords`'s own multi-source stacking (repo, profile
chain, user) is implemented as "concatenate every source into one flat
list, then fold by atom specificity alone." Real portage instead applies
each *source* fully before moving to the next (specificity only breaks
ties *within* one source) — real `USE_ORDER`'s full precedence sequence.
This is why a negating entry that crosses a source boundary can resolve
differently here than in real portage (documented explicitly in the
`package.accept_keywords` negation paragraph, `README.md`) and blocks item
8 (`package.use`'s own full `USE_ORDER`, which needs a distinct
`configdict["repo"]`/`configdict["defaults"]` layering this pilot's flat
model doesn't have at all).

## Tier 1 — mid-sized features

### 5. `--changed-slot`
Real `depgraph.py`'s `_changed_slot`: reinstalls/rebuilds when an
installed package's `(slot, sub_slot)` differs from the current ebuild's.
Needs item 1.

### 6. `--with-test-deps`
Real `depgraph.py`: pulls in a directly-requested (depth-0) package's own
`DEPEND` `"test?"` conditional atoms even though the `test` USE flag stays
off elsewhere. Real portage uses `use_reduce`'s own `subset=` parameter to
extract just the test-conditional portion (item 2) — a flat-set-difference
approximation (test-forced-on minus test-off) is possible without it, as a
documented, narrower simplification, similar in spirit to how
`--changed-deps` already approximates structured comparison with a flat
one.

### 7. Overlay repos' own `package.mask`/`.unmask`/`profiles`/`license_groups`
Only the main repo's own repo-level masking/profile data is read; an
overlay's own equivalent files are never consulted (see the overlays
paragraph, `README.md`). Independently scopable, or naturally falls out of
item 3.

### 8. `package.use`'s own full `USE_ORDER` precedence
Currently flat-concatenated across repo/profile/user sources (a
deliberate, confirmed-with-the-user simplification — see the
`package.use` profile-chain-stacking paragraph, `README.md`); real
`package.use` needs per-level interleaving with that level's own
`make.defaults` USE. Needs item 4.

### 9. `--deselect` world_sets/custom-set integration
`run_deselect`'s own world-atom matching isn't integrated with
`world_sets`/custom sets at all (`emerge --deselect @some-set` isn't
supported) — a deliberate, documented cut from the nested-`@set`-
references slice (see `run_deselect`'s own doc comment, `pretend.rs`).
Real `action_deselect` operates against the same combined `world_set`
`@world` itself now fully resolves.

### 9b. Real `Atom.intersects()` algebra for `--deselect`
`run_deselect` uses a narrower category/package(+slot) equality check
instead of real `Atom.intersects()`'s full version-range/USE-dep
compatibility algebra — sufficient for the dominant plain-atom usage, but
a real, documented gap (see `run_deselect`'s own doc comment).

### 10. Cross-repo profile parents (`reponame:path` syntax)
A profile's own `parent` file can reference another repo's profile
directory by name; only same-repo parents are resolved today (see the
profile-chain paragraph, `README.md`). Benefits from item 3's own
repo-name-to-location lookup machinery.

## Tier 2 — smaller, independently bounded fixes

### 11. `USE_EXPAND` corners
`USE_EXPAND_UNPREFIXED`, IUSE-aware wildcard expansion (e.g.
`linguas_*` — needs a specific package's own IUSE, which global config
resolution doesn't have access to today), and `USE_EXPAND_HIDDEN`/
`_IMPLICIT` (real `emerge --info` display-only concerns) are all
confirmed-real, named, out-of-scope corners of the `USE_EXPAND` slice
(see that paragraph, `README.md`).

### 12. `accept_keywords_defaults` bare-atom substitution
A bare `package.accept_keywords` atom (no keyword tokens at all) has an
implicit real meaning — accept the `~`-prefixed unstable form of every
currently-accepted keyword — that this pilot treats as a no-op instead
(see `keywords_accepted`'s own doc comment, `portage-repo/src/lib.rs`).

### 13. `strip_libc_deps` in `--changed-deps`
Real `_changed_deps` strips libc-specific dependency atoms before
comparing (needs its own "what package provides libc" lookup this pilot
has nowhere else). Unaddressed; no fixture package represents libc, so
currently no observable effect (see `deps_changed`'s own doc comment,
`portage-repo/src/lib.rs`).

### 14. `--changed-deps-report`
Real `depgraph.py`: a cosmetic-only "you might want `--changed-deps`"
notice when it's off, with no reinstall of its own. Stays
recognized-but-unimplemented (see the `--changed-deps` paragraph,
`README.md`).

### 15. `--with-bdeps-auto`
The only other real lever on the same `bdeps` value `--with-bdeps` sets;
relevant only once binary-package support (item 19) exists. Stays
recognized-but-unimplemented (see the `--with-bdeps` paragraph,
`README.md`).

### 16. Real atom-grammar wildcards/build-ids
`portage-dep`'s own top-level atom grammar has a deliberately bounded
wildcard matcher (`*/*`, `category/*`, `*/package` only, for
`package.mask`-style matching) rather than real portage's fuller
wildcard/glob/build-id support (see the wildcard-atom paragraph,
`README.md`). Distinct from item 11's `USE_EXPAND`-specific wildcards.

## Tier 3 — large, explicitly deferred by `PROMPT.md` (not oversights)

These are standing architecture boundaries stated in `PORTING/PROMPT.md`
itself, not gaps found by this scan — listed here for completeness and
dependency visibility, not as "pick this next" candidates in the same
sense as Tiers 0–2.

### 17. `--autounmask*` family
Auto-suggests (and, with `--autounmask-write`, writes) `package.use`/
`.mask`/`.accept_keywords`/`.license` changes to make an otherwise-masked
target resolve. `--autounmask-write` conflicts with this pilot's own
"never writes" invariant; a read-only "suggest changes" mode is at least
theoretically scopable independent of that. Entirely unimplemented today.

### 18. `--root-deps`/cross-ROOT dependency resolution
Real portage distinguishes the build host's own root (`ESYSROOT`) from a
cross-compilation target `ROOT` for `DEPEND` resolution; this pilot has no
ROOT-cross distinction anywhere (`DEPEND` resolves against the same
repo/vdb pool as `RDEPEND`, a pre-existing, pervasive simplification).

### 19. Binary package support
`--usepkg`/`--getbinpkg` and everything gated on it
(`--rebuilt-binaries`, `--rebuild-if-new-slot`/`-rev`/`-unbuilt`,
`--binpkg-respect-use`, `--usepkg-exclude`, etc.) — "this pilot has no
binary-package support anywhere" is a standing architectural statement
repeated across many slices' own doc comments, not an oversight. Blocks
item 15 and the real usefulness of several other recognized-but-
unimplemented flags.

### 20. Real ebuild phase execution
`PROMPT.md`'s own "Deferred: ebuild phase execution" — `pkg_setup`,
`src_compile`, `src_install`, etc. Requires shelling out to system bash
(an accepted, deliberate dynamic dependency at that later stage, in
tension with the minimal-Linux goal, which is why it's deferred rather
than solved now) and real EAPI-gated bash-version checking (see
`bin/ebuild.sh`'s own `__check_bash_version`).

### 21. Real merge/install/filesystem mutation
The whole pilot is dry-run-only by design (`PROMPT.md`'s own original
scope: "No real merges, installs, or filesystem mutations in the first
port"). `--depclean`/`--unmerge`/an actual `emerge foo` that installs
something, and every world-file/vdb *write* path (including
`--deselect`'s and `--select`'s own real write branches, both already
confirmed unreachable in this pilot) all depend on lifting this. The
largest single item in this backlog.
