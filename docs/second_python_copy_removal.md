# Removing the second Python copy (`emerge_pretend_reference.py`)

Status: **decided 2026-09-14, removed 2026-09-15** (branch
`backlog/python-copy-removal`). This document records why, the checks
that replace it so the bugs it used to catch are still caught, and (in
"Where each check stands" at the end) what landed and what is still open.

## Scope

- **Removed:** `python/emerge_pretend_reference.py` and the Rust == Python
  `CASES` comparisons in `tests/test_emerge_pretend_contract.py`.
- **Kept:** `python/versions_harness.py`, `atom_harness.py`,
  `use_reduce_harness.py`, `required_use_harness.py` (~446 lines). They
  wrap real `portage.versions` / `portage.dep`, so they are a genuine
  Rust-vs-Portage differential, not a second portuale.

## Why

- **It is not a middle layer between Portage and Rust.** Its docstring:
  "Mirrors the exact same algorithm as the Rust side … NOT a wrapper".
  Resolver, config, backtracking and merge order are portuale-authored;
  real Portage is reused only for primitives (`Atom`, `match_from_list`,
  `use_reduce`, `vercmp`; ~175 call sites over 472 functions).
- **In practice the flow is Portage → Rust → Python.**
  `024-slot-operator-plan.md:455` ("Python mirror from the finished Rust
  diff (M may write it)"), `024-oracle.md:53` ("mirrors the Rust cuts"),
  `023-backtracking_resolve.md:96` (Rust-only Phase A). Of 337 commits
  touching `python/`, none leaves `rust/` untouched.
- **The contract suite proved Rust == mirror, not Rust == Portage**
  (`022_dep_zapdeps.fable.md:27`). Shared mistakes pass: the IUSE-dedup
  bug went uncaught because the reference "mirrored the bug".
- **It blocked work for reasons unrelated to Portage.** Backlog #26's
  built-`:=` append stays off partly because the mirror folds a solvable
  slot conflict (F-A1, `025-tier2-closeout.deepseek.md:525-540`).
- **Cost:** roughly doubles implementation per resolver slice
  (`022_dep_zapdeps.fable.md:15`); complete-mode real-tree graphs take
  minutes in Python (`025-tier2-closeout.deepseek.md:495`).
- **Contradicts hard goal 1** in `agent-context.md:46-52` ("portability of
  change, not of source … not structural mirroring").
- **Coexistence with Portage** needs an oracle tied to real Portage. The
  mirror has no authority on Portage behaviour and doesn't follow upstream
  changes.

## What the mirror did catch, and what replaces it

| Past bug | Class | Replacement check |
|---|---|---|
| Destructive `required_by_map.remove`: later slots got `required_by: []` (`what-this-proves.md:4187`) | Graph invariant | §1 invariant checker, §2 cross-mode consistency |
| `required_by` post-pass wiped the root-deps build entry's owner (`what-this-proves.md:7191`) | Graph invariant | §1, §2 |
| USE-dep atoms silently dropped from the BFS | Rust grammar narrower than real Portage | §3 tree-wide primitive differential, §4 no silent drops |
| IUSE `x x` rendered twice (both copies had it) | Real input absent from fixtures | §5 metamorphic tests, §6 real `emerge` oracle |
| `--deselect` assumed the target was installed (`what-this-proves.md:1376`) | Misread of upstream source | §7 upstream test translation, §8 re-pin diff review |
| PYTHONHASHSEED-dependent merge order | Nondeterminism | §9 determinism checks |

Most real catches came from the Python side calling real Portage helpers,
not from the duplicated logic. §3 keeps that property.

## Replacement checks

### 1. Output invariant checker

A pytest hook re-runs every contract case with `--json` and asserts
properties any correct resolver output must have, with no expected value:

- every non-seed entry has a non-empty `required_by`, and each owner is an
  entry or an installed package;
- every entry is reachable from the seeds;
- no duplicate `cat/pkg:slot` unless a slot conflict is reported;
- merge order respects dependency edges, except reported cycles and
  PDEPEND;
- each USE flag appears once per entry;
- summary counts equal list lengths.

Also run the checker over portuale's L0 output (~120 real atoms).

### 2. Cross-mode consistency

Plain, `--tree`, `--json` and `--quiet` describe the same graph. Assert the
same entry set, and that `--tree` nesting agrees with `required_by`. (The
first `required_by` bug was visible as a flush-left "never reached"
`--tree` line.)

### 3. Tree-wide primitive differential

Extend the four kept harnesses from hand-picked cases to a whole real
`metadata/md5-cache` (the L0 image has one):

- every atom from DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND through
  `atom_harness`, real vs Rust;
- `use_reduce` on every dependency string under a few USE sets;
- `required_use` on every REQUIRED_USE string.

Store mismatches as regression cases. Re-run on every `3rdparty/portage`
re-pin.

### 4. No silent drops

Every place in `portage-repo` that skips a dependency token it can't parse
increments a counter and warns in debug builds. L0 asserts the counter is
zero on the real tree.

### 5. Metamorphic tests

Input changes that must not change output:

- duplicate an IUSE token;
- add an unrelated package to the repo;
- add an overlay with nothing relevant;
- rename a fixture category consistently;
- reorder non-overlapping `package.use` lines;
- run the same command twice.

### 6. Real `emerge` as fixture oracle

Fixtures are already driven via `PORTAGE_CONFIGROOT`/`ROOT`
(`tests/test_emerge_pretend_contract.py:5902`). Run
`3rdparty/portage/bin/emerge -p` (likely inside the test container) on the
same trees and use its output as the expected value. Each Rust difference
is adjudicated as a bug or recorded in
`TEST/compare/known-divergences.yaml`. This is the only check here that
catches bugs portuale would share with any portuale-authored copy.

**Unverified:** whether the hand-built md5-cache fixtures work with real
`emerge` as-is. Spike on one fixture first.

### 7. Bulk translation of upstream resolver tests

`3rdparty/portage/lib/portage/tests/resolver/` has 106 files
(112 `ResolverPlayground` references); ~34 cases are hand-translated so far
(`023-oracle.md`, `024-oracle.md`). The tests are data-shaped (ebuilds,
installed, world, expected mergelist), so a script can emit `fixtures/`
trees plus an expected-mergelist pin. Upstream's mergelist is the oracle.

**Unverified:** how much of the translation can be automated. Spike on one
file first.

### 8. Re-pin diff review

Rust already cites upstream source (110 `depgraph.py:NNN`, 48 other
`_emerge`/`portage` references). On each `3rdparty/portage` re-pin, a
script runs `git diff old..new` over `lib/_emerge` and `lib/portage`, lists
changed functions, and greps the Rust citations for them, producing a
checklist of Rust code to re-read. This is the main defence while Portage
and portuale coexist.

### 9. Determinism checks

`portage-repo/src/lib.rs` has ~101 `HashMap<` uses. Rust hash seeds already
vary per process, so run each contract case 3–5 times and diff. In a test
mode, also shuffle directory-read order and `repos.conf` section order.
Fix differences with ordered collections.

### 10. Mutation testing

Run `cargo-mutants` on `portage-repo` nightly or weekly. Surviving mutants
mark logic no test distinguishes — likely places that were only "covered"
because Python agreed. Target them first with §1/§5/§6/§7 cases.

### 11. Harvest the Python once before deletion

Before removing the file, generate an expanded corpus: every fixture atom ×
the main option combinations (`-u`, `-D`, `-N`, `--nodeps`, `--tree`,
`--json`, …). Record outputs where Python and Rust agree. After deletion, a
Rust change on those cases is flagged for review, not auto-failed.

## Output with no Portage counterpart

`--json`, portuale's `--help` text and other portuale-only surfaces are
pinned against stored goldens only; §1 and §2 still apply to `--json`.

## Suggested order

1. §1, §2, §9 — about a day; would have caught three of the six examples.
2. §11 — must land before the file is deleted.
3. §3, §4 — primitive-level differential against real Portage.
4. §6, §7 — long-term resolver oracle (spike each on one case first).
5. §8, §10 — ongoing upkeep while the codebases coexist.

## Follow-up doc changes when removal lands

- `AGENTS.md` step 4 (lockstep) and step 6 (`CASES` entry).
- `agent-context.md` references to `emerge_pretend_reference.py`.
- `backlog-tasks.md:10` rule ("ship Rust + `python/emerge_pretend_reference.py`").
- `3rdparty/repos.toml` `[portage]` description.
- `tests/conftest.py` `EMERGE_PRETEND_PYTHON_REFERENCE`.

## Where each check stands (2026-09-15)

| § | Check | State | Where |
|---|---|---|---|
| 1 | Output invariants | **Done.** All 479 `--json`-capable `CASES`, every fixture package under `-p`/`-puD`/`-pe`, the last L0 run and this host's `emerge -puD @world` (1770 entries): 0 violations. Merge-order edges an installed instance satisfies are exempt (real `DepPriority.satisfied`; the host `@world` run showed the need). Checker self-tests plant each past bug class. | `tests/output_invariants.py`, `tests/test_output_invariants.py`, `TEST/compare/check-invariants.py` |
| 2 | Cross-mode consistency | **Done**, same tests: plain / `--tree` / `--quiet` entry sets equal `--json`; `--tree` nesting agrees with `required_by` (through non-displayed owners). `--onlydeps` and `--autounmask-only` handled; `--columns` skipped. | same |
| 3 | Tree-wide primitive differential | **Done.** Whole gentoo md5-cache: 46165 atoms, 136575 `use_reduce`, 13464 `REQUIRED_USE` inputs. One mismatch, in the *Python harness* (explicit `-r0` reported as no revision); fixed and pinned. Re-run on every `3rdparty/portage` re-pin. | `scripts/primitive_tree_differential.py`, `tests/primitive_regressions/` |
| 4 | No silent drops | **Done** for the two dependency walks (resolver queue, merge-order digraph): `note_unparsed_dep_token` counts, `PORTUALE_REPORT_UNPARSED_DEP_TOKENS` reports, the invariant tests and L0 require 0. Fixtures and host `@world`: 0. Other `parse_atom` call sites (config files, sets) are not dependency tokens and are not counted. | `portage_repo::note_unparsed_dep_token` |
| 5 | Metamorphic tests | **Not started.** | — |
| 6 | Real `emerge` as fixture oracle | **Spiked, blocked on staging.** Real `emerge -p dev-libs/diamond` on a copy of `fixtures/` needs: absolute `repos.conf` locations (real rejects the relative `location = repo`), `PORTAGE_REPOSITORIES` to hide the host's repos, and a clean `/etc/portage` (real still read the host `make.profile` for the running root); several fixture inputs are portuale-only syntax real rejects (`${PORTAGE_CONFIGROOT}` in `binrepos.conf`, comment lines in `profiles/updates`, `*/pkg` in `package.use.force`). Run it inside the test container with a staging step. | — |
| 7 | Upstream resolver test translation | **Not started.** `docs/023-oracle.md` "R2 — genuine upstream oracle" shows the `ResolverPlayground` route works for single cases. | — |
| 8 | Re-pin diff review | **Done.** Lists changed functions between two pins and the Rust lines citing them (by line range or distinctive name). `portage-3.0.81.3 → 3.0.82.2`: 693 changed functions. | `scripts/portage_repin_review.py` |
| 9 | Determinism | **Done** for repeated runs (every `CASES` entry ×3). It found a real race: concurrent cache-miss resolutions shared the depend phase's metadata file (`dc8022b`). Shuffling directory-read and `repos.conf` section order in a test mode is **not started** (≈56 `read_dir` sites). | `test_repeated_runs_are_byte_identical` |
| 10 | Mutation testing | **Not started** (`cargo-mutants` is not installed on this host). | — |
| 11 | Harvest before deletion | **Done.** 1057 contract calls and 4900 grid cases (510 fixture packages/sets × 10 option sets) where Rust and Python agreed. 13 contract disagreements (`--info` host state the tests normalise, the known F-A2 trailer, one `--debug` narration case where Rust matches real) and 200 grid disagreements (all cache-less fixtures, D2) were not stored. Drift is a warning; `_assert_harvested` is a strict pin at the 20 call sites whose only check was Rust == Python. | `tests/corpus.py`, `tests/corpus/`, `tests/test_harvested_corpus.py`; harvest tooling in commit `2738e9c` |

What the removal changed in the contract suite: 222 test functions lost
their Python parameter, ~630 comparison statements were removed or
reduced to their Rust half, and 19 tests named `…_matches_between_implementations`
/ `…_rust_and_python` were renamed `…_pinned_output`. Two strict xfails
that existed only because of Python bugs now pass as ordinary tests:
the A1 (#26) mirror-drift xfail and the `--debug` post-flip `Child:`
line (Rust matches the oracle capture). With the append gate
(`PORTUALE_DYNAMIC_DEPS_APPEND=1`) on, only `test_oracle_slotop_undo_cascade`
still fails: F-A1's Python half is gone, its Rust half (the cascade
drops a consumer) remains.

Follow-up doc changes listed above: done in the same branch
(`AGENTS.md`, `agent-context.md`, `backlog-tasks.md`, `repos.toml`,
`conftest.py`, plus the Tier 5/2 plans and `scope-backlog.md`).
