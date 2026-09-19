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
  `history/024-slot-operator-plan.md:455` ("Python mirror from the finished Rust
  diff (M may write it)"), `history/024-oracle.md:53` ("mirrors the Rust cuts"),
  `history/023-backtracking_resolve.md:96` (Rust-only Phase A). Of 337 commits
  touching `python/`, none leaves `rust/` untouched.
- **The contract suite proved Rust == mirror, not Rust == Portage**
  (`history/022_dep_zapdeps.fable.md:27`). Shared mistakes pass: the IUSE-dedup
  bug went uncaught because the reference "mirrored the bug".
- **It blocked work for reasons unrelated to Portage.** Backlog #26's
  built-`:=` append stays off partly because the mirror folds a solvable
  slot conflict (F-A1, `history/025-tier2-closeout.deepseek.md:525-540`).
- **Cost:** roughly doubles implementation per resolver slice
  (`history/022_dep_zapdeps.fable.md:15`); complete-mode real-tree graphs take
  minutes in Python (`history/025-tier2-closeout.deepseek.md:495`).
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
(`history/023-oracle.md`, `history/024-oracle.md`). The tests are data-shaped (ebuilds,
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
| 1 | Output invariants | **Done.** All 479 `--json`-capable `CASES`, every fixture package under `-p`/`-puD`/`-pe` and this host's `emerge -puD @world` (1770 entries): 0 violations. Full L0 `l0-20260915T092215Z` (with `vdb-list.txt` + `dep-classes.tsv` snapshots): the 18 baseline rows triaged to 14 cp-level multi-instance fan-out and 3 soft (RDEPEND/PDEPEND) edges real's `ignore_priority` ladder may relax, leaving 1 genuine violation filed as backlog #53. Merge-order edges an installed instance satisfies are exempt (real `DepPriority.satisfied`); checker self-tests plant each past bug class and each exemption. | `tests/output_invariants.py`, `tests/test_output_invariants.py`, `TEST/compare/check-invariants.py`, `TEST/layers/l0/in-container.sh` |
| 2 | Cross-mode consistency | **Done**, same tests: plain / `--tree` / `--quiet` entry sets equal `--json`; `--tree` nesting agrees with `required_by` (through non-displayed owners). `--onlydeps` and `--autounmask-only` handled; `--columns` skipped. | same |
| 3 | Tree-wide primitive differential | **Done.** Whole gentoo md5-cache: 46165 atoms, 136575 `use_reduce`, 13464 `REQUIRED_USE` inputs. One mismatch, in the *Python harness* (explicit `-r0` reported as no revision); fixed and pinned. Re-run on every `3rdparty/portage` re-pin. | `scripts/primitive_tree_differential.py`, `tests/primitive_regressions/` |
| 4 | No silent drops | **Done** for the two dependency walks (resolver queue, merge-order digraph): `note_unparsed_dep_token` counts, `PORTUALE_REPORT_UNPARSED_DEP_TOKENS` reports, the invariant tests and L0 require 0. Fixtures and host `@world`: 0. Other `parse_atom` call sites (config files, sets) are not dependency tokens and are not counted. | `portage_repo::note_unparsed_dep_token` |
| 5 | Metamorphic tests | **Done.** `tests/test_metamorphic.py`: five transforms (duplicate IUSE token, unrelated package, empty overlay, consistent category rename, reordered `package.use`) × 13 representative `CASES`, each on a private `fixtures/` copy, byte-identical output required; 65 passed, and every transform is asserted to have changed the tree. | `tests/test_metamorphic.py` |
| 6 | Real `emerge` as fixture oracle | **Done-PARTIAL, layer green.** `TEST/run/l0-fixture-oracle.sh` stages `fixtures/` in the container (`stage.sh`: 12 documented deltas incl. absolute repo locations, `PORTAGE_REPOSITORIES`, categories, per-ebuild Manifests, running-root vdb swap) and compares real `emerge -p` to portuale through `resolve-compare.py`. `l0-fx-20260915T101508Z`: 12 cases, 5 clean, 20 explained, **0 unexplained** — and it caught **backlog #54** (installed-consumer pin over-approximation: `--update paired` / `needer` / `=paired-2.0` diverge from real), suppressed by two `owner: portuale-bug` allowlist entries until fixed. Evidence: `TEST/findings/l0-fixture-oracle.md`. | `TEST/run/l0-fixture-oracle.sh`, `TEST/layers/l0-fixture-oracle/`, `TEST/compare/known-divergences-fixture-oracle.yaml` |
| 7 | Upstream resolver test translation | **Not started.** `docs/history/023-oracle.md` "R2 — genuine upstream oracle" shows the `ResolverPlayground` route works for single cases. | — |
| 8 | Re-pin diff review | **Done.** Lists changed functions between two pins and the Rust lines citing them (by line range or distinctive name). `portage-3.0.81.3 → 3.0.82.2`: 693 changed functions. | `scripts/portage_repin_review.py` |
| 9 | Determinism | **Done.** Repeated runs (every `CASES` entry ×3) found a real race (`dc8022b`: concurrent cache-miss resolutions shared the depend phase's metadata file). The shuffle half: `portage-util::read_dir_entries` is the single seam all 56 production `read_dir` sites now use — sorted by default, seeded Fisher-Yates under `PORTUALE_SHUFFLE_DIRS` (test/CI-only) — and `test_output_is_identical_under_shuffled_directory_order` runs every `CASES` entry under 3 seeds, byte-identical. `test_repos_conf_section_order_does_not_change_output` runs them against a configroot whose `repos.conf` sections are reversed. | `rust/portage-util`, `tests/test_output_invariants.py` |
| 10 | Mutation testing | **Started** (`cargo-mutants` v27.1.0 installed). First runs: `resolver_trace.rs` 35/35 missed (trace instrumentation, no unit-test surface), `solver_bridge.rs` 44 caught / 26 missed / 16 unviable; all 26 survivors are in the parked `--solver=pubgrub`/`resolvo` bridge (#33/#34) and were triaged as a Tier-4 test gap, not a default-resolver gap. `merge_order.rs`/`lib.rs` not yet run. | `TEST/findings/mutants.md` |
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

## Implementation plan for the open items (Tier 6, plan added 2026-09-15)

**Tier 6 is in progress** (2026-09-15: #48 done; #49 done-partial
with the bed green and #54 filed; #50 tooled with its verdict recorded;
#51 and #52 §5 landed; #52 §10 running; #50's batch translation and
#52 §10's ongoing upkeep are the open halves).
`docs/backlog-tasks.md` "Tier 6" lists the five items (#48-#52); they map
onto §1/§2's L0 confirmation run, §5, §6, §7, and the second half of §9
plus §10 above. §1-4, 8, 9 (repeated-run half) and 11 are Done per the
table; this section is the plan for the rest.
Read `AGENTS.md` (steps 1, 4's real-execution carve-out, 5, 8) and this
whole file first. Model tiers follow the repo convention (**F** frontier
Opus 5/Fable 5.1, **M** mid Sonnet 5, **S** small Haiku 4.5; "F review" =
a frontier model reads the full diff before the user is asked to commit).

### #48 — confirm the L0 invariant run, and resolve the 18-violation residue (S/M, 2-4 h)

**State:** `TEST/run/l0-resolver.sh` has run `check-invariants.py` on
every L0 invocation since the harness landed (it is unconditional, not
opt-in — see `TEST/run/l0-resolver.sh`'s own `check-invariants.py` call).
Every recent run (`l0-20260915T070018Z`, `l0-20260915T073208Z`, and the
R3b/R3e′ runs before them) reports the identical **18** "merge order: X
merges after its owner Y" violations and 0 control violations on real's
own output — stable across every code change made this month, so #48's
literal ask ("run it in the container") is already satisfied by
side-effect. What's missing is the triage: nobody has adjudicated
whether the 18 are real portuale bugs or checker limitations, so the
item can't close honestly yet.

**A concrete lead, found while scoping this plan, not yet verified against
an actual instance:** `check_json`'s `installed_child` exemption
(`tests/output_invariants.py` ~230-240) is

```python
installed_child = any(
    entries[k]["outcome"] != "new" for k in by_cp[child_cp]
) or (root is not None and _installed(root, child_cp))
```

`_installed(root, cp)` (~83-89) already degrades gracefully — its own
docstring says `# cannot tell; don't flag`, returning `True` when
`root is None`. But the `root is not None and` guard short-circuits
before `_installed` is ever called, so on L0 (`check-invariants.py`
passes no `root` — the container's vdb isn't visible from the host,
per the module docstring) that whole disjunct is always `False`, and
the exemption falls back to "does this cp have another graph entry
with outcome != new" — which does **not** know about a real installed
version that isn't itself a graph entry. This is exactly the kind of
gap the #25 R3c investigation (`docs/recap-of-backlog-ops-2026-09-15.md`)
found: real's edge softness is decided by "installed at insertion
instant", not by the eventual outcome, and portuale doesn't carry that
per-edge fact. So the 18 may be false positives from a checker
limitation, or they may be genuine portuale merge-order bugs the
checker correctly caught — **don't assume either without evidence.**

**Steps:**
1. Pull the 18 violation lines from a fresh `TEST/logs/l0-<stamp>/invariants.txt`
   (or re-run `L0_SKIP_MULTI=1 TEST/run/l0-resolver.sh` if none is
   current) and, for 3-5 of them, manually trace the named `cat/pkg` in
   the matching `<slug>.json.txt`: is there a genuinely-installed
   version of the child on the container's vdb (check the `real/`
   sibling output's owner-version bracket, or capture a vdb listing
   alongside the run) that the exemption is missing?
2. If yes for all/most: either (a) have `TEST/layers/l0/in-container.sh`
   also snapshot `/var/db/pkg` category listings (a `vdb-list.txt` per
   run, cheap — just `find /var/db/pkg -maxdepth 2`) and pass that as
   `root`-equivalent data to `check-invariants.py` (it doesn't need the
   full vdb, only "does a version of this cp exist"), or (b) drop the
   `root is not None and` guard and accept `_installed`'s own
   `root is None -> True` degradation at this one call site (the
   simpler fix, if (a) is judged not worth the harness change). Either
   way, re-run and confirm the 18 either explain fully or shrink to a
   named, understood residue.
3. If some instances are real portuale merge-order bugs (not the
   `installed_child` gap): file them as their own backlog items with a
   fixture reproduction — do not fold them into #48's closure.
4. Whatever residue is left after 1-3 (explained checker limitation,
   genuine accepted architecture gap, or zero), write it into
   `TEST/findings/l0.md` under its own "#48" heading with the exact
   command and the violation lines, and update `docs/backlog-tasks.md`
   #48 to DONE (or DONE-PARTIAL with the named residue) and the "Where
   each check stands" table's §1/§2 row to note the L0 confirmation.

**Acceptance:** every one of the 18 violations (or whatever L0 reports
at the time) has a written explanation with evidence, not silence;
`docs/backlog-tasks.md` #48 updated either way.

**Done 2026-09-15.** Full run `TEST/logs/l0-20260915T092215Z`
reproduced the 18 (0 control). The `installed_child` hypothesis was
**disproven** — the new `vdb-list.txt` snapshot (320 packages) contains
none of the 18 child cps. Instead: 14 rows were the cp-level
`required_by`/per-instance mismatch (another instance of the child cp
already merged before the owner; `check_json` gained the mirror of its
multi-slot-owner exemption), 3 were soft edges on a checker that passed
no repo roots (2 PDEPEND, 1 RDEPEND; the new in-container
`dep-classes.tsv` snapshot supplies the md5-cache variable classes, and
soft-only edges are exempt), and **1 is a genuine portuale order bug**:
`app-containers_podman`'s `dev-lang/go` merges after `dev-go/go-md2man`
despite a hard `BDEPEND=>=dev-lang/go-1.24.11:=` edge and the `--tree`
nesting. Filed as backlog #53 (its fixture repro is blocked on the
probe's whole-resolution divergence; next step is a minimal
`:=`-BDEPEND fixture). Evidence: `TEST/findings/l0.md` "#48";
`check-invariants.py` is now at 1 violation on that run. `[I]`

### #49 / §6 — real `emerge` as a fixture oracle (F for the staging step, M for the rest; 6-12 h)

**State:** spiked, blocked exactly as recorded: real `emerge -p` on a
copy of `fixtures/` needs absolute `repos.conf` locations (real rejects
the relative `location = repo` portuale's own config reader accepts),
`PORTAGE_REPOSITORIES` set to hide the host's own repos, a clean
`/etc/portage` (a bare run still read the host's `make.profile` for the
running root), and rejects several portuale-only fixture conveniences:
`${PORTAGE_CONFIGROOT}` interpolation in `binrepos.conf`, comment lines
in `profiles/updates`, `*/pkg` wildcards in `package.use.force`.

**Steps:**
1. **S0 (F, 2-3 h) — staging script.** `TEST/layers/l0-fixture-oracle/stage.sh`
   (or similar): copy `fixtures/` into a throwaway container path,
   rewrite `repos.conf`'s `location` to the absolute in-container path,
   export `PORTAGE_REPOSITORIES` (JSON, matching real's own format —
   check `3rdparty/portage/lib/portage/repository/config.py` for the
   shape) covering exactly the fixture repo, and point `PORTAGE_CONFIGROOT`
   at a container-local `/etc/portage` seeded from `fixtures/etc/portage`
   only (never the image's own `/etc/portage`). Fix the four portuale-only
   syntax spots one at a time: absolute-path the `binrepos.conf`
   interpolation (or write it already-resolved for this staged copy
   only, never touching the checked-in fixture), drop/relocate the
   `profiles/updates` comment lines if real's parser truly rejects them
   (verify first — it may only warn), and check whether the
   `package.use.force` wildcard is fixture-only cruft that can be
   removed without changing any pinned contract test's expectation.
2. **S1 (M, 2-3 h) — spike one case.** Get `emerge -p dev-libs/diamond`
   (the existing multi-package diamond fixture, already used elsewhere
   in the contract suite) to run clean under real inside the staged
   copy, and diff its output against portuale's own `-p` for the same
   atom. This is the go/no-go: if real still rejects the staged fixture
   tree for a reason not in the four already known, stop and report the
   new blocker rather than patching around it fixture-by-fixture.
3. **S2 (M, 3-5 h) — widen.** Once one case works, run it over a handful
   more fixtures spanning the shapes the contract suite already covers
   (slot conflict, `||` group, REQUIRED_USE, autounmask) and adjudicate
   each Rust-vs-real difference: a genuine bug (fix it, cite the real
   source line), or an accepted divergence filed in
   `TEST/compare/known-divergences.yaml` with a `layer: l0-fixture-oracle`
   (or similar) tag and a `reason:`.
4. **S3 (S, 1 h) — wire it in.** A `TEST/run/l0-fixture-oracle.sh`
   entry point (reusing `TEST/run/lib.sh`'s image/build helpers), run
   ad hoc for now — this is real-execution-only (AGENTS.md step 4's own
   carve-out; no Python mirror, no contract `CASES` entry), so it does
   not need to be part of the default `TEST/run/l0-resolver.sh` sweep.

**Acceptance:** at least the diamond fixture (and 3-4 more spanning
different resolver shapes) run clean under real inside a staged
`fixtures/` copy, with every difference adjudicated; the staging script
and its four fixture-syntax workarounds are documented so a future
fixture addition doesn't silently break it again.

**Done 2026-09-15, with a find.** The harness shipped
(`TEST/run/l0-fixture-oracle.sh` + `TEST/layers/l0-fixture-oracle/` +
`TEST/atomlists/l0-fixture-oracle.txt` + its own allowlist) and the 12
staging deltas are documented in `TEST/findings/l0-fixture-oracle.md`
(the four known ones plus eight more: per-ebuild Manifests, backquoted
fixture descriptions real's depend phase executes as shell, categories,
`repo_name`, the running-root vdb swap, comment lines in
`profiles/updates`, `*/pkg` atoms, `masters` in overlay `layout.conf`).
`l0-fx-20260915T101508Z` is green: 12 cases, 5 clean, 20 explained, 0
unexplained — diamond, `anyof`, `iusedefaultpkg`, `dualslotpkg` and
`blockusedeptarget` produce byte-identical merge lists; the rest differ
only in staging-path/wording
(allowlisted). The oracle immediately earned its keep: it caught
**backlog #54**, the installed-consumer pin over-approximation from the
Tier 2.25 port — real 3.0.82.2 only walks an installed consumer reachable
from `@world ∪ @selected ∪ @system` over the installed dependency graph
(`_complete_graph`'s required-set walk, `depgraph.py:8677-8731`; the
attribution above to an existing slot conflict alone was incomplete —
auto-enable also fires on any plain version/USE/slot change,
`depgraph.py:8591-8647`), so `--update paired`, `needer` and
`=paired-2.0` behaved differently in portuale, and the triangle's block
named keeper where real named only `othermod`. **DONE 2026-09-15**:
fixed by gating the consumer scan on `ResolveCtx::slot_op_reachable`
(`docs/backlog-tasks.md` #54); the contract pins that encoded the
divergent behavior were rewritten in `TEST/findings/l0-fixture-oracle.md`
"#54 S0"-"S2". A narrower residue (the triangle's block disappearing
entirely rather than gaining the right parent) was filed separately as
#57.

### #50 / §7 — bulk translation of upstream resolver tests (M with F review; time-boxed, see step 1)

**State:** not started as *bulk* translation, but the *manual* pattern
is proven and already used ~34 times — see the two case studies in
`docs/recap-of-backlog-ops-2026-09-15.md`'s "Oracle methodology"
section (translate the matching `lib/portage/tests/resolver/*.py` case
into a standalone script that imports `ResolverPlayground` directly
against the vendored, pinned portage, read its actual output, watch for
a test file that rebinds its own `test_cases` tuple before the loop
that consumes it, pin `PYTHONHASHSEED=0`). `3rdparty/portage/lib/portage/tests/resolver/`
has 106 files / 112 `ResolverPlayground` references; the goal is a
script that emits a `fixtures/` tree plus an expected-mergelist pin per
case, not 106 more manual translations.

**Steps:**
1. **S0 (F, time-boxed 4-6 h) — spike one file's automation.** Pick a
   file not already hand-translated (grep the existing `test_oracle_*`
   docstrings in `tests/test_emerge_pretend_contract.py` for "upstream
   `test_X.py`" citations to find what's covered) with a simple,
   representative shape (a handful of `ebuilds{}`/`installed{}` dicts
   and one `ResolverPlaygroundTestCase`). Write a script that parses
   the file's own Python AST (or, more simply, `exec`s it inside a
   sandboxed namespace capturing the `ebuilds`/`installed`/`world`
   dicts and the `test_cases` tuple — the shape is already extremely
   regular across the 106 files) and emits: (a) `fixtures/repo/<cat>/<pkg>/`
   ebuilds + md5-cache from each `ebuilds{}` entry, (b) vdb entries from
   `installed{}`, (c) a world file, (d) for each case, run the *real*
   `ResolverPlayground` (not just read `mergelist=[...]` off the test
   source — the file-rebinds-`test_cases` trap from §7's existing note
   means the literal source isn't always the executed oracle) and
   record its actual mergelist as the pin.
2. **S1 (F review, time-boxed) — verdict.** If the automation handles a
   clear majority of the 106 files' shapes with only hand-fixups for
   outliers (custom `world`/`package.use`/multi-slot setups): continue
   to S2. If the shapes are too varied for one script to be worth it:
   stop, document why in this file's own table, and keep doing the
   existing one-at-a-time manual translation for whichever cases a
   later resolver item specifically needs (as #23/#24/#35/#36 already
   did) rather than bulk-translating speculatively.
3. **S2 (M, ongoing) — run it.** One commit per batch of translated
   files (not all 106 at once — each batch needs its own fixture-name
   collision check per `AGENTS.md` step 5, and each new pin is real
   evidence, not just more test count). A case that reproduces a known,
   already-tracked divergence gets `xfail(strict=True)` with a pointer
   to the tracking item, not a silent skip.

**Acceptance:** the go/no-go verdict from S1 is recorded here either
way (bulk translation attempted-and-worked, or attempted-and-not-worth-it
with the reason); if it proceeds, a running count of upstream files
translated vs remaining, kept in this table.

**Verdict 2026-09-15: GO.** `scripts/upstream_resolver_translate.py`
captures the whole corpus without parsing test sources: it wraps the
real `ResolverPlayground` (constructor inputs + every `run()` result) and
dumps JSON, so the oracle is the executed resolution, not the source's
`mergelist` literal. A full sweep of the pinned checkout:

```sh
scripts/upstream_resolver_translate.py --all --stats
# -> 99 test_*.py files (the directory's other 7 entries are the
#    playground/package files), 99 captured, 95 with cases, 1032 oracle
#    cases, 0 upstream test errors, 0 import errors, 0 crashes
```

`--emit-fixtures DIR` writes a `fixtures/`-shaped tree per playground
(ebuilds + md5-cache from `ebuilds{}`, vdb entries from `installed{}`, a
world file) plus `cases.json` with the recorded mergelists; a shape
report marks what needs hand translation. A 12-file sample: of 32
playgrounds, 4 are ebuilds-only, 17 carry `installed`/`world` (emitted),
and 11 need `user_config`/`repo_configs`/`profiles`/`sets`/`binpkgs`
mapped by hand per batch. Running count: **99 upstream files / 1032
cases captured, 0 bulk-emitted into the contract suite** (the ~34
hand-translated cases predate this tool). Each batch still needs the
`AGENTS.md` step-5 collision check and its own pins.

### #51 / §9 (second half) — determinism under shuffled input order (M, 4-8 h)

**State:** repeated-run determinism (§9 first half — the same `CASES`
entry run 3-5 times, diffed) is Done and already found one real race
(`dc8022b`). The **shuffle** half — deliberately randomizing `read_dir`
result order and `repos.conf` section order, then confirming output is
unchanged — is not started. 56 production `read_dir(` call sites exist
across the workspace (confirmed count, `rust/{mrg-director,portage-profile,
portage-repo}/src/lib.rs` + 15 `rust/portuale/src/*.rs` files) plus
whatever `repos.conf` section-order dependence `portage-profile`'s repo
loader has.

**Steps:**
1. **S0 (M, 1-2 h) — a shufflable `read_dir` seam.** Rather than
   auditing and shuffling 56 call sites individually, introduce one
   thin wrapper (e.g. `portage_util::read_dir_sorted_or_shuffled` or a
   `cfg(test)`-only override) that every production call site already
   funnels through if one exists, or a new one all 56 are migrated to —
   check first whether they already share a helper (some may already
   sort deterministically, which would make "shuffle" a true stress
   test rather than a first-time determinism fix). A `PORTUALE_SHUFFLE_DIRS=<seed>`
   env var (test/CI-only, documented as such) makes the wrapper
   fisher-yates-shuffle its `read_dir` results using that seed before
   returning them.
2. **S1 (M, 2-3 h) — `repos.conf` section order.** `portage_profile`'s
   repo-config reader: confirm it already produces a canonical,
   priority-sorted repo list regardless of file order (most of these
   readers do, since `main-repo`/priority fields exist precisely to
   avoid file-order dependence) — if it does, this half is a
   regression test, not a fix; if it doesn't, sort at load time.
3. **S2 (M, 2-3 h) — the test mode.** Extend `test_repeated_runs_are_byte_identical`
   (or a sibling test) to run each `CASES` entry N times with
   `PORTUALE_SHUFFLE_DIRS` set to N different seeds (plus a `repos.conf`
   with its `[repo]` sections written in reverse/randomized order for
   the repo-config half) and assert byte-identical output across all of
   them. Any difference found here is a real, previously-undetected
   nondeterminism bug — fix it with an explicit sort at the point the
   directory listing (or repo list) is consumed, not by removing the
   shuffle.

**Acceptance:** the shuffle wrapper exists and is wired through every
production `read_dir` site (grep-verifiable, the same style §4's "one
producer site" acceptance uses elsewhere in this repo); the extended
determinism test passes across multiple shuffle seeds; any bug the
shuffle found is fixed and cited in this file's table.

**Done 2026-09-15.** `rust/portage-util` is the new seam:
`read_dir_entries` sorts by file name and Fisher-Yates-shuffles under
`PORTUALE_SHUFFLE_DIRS` (seeded per (seed, directory); unit test pins
sorted-default, same-seed reproducibility and different-seed
difference). **All 56 production `read_dir(` sites across
`portage-repo`, `portage-profile`, `mrg-director` and `portuale` now call
it** (the only remaining `std::fs::read_dir` is inside the helper), so
the grep-verifiable acceptance holds. `repos.conf` section order: the
repo list was already priority-sorted; `test_repos_conf_section_order_
does_not_change_output` now pins it (reversed sections in a copied
configroot, symlinked repos). The shuffle found no
order-dependence bug — every `CASES` entry × 3 seeds is byte-identical,
as is the reversed-`repos.conf` run.

### #52 / §5 + §10 — metamorphic tests and mutation testing (M for §5, S for the §10 install step + F review of survivors; §5 4-8 h, §10 ongoing)

**§5 metamorphic tests — not started.** Six input transforms that must
never change output, per this file's own §5 list: duplicate an IUSE
token; add an unrelated package to the repo; add an overlay with
nothing relevant; consistently rename a fixture category; reorder
non-overlapping `package.use` lines; run the same command twice (this
last one is already §9's job — drop it here to avoid duplicate
coverage).

**Steps:**
1. **S0 (M, 4-8 h).** A pytest fixture-transform helper (parallel to
   the existing `fixtures/` machinery, not a new fixture *tree* per
   transform): given an existing `CASES` entry's fixture inputs, apply
   one of the five remaining transforms programmatically (append a
   duplicate IUSE token to one ebuild's metadata, copy in an unrelated
   `dev-libs/mtunrelated` package, add an empty overlay repo to
   `repos.conf`, rename `dev-libs` to `dev-libs-renamed` everywhere
   including every atom string, shuffle two non-conflicting
   `package.use` lines), re-run the same `CASES` entry against the
   transformed tree, and assert byte-identical output to the
   untransformed run. Five transforms × a representative subset of
   `CASES` (not all ~480 — pick the ones already flagged as
   IUSE/IUSE_EXPAND/IUSE-heavy in the suite) is enough; this is a
   property test, not an exhaustive one.
2. Any transform that changes output is a genuine bug (the IUSE-dedup
   bug this file's own motivating table cites is exactly this shape) —
   fix it, and keep the failing case as a permanent regression fixture
   rather than reverting to "must not change" as an assumption.

**§10 mutation testing — not started, blocked on tool install.**
`cargo-mutants` is not installed on this host (`which cargo-mutants`
finds nothing) or apparently anywhere it's been checked before.

**Steps:**
1. **S0 (S, install + first run, time-boxed 2-4 h; expect this to be
   slow — `cargo-mutants` rebuilds the crate under test per mutant).**
   `cargo install cargo-mutants`, then `cargo mutants -p portage-repo
   --timeout <N>` (start with a subset via `--file` on one smaller
   module rather than the whole ~35k-line `lib.rs` on the first run, to
   get a feel for wall-clock cost before committing to the full crate).
2. **S1 (F review, ongoing) — triage survivors.** A surviving mutant
   means no test distinguishes the mutated behaviour from the original
   — exactly the failure mode this whole Tier-6 effort exists to catch
   now that the Python mirror can't catch it by accident. Target
   survivors with §1 (an output-invariant gap), §5 (a metamorphic case),
   §6 (a real-oracle case), or §7 (an upstream-translated case) first,
   in that order, before writing a bespoke unit test for a mutant that a
   broader check would have caught too.
3. Run nightly/weekly once installed, not per-commit (the repo's own
   §10 framing) — document the cadence and where results land
   (a `TEST/findings/mutants.md` or similar) once S0 shows the
   wall-clock cost.

**Acceptance for #52:** §5's five transforms pass over a representative
`CASES` subset (or a bug they found is fixed and kept as a fixture);
`cargo-mutants` runs at least once against `portage-repo` with a
recorded survivor count and at least the top few survivors triaged into
one of the categories above.

**§5 done 2026-09-15.** `tests/test_metamorphic.py` applies the five
transforms to 13 representative `CASES` (at most three per IUSE-heavy
fixture atom), each on a private `fixtures/` copy with the transform
asserted to have changed the tree; pristine output is cached per case.
65 passed, no transform changed the output — including the duplicated
IUSE token, which is exactly the class the removed mirror shared. The
sixth item (same command twice) is §9's existing repeated-run test.
**§10 in progress:** `cargo-mutants` v27.1.0 installed on this host;
first-run results are recorded in the table above / backlog #52.
