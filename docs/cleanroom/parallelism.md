# Parallelism options: using more than one CPU

Status: **proposal (not implemented)**, revised 2026-09-20 after the
`#102`–`#108` memoisation batch (`bf52b1a`…`662b15d`) landed. The first
draft (`fa4fb6c`, same day, 19:24) was written against `ce68f77`, i.e.
*before* that batch, and three of its load-bearing premises no longer
hold — see §-1. Read §-1 before acting on any slice below.

Goal: let portuale use N CPUs where it is safe, without breaking the
determinism contract or the musl-static story.

Source grounding: cites below name **symbols, not line numbers** — the
repo convention (`docs/08.102-108-perf-memoisation.md` header: "find
things by symbol name, not line number"); the first draft's `path:line`
cites had already drifted in 16 of 19 cases within hours of being
written. Files: `rust/portuale/src/*.rs`,
`rust/portage-repo/src/lib.rs`, `rust/portage-profile/src/lib.rs`,
`rust/portage-profile/src/phase_environ.rs`,
`rust/portage-util/src/lib.rs`, `rust/portage-dep/src/lib.rs`.

## -1. Re-grounding (2026-09-20, post-`#102`–`#108`)

Three corrections, each measured on this host (24 CPU, 2089 installed
packages, release build of `662b15d` minus the docs-only close-out):

**(a) `--pretend` is no longer slower than real.** The `#102`–`#108`
batch took the reference workload from 7.71–7.92 s to 3.98–3.99 s; a
re-run today gives **3.88–3.90 s wall / 2.85–2.90 s user / ~1.0 s sys**
against real Portage 3.0.82.2's 4.50–5.11 s / 4.07–4.16 s / ~0.2 s
(`docs/performances-tuning.md`, "2026-09-20 batch"). The first draft's
priority 1 ("the user is waiting for the plan") was written against a
2 s deficit that is now a 0.6 s surplus. Parallelising the resolver is
therefore no longer a *user-facing* win; it is a "be even faster than
real" want, and it must be ranked below the two single-threaded items
in §2.0 that are still unclaimed.

**(b) The metadata-read premise is wrong; the I/O is the vdb.** The
draft's §0/§2 assumed md5-cache reads dominate ("~20k+
`metadata/md5-cache` files and the resolver touches hundreds per
plan"). Measured call counts (plan §1) say `read_md5_cache` is
36,811 calls / **33,608 hits** (3,203 actual file reads) and
`list_candidates` is 17,765 / **17,157 hits** (608 misses). Neither
appears in the CPU flat profile, and md5-cache does not appear in the
top 25 opened paths at all. Today's `strace -c` on the reference
workload: 160,351 `openat` (**40,651 ENOENT**), 202,281 `read`,
294,450 `statx`, 42,524 `getdents64`. Of the successful opens, ~88 k
are `/var/db/pkg/<cat>/<pf>/{USE,RDEPEND,repository,SLOT,BDEPEND,
DEPEND,IUSE,IDEPEND,PDEPEND}` — one uncached `fs::read_to_string` per
key per call, in `read_vdb_string` / `read_vdb_flag_set`. That is the
residual ~1.0 s sys, and it is a **memoisation / real-parity** target
(§2.0), not a parallelism one.

**(c) Constraint "the caches are already thread-safe" is now false.**
This is the correction that changes the *design*, not just the ranking
— see §1.4.

What survives unchanged: every constraint in §1 except 4; the whole of
§3 (the merge copy loop is still serial, and `#96` made it *more*
parallel-friendly, §3.0); §4's two-knob split; §6's verification
recipe.

## 0. Where the single-CPU ceiling comes from

Still accurate as a structural description; the cost weights have moved
(§-1b).

- The resolver (`portage_repo::resolve_pretend_graph`, via
  `pretend::run`) is a single-threaded BFS. Per visited package it
  calls `list_candidates` (cached, 96.5 % hit) →
  `list_candidates_uncached` (one `read_dir_entries` + one
  `repo_aux_metadata` per `.ebuild`, in a `for` loop) → per-candidate
  `is_visible` → `metadata_key_accepted` → `effective_use_flags` →
  `match_from_list` + `use_reduce_flat`. `--ask` shares this exact path
  (it resolves first, then prompts).
- **Where the CPU actually goes now** (plan §1's call graph, totals
  with children, overlapping): `is_visible` 25.4 %,
  `effective_use_flags` 25.0 %, `installed_candidates` 21.9 %,
  `metadata_key_accepted` 21.7 %, `disjunction_preference` 18.9 %,
  `installed_cp_sources` 12.9 %, `resolved_use_mask_or_force` 11.3 %.
  Four of those seven were memoised by `#102`–`#104`/`#108`; the
  un-memoised remainder is `metadata_key_accepted` (pure in its eight
  args — a memo candidate) and `disjunction_preference` (takes
  `&[GraphEntry]`, i.e. live graph state — neither a memo nor a safe
  parallel target; see §2.3).
- Config load (`portage_profile::resolve_config`) is a serial walk of
  the profile `parent` chain + `make.conf` + every `package.*` file,
  all through the single sorted-`readdir` seam
  (`portage_util::read_dir_entries`).
- Binary merge (`ebuild_merge::merge_binpkg` → `merge_tree`, reached
  from `run_merge`) is a serial `stack`-driven DFS copy: one
  `symlink_metadata` + (for regular files) one MD5 + one
  atomic replace + one `CONTENTS` line per path, in one thread.
- Fetch (`fetch::fetch_src_uri`) loops serially over
  `group_by_filename` groups: verify → lock → download → verify, one
  file at a time, one `wget` subprocess at a time.
- Already parallel (do not re-solve): source-build scheduling
  (`emerge_build::run_build_scheduler`, `std::thread::scope` + `mpsc`,
  gated by `mrg-director::SchedulerPolicy` jobs/load-average)
  parallelizes **package builds** under `-jN`/`--jobs`;
  `regen::run_parallel` does the same for `--regen --jobs`. Merges
  inside the scheduler stay deliberately serial (`--jobs`' own parse
  comment in `pretend.rs`: "The vdb merge step is always serialized
  regardless -- only the `install` phase runs in parallel, matching
  real portage"; `--buildpkgonly` and the binary-merge paths stay
  serial too).
- **Not** work parallelism, despite using threads — do not count these
  as precedent: the decompressor stdin pump in `binpkg::
  read_inner_metadata`, the phase output pumps in `ebuild_phases`, and
  the test HTTP servers. Dependencies: `Cargo.toml` still has no
  `rayon`/`threadpool`; `tokio` is `rt` + `rt-multi-thread` for the
  brush backend only, not a work pool.

So `-j24` already helps a 24-package source build, but does nothing for
`--pretend`, for a binpkg-only merge run, or for the file copies inside
any single merge.

## 1. Hard constraints any design must respect

1. **Deterministic output.** The contract suite pins `--pretend` output
   byte-for-byte (plus the harvested corpus) and directory reads go
   through the single `read_dir_entries` seam (sorted by default, seeded
   shuffle only under test-only `PORTUALE_SHUFFLE_DIRS`). Parallel work
   must join into a deterministic order (collect-then-sort, as
   `regen::run_parallel` already does) — never emit plan lines,
   `CONTENTS` lines, or VDB entries in completion order.
2. **musl-static, near-zero dependencies.** The binary ships statically
   linked with pure-Rust deps (see `rust/portuale/Cargo.toml` waivers).
   Prefer `std::thread::scope` + `mpsc` (the two existing parallel sites
   already use exactly this) over adding `rayon` or a tokio work pool.
   No new C linkage, no new external fetch.
3. **Merge correctness is serial at the package level.** Real
   `dblink.merge()` merges one package at a time (collision-protect,
   `CONTENTS`, `cfgfiledict`, preserve-libs registry). Keep that:
   parallelize **within** one package's merge, never two packages'
   merges against the same `${ROOT}` concurrently — unless behind an
   explicit opt-in knob with the race documented (see §3.3).
4. **The hot caches are `thread_local` + `Rc`, NOT shared.** *(Corrected
   2026-09-20 — the first draft asserted the opposite, and the design
   in §2.2 rested on it.)* After `#102`–`#108`, every memo on the hot
   resolver path is thread-local:
   - `EUF_CACHE` (`effective_use_flags`) → `Rc<HashSet<String>>`
   - `RUMF_CACHE` (`resolved_use_mask_or_force`) → `Rc<HashSet<String>>`
   - `CANDIDATES_CACHE` (`installed_candidates`)
   - `BINPKG_CACHE` (`local_binpkg_index`) → `Arc<BinaryIndex>` in a
     thread-local cell
   - `cp_bucket_index`'s `CACHE` → `Rc<CpBucketIndex>`
   - `portage_dep`'s `ATOM_CACHE` / `CANDIDATE_CACHE`
   - `AUX_CACHE` (`vdb_aux_get`'s per-instance aux memo, `#109` S4) →
     `Rc<HashMap<String, String>>`
   - `MKA_CACHE` (`metadata_key_accepted`, `#110`) → `bool`

   Only `read_md5_cache`, `list_candidates`, `cached_binary_index`,
   `md5_dict`'s two `MEMO`s and the vdb `InstalledCache` are
   `OnceLock<RwLock<…>>` + `Arc`, i.e. shareable as the draft assumed.
   Three consequences:
   - `Rc` is `!Send`: a function returning `Rc<…>` cannot hand its
     result across a `std::thread::scope` boundary. Calling it *inside*
     a worker is fine; returning the `Rc` is not.
   - **Worker threads start with cold memos.** `effective_use_flags`
     runs at a 98.1 % hit rate (70,237 / 71,572) on one thread. N
     workers means up to N independent fills — more total work and N×
     the memo memory, against a baseline that is already faster than
     real. Fanning out the *consumers* of these memos can be net
     negative, and nothing in the current numbers says it won't be.
   - Therefore §2.2 has a **prerequisite**, not just an
     implementation: convert its target memos back to
     `OnceLock<RwLock<…>>` + `Arc`. That re-introduces per-hit lock
     traffic on the single hottest path in the program (~100 k lookups
     per resolve) and partially undoes `#103`/`#104`. Measure the
     lock-only change **first**, as its own commit, against the §6
     benchmark — if `Arc` + `RwLock` alone costs more than the fan-out
     can win back, §2.2 is dead and should be recorded as such.
     `performances-tuning.md`'s own close-out line states the house
     style this cuts against: "The `EUF_CACHE`-style thread-local
     `Rc`/`Arc` memo is now the established shape for per-resolve pure
     functions — reuse it before inventing a new one."
5. **`--load-average` and `--quiet-build` semantics stay.** Any new pool
   reuses `emerge_build::system_loadavg_1min` and the `SchedulerPolicy`
   trait rather than inventing a second gate.
6. **No new pool may be live across a `std::env::set_var`.** *(New,
   2026-09-20.)* Two production sites still mutate process-global env:
   `elog.rs`'s hook-env apply/restore and `remote.rs`'s
   `ConfigRootGuard` (`PORTAGE_CONFIGROOT`). Under edition 2024
   `set_var` is `unsafe` precisely because it races concurrent readers,
   and `read_dir_entries` reads `PORTUALE_SHUFFLE_DIRS` via
   `std::env::var` on **every call**. This binds §3/§4 (merge and build
   paths), not §2.

## 2. Resolver: `emerge --pretend` / `--ask`

Re-ranked: the two single-threaded items in §2.0 come first, because
they are larger, measured, real-grounded, and carry none of §1.4's risk.

### 2.0 Do these before any thread (recommended, not parallelism) — DONE 2026-09-21

**(a) Read the vdb `metadata` snapshot, like real does. DONE** (#109,
S1–S4 `bcc69fd`/`c909bfa`/`6f158d7`/`5759a29`, branch `feat/parallel`):
`vdb_aux_get` validates the consolidated `metadata` file like real
`_read_metadata_file` and serves in-set keys from it, with a
per-instance memo keyed on the package dir's `st_mtime_ns`
(`_aux_cache`'s shape). `strace` on the reference workload: `openat`
160,351 (**40,651 ENOENT**) → 33,685 (105), `read` 202,281 → 30,041;
sys 1.07–1.08 → 0.56–0.67 s. S3 alone was a small regression (per-call
snapshot reads); S4's memo is the win. Full suite byte-identical,
merge-path gate green at S2/S3/S4. Detail:
`performances-tuning.md` "2026-09-21 batch", `on-disk-caches.md` §1.

**(b) Memoise `metadata_key_accepted`. DONE** (#110, S5 `d034557`):
23.30 % with children on S0's re-profile → 13.13 %; wall 3.48 → 3.07–3.30 s,
user 2.47–2.55 s. Keyed on an explicit `MetadataKey` discriminant plus
`use_context_fingerprint`, the candidate/value strings and a new
accept-list fingerprint (the plan's literal key collided on the crate's
own masking fixtures).

**Verdict on the thread question (2026-09-21).** Both landed, and the
re-profile answers the section's own question: the resolver is now
~1.9–2.0 s wall / ~1.5 s user / ~0.4–0.5 s sys after the same-day #105
(USE-context digest frozen), #117 (`binpkg_respect_use_ok` memo), #114
(`is_visible` memo), #112 (one `statx` per vdb lookup) and #119 (lazy
`*DEPEND` rewrite) follow-ups. The residual is
`list_remote_binary_candidates` (13.03 %, first materialisation per
`(cp, visit)` -- its sharing was tried and withdrawn as #118, an honest
non-result), `binpkg_respect_use_ok`'s memo residual (12.63 %),
`parse_atom` (10.49 %, memo-hit clones), `effective_use_flags` (10.09 %)
and `repo_aux_metadata` (10.08 % after #119, cold fills/validation), plus
the second `run_pass` (#107, 75.46 % subtree — diagnosed 2026-09-21: the
restart is the reverse-dependency pin feedback, and removing it needs
real's complete-graph parent-atom model, so it is parked, not a thread
target) — **none of it parallel-friendly**. §2.2's visibility pool is
blocked on §1.4 and would now buy well under a second of mostly-serial
work at the cost of the `Rc → Arc` migration; **do not write it.** §2.1
was already demoted to "not worth ~600 cold reads" and the batches
confirm it. The only pool still worth considering is §3.1's merge copy
loop, which these batches did not touch.

### 2.1 Parallel metadata prefetch (was the first slice — now demoted)

What: parallelize the inner loop of `list_candidates_uncached` (one
task per `(repo, ebuild)` entry: `repo_aux_metadata` + `split_slot` +
struct build).

**Why demoted.** `list_candidates` is 96.5 % cached (608 misses per
resolve) and `read_md5_cache` 91.3 % (3,203 reads); neither appears in
the CPU flat profile or the top-25 opened paths (§-1b). The upside is
bounded by a few hundred cold reads.

**Also: the draft's safety argument cites a sort that does not exist.**
`list_candidates_uncached` returns candidates in `read_dir_entries`
order, per repo, in repo order — there is **no** trailing
`sort_by (repo_priority, version)`. Ordering is therefore *not*
"re-imposed by the existing sort"; a parallel version must reassemble
by index explicitly (`Vec<Option<Candidate>>` indexed by entry
position, or collect `(index, Candidate)` + `sort_by_key(index)`)
before returning, or the plan can change. Keep `PORTUALE_SHUFFLE_DIRS`
semantics: shuffle *before* dispatch, reassemble by index *after* join.

Knob: none new; a shared worker count (§4), `0`/unset = serial.

### 2.2 Parallel per-candidate visibility + USE (blocked on §1.4)

What: `is_visible` + `metadata_key_accepted` + `effective_use_flags` +
`match_from_list` per candidate in `list_candidates` consumers (notably
`visible_tree_matches` and the best-visible selection path). These are
pure functions of `(candidate metadata, config)` — dispatch one task
per candidate, join, then pick best exactly as today.

**Blocked, not merely "second":** see §1.4. The prerequisite
`Rc → Arc`/`RwLock` conversion must be measured on its own before any
fan-out work is written, and §2.0(b) may remove most of the target cost
single-threaded anyway.

### 2.3 Parallel BFS frontier (defer — now with a second reason)

What: pop K ready packages off the BFS queue and expand them
concurrently, merging into the shared graph under a mutex, with
deterministic commit order.

Why deferred: the BFS carries dedup/slot-conflict/backtracking state
(`runtime_pkg_mask`, autounmask feedback, `required_by` edges) that is
order-sensitive today. Second reason, from the current profile:
`disjunction_preference` is 18.9 % of the run and takes `&[GraphEntry]`
— the live graph — plus `&[QueueItem]`, so the frontier's *hot*
work reads exactly the mutable state a parallel expansion would have to
serialize. Do §2.0 first, measure; 2.3 only if pretend is still the
bottleneck, which on today's numbers it is not.

Backtracking itself stays serial in all levels (it is a short retry
loop over masks, not a throughput stage). Note `#107`: the second
`run_pass` is ~30 % of the run and is a **parity** bug, not a
throughput one — fixing it beats parallelising the pass, and
parallelising it would hide it.

## 3. Binary-package filesystem merge (now priority 1 of this doc)

`merge_binpkg` is: unpack outer container (GNU `tar` process today) →
read inner metadata (in-process `tar` crate) → `merge_tree` copy loop →
`write_vdb_entry` → `env_update`/ldconfig hooks. The copy loop
dominates wall time on a many-small-files package, and unlike §2 no
memoisation batch has touched it.

### 3.0 What `#96`/`#97` changed (2026-09-20, after the first draft)

`#96` (`34060fce`) made every merge write atomic: regular files go
through `replace_file_atomic`, symlinks through
`replace_symlink_atomic`, each copying to `unique_sibling_path(dest)`
(`.{basename}._portage_merge_.{pid}[.N]`, real `movefile.py`'s own
prefix), setting owner/mode/mtime, then `rename(2)` over `dest`. Two
consequences for §3.1, both favourable:

- Each work item is self-contained and touches only `dest` plus a temp
  **named after `dest`'s own basename**, so two workers on different
  paths can never collide. Phase B needs no new naming scheme — the
  existing one is already parallel-safe by construction (its doc
  comment even names "parallel-merge" as the counter-suffix case).
- `unique_sibling_path`'s check-then-create loop is a TOCTOU only
  *across processes*, which is the pre-existing situation, not a new
  one introduced by threads.

One thing the draft understated: `merge_tree` today is an explicit
`stack`-driven DFS that `read_dir_entries` each directory *lazily
during* the walk. Phase A is therefore a genuine restructure (one full
walk up front, building the list), not a refactor of a list that
already exists.

`#97` added `mrg-director`'s client merge driver as a second atomic
replace path — any `--install-jobs` work must decide whether it covers
that driver too, or explicitly does not.

### 3.1 Intra-package parallel file copy (recommended)

Two phases inside `merge_tree`, keeping today's semantics:

- **Phase A — scan (serial):** walk `${D}` once, building the ordered
  work list (path, type, stat, CONFIG_PROTECT decision inputs).
  Protection decisions that depend on live-destination state
  (`protect_decision`, `new_protect_filename`, `._cfgNNNN_` allocation)
  stay here, serial, so numbering and `cfgfiledict` updates keep
  today's exact behavior.
- **Phase B — copy (parallel):** dispatch fixed-size chunks of the work
  list to `std::thread::scope` workers: regular files → MD5 +
  `replace_file_atomic`; symlinks → `readlink` +
  `replace_symlink_atomic`; dirs → `create_dir_all`; special files →
  today's `rename`. Each worker returns
  `(index, CONTENTS line, error)`; the joiner writes `CONTENTS` in
  index order and returns the first error deterministically
  (lowest-index failure wins, not first-completed).

What stays serial: `collision-protect` scan, `cfgfiledict` read/write,
VDB entry write, `env_update`, preserve-libs registry update. Expected
gain: near-linear in file count up to ~8 workers on SSD/NVMe; small
packages (<~50 files) should skip the pool outright (threshold knob).

### 3.2 Parallel hashing (fold into 3.1)

MD5 (`md-5` crate, pure Rust) per regular file is the CPU half of the
copy loop. Do not add a separate hash pass — hash inside the copy
worker after the copy (or before, for the skip-if-identical check), so
hashing parallelizes for free with 3.1.

### 3.3 Inter-package parallel merge (NOT recommended by default)

Merging two packages into the same `${ROOT}` concurrently reintroduces
exactly the `collision-protect` / `CONTENTS` / `cfgfiledict` races real
portage serializes away — and portuale's own `--jobs` parse already
documents the same rule ("the vdb merge step is always serialized
regardless"). Options if ever wanted: (a) keep package merges serial
(recommended — binpkg runs are usually I/O-bound inside one large
package anyway); (b) allow it only behind an explicit `--merge-jobs=N`
with documented "same-file collisions between concurrently merging
packages are detected after the fact, not prevented" semantics. Do not
do (b) in the first slice. §1.6 (`set_var`) also binds here.

## 4. Source builds vs. install-to-filesystem (two knobs)

- **Build parallelism (exists): `-jN` / `--jobs[=N]`** — package-level
  concurrency in `run_build_scheduler`, plus `--load-average` gate.
  Keep as-is.
- **Inner-toolchain parallelism: already done and real-faithful.**
  *(Corrected 2026-09-20 — the draft listed "verify it is actually
  threaded" as an open action and mis-cited `pretend.rs`, which holds
  `BUILD_VARS`, not the make-flag default.)*
  `portage_profile::phase_environ` fills `MAKEOPTS=-j$nproc` and
  `GNUMAKEFLAGS=--load-average $nproc --output-sync=line` when neither
  `MAKEOPTS` nor `MAKEFLAGS` is set — real `doebuild.py:646-653`'s own
  behaviour, pinned by a unit test — and
  `ebuild_package::makeopts_to_job_count` mirrors real
  `util/cpuinfo.py:55-70`'s greedy regex, also unit-pinned. So
  `emerge -j8` on a 24-CPU host already expands to 8 × 24 make jobs by
  default, **exactly as real portage does**. Nothing to verify and
  nothing to fix; the oversubscription note in `--help` is optional
  documentation (real's own man page carries the same warning).
- **Install/merge parallelism (new, separate option):** the
  `src_install` phase + `merge_tree` + `pkg_preinst/postinst` half is
  filesystem- and single-package-scoped, while builds are CPU-scoped —
  one number cannot serve both. Proposed surface (portuale-specific;
  real portage has only `--jobs`):
  - `--install-jobs=N` (name bikeshed-able; `--merge-jobs` is the
    alternative): worker count for §3.1's copy pool and for parallel
    `src_install` file staging where safe. Default: follow `--jobs`
    when set, else `available_parallelism` capped small (e.g. 8), with
    small-package bypass. `--install-jobs=1` restores today's behavior
    exactly (needed for the determinism/corpus tests).
  - Fetch parallelism rides along: `fetch_src_uri`'s per-file loop
    becomes a bounded pool (default = install-jobs value), one
    `PortageLockfile` per file as today (per-file locks already compose
    — the lock is per-`dest`, acquired inside the worker). Keep the
    "first checksum failure switches to primary URIs then stops at the
    cap" logic serial per file; parallelize *across* files only.

Suggested `--help` wording keeps the split explicit:

```text
-j, --jobs[=N]         run up to N package builds in parallel (source builds)
--install-jobs[=N]     parallelize within one package: file copies/merges/fetches
--load-average LA      don't start new builds above this 1-min load average
```

Env defaults (`EMERGE_DEFAULT_OPTS` already prepends, so both flags
compose there) plus `--ignore-default-opts` cover the rest — no new env
vars needed, except honoring `PORTUALE_SHUFFLE_DIRS` in the new pools'
test mode.

## 5. Suggested implementation order (revised)

1. ~~**§2.0(a)** — read the vdb `metadata` snapshot (backlog `#109`).~~
   **DONE 2026-09-21** (S1–S4 `bcc69fd`/`c909bfa`/`6f158d7`/`5759a29`):
   3.93–3.98 → 3.48 s at S4, `openat` 160 k → 34 k.
2. ~~**§2.0(b)** — memoise `metadata_key_accepted`.~~ **DONE 2026-09-21**
   (#110, S5 `d034557`): 3.48 → 3.07–3.30 s, 23.30 % → 13.13 % with
   children.
3. ~~**Re-profile.**~~ **DONE 2026-09-21** (`performances-tuning.md`,
   "2026-09-21 batch"): sys is 0.56–0.67 s, `is_visible` 19.63 % with
   children, and the next items are `#105` (fingerprint caching,
   23.77 %), `#107` (the second `run_pass`) and `#112` (the residual
   `statx`) — **none of them parallelism**. The resolver thread
   question is answered: do not write a resolver pool (§2.0's verdict).
4. **§3.1 + §3.2** — two-phase `merge_tree` with `--install-jobs`.
   Still the highest-value *parallelism* slice: the merge copy
   loop is untouched by any memoisation batch, and `#96` already made
   each work item independent (§3.0). Oracle: L1 filesystem+VDB diff
   must stay clean; add a Rust unit test asserting `CONTENTS` order is
   dispatch-order-independent (shuffled work list → identical bytes).
5. **Fetch pool** on the `--install-jobs` count. Oracle: existing fetch
   tests + a multi-file download test.
6. **§2.1** only with an explicit index-order reassembly (§2.1) and a
   benchmark note justifying ~600 cold reads' worth of complexity.
7. **§1.4's `Rc → Arc` measurement** as its own commit, before any of
   §2.2 is written. A negative result closes §2.2.
8. **Defer §2.3 and §3.3** indefinitely.

Non-goals for this track: changing the resolver's *decisions* (slot
conflicts, backtracking, autounmask), changing merge *semantics*
(protection, collisions, preserve-libs), adding `rayon`/tokio pools,
touching the VDB format, or winning a benchmark by skipping a
`run_pass` (that is `#107`, a parity bug).

## 6. How to verify (per slice, not just at the end)

- **Correctness:** workspace-root `cargo fmt --check`, `cargo clippy
  --release --all-targets` (zero warnings), `cargo test --release`;
  then `python3 -m pytest pytests-contract-suite -q` in `../pmtest`
  (byte-pinned pretend output + corpus-drift check catch ordering
  bugs). Merge slices additionally need L1
  (`differential-test-bed/run/l1-merge-from-binpkg.sh`) **and** the
  merge-path safety gate (a real test merge of `sys-libs/glibc` +
  `app-shells/bash`, `agent-context.md`); resolver slices need L0
  (`differential-test-bed/run/l0-resolver.sh`) — the only oracles at
  real-tree scale.
- **Determinism:** run the new pools under `PORTUALE_SHUFFLE_DIRS=1`
  repeatedly; plans, `CONTENTS`, and VDB bytes must be identical across
  runs and against serial (`--install-jobs=1`).
- **Performance:** the same workload every other perf slice uses —
  `/usr/bin/time -v rust/target/release/emerge -puD --getbinpkg
  net-libs/rest`, best of 3 warm, interleaved with real `emerge` on the
  same tree — plus a fixed binpkg-set merge for §3. Record wall / user
  / sys / peak RSS before and after, in the commit body **and** in
  `performances-tuning.md`'s running table. Two lessons from
  `#102`–`#108`: benchmark before/after **back-to-back** (a mid-batch
  binhost publish already faked one regression) and spot-check a second
  graph shape so a win that only helps one plan is visible. Parallel
  work additionally must report memory: N cold thread-local memos is N×
  the cache, and §1.4 makes that a first-class cost, not a footnote.
