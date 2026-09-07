# Performance tuning

> Why `portuale emerge -puD` was several times slower than real `emerge` on a
> live tree, where the time actually goes, and what to change. Written from a
> `perf` + call-counter investigation of
> `portuale emerge -puD --getbinpkg net-libs/rest` on a real amd64 desktop
> (~2070 installed packages, gentoo + one overlay).

---

## Status

| build | `-puD --getbinpkg net-libs/rest` wall time |
|---|---|
| real `emerge` | ~16 s |
| portuale, before | **~77 s** |
| portuale, with `parse_atom` / `parse_candidate` memoised | **~20.6 s** |

**Shipped: a thread-local memo cache on `parse_atom` and `parse_candidate`**
(`rust/portage-dep/src/lib.rs`). It does not touch the resolver algorithm at
all — it just stops re-running the backtracking regex on strings already
parsed. That single change took the run from 77 s to 20.6 s (3.7×), close to
real-portage parity, with byte-identical output and the full suite
(`portage-dep` / `portage-repo` / `portuale` unit tests + 1107 contract
tests) green. The rest of this document is the original analysis and the
options for closing the last ~4 s and going further.

## The symptom (before the cache)

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

## The memo cache (shipped)

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

### Post-cache profile

`perf` on the 20.6 s run — the regex is gone from the top; the new cost is the
cache machinery itself plus string churn that was always there:

| % | symbol | meaning |
|---:|---|---|
| 15 % | `__memmove_avx` | copying strings (HashMap resize, `String` clones, `apply_updates` splits) |
| 13 % | `__memcmp_avx2` | comparing cache keys and config-atom strings |
| ~12 % | SipHash (`sip::Hasher::write`, `hash_one::<&str>`) | hashing the (long) candidate strings used as cache keys |
| ~10 % | `LocalKey::with` + `Atom`/`Candidate`/`String` clone | the cache's own hit path |
| 6 % | `apply_updates_to_cp` | profile package-move application (pure string scan in a loop) |
| ~16 % | malloc/free | still per-call `Vec`/`HashSet`/`String` |

So the remaining opportunities are: a faster hasher for the cache keys
(SipHash → FxHash/ahash would reclaim most of the 12 %), returning
`Rc<Atom>` / `Rc<Candidate>` from the cache to skip the hit-path clone, and —
the real fix — not calling the parsers 34 M times in the first place (below).

## What to change next, in priority order

### 1. Bucket `package.*` config by `cp`, and pre-parse the atoms once  — biggest structural win

In `portage_profile::Config`, replace each `Vec<(String, Vec<String>)>`
(`package_use`, `package_use_user`, `package_env_use`, `package_use_force`,
`package_use_mask`, `package_use_stable_*`, `package_accept_keywords`,
`package_mask`, …) with a struct that:

- parses every entry's atom string into `portage_dep::Atom` **at load time**, and
- indexes entries in a `HashMap<(String,String), Vec<Entry>>` keyed by
  `(category, package)`, plus a `Vec<Entry>` for `*/*` and a
  `HashMap<String, Vec<Entry>>` for `cat/*` (portage's `ExtendedAtomDict`).

`matches_config_entry` / `specificity_ordered_flags` then iterate only
`bucket.get(&(cat,pkg))` ∪ `cat_star.get(cat)` ∪ `star_star` — typically < 10
entries — and match against the pre-parsed `Atom` (no `parse_atom`, no
`parse_candidate` on the config side). This alone should remove ~90 % of the
34 M `match_from_list` calls — which the memo cache currently absorbs at a
cost of ~35 % of the post-cache run (hash + compare + clone). Doing both is
strictly better; if only one is done, this is the more principled one.

### 2. Add an in-process metadata cache for `read_md5_cache`

`read_md5_cache` (`:1184`) re-reads the file, re-splits every line, and
re-runs `apply_updates_to_dep_string` over 5 keys on every call. Wrap it in a
process-global `OnceLock<RwLock<HashMap<PathBuf, Arc<HashMap<String,String>>>>>`
(the pattern already used for the binary index at `:1555`). The md5-cache is
immutable for the life of the process. Fold the `updates/` application into
the cached value so it happens once per file, not once per read.

### 3. Cache `list_candidates` per `(repo-set, cp)`

Same shape — the ebuild directory listing + per-version metadata for a `cp`
does not change during a resolve. Key by `(category, package)`; the value is
`Arc<Vec<Candidate>>`. Removes the ~4× redundancy and all the `read_dir`
syscalls behind it.

### 4. Memoize `effective_use_flags` per package

Key on `(candidate_str, is_stable-relevant inputs)` — or, better, compute it
once per `Candidate` when the candidate is first materialised and store the
`HashSet<String>` on the `Candidate` (portage's `pkg.use`). The USE context
(make.conf, `$USE`, autounmask accumulator) is constant within a backtrack
pass; invalidate the memo when the `'backtrack` loop folds in a new
`--autounmask-use` flip (rare).

### 5. Cheaper cache, or a hand-written parser

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
  `strip_version_prefix`). Only matters for the miss path once 1 and the
  cache are both in; low priority now.

### 6. Reduce redundant whole-graph passes

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

Call counts were obtained by adding `AtomicU64` counters to `parse_atom`,
`parse_candidate`, `match_from_list`, `read_md5_cache`, `effective_use_flags`,
`list_candidates`, and `apply_updates_to_dep_string`, dumped from `main` when
`PORTUALE_PERF_COUNT` is set. That instrumentation was reverted after the
investigation; re-add it the same way to check progress against the numbers in
this document. Note the memo cache means `parse_atom` / `parse_candidate` now
count *calls*, not regex runs — add the counter inside `*_uncached` to see
miss counts.

## Target

The memo cache already reached ~20.6 s (real `emerge` ≈ 16 s). Item 1
(cp-bucketed config) plus a cheaper cache should take the 34 M
`match_from_list` calls well under 1 M and leave the run I/O- and
graph-walk-bound rather than string-bound. The Rust graph walk itself is not
the bottleneck — the per-node recomputation is.
