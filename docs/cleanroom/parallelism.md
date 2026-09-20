# Parallelism options: using more than one CPU

Status: proposal (not implemented). Goal: let portuale use N CPUs where
it is safe, without breaking the determinism contract or the musl-static
story. The three targets, in priority order, are:

1. `emerge --pretend` / `--ask` (the user is waiting for the plan),
2. filesystem merge of binary packages (build happened elsewhere, so
   this step should be fast),
3. source builds, with a **separate** knob for the install-to-filesystem
   half.

Source grounding: all `path:line` cites are live code at the time of
writing (`rust/portuale/src/*.rs`, `rust/portage-repo/src/lib.rs`,
`rust/portage-profile/src/lib.rs`, `rust/portage-util/src/lib.rs`).

## 0. Where the single-CPU ceiling comes from

Today portuale saturates exactly one core because every hot path except
two is a serial loop over blocking file I/O, hashing, and string work:

- The resolver (`portage-repo/src/lib.rs:23338`
  `resolve_pretend_graph`, via `pretend.rs:run`) is a single-threaded
  BFS. Per visited package it calls `list_candidates` (`:2035`, cached)
  → `list_candidates_uncached` (`:2065`, one `read_dir_entries` +
  one `repo_aux_metadata` per `.ebuild`, in a `for` loop) → per-candidate
  `is_visible` (`:4387`) → `effective_use_flags` (`:3178`) →
  `match_from_list` + `use_reduce_flat`. Each `repo_aux_metadata`
  (`:1508`) is one `read_md5_cache` (`:1409`, file read + `KEY=value`
  parse) plus validation. All serial. `--ask` shares this exact path
  (it resolves first, then prompts), so it benefits from the same fix.
- Config load (`portage-profile/src/lib.rs:2490` `resolve_config`) is a
  serial walk of the profile `parent` chain + `make.conf` + every
  `package.*` file, all through the single sorted-`readdir` seam
  (`portage-util/src/lib.rs:27` `read_dir_entries`).
- Binary merge (`ebuild_merge.rs:3684` `merge_binpkg` → `:1552`
  `merge_tree` → `:2968` `run_merge`) is a serial depth-first copy:
  one `symlink_metadata` + (for regular files) one MD5 + one
  `copy`/`symlink` + one `CONTENTS` line per path, in one thread.
- Fetch (`fetch.rs:655` `fetch_src_uri`) loops serially over
  `group_by_filename` groups: verify → lock → download → verify, one
  file at a time, one `wget` subprocess at a time.
- Already parallel (do not re-solve): source-build scheduling
  (`emerge_build.rs:1282` `run_build_scheduler`, `std::thread::scope` +
  `mpsc`, gated by `mrg-director::SchedulerPolicy` jobs/load-average)
  parallelizes **package builds** under `-jN`/`--jobs`; `regen.rs:409`
  `run_parallel` does the same for `--regen --jobs`. Merges inside the
  scheduler stay deliberately serial (collision-protect / `CONTENTS`
  races — see `run_build_scheduler`'s doc comment). Dependencies:
  `Cargo.toml` has no `rayon`/`threadpool`; `tokio` is `rt` +
  `rt-multi-thread` for the brush backend only, not a work pool.

So `-j24` already helps a 24-package source build, but does nothing for
`--pretend`, for a binpkg-only merge run (comment at
`pretend.rs:8728-8739`: "binary-merge paths stay serial"), or for the
file copies inside any single merge.

## 1. Hard constraints any design must respect

1. **Deterministic output.** The contract suite pins `--pretend` output
   byte-for-byte (plus the harvested corpus) and directory reads go
   through the single `read_dir_entries` seam (sorted by default, seeded
   shuffle only under test-only `PORTUALE_SHUFFLE_DIRS`). Parallel work
   must join into a deterministic order (collect-then-sort, as
   `regen.rs:run_parallel` already does) — never emit plan lines,
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
4. **Existing caches are already thread-safe.** `read_md5_cache` and
   `list_candidates` memoize behind `OnceLock<RwLock<…>>`; worker threads
   can share them with no redesign (clone the `Arc`, never the map —
   the old clone-per-hit cost is already fixed, see `read_md5_cache`'s
   doc comment).
5. **`--load-average` and `--quiet-build` semantics stay.** Any new pool
   reuses `system_loadavg_1min` (`emerge_build.rs:1294`) and the
   `SchedulerPolicy` trait rather than inventing a second gate.

## 2. `emerge --pretend` / `--ask` (priority 1)

This is CPU + I/O bound on metadata reads, not on graph logic: a real
tree has ~20k+ `metadata/md5-cache` files and the resolver touches
hundreds per plan (once per candidate per visited package, amortized by
the caches). Three cumulative levels, easiest first:

### 2.1 Parallel metadata prefetch (easy, safe, recommended first slice)

What: when the BFS dequeues package P and extracts its dependency atoms,
spawn the `repo_aux_metadata` reads for the *next frontier's*
`list_candidates` sets ahead of use — or, more simply, parallelize the
inner loop of `list_candidates_uncached` itself (one task per
`(repo, ebuild)` entry: `repo_aux_metadata` + `split_slot` + struct
build, then sort by `(repo_priority, version)` exactly as today).

Why safe: entries are independent (immutable tree files + the
thread-safe `read_md5_cache`); ordering is re-imposed by the existing
sort, so the plan cannot change. Expected gain scales with file count,
not graph depth — the common `--pretend firefox` / `@world` case.

Knob: none new. Honor a shared worker count (see §4): default
`min(available_parallelism, 16)`-ish cap, `0`/unset = serial for
determinism tests. Keep `PORTUALE_SHUFFLE_DIRS` semantics: shuffle
*before* dispatch, sort *after* join.

### 2.2 Parallel per-candidate visibility + USE (medium, recommended second)

What: `is_visible` + `effective_use_flags` + `match_from_list` run per
candidate in `list_candidates` consumers (notably the
`visible_tree_matches` / best-visible selection path around
`lib.rs:9915`). These are pure functions of
`(candidate metadata, config)` — dispatch one task per candidate,
join, then pick best exactly as today.

Why second: touches more call sites than 2.1, but each site keeps its
serial decision logic; only the *evaluation* fans out. The
`match_from_list` string work and `use_reduce_flat` group evaluation
are the CPU-heavy remainder once I/O is prefetched.

### 2.3 Parallel BFS frontier (harder — defer)

What: pop K ready packages off the BFS queue and expand them
concurrently (dependency extraction + candidate resolution), merging
into the shared graph under a mutex, with deterministic commit order
(frontier sorted by `(category, package)` before commit, as the
`merge_order.rs` topological pop already assumes).

Why deferred: the BFS carries dedup/slot-conflict/backtracking state
(`runtime_pkg_mask`, autounmask feedback, `required_by` edges) that is
order-sensitive today. Parallelizing it without changing *which*
backtrack fires first requires a deterministic reduction step that is
easy to get subtly wrong and hard to catch below L0 scale. Do 2.1 +
2.2 first, measure; 2.3 only if pretend is still the bottleneck.

Backtracking itself stays serial in all three levels (it is a short
retry loop over masks, not a throughput stage).

## 3. Binary-package filesystem merge (priority 2)

`merge_binpkg` (`ebuild_merge.rs:3684`) is: unpack outer container
(GNU `tar` process today) → `unpack_metadata` (in-process `tar` crate)
→ `merge_tree` copy loop → `write_vdb_entry` → `env_update`/ldconfig
hooks. The copy loop dominates wall time on a many-small-files package.

### 3.1 Intra-package parallel file copy (recommended)

What: two phases inside `merge_tree`, keeping today's semantics:

- **Phase A — scan (serial):** walk `${D}` once, building the ordered
  work list (path, type, stat, CONFIG_PROTECT decision inputs).
  Protection decisions that depend on live-destination state
  (`protect_decision`, `new_protect_filename`, `._cfgNNNN_` allocation)
  stay here, serial, so numbering and `cfgfiledict` updates keep
  today's exact behavior.
- **Phase B — copy (parallel):** dispatch fixed-size chunks of the work
  list to `std::thread::scope` workers: regular files → MD5 + copy +
  `lchown` + mtime preserve; symlinks → `readlink` + `symlink` +
  `lchown`; dirs → `create_dir_all`. Each worker returns
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
worker after the copy (or before, for the skip-if-identical check),
so hashing parallelizes for free with 3.1.

### 3.3 Inter-package parallel merge (NOT recommended by default)

Merging two packages into the same `${ROOT}` concurrently reintroduces
exactly the `collision-protect` / `CONTENTS` / `cfgfiledict` races real
portage serializes away. Options if ever wanted: (a) keep package merges
serial (recommended — binpkg runs are usually I/O-bound inside one
large package anyway); (b) allow it only behind an explicit
`--merge-jobs=N` with documented "same-file collisions between concurrently
merging packages are detected after the fact, not prevented" semantics.
Do not do (b) in the first slice.

## 4. Source builds vs. install-to-filesystem (priority 3, two knobs)

These are different resources and need different knobs:

- **Build parallelism (exists): `-jN` / `--jobs[=N]`** — package-level
  concurrency in `run_build_scheduler`, plus `--load-average` gate.
  Keep as-is. Inner-toolchain parallelism (`MAKEOPTS=-j…`, ninja, cargo)
  already flows into the phase env (`pretend.rs:6858` compiler/make-flag
  set); verify it is actually threaded for the `-j` path and document
  the recommended pairing (`emerge -j8` with `MAKEOPTS=-j4` oversubscribes
  — say so in `--help`, as real portage does).
- **Install/merge parallelism (new, separate option):** the `src_install`
  phase + `merge_tree` + `pkg_preinst/postinst` half is filesystem- and
  single-package-scoped, while builds are CPU-scoped — one number cannot
  serve both. Proposed surface (portuale-specific; real portage has only
  `--jobs`):
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

## 5. Suggested implementation order

1. **§2.1** — parallelize `list_candidates_uncached`'s per-ebuild
   `repo_aux_metadata` fan-out (`lib.rs:2065`). Pure win for pretend,
   smallest blast radius, no output-format risk. Gate with a shared
   `worker_count()` helper + serial fallback; assert byte-identical
   plans over the fixture suite + L0.
2. **§2.2** — parallelize per-candidate `is_visible` /
   `effective_use_flags` evaluation. Same gate, same oracles.
3. **§3.1 + §3.2** — two-phase `merge_tree` (`ebuild_merge.rs:1552`)
   with `--install-jobs`. Oracle: L1 filesystem+VDB diff must stay
   clean; add a Rust unit test asserting `CONTENTS` order is
   dispatch-order-independent (shuffled work list → identical bytes).
4. **Fetch pool** (`fetch.rs:655`) on the `--install-jobs` count.
   Oracle: existing fetch tests + a multi-file download test.
5. **Defer §2.3 and §3.3** until 1–4 are measured. Each needs its own
   L0/L1 run and a benchmark note (see §6) justifying the complexity.

Non-goals for this track: changing the resolver's *decisions* (slot
conflicts, backtracking, autounmask), changing merge *semantics*
(protection, collisions, preserve-libs), adding `rayon`/tokio pools,
or touching the VDB format.

## 6. How to verify (per slice, not just at the end)

- **Correctness:** `cargo fmt --check`, `cargo clippy --release
  --all-targets` (zero warnings), `cargo test --release` here; then
  `python3 -m pytest pytests-contract-suite -q` in `../pmtest`
  (byte-pinned pretend output + corpus-drift check catch ordering bugs).
  Merge slices additionally need L1
  (`differential-test-bed/run/l1-merge-from-binpkg.sh`); resolver slices
  need L0 (`differential-test-bed/run/l0-resolver.sh`) — the only oracles
  at real-tree scale.
- **Determinism:** run the new pools under
  `PORTUALE_SHUFFLE_DIRS=1` repeatedly; plans, `CONTENTS`, and VDB bytes
  must be identical across runs and against serial (`--install-jobs=1`).
- **Performance:** time `emerge --pretend` over 2–3 fixed real-tree
  atoms (e.g. a leaf, a mid-size lib, `@world`) and a fixed binpkg-set
  merge, at `--jobs=1` vs N on the 24-CPU host, before/after each slice.
  Record wall time + peak RSS (parallel metadata reads raise memory with
  worker count — the `Arc`-shared caches bound this, but say the number).
  No new benchmark harness needed; the differential bed + `time` suffice
  for this track.
