# Performance tuning

> Why `portuale emerge -puD` was several times slower than real `emerge` on a
> live tree, where the time actually goes, and what to change. Written from a
> `perf` + call-counter investigation of
> `portuale emerge -puD --getbinpkg net-libs/rest` on a real amd64 desktop
> (~2070 installed packages, gentoo + one overlay).

---

## Status

| build | `-puD --getbinpkg net-libs/rest` | vs before |
|---|---|---|
| real `emerge` | ~16 s | — |
| portuale, original | **~77 s** | 1.0× |
| + `parse_atom` / `parse_candidate` memoised | **~20.6 s** | 3.7× |
| + `package.*` config bucketed by `cp` | **~9.5 s** | 8.1× |
| + `profiles/updates/` move chains precomputed | **~6.9 s** | 11× |
| + `read_md5_cache` memoised per path | **~5.9 s** | 13× |
| + `effective_use_flags` memoised per (config, candidate) | **~4.6 s** | 17× |
| + `read_md5_cache` / `list_candidates` return shared `Arc` | **~4.5 s** | **17×** |
| + `#102`–`#108` memoisation batch (vdb scan, `Rc` EUF, mask/force, binpkg index) | **~4.0 s** | 19× |
| + `#109`–`#110` vdb `metadata` snapshot + aux memo + `metadata_key_accepted` memo | **~3.1 s** | 25× |
| + `#105` USE-context fingerprint frozen at config resolution | **~2.45 s** | 31× |
| + `#117` `binpkg_respect_use_ok` memo | **~2.25 s** | 34× |
| + `#114` `is_visible` memo | **~2.08 s** | 37× |
| + `#112` one `statx` per `vdb_aux_get` (was two) | **~2.01 s** | **38×** |

All twelve changes are **shipped** and keep byte-identical output with
the full suite green (`portage-dep` / `portage-repo` / `portuale` unit
tests + contract tests). portuale is now ~2.01 s wall / ~1.5 s user /
~0.5 s sys on this workload, against real's last measured 4.50–5.11 s
(real aborts there today, backlog #111). None of them alters the
resolver algorithm — they remove redundant work the algorithm was doing.

1. **`parse_atom` / `parse_candidate` memo cache** (`rust/portage-dep/src/
   lib.rs`): a `thread_local!` `HashMap<String, Option<…>>` in front of each
   parser, so the ~15-group backtracking regex runs once per distinct string
   instead of 34 M times. 77 s → 20.6 s.
2. **cp-bucketed `package.*` config** (`rust/portage-repo/src/lib.rs`,
   `CpBucketIndex` + `config_entries_matching` / `any_config_entry_matches`):
   portuale's stand-in for real portage's `ExtendedAtomDict`. Each config
   list is bucketed by `cat/pkg` once (memoised), so a per-package lookup
   visits the ~0–5 entries that name that `cp` (plus the wildcard tail)
   instead of linearly scanning all ~1,900. 20.6 s → 9.5 s.
3. **precomputed package-move chains** (`rust/portage-repo/src/lib.rs`,
   `move_chain_map` / `slot_move_map`): `apply_updates_to_cp` /
   `apply_updates_to_slot` replayed all ~530 `profiles/updates/` `move`
   commands on every call (once per installed package × ~10 resolver call
   sites). Now a single `HashMap` lookup against the fully-resolved chain,
   built once — real portage's model, which bakes moves into the vdb/cache at
   sync. `all_installed_packages` is also memoised per `root` (fingerprinted
   by vdb dir mtimes) so the ~2,000-`SLOT`-file scan runs once, not ~10×.
   9.5 s → 6.9 s (`apply_updates_to_cp` 15 % → 1.4 % of the run).
4. **`read_md5_cache` memoised per file path** (`rust/portage-repo/src/
   lib.rs`): the resolver reads the same md5-cache entry many times over one
   `emerge -pu` (`list_candidates`, then every visibility / USE / slot-op /
   changed-deps check for the candidate), each doing a file open + line parse
   + `apply_updates_to_dep_string` over 5 keys. The tree's md5-cache is
   immutable for the process (real `portdbapi` keeps the same entries in its
   `_aux_cache`); `--regen`, portuale's only writer, is a separate process.
   `OnceLock<RwLock<HashMap<PathBuf, Arc<…>>>>` keyed by full path.
   6.9 s → 5.9 s.
5. **`effective_use_flags` memoised** (`rust/portage-repo/src/lib.rs`,
   `effective_use_flags_uncached` + `use_context_fingerprint`): it was called
   ~20 k times for a few hundred distinct candidates — `is_visible`, then
   again for `keywords_accepted` / `is_stable` / `binpkg_respect_use_ok` /
   `use_flags_if_conditional` / the slot-op + changed-deps scans — each
   rebuilding a ~200-entry `HashSet<String>` from the full `USE_ORDER` walk.
   `thread_local!` `HashMap<(u64, String), Rc<HashSet<String>>>` keyed by
   `candidate_str` + a hash of `iuse`/`keywords` + a **sampled fingerprint of
   the ~20 `Config` USE fields** the function reads (length + first/mid/last
   entry of each; `autounmask_use`, the only field the `'backtrack` loop
   mutates, hashed in full). Sound because the result is a pure function of
   those inputs and two genuinely different configs can't match a ~20-field
   sample in production (one config per process). 5.9 s → 4.6 s
   (`effective_use_flags` drops out of the profile entirely).
6. **`read_md5_cache` and `list_candidates` return `Arc`** — both were
   handing back a full clone of a cached structure on every call.
   `read_md5_cache` now returns `Arc<HashMap<String,String>>` (~24 call
   sites, almost all unchanged via `Deref`). `list_candidates` gained the same
   per-process cache (ebuild trees never change during a portuale run) and
   returns `Arc<Vec<Candidate>>`; the ~10 sites that consumed the `Vec` by
   value take a `.to_vec()` / `.cloned()` where they genuinely need ownership
   (the resolver's mutable candidate pool, one clone per resolved atom). 4.6 s
   → 4.5 s. Also folded in: `apply_incremental_iter(&[S], …)` replacing
   `apply_incremental(&tokens.join(" "), …)`'s join-then-resplit in
   `apply_matching` / `specificity_ordered_flags` / `keyword_provenance`.

At 4.5 s the profile is ~35 % raw malloc/free spread across the whole run,
~7 % SipHash (almost all of it *building* maps/sets, not probing the caches —
see item 3), ~2.4 % `apply_updates_to_cp`, ~1 % the `--getbinpkg`
binpkg-index parse, and a long tail of sub-2 % items — no hot loop, no single
function above ~3 %. The per-node recomputation that made portuale a slower
*algorithm* than portage is gone; what remains is ordinary allocation
overhead.

## The symptom (original, 77 s)

| command | real `emerge` | `portuale emerge` |
|---|---|---|
| `-puD --getbinpkg net-libs/rest` | ~16 s | ~77 s |
| `-puD --getbinpkg sys-devel/gcc` | similar gap | similar gap |
| `-puD --getbinpkg app-crypt/gnupg` | similar gap | similar gap |

`portuale` is **CPU-bound and single-threaded** for the whole run: 75 s user /
2 s system, 99 % of one core, 101 MB RSS, negligible I/O (11 k fs inputs). The
merge list it produces is only **15 packages**. So ~75 s of CPU is spent
*deciding* on 15 packages while walking a ~2000-package deep graph — real
portage does the same walk in ~16 s.

This is not a constant-factor Rust-vs-Python problem. Rust starts ~10–50×
ahead per primitive operation. Being 5× *behind* means portuale is doing
roughly two orders of magnitude more work than real portage. The algorithm is
fundamentally different — it just turned out that a memo cache papers over
most of the difference.

## Where the time goes (`perf record`, release + line tables)

> This section and the two below record the **original 77 s investigation**.
> `file:line` references are as of that tree (commit `5a8f3f5`); the functions
> named still exist but have since moved and been split — grep the name.

Flat profile, self time:

| % | symbol | meaning |
|---:|---|---|
| 50 % | `regex_automata … BoundedBacktracker::search_imp` | executing a regex |
| 8 % | `Captures::get_group_by_name` | named-capture-group lookup (linear scan of group names) |
| 6 % | `regex_automata::hybrid::search::find_fwd` | regex DFA prefilter |
| 6 % | `__memcmp_avx2` | mostly regex + string compares |
| 5 % | `RandomState … hash_one::<&str>` | `HashSet<String>` churn in `effective_use_flags` |
| ~4 % | malloc/free | per-call `Captures` / `Vec` / `HashSet` allocation |
| ~2 % | `apply_updates_to_cp` | profile package-move application |
| ~1 % + ~1 % | `parse_candidate`, `parse_atom` | (their own body; the regex they call is the 50 % above) |

**~65 % of the entire run is regex**, almost all of it inside two functions:
`portage_dep::parse_atom` (`rust/portage-dep/src/lib.rs:380`, uses
`atom_regex()` at `:271`) and `portage_dep::parse_candidate` (`:505`, uses
`candidate_regex()` at `:495`). Both regexes are ~15 named capture groups and
compile to the bounded backtracker.

## Why the regex runs so many times (call counters)

Counters added to the hot functions for one full run:

| function | calls |
|---|---:|
| `portage_dep::parse_atom` | **35,639,563** |
| `portage_dep::parse_candidate` | **33,983,531** |
| `portage_dep::match_from_list` | **34,195,241** |
| `portage_repo::read_md5_cache` | 35,124 |
| `portage_repo::effective_use_flags` | 20,817 |
| `portage_repo::list_candidates` | 7,493 |
| `portage_repo::apply_updates_to_dep_string` | 100,682 |

The run makes **34 million** `match_from_list` calls — each one re-parses an
atom string *and* a candidate string with the backtracking regex. For a
15-package result. That 34 M is the whole runtime.

Two multiplicative causes:

### Cause 1 — every per-package computation is O(entire `package.*` config), with a regex per entry

`effective_use_flags` (`rust/portage-repo/src/lib.rs:2186`) computes a
package's USE by walking `config.package_use`, `package_use_user`,
`package_env_use`, `package_use_force`, `package_use_mask`,
`package_accept_keywords` (the last via `is_stable`) and, for each, calling
`specificity_ordered_flags` (`:2452`) / `matches_config_entry` (`:2065`),
which does:

```rust
portage_dep::match_from_list(entry, &[candidate_str])   // 1 parse_atom + 1 parse_candidate, both regex
```

**for every entry in the list**. On the test system those lists total ~1,900
entries:

```
package_mask=370  package_accept_keywords=99  package_use(profile)=138
package_use_user=112  package_env_use=15  package_use_force=261
package_use_mask=650  (profile_use_layers=18, contributing the 138)
```

So one `effective_use_flags` call ≈ **1,300–1,600 regex-backed
`matches_config_entry` calls**. 20,817 × ~1,600 ≈ 33 M — matches the counter.

The candidate string (`cat/pkg-ver:slot::repo`) is *identical* across all
~1,600 iterations of a single call and is re-parsed every time. Each config
atom is re-parsed on every call for every package, forever.

**Real portage does not do this.** It parses each `package.use` /
`package.mask` / … line **once** at config load into an `Atom`, and its
`UseManager` / `_MaskManager` (`lib/portage/package/ebuild/_config/`) store
them in an `ExtendedAtomDict` (`lib/portage/dep/__init__.py`) keyed by `cp`,
with separate `*/*` and `cat/*` buckets. Looking up USE for `net-libs/rest`
visits only the handful of entries whose key is `net-libs/rest`, `net-libs/*`,
or `*/*` — typically 0–5, never 1,900 — and compares against already-parsed
`Atom` objects. The result is then cached on the `Package` object (`pkg.use`,
`_pkg_str`) and never recomputed for that package.

### Cause 2 — no memoization: the same package is recomputed ~10× per resolve

`effective_use_flags` is called 20,817 times but there are only ~2,000
packages in the deep graph — ~10× per package. `list_candidates` 7,493 /
~2,000 cp ≈ 4×. `read_md5_cache` 35,124 for ~4,000 ebuild files ≈ 9×. And the
`'backtrack` loop ran **2 passes**, re-doing the entire graph walk the second
time.

`list_candidates` (`:1364`) and `effective_use_flags` are called from ~40 call
sites in `rust/portage-repo/src/lib.rs` — the BFS queue (`:12013`),
re-resolution (`:12238`), the slot-operator-rebuild scan, `--changed-deps`,
`_complete_graph`, keyword/mask/visibility checks — **none of which share a
cache**. Every visit re-reads directories, re-reads and re-parses md5-cache
files, and re-applies profile `updates/` to every `*DEPEND` string
(`apply_updates_to_dep_string`, `:614`, 100 k calls) from scratch.

Real portage reads `metadata/md5-cache` through `portdbapi` with an in-process
LRU (`self._aux_cache`), builds each `Package` once, and the depgraph keeps a
single `Package` instance per cpv for the life of the resolve.

## The shipped fixes

### `parse_atom` / `parse_candidate` memo cache

`parse_atom` / `parse_candidate` each gained a `thread_local!`
`RefCell<HashMap<String, Option<…>>>` front. A hit is one hash lookup plus one
clone of the parsed struct; a miss runs the old regex body and inserts.
Thread-local, so the parse path stays lock-free (the resolver is
single-threaded; other threads get their own table). Never invalidated — a
parse result is a pure function of the input string for the life of the
process.

A crate (`cached`, `quick_cache`, `lru`, …) would also work, but the table is
tiny (tens of thousands of distinct strings, never evicted), the project keeps
its dependency surface deliberately small, and a bare `HashMap` behind a
`thread_local!` is ~15 lines with no lock and no eviction bookkeeping. If the
table size ever becomes a concern, swapping in `quick_cache` or an `lru` bound
is a local change.

Profile after this change (20.6 s run): the regex is gone from the top; the
new cost was the ~34 M cache lookups themselves — `__memmove` 15 %,
`__memcmp` 13 %, SipHash of the long candidate cache-keys ~12 %,
`Atom`/`Candidate`/`String` clone on the hit path ~10 %, `apply_updates_to_cp`
6 %, malloc/free ~16 %. Almost all of that traced back to the linear config
scan below.

### cp-bucketed `package.*` config

`CpBucketIndex` in `rust/portage-repo/src/lib.rs` is portuale's
`ExtendedAtomDict`. For one config list it holds `by_cp: HashMap<String,
Vec<u32>>` (`"cat/pkg"` → file-order positions of the plain-atom entries for
that exact `cp`) and `other: Vec<u32>` (wildcard / unparseable atoms, always
re-checked — a handful of `*/*` lines on a real tree). It's built once per
distinct list and memoised in a `thread_local!` keyed by the list's
`(as_ptr, len)` with the first/last atom strings as a fingerprint.

`config_entries_matching(entries, candidate_str, cat, pkg)` and
`any_config_entry_matches(...)` replace the
`entries.iter().filter(|e| matches_config_entry(...))` /
`.any(...)` scans in `effective_use_flags`, `specificity_ordered_flags`,
`resolve_accept_tokens`, `keyword_provenance`, and the `package.mask` /
`package.unmask` checks. They return entries in original file order, so the
downstream stable `sort_by_key(atom_specificity)` is unaffected — output is
byte-identical. A per-package lookup now visits ~0–5 entries instead of
~1,900. 20.6 s → 9.5 s.

The `Config` field types were left as `Vec<(String, Vec<String>)>` — the index
lives in `portage-repo` (which owns `portage_dep`), keyed by the vec's
identity, rather than moving atom parsing into `portage-profile` (which has no
`portage-dep` dependency and ~60 struct-literal test call sites). Same effect,
smaller blast radius.

## What to change next, in priority order

**Re-ranked 2026-09-21 after `#112`** (`perf record -F 400 -g` on the
post-slice binary; the numbers are in "2026-09-21 batch" below). The
resolver is now ~2.01 s wall / ~1.5 s user / ~0.5 s sys on the reference
workload:

1. **`#107` — the second `run_pass` (parked 2026-09-21).** `run_pass` is
   75.53 % with children and the loop still restarts where real reports
   `backtrack: 0/20`. Diagnosed: the restart is the reverse-dependency
   pin feedback, and removing it needs real's complete-graph
   parent-atom model (a prototype that applied the reachable consumers'
   atoms at selection lost an update and the withheld-update warning —
   see the #107 backlog entry). Algorithmic/parity; do not win it by
   skipping the pass. Revisit only with the complete-graph model.
2. **`repo_aux_metadata` (14.69 % with children)** — the top named cost:
   cold md5-cache fills + validation, plus `apply_updates_to_dep_string`
   rebuilding every token of a cache-miss dep string even when no
   `profiles/updates/` move matches (the fast-path slice).
3. **`list_remote_binary_candidates` (12.08 %)** — its sharing was tried
   and withdrawn (`#118`, honest non-result: within noise at +16 MB RSS);
   the remaining cost is first materialisation per `(cp, visit)` and
   cheaper construction is the only lever, under the noise floor.
4. **Allocation churn** — `effective_use_flags` 10.19 %,
   `binpkg_respect_use_ok`'s memo residual 9.70 %, `String::clone` 9.62 %,
   `parse_atom` 9.38 %, `malloc` 9.84 %: no single item above ~3 %.
5. **`#111` — the real-abort tree divergence** (parity, not performance):
   real aborts `-uD --getbinpkg` where portuale resolves on the
   2026-09-21 tree; blocks the interleaved real comparison until fixed.

Deprioritised: `#106` (`candidate_positions`, 0.31 % — closed by
`#102`–`#108`), `#113` (`shuffle_seed`'s per-call env read, trivial).

The pre-batch ranking (diminishing returns at the 4.5 s run: deep-clone
`Candidate`, `installed_candidates` per cp — since shipped as `#102` —
parse-cache options, whole-graph passes — now `#107` — and the release
profile, shipped) is kept below as history.

### Pre-batch ranking (2026-09-20, kept as history)

#### Parse cache — remaining options (both low-yield)

- **FxHash instead of SipHash on the memo `HashMap`s — measured, ~1 %,
  not worth it.** Prototyped a vendored `FxHasher` (`rustc-hash`'s classic
  algorithm) on `ATOM_CACHE` / `CANDIDATE_CACHE` / `EUF_CACHE` / the
  `CpBucketIndex` cache / `use_context_fingerprint`. User time 2.79 s →
  2.77 s (within run-to-run noise). SipHash still shows ~7 % in `perf`
  afterward — but almost none of it is *cache lookups* (a probe on a
  ~50-char key is ~40 ns × ~50 k = 2 ms). It's map/set **construction**:
  `read_md5_cache` building its ~20-key map on each of ~4 k cold misses,
  `effective_use_flags_uncached`'s `HashSet<String>` on misses,
  `Candidate.binary_deps`, and the slot-conflict / `USE_EXPAND` display
  maps. Converting *those* to Fx is a much wider change, and some feed
  ordered output (`HashSet` iteration order → display order), so it's
  higher-risk for ~3–4 %. Not pursued.
- **`Rc<Atom>` / `Rc<Candidate>` return from the parse cache** — the hit
  path clones the struct. After the config-bucketing change the parsers
  are called far less, so this is now ~1 % and needs `Rc` threaded through
  ~40 call sites. Skip.
- **Hand-written parser** replacing the 15-group backtracking regex — only
  runs on cache *misses* now (once per distinct string); the regex is
  <1 % of the current run. Not worth it unless a workload with far more
  distinct atoms appears.

#### Reduce redundant whole-graph passes

The `'backtrack` loop re-ran the entire BFS for a case that produced no
backtracking-relevant change on pass 2. Confirm each pass is genuinely needed
(real portage's `_backtrack_depgraph` only re-runs `_create_graph`, reusing
already-resolved state); a wasted pass doubles everything above.

#### Release-profile `lto` + `codegen-units`

`[profile.release]` in `rust/Cargo.toml` sets `panic = "abort"` but leaves
`lto` and `codegen-units` at their defaults. The `rust-skills` `opt-` audit
(was `docs/refactor-HIGH.md`, now `docs/history/`) flagged
`lto = "thin"` + `codegen-units = 1` as the one broadly-applicable
release-build win — cross-crate inlining across the ~10 workspace crates,
~5–10 % typical, at the cost of a slower release build. Gated on a
user/CI call; measure against the `-puD --getbinpkg` workload before
keeping.

**Shipped as a Tier-1 slice (settings kept):** `lto = "thin"` +
`codegen-units = 1` are now set. Measured off the live tree (no live
tree in this environment): release `portuale` 15.5 MB → 13.2 MB
(-15 %), and a 30× fixture-resolve loop identical within noise
(startup-dominated, too small to discriminate -- honest non-result).
The `-puD --getbinpkg` timing from "How to reproduce" below stays a
re-run item for a machine with the real tree.

## How to reproduce the measurement

```sh
# wall-clock + CPU split
/usr/bin/time -v rust/target/release/portuale emerge -puD --getbinpkg net-libs/rest

# profile (needs debug line tables in release)
cd rust && CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release -p portuale
perf record -F 400 -g --call-graph dwarf,16384 -- \
    rust/target/release/portuale emerge -puD --getbinpkg net-libs/rest >/dev/null
perf report --stdio --no-children | head -40
```

The original call counts (77 s run) were obtained by adding `AtomicU64`
counters to `parse_atom`, `parse_candidate`, `match_from_list`,
`read_md5_cache`, `effective_use_flags`, `list_candidates`, and
`apply_updates_to_dep_string`, dumped from `main` when `PORTUALE_PERF_COUNT`
is set. That instrumentation was reverted; re-add it the same way to check
progress. Note the memo cache means `parse_atom` / `parse_candidate` now count
*calls*, not regex runs — put the counter inside `*_uncached` to see miss
counts.

## Target

The six shipped fixes reached ~4.5 s — about 3.5× faster than real `emerge`
(~16 s) on this workload. The per-node recomputation that made portuale a
different, slower algorithm is now all memoised; what's left is ordinary
allocation overhead with no hot loop, so further work is incremental
(items 1–3, ~1–2 % each). The Rust graph walk itself was never the
bottleneck.

## Re-measurement 2026-09-19 (commit `e7f4ec8`, release rebuilt same day)

Same workload, same host (now 2088 installed packages):

| command | wall | user | sys | RSS |
|---|---|---|---|---|
| `portuale emerge -puD --getbinpkg net-libs/rest` (×2, identical) | **6.75 s** | 4.95 s | 1.79 s | ~158 MB |
| real `emerge -puD --getbinpkg net-libs/rest` (×2) | **5.2–5.5 s** | ~4.7 s | ~0.25 s | ~183 MB |

Honest delta vs the 4.5 s / 3.5× days: user time is now at parity
(4.95 vs ~4.7 s) and the wall gap is almost entirely sys time plus a
faster real (5.5 s today vs 16 s then — tree, host and real-Portage
drift cut both ways). Since the 4.5 s run the resolver gained real
work: the all-installed blocker scan (#77), kept-branch derivation
(#76/#84), live-metadata reads (#86), the respect-use repair (#69 R1),
`installed_closure` backing (#68 S3) — none of it memoised away yet.

`perf record -F 400` on the 6.75 s run: still no hot loop. Top frames
are SipHash `write` (~14 % total), `__memmove` 7.2 %, malloc family
~20 %+, `HashMap::clone/reserve/insert` ~5 %; the top *named* frame is
`apply_updates_to_cp` 3.1 % reached via `installed_cp_sources` →
`installed_candidates` → `installed_versions` → `binary_deps_changed`
— i.e. "What to change next" item 2 (cache `installed_candidates`
per cp) is now the single biggest named cost. `parse_atom` +
backtracker combined are <1.5 %: the memo caches hold.

Next step, in order: item 2 first (per-cp installed cache), then
re-profile; items 1/3 stay ~1–2 % each. Re-run this section's table
after any resolver-shape change, not just perf work.

## 2026-09-20 batch (#102–#104, #108): shipped

Re-profiled 2026-09-20 (release `ce68f77`, same workload, now 2088
installed packages): portuale 7.71–7.92 s wall / 5.79–6.00 s user /
1.83–1.92 s sys vs real 3.0.82.2 5.70–5.89 s / 4.97–5.00 s /
0.19–0.24 s — portuale behind again, on per-visit recomputation rather
than the graph walk. Shipped the same day on `backlog/102-108`
(plan [`08.102-108-perf-memoisation.md`](08.102-108-perf-memoisation.md)),
each slice output-byte-identical over the full contract suite:

| slice | change | wall (best of 3 warm) |
|---|---|---|
| baseline | — | 7.71–7.92 s |
| #102 S1 | per-cp `installed_candidates` cache + inverted move map + `d_type` scan | 5.45–5.66 s |
| #103 S2 | `Rc<HashSet>` out of `effective_use_flags` (+ through `candidate_iuse_and_use`) | 4.51–4.55 s |
| #104 S3 | `resolved_use_mask_or_force` memo | 4.13–4.19 s |
| #108 S4 | `local_binpkg_index` memo (`Arc<BinaryIndex>`) | 3.98–3.99 s |

(Mid-batch the binhost published new `-1` binpkg revisions, shrinking
both PMs' graphs: S1's numbers above are pre-shift, S2–S4 post-shift
with same-env before/after each. One scare on the way: the first S2
diff was bisected to the environment, not the change — the lesson is to
benchmark before/after back-to-back and always re-verify identity on the
same tree.)

Final, interleaved with real: portuale **4.01–4.18 s** wall /
2.93–3.09 s user / ~1.0 s sys vs real **4.50–5.11 s** / 4.07–4.16 s /
~0.2 s sys — portuale is faster than real `emerge` on this workload
again, with user time at ~0.7× real's. The remaining sys gap (~0.8 s) is
ordinary file reads (md5-cache, config, repo scans), not re-scanning:
`statx` went 1,202,010 → 297,382 per run. The call counts and the
temporary-counter recipe (`PORTUALE_PERF_COUNT`, reverted after
measuring) are in the plan's §1.

Next, in order: #105 (`use_context_fingerprint`, ~8 % and growing as the
other costs fall), #106 (`candidate_positions`), then the #107
algorithmic question. The `EUF_CACHE`-style thread-local `Rc`/`Arc`
memo is now the established shape for per-resolve pure functions --
reuse it before inventing a new one.

## 2026-09-21 batch (#109–#110): shipped

Re-profiled after the `#102`–`#108` batch (release `662b15d`): 3.88–3.90 s
wall / 2.85–2.90 s user / 0.99–1.03 s sys, 174 MB RSS — portuale ahead
of real, and the ~1.0 s sys was the largest identified remaining cost,
all of it vdb reads (`strace`: 160,351 `openat` with **40,651 ENOENT**
and 202,281 `read`; the top successful-open paths are per-key
`/var/db/pkg/CAT/PF/{USE,RDEPEND,repository,SLOT,BDEPEND,...}`). Shipped
the same day on `feat/parallel` (plan
[`08.109-110-vdb-metadata-snapshot.md`](08.109-110-vdb-metadata-snapshot.md)),
each slice re-measured and byte-identical over the full contract suite:

| slice | change | wall (best of 3 warm) |
|---|---|---|
| baseline (#102–#108) | — | 3.93–3.98 s |
| #109 S1 | normalise `read_vdb_string` like real `_aux_get` | 4.06–4.08 s (no change) |
| #109 S2 | move the field set/version to `portage-repo` | 4.06 s (no change) |
| #109 S3 | `vdb_aux_get` reads the snapshot | 4.33–4.35 s (**regression**) |
| #109 S4 | per-instance aux memo keyed on the dir mtime | 3.48 s |
| #110 S5 | memoise `metadata_key_accepted` | **3.07–3.30 s** |

Final, interleaved on the same tree: portuale **3.07–3.30 s** wall /
2.51–2.63 s user / 0.56–0.67 s sys / ~182 MB RSS vs baseline
3.93–3.98 / 2.86–2.90 / 1.07–1.08 / 174 MB. `strace`: `openat` 160,351
(40,651 ENOENT) → 33,685 (105), `read` 202,281 → 30,041, `close`
119,772 → 33,652, total syscalls 845,480 → 506,848; `statx` **up**
294,450 → 339,804 (the per-call validity stat is kept by design, plus
`vdb_pkg_dir`'s `is_dir()` — #112). The second-shape spot check
(`sys-devel/gcc`) agrees: 3.97–4.22 s → 3.06–3.17 s wall, sys 1.04–1.23
→ 0.56–0.58.

**S3 is an honest intermediate regression.** Reading and parsing the
whole 23-field snapshot per key call costs more than the tiny field read
it replaced — the ENOENT probes vanish (40,651 → 105) but opens stay
~160 k and `read` grows — which is exactly why S4's per-instance memo
follows immediately. The plan's S3 expectation (~0.3–0.5 s sys) did not
materialise; the pair is what ships.

**Real comparison is blocked today (#111).** Real `emerge -puD
--getbinpkg net-libs/rest` now exits 1 on this tree
(`~dev-qt/qtbase-6.11.2:6[...]` USE conflict through the installed
qtdeclarative chain) while portuale resolves the same 27-package plan
rc 0; `-pv --getbinpkg dev-qt/qtbase` resolves on both. This is a
tree-state change, not a #109/#110 effect (the pre-batch baseline binary
resolves too), but it means the interleaved real wall-time comparison is
unavailable until #111 is diagnosed.

Next, in order: #105 (`use_context_fingerprint`, 23.77 % with children),
then #107 (`run_pass`, 85.44 %, still the algorithmic item), then #112
(the remaining `statx`). #106 (`candidate_positions`) is 0.31 % now —
effectively closed by #102–#108.

### #105 follow-up (same day): freeze the USE-context digest

`use_context_fingerprint` re-hashed ~20 config fields (big `HashSet`
folds included) for every `effective_use_flags` /
`resolved_use_mask_or_force` / `metadata_key_accepted` memo key --
23.77 % with children on the post-#109/#110 profile and the top named
single-threaded cost. `resolve_config` now freezes the immutable part
once into `Config::use_context_base` (new
`use_context_base_fingerprint` in `portage-profile`); the memo key
hashes that `u64` plus the live `autounmask_use`, the one field the
`'backtrack` loop mutates (clones carry the base, which is correct
because they differ only there). A hand-built config (tests) has no base
and falls back to the full content hash, so an in-place edit of a
context field is still seen.

Interleaved, same tree: 3.00-3.16 s wall / 2.46-2.56 s user -> **2.44-2.50 s /
1.89-1.97 s** (second shape `sys-devel/gcc`: 3.00-3.08 -> 2.43-2.47).
`use_context_fingerprint` and `metadata_key_accepted` both leave the
profile's top list; `run_pass` is now 81.20 % with children and
`binpkg_respect_use_ok` 24.78 %. Full contract suite byte-identical
(1739 passed, same 3 pre-existing movepkg failures, corpus drift list
unchanged).

### #117 follow-up (same day): memoise `binpkg_respect_use_ok`

After #105 the binary-candidate USE check was the top named function
(24.78 % with children, 0.25 % self) even though its children are the
already-memoised USE machinery: the residual is rebuilding each
downstream memo key plus the `old_iuse`/enabled-set allocations. A
temporary counter showed the reference workload calls it ~13 k times
with only **1,197 distinct inputs** (10x repetition: the two retain
loops in `resolve_pretend`/the pass re-derivation, across passes). The
function is pure in its eight arguments, so it now memoises per
`(config USE context, both candidates' full metadata, the three flags)`
in a thread-local map, hashing the baked USE set with an order-independent
fold.

Interleaved, same tree: 2.58-2.69 s wall / 1.96-2.02 s user -> **2.22-2.28 s /
1.62-1.65 s** (second shape `sys-devel/gcc`: 2.51-2.58 -> 2.18-2.24);
`binpkg_respect_use_ok` falls from 24.78 % to 10.16 % with children.
Byte-identical over the full contract suite + corpus.

### #114 follow-up (same day): memoise `is_visible`

After #117 `is_visible` was the top named function (13.12 % with
children, 0.43 % self). A temporary counter measured ~40 k calls against
**1,503 distinct inputs** and one config (96 % repeat), each re-formatting
the candidate string, walking `package.mask`/`.unmask` and re-deriving
the license/keyword/property/restrict verdicts. It now memoises per
`(visibility fingerprint, candidate identity + full metadata)` in a
thread-local map. The config side is a second frozen base
(`portage-profile::is_visible_base_fingerprint`: `package.mask`/`.unmask`,
the LICENSE/PROPERTIES/RESTRICT accept lists, `package.accept_keywords`
hashed in full, `license_groups`) plus the live `autounmask_use`; a
hand-built config falls back to the live digest. The candidate's
`license`/`properties`/`restrict`/`iuse`/`keywords` are in the key
because a `Packages`-index binary and the ebuild for the same
`candidate_str` can carry different metadata.

Interleaved, same tree: 2.50-2.64 s wall / 1.91-1.98 s user -> **2.08-2.09 s /
1.47-1.52 s** (second shape `sys-devel/gcc`: 2.44-2.52 -> 2.03-2.07);
`is_visible` leaves the profile's top list. Byte-identical over the full
contract suite + corpus.

### Withdrawn: sharing the binary-candidate pools (#118, honest non-result)

The profile made `list_remote_binary_candidates` look like the next
memoisation target (13.79 % with children; 7.92 % in
`binary_candidates_from_index`). Implemented the sharing -- thread-local
`Rc<Vec<Candidate>>` pools per `(index, cp, remote)` with the
`cp_bucket_index`-style `(ptr, len)` + first/last guard, borrowed
filtering at the two hot call sites, pools cleared per `run_pass` to
bound retention -- and measured it: 2.11-2.13 s -> 2.07-2.10 s median
over 8 alternating pairs (~0.03 s, inside the run-to-run spread) at
**+16 MB RSS** (183 -> 199 MB). Keeping the pools process-lifetime
reached ~0.06-0.1 s in one batch but +22 MB with no reliable repeat. So
the 7.92 % is the first materialisation per `(cp, visit)`, not redundant
repeats -- a memo cannot amortise it, only cheaper construction could.
Reverted; the backlog entry records the non-result so it is not
re-attempted.

### #112 follow-up (same day): one stat per vdb lookup

`vdb_aux_get` resolved the package dir with `vdb_pkg_dir` (whose
`is_dir()` was one `statx`) and then stat'ed it again for the
validity/`st_mtime_ns` signal -- two per key lookup, ~126 k of the
~340 k `statx`/run. `vdb_pkg_dir_meta` now returns the `Metadata` the
resolution already paid (`Some` only for a real directory, following
symlinks exactly like `is_dir()` did), and `vdb_aux_get` consumes it;
`vdb_pkg_dir` stays a thin wrapper for its other callers.

Interleaved, same tree: 2.09-2.23 s wall / 1.51-1.53 s user -> **2.01-2.03 s /
1.48-1.49 s** (second shape `sys-devel/gcc`: 2.07-2.11 -> 2.00-2.02);
`strace`: `statx` 340,008 -> **211,165**, sys 0.58-0.69 -> 0.50-0.52.
Byte-identical over the full contract suite + corpus; the merge-path
gate (glibc+bash) is green (`l1-20260921T094230Z`) because the merge
readers share the helper.
