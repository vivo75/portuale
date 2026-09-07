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
| + `read_md5_cache` memoised per path | **~5.9 s** | **13×** |

All four changes are **shipped** and keep byte-identical output with the full
suite green (`portage-dep` / `portage-repo` / `portuale` unit tests + contract
tests). portuale is now **~3× faster than real `emerge`** on this workload.
None of them alters the resolver algorithm — they remove redundant work the
algorithm was doing.

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
   `OnceLock<RwLock<HashMap<PathBuf, Arc<…>>>>` keyed by full path; the
   `pub` signature still returns an owned `HashMap` (a clone of the cached
   `Arc`). 6.9 s → 5.9 s.

At 5.9 s the profile is ~16 % SipHash + ~7 % `HashSet<String>` insert/rehash +
~6 % `split_whitespace` + ~2.4 % `apply_incremental` — that cluster is
`effective_use_flags` building a fresh ~200-flag `HashSet<String>` ~20 k times
(item 1 below) — plus ~30 % malloc/free spread across it and the
candidate-string plumbing. `installed_candidates` (a per-cp vdb scan, ~3 %)
and `list_candidates` are the other repeat-work targets (item 2).

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

### 1. Memoize `effective_use_flags` — new #1 (needs care)

Now the biggest cluster (~20–25 % of the 5.9 s run): a fresh
`HashSet<String>` of ~200 USE flags built by `apply_incremental`
token-splitting, ~20 k times, for ~few-hundred distinct candidates.

The catch is the cache key. `effective_use_flags` is a pure function of
`(candidate_str, whole Config)`, and every `Config` in one resolve is
`req.config` **or a `.clone()` of it with only `autounmask_use` replaced**
(`backtracking_resolve`'s `backtrack_config` at `:14329`, the autounmask tier
loop at `:14487`, `flag_is_settable`'s `probe` at `:3971` — verified, that's
the complete list). So *within* a resolve, `(config_ptr, hash(autounmask_use),
candidate_str)` is a sound key. It is **not** sound across resolves: a
sequential resolve's `req.config` (stack-allocated in the caller) can reuse
the same address with the same (empty) `autounmask_use`, and the test suite
runs many resolves + ~30 direct `effective_use_flags` calls in one process.

Two ways to make it safe, pick one:
- A `RESOLVE_GENERATION: AtomicU64` bumped at each resolve entry point
  (`backtracking_resolve`, `resolve_pretend`, `resolve_pretend_graph`); key on
  `(generation, hash(autounmask_use), candidate_str)`. Direct unit-test calls
  share one generation but use distinct enough configs+candidate_strs — still,
  audit those ~30 tests.
- A cheap `Config` fingerprint: `(len, first, last)` of each of the ~20 USE
  fields `effective_use_flags` reads. Sub-µs, collision only if two configs
  match on all 20 — never in practice. More robust, more code, must track
  which fields the function reads.

Cheaper independent win, no cache: `apply_matching` / `specificity_ordered_
flags` call `apply_incremental(&tokens.join(" "), …)` — a `Vec<String>` joined
into a `String` that `apply_incremental` immediately splits again. An
`apply_incremental_iter(&[S], …)` that skips the round-trip is ~3–5 %.

### 2. Cache `list_candidates` / `installed_candidates` per `(repo-set, cp)`

`list_candidates` (ebuild dir listing + per-version md5-cache) and
`installed_candidates` (a `var/db/pkg` scan, ~3 % of the run) are re-run per
cp per graph-walk visit. Key by `(category, package)`; value `Arc<Vec<…>>`.
Safe like `all_installed_packages` — fingerprint by dir mtime if a test could
mutate mid-process, otherwise a plain per-process cache. Removes the remaining
`read_dir` traffic.

### 3. Cheaper parse cache, or a hand-written parser

With the memo cache in place the regex only runs on cache *misses* (once per
distinct string). Two follow-ups, in increasing effort:

- **Faster hasher for the cache** — the cache keys are long
  `cat/pkg-ver:slot::repo` strings and SipHash is ~12 % of the post-cache run.
  An FxHash/ahash-style hasher (either a small vendored implementation or the
  `rustc-hash` crate) on just these two `HashMap`s reclaims most of it.
- **`Rc<Atom>` / `Rc<Candidate>` return** — the hit path currently clones the
  struct (~10 %). Returning a shared handle avoids it, at the cost of
  threading `Rc` through the ~40 call sites.
- **Hand-written parser** — the 15-group backtracking regex plus linear-scan
  named-group lookup (`get_group_by_name`, 8 % of the *old* run) is ~10×
  slower than a direct scan. The grammar is simple and fully specified
  (PMS 8.3; the crate already has hand-written helpers like
  `strip_version_prefix`). Only matters for the miss path; low priority now.

### 4. Reduce redundant whole-graph passes

The `'backtrack` loop re-ran the entire BFS for a case that produced no
backtracking-relevant change on pass 2. Confirm each pass is genuinely needed
(real portage's `_backtrack_depgraph` only re-runs `_create_graph`, reusing
already-resolved state); a wasted pass doubles everything above.

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

The four shipped fixes reached ~5.9 s — about 3× faster than real `emerge`
(~16 s) on this workload. Beyond this the run is bounded by `HashSet<String>`
building + hashing in `effective_use_flags` and general string allocation
rather than any single hot loop; items 1–2 above (memoising the per-package
recomputation) are what remains of the "different algorithm" gap. The Rust
graph walk itself was never the bottleneck.
