# Strong review of backlog #441 (`@world` plan pre-elaboration)

Date: 2026-10-20. Reviewer: Claude Sonnet 5.5.
Reviewed: the design [`2026-10-20-world-plan-preelaboration.md`](2026-10-20-world-plan-preelaboration.md)
and its first review [`...review.md`](2026-10-20-world-plan-preelaboration.review.md)
(called "review 1" below).
Method: every code claim in both documents was re-read in the tree at
`b11e5ab0`; the `@world` run was re-timed and stack-sampled on this host
(evidence at the end). Portage behaviour was **not** re-probed: nothing here
rests on a new Portage claim except where marked "needs oracle".

## 0. Verdict

The direction (derived caches with transparent fallback, L0 stays the truth)
is right. The **plan is not ready to implement as written**, for four reasons:

1. Nobody has measured where the time goes, and both documents already
   *state conclusions* (review 1: "tens of milliseconds", "2000x", "<30 us")
   that no number supports. My own 24-sample profile (§3) is **diffuse**: it
   does not point at one multiplier.
2. L3a (replaying rendered stdout/stderr) is unsafe as specified: the output
   contains things that are not functions of the fingerprint (§2.1).
3. Review 1 contains several confident claims that are wrong or unproven
   (§2.3). Two of its proposed fixes would change Portage-visible behaviour.
4. There is no frozen, reproducible benchmark. The 194 s figure came from
   the owner's live system; on this host today the same command aborts with
   an autounmask report after 72 s (§3). A target of "<= 1 s" cannot be
   accepted against a moving workload.

What survives and is worth doing first: S0 (profile), the
`all_installed_packages` clone (§4, new), the `-D` re-walk (§2.2, with a
caveat), and `ResolveCtx::new`'s three BFS runs.

## 1. Claims verified true

| Claim (source) | Evidence |
|---|---|
| `ResolveCtx::new` runs `required_set_reachable_cps` three times (review 1 §2.2.A) | `crates/portage-repo/src/resolve/ctx.rs:144-171`; each call re-reads `USE` and four `*DEPEND` strings for every reachable installed package and re-parses atoms (`slot_operator/eliminate.rs:311-361`). `driver.rs:196` builds a second ctx after restarts. |
| `-uD` with `@world` takes the `else if auto_enable` branch and re-runs phase 1 with `&empty_reuse` (review 1 §2.2.B) | `pretend/resolve_phases.rs:443,466`: `want_complete` needs `deep == NotRequested`; with `-D` it is false. The branch comment itself says "a deterministic input means an identical walk". |
| `select_nodes` has the `position`-in-sort and the per-round `HashSet` rebuilds (review 1 Alg. 6.1) | `merge_order/select_nodes.rs:75,82-113,191`. The three sets are empty whenever `removals` is empty. |
| `installed_slots` stores the version `String`, not an index (review 1 §2.2.E) | `resolve/pass_state.rs:92`. |
| `DefaultHasher` + xor-fold is wrong for persistent keys (review 1 §3.5) | `portage-profile/src/fingerprints.rs:11-26`. Correct, and the design should say so in §6.1. |
| `redb` refuses a second open in-process/cross-process (review 1 §3.2) | test `a_held_redb_file_is_busy_in_process`, `crates/portuale/src/mrg.rs:2479`. |
| `-N` has no memo in `reinstall.rs` | no `memo`/`thread_local` in the file. Callers may memoise upstream: not checked, S0 must count calls. |

## 2. Defects

### 2.1 Design: what L3a would replay wrongly (blocking)

The design says L3a is safe because output is "deterministic". It is not,
even for the owner's own command. The run I did printed, after the plan:

```
 * IMPORTANT: 1 news items need reading for repository 'gentoo'.
```

and, on stderr, `[gentoo] Local copy of remote index is up-to-date and will
be used.` (the `--getbinpkg` binhost refresh, `emerge_getbinpkg::refresh_binhost_indexes`).

- The **news notice** depends on the news read-marker files and on
  `metadata/news`, none of which is in `StateFingerprint` (§6.1). A cached
  stdout would keep printing a stale or missing notice after
  `eselect news read`. Fix: the news notice (and any other post-plan
  notice) is rendered live, never cached.
- **Binhost freshness** is a time-dependent input: Portage re-checks the
  remote `Packages` against its TTL on every `--getbinpkg` run. A plan
  cache hit that skips `refresh_binhost_indexes` changes semantics and can
  plan against a binhost that has since moved. The key needs the binhost
  `TIMESTAMP`/TTL decision, or L3 must run the refresh first (a network
  round trip, so "<= 1 s" is then bounded by the binhost).
- The fingerprint must also cover everything `resolve_config` reads that
  review 1 also did not list: `metadata/layout.conf` per repo,
  `profiles/updates/` (the `move`/`slotmove` rewrites applied in
  `installed.rs`), `package.provided`, `repos.conf` priorities, the
  `FEATURES` that gate `metadata-transfer`, and `/etc/portage/sets`.

**Recommendation:** drop L3a from S3. Cache `GraphResult` only (L3b) and
re-render every time; rendering is only worth caching after a profile says
display is large, and the sample suggests `use_display` is a real but
partial cost (§3). This is also review 1's own recommendation 3, which I
endorse for a stronger reason than the one it gives.

### 2.2 Design: the `-D` re-walk is real, but review 1's fix is unsafe

`resolve_phases.rs:421-424` records a finding from slice r25: *"an unseeded
first walk can misfire a probe the seeded re-walk avoids"*. So the seeded
walk is not just a repeat; it exists because the unseeded one can be wrong.
Review 1's "pass `with_seeds = true` from the first pass" would make the
`auto_enable` decision from a different walk, and its "Phase 2 is
mathematically a no-op when no external consumer is impacted" ignores
`backtrack_restarts`, `rebuild_if_reinstalls` and the slot-op scan, all of
which the re-walk can change (`resolve_phases.rs:423-424,471-474`).

**Needs oracle** (use the ResolverPlayground oracle, see memory
`resolver-playground-oracle`): on fixtures where a `-D` run reaches this
branch, compare (a) current output, (b) first-walk-seeded output, (c)
Portage. Only if (a)=(b)=(c) over the L0 cells and a world-shaped fixture is
skipping the second walk acceptable. The cheaper, safe step that needs no
oracle: have the second walk reuse the first walk's caches (the VDB-derived
reachability sets do not depend on seeds beyond their inputs) and share the
installed-metadata memo, instead of removing the walk.

### 2.3 Review 1: claims to correct before anyone builds on them

1. **"<= 4/3 approximation" for HLFET (Alg. 6.2)** is wrong. List
   scheduling for `P|prec|Cmax` has Graham's bound `2 - 1/m`; `4/3` is the
   LPT bound for *independent* tasks. The surrounding stage design (Tarjan,
   build/merge split, `RootMergeLock`) is also an `mrg` scheduler redesign,
   unrelated to the 194 s `emerge -p` problem. Move it out of #441; it should
   not appear in the roadmap that gates the speed-up.
2. **Alg. 6.2 "RDEPEND is hard when the package has `pkg_preinst`/`src_test`"**
   is invented. Portage's priorities (`_emerge/DepPriority.py`) are the
   authority; this rule appears nowhere in them.
3. **Alg. 7 (ctime/Merkle)**: a category directory's `ctime`/`mtime` change
   when entries are added or removed, **not** when a file *inside* an entry
   is rewritten in place. So "102 `statx` catch everything" is false for the
   exact in-place-edit case the design lists as a risk (§18). The needed
   guarantee is Portage's `_bump_mtime` contract, which the code already
   uses: `all_installed_packages` is keyed on
   `portage_vdb::for_root(root).generation()` (`installed.rs:718`, "~30
   `stat`s"). Algorithm 7 mostly re-proposes machinery that exists; the "102"
   count is a guess.
4. **Alg. 8 (L4-A "plan contraction")** asserts that after merging a subset
   of the planned versions the new plan equals the old one with those nodes
   promoted to `AlreadyInstalled`. That fails whenever the new installed
   state changes a decision: `||` installed-preference (`AltPreference`),
   `-D` rev-dep constraints, `:=` rebuild triggers, `--newuse` against the
   newly built package. It is a hypothesis to test, not "esattezza formale".
   The L4-B read-footprint argument is only valid if the footprint records
   **negative** lookups (absent cp), config reads and iteration order; the
   text does not say so.
5. **Alg. 2 (bitsets)** ("~5 ns", "2000x", "99% fast path") has no
   measurement behind it. Also `reinstall_flags_for_use_change` must honour
   `use_stable_force/mask`, `(+)/(-)` defaults and `USE_EXPAND` prefixes
   (see memory `autounmask-use-plus-default-flag`); a global bit universe has
   to be built per config fingerprint, and `use_force | use_mask` is not a
   single mask in the presence of stable variants. Worth doing only if S0
   shows `-N` is a top cost.
6. **Alg. 3 (CSR from the VDB)**: valid for `required_set_reachable_cps`,
   which reads only VDB-recorded strings. **Not** valid as stated for
   `reverse_dependency_constraints`, `collect_unwalked_installed_blockers`
   and `vdb_edges`, which read the *live repo* metadata under
   `dynamic_deps`/`ignore_built_slot_operator_deps`
   (`live_metadata_for_installed`). The key must include those flags and the
   repo snapshot, as review 1 half-says ("`dynamic_deps`") but its
   complexity claim ignores.
7. **"Tens of milliseconds / <30 us / 0.5-1.5 s"**: unmeasured. Strike them
   from any acceptance text; they are hypotheses.
8. **Evidence is not reproducible**: review 1 cites `helpers/vdb_to_db.md`
   for the "447,658 `statx`". `helpers/` is gitignored, and that figure is
   from a **27 s** run (`emerge -puDN @world`), not the 194 s run. Do not
   cite untracked files as evidence; copy the numbers into
   `LLM/evidence/`.
9. **Decision 5** ("`repo.postsync.d` rebuilds only the `RepoSnapshot`") is
   a good point and I agree, but it presumes the snapshot exists, which is
   S2. See ordering in §5.

### 2.4 Design, smaller

- §2.3 lists nine files for "linear scans" without line numbers or counts;
  only `select_nodes.rs` is confirmed above. Treat the rest as unchecked.
- §9 S0 says "when the terminal that implements the backlog is free"; that
  gate is stale. S0 needs no tree changes beyond counters.
- No acceptance criterion for **memory** (snapshots of ~30k ebuilds plus
  `reverse_index`) or for the hit-path cost with 2099 VDB entries
  (`generation()` is ~30 `stat`s; fine, but say so).
- Hand-rolled mmap format: the project is `unsafe`-averse (check the
  workspace lint level before committing to `mmap`); reading the whole file
  is the safer default and probably fast enough at the sizes quoted.
- Non-root users: `emerge -p` is exempt from the privilege gate, so it must
  work with an unwritable cache dir. The design says "fallback" but §8.3's
  "files 0644 in a root dir" means non-root readers always hit and never
  write; state it.
- Cross-reference `LLM/code-notes/` conventions: any new `Portage:` cite goes
  to code-notes, not into comments (AGENTS.md).

## 3. What I measured (a first slice of S0)

Command: `portuale emerge -uDpvN @world` (release binary of `b11e5ab0`),
stack-sampled every 3 s with `eu-stack` (24 samples, top 16 frames).
Result on this host today: **rc 1 after 72 s**, an `--autounmask` report
(`dev-cpp/simdutf atomic-base64`, `app-misc/mime-types nginx`), not the
194 s completed plan the design measured. So the owner's workload and this
host's differ, and the failure path is not the success path.

First leaf-ish `portage_*`/`portuale` frame per sample (counts of 24):

| Where | Samples |
|---|---|
| `merge_order::installed_closure::installed_candidates_by_cp` / `all_installed_packages` / `InstalledRef::clone` | 4 |
| `portage_vdb::files::aux_get(_field)` via `vdb_aux_get` | 3 |
| `use_display::build_use_expand_display(s)` | 2 |
| `parse_atom` / `parse_match_atom` / `match_atom_matches_candidate` (digraph prelude, `config_match`) | 4 |
| `resolve::cycles::find_hard_cycles` (circular dead-end step) | 2 |
| `emerge_getbinpkg::refresh_binhost_indexes` (binhost freshness) | 3 |
| binary-candidate collection / `binpkg_respect_use_ok` | 2 |
| `schedule_graph` / `DepEdge`/`GraphEntry` clones | 1 |
| `run_pass` initial outcome, `resolve_queue_atom` | 2 |
| config load (`resolve_config`, `make_conf_lookup`) | 1 |

Reading: no single hotspot; the largest identifiable item is repeated
cloning/indexing of the installed list, then atom parsing/matching. The
three samples in the binhost refresh are a **fixed cost that no plan cache
removes**. Twenty-four samples are too few for percentages. Use this only
to order the S0 counters, not to pick a fix.

## 4. New finding the other two documents missed

`all_installed_packages` returns `packages.as_ref().clone()`
(`crates/portage-repo/src/installed.rs:723,732`): even on a cache hit it
deep-clones a `Vec<InstalledPackage>` of ~2100 entries (several `String`s
each). It is called from at least: `blockers.rs:394,938`,
`rebuild_scan.rs:102`, `rev_dep.rs:172,411`, `installed_closure.rs:54,132`,
and `required_set_reachable_cps` (3x per ctx). Returning
`Arc<Vec<InstalledPackage>>` (or a slice) removes thousands of 2100-entry
clones per run for a signature change and zero behaviour change. This is the
obvious S1 first step, independent of every other proposal, and the sample
already shows it. Filed as #454.

## 5. Proposed ordering

The design's S0 -> S1 -> S2 -> S3 stays, with these changes:

1. **S0 (measure)**: freeze a benchmark first (a bed or fixture-based
   `@world`-shaped input, committed or reproducible from the bed image),
   *then* counters and a real profiler run (`perf` is not installed here;
   needs install or a `PORTUALE_PROFILE=1` phase timer). Accept: a table of
   top costs with percentages on the frozen input. #455.
2. **S1a**: `Arc` installed list (§4). **S1b**: the three reachability BFS
   share one pass and one `USE`/dep parse per package (safe: same inputs,
   same outputs; diff the three sets in a test). **S1c**: `select_nodes`
   rank map and `removals.is_empty()` skip. **S1d**: phase-2 reuse of
   caches (§2.2); the walk-skipping variant only after the oracle check.
   Every step byte-identical on the contract suite; no oracle needed except
   S1d's variant.
3. **S2** only for what S0 still shows large after S1. Re-measure.
4. **S3**: L3b only, no stdout/stderr replay (§2.1), key per §2.1 plus
   review 1 §3.3's resolve/render split, SHA-256 or BLAKE2 (both already
   dependencies).
5. **S4+** (`mrg` server, scheduler, delta, portfolio): out of #441's
   acceptance; file separately.

## 6. Time per phase with cheap agents

These are **estimates**, not measurements. Basis: the cost of past slices in
this repo (a normal slice incl. verification is a day of wall-clock with
opencode `muse-*`/gemini executing and me reviewing), and the fixed gates
that cannot be sped up by a cheaper model: `scripts/pytest-parallel.sh`
~90 s, `cargo test --release` + clippy a few minutes, the L0 bed
`l0-resolver.sh` heavy (tens of minutes), glibc/bash test merge only if the
merge path is touched (it is not here). "Cheap" = Haiku for mechanical work
(`verify-pass`, `probe-capture`, `closeout-scribe`, `fixture-smith`),
opencode agents for code. Judgment steps (parity decisions, review of code and
tests) stay with me, as per the agent-delegation rules, and are included as
"review".

| Phase | Agent work | My review + gates | Retry risk with cheap models | Total (wall-clock) |
|---|---|---|---|---|
| Frozen benchmark + S0 counters/profile | 1-2 h (counters behind an env var, 1 run) | 1 h | low; counters are mechanical, the report needs a human read | **0.5 day** |
| S1a `Arc` installed list | 1-2 h (signature ripple across ~8 sites) | 1 h (workspace-wide build/clippy, contract suite) | low | **0.5 day** |
| S1b shared reachability pass | 3-5 h | 2 h (diff-test the three sets) | medium: Haiku tends to merge sets that differ by seeds | **1 day** |
| S1c `select_nodes` fixes | 2-3 h | 1-2 h | low-medium | **0.5-1 day** |
| S1d phase-2 reuse | 4-6 h | 3 h + oracle probe (portage-oracle, frontier) | **high**: parity-sensitive; cheap agents will "fix" the output rather than stop | **1-2 days** |
| S2 snapshots (repo/binpkg/installed, format, fallback) | 2-4 days across several agents | 1-2 days (format review, fuzz/corruption tests, parity vs md5-cache) | medium-high: binary format and invalidation are where cheap models silently cut corners | **4-6 days** |
| S3 L3b plan cache + key + sensitivity tests | 2 days | 1 day (the "one mutation per key component" tests must each fail first) | high: false hits are the failure mode and tests that pass vacuously are likely (memory `metamorphic-test-was-vacuous`) | **3 days** |
| S4 `mrg` server | 3-5 days | 2 days | high | **5-7 days**, out of #441 scope |
| S5-S7 | research, not scheduled | | | not estimated |

So **S0+S1 (the part I would approve now): about 3-4 working days**, S2+S3
another 7-9. The headline risk to schedule is not agent speed: it is that S0
may show the cost is spread so thinly that the whole plan reaches 20-40 s,
not 1 s, which changes whether S2/S3 are worth it. Decide after S0.

## 7. Backlog follow-ups filed from this review

- #454 `all_installed_packages` returns a deep clone per call (§4).
- #455 frozen `@world`-shaped benchmark and S0 profile for #441 (§5).

#441 stays OPEN; its text now points here.

## Evidence

Sampling script and 24 stack files: scratchpad of this session, not kept.
Reproduce: build `b11e5ab0`, run `portuale emerge -uDpvN @world` while
`eu-stack -p <pid>` is looped every 3 s; expect rc 1 and the autounmask
report on a Gentoo tree matching the owner's today. No tracked file was
changed by the run.
