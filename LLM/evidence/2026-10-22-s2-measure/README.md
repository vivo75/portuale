# #441 S2 measure-first: `@world` cost attribution, 2026-10-22 (backlog #441 / #457)

Question from the slice brief: which share of `emerge -uDpvN @world` is
(a) repo md5-cache reads/parses, (b) VDB reads/parses, (c) atom/dep-string
parsing, (d) binary-index (Packages) parsing, (e) everything else — and does
any snapshot-addressable component reach ~25%?

Method of `LLM/evidence/2026-10-21-world-profile/README.md` (same frozen
input, same tools): `PORTUALE_PROFILE=1` phase timers plus `eu-stack`
samples every 0.4 s aggregated with `LLM/tools/stack_agg.py` (symbols
demangled with `llvm-cxxfilt`).

Binary: `target/release/portuale` built from `682149ab` (no source file
newer than the binary; `cargo build --release` a no-op). All runs rc 1
with the same autounmask report as the 2026-10-21 evidence (the abort
path, not the owner's 194 s completed plan).

Frozen input re-verified before the runs: gentoo
`metadata/timestamp.chk` = `Sun, 18 Oct 2026 23:15:53 +0000` (bed pin),
buildovl HEAD `cd571fc637a805ebde0e8497eab6f124b41c8f96` (bed pin),
VDB 2146 packages (matches the 2026-10-21 digest input), world 209
entries. `PORTAGE_BINHOST` unset. Command (the brief's exact flags):

```
PORTUALE_PROFILE=1 target/release/portuale emerge -uDpvN @world
```

## Phase timers (median of 3 runs)

stdout byte-identical across all 3 timer runs and all 3 sampled runs
(md5 `8eccc0a1eb3e05d7519973d2334420c6`); non-`[profile]` stderr
identical too.

| span | run 1 | run 2 | run 3 | median |
|---|---|---|---|---|
| expand_targets | 0.150 | 0.144 | 0.123 | **0.144** |
| refresh_binhost_indexes | 9.906 | 9.783 | 9.757 | **9.783** |
| plan_resolve | 9.939 | 9.816 | 9.791 | **9.816** |
| ResolveCtx::new | 0.077 | — | — | **~0.08** |
| resolve_once | 14.413 | 14.255 | 14.122 | **14.255** |
| run_resolve_phases | 14.416 | 14.259 | 14.126 | **14.259** |
| serialize_merge_order select_nodes | 0.020 | 0.019 | 0.020 | **0.020** |
| display_and_post | 0.005 | 0.005 | 0.005 | **0.005** |

Wall ≈ 24.2 s (down from 40.3 s on 2026-10-21: S1 shipped
`collect_memoized` pool scope, VDB freeze scope and seven memo cuts;
resolve 30.4 s -> 14.3 s). The refresh is 40% of wall and is a full
gentoo re-download (TTL expired again); S1's `If-Modified-Since` only
helps when the server answers 304.

## Stack attribution (178 samples over 3 runs, inclusive per-frame shares)

Bucket mapping (frame contains any of): (a)
`read_md5_cache|repo_aux_metadata|list_candidates|md5_cache_entry_is_valid|…`;
(b)
`vdb_aux_get|read_vdb_flag_set|installed_candidates|all_installed_packages|category_generation|libc_provider_cps|…`;
(c)
`parse_atom|parse_match_atom|parse_candidate|match_atom_matches_candidate|match_from_list|match_pair|use_reduce|resolve_disjunctions|evaluate_atom_conditionals|…`;
(d) `parse_packages_index` (parse) vs `refresh_binhost_indexes|download_*`
(network, kept separate: freshness semantics, not snapshot work).

| component | samples | share | ≈ s of 24.2 |
|---|---|---|---|
| (d-parse) binary `Packages` parsing (`parse_packages_index`) | 0 | **0%** | ~0 |
| (a) repo md5-cache read+parse (`repo_aux_metadata` 3, `list_candidates` 1; `read_md5_cache` 0) | 3 | **2%** | 0.4 |
| (b) VDB reads (`read_vdb_flag_set` 5, `installed_candidates` 5, `vdb_aux_get` 4, rest reinstall/meta CPU) | 25 | **14%** | 3.4 |
| (c) atom/dep parse+match (`parse_atom`/`match_from_list`/`match_pair`/`use_reduce` 8 each, `effective_use_flags` 4) | 29 | **16%** | 3.9 |
| network binhost refresh (`refresh_binhost_indexes` 72, `download_*` 69, all in wget `read`) | 72 | **40%** | 9.8 |
| (e) walk machinery (`step_queue_item` 43%, `resolve_pretend`/`resolve_queue_atom` 22%, `record_unresolved_dep`/`select_resolved_version` 10% each, `collect_pass_blockers` 8%, `schedule_graph` 4%, `enqueue_*` 6-8%, display/config/select_nodes ~2%) | — | **remainder** | ~6.7 |

Raw samples: `stacks.txt` (178 `===`-separated `eu-stack -n 60`
captures, demangled); timers of run 1: `stderr.txt`. Sample noise is
±1 sample ≈ ±0.6 pp; nothing near a boundary depends on it.

Why (a) is only 2%: S1-era process memos already hold — `read_md5_cache`
is memoised per path and `list_candidates` per (repos, cp), so the
33,606 md5-cache files (171 MB) are read once per process and never
reappear on-stack (`read_md5_cache` itself: 0 samples). A persistent
`RepoSnapshot` could only remove that single first read (~0.4 s).
Same for (b): the VDB freeze scope + walk-long installed-meta memo
removed the per-pass re-reads; what is left is first-touch I/O plus
per-pass CPU (`reinstall_flags_for_use_change`, iuse checks).

## S2 decision: implement nothing

No snapshot-addressable component reaches ~25%, alone or together:

- (a) repo snapshot would accelerate ≈ 2%.
- Installed snapshot would accelerate at most (b) ≈ 14%.
- (a)+(b)+(d-parse) together ≈ 16% — still under the bar.
- (c) ≈ 16% is not snapshot work at all: pure per-pass CPU
  (atom parse proper is 4%; the rest is match/use-reduce evaluation
  already covered by in-process memos). A persistent atom table is a
  memo, not a snapshot, and caps at ~4%.
- The 40% network refresh is freshness + download, not parsing
  (`parse_packages_index`: 0 samples). Skipping it changes semantics
  (strong review §2.1: binhost TIMESTAMP/TTL is a time-dependent input);
  S1's conditional fetch already takes the 304 case. No snapshot can
  legally remove a changed-upstream full download.
- (e) is walk-inherent (queue stepping, outcome resolution, blocker
  scans, merge-order graph) — the S1 line of attack, not snapshots.

The plan's §11 snapshot layer (and the `portuale cache build` /
`--no-cache` surface) is therefore **deferred, not cancelled**: revisit
only if a later profile on a colder cache or a completed-plan workload
(not this abort path) shows a first-read component above the bar. The
in-process memos that S1 shipped are the snapshot's cheap half and stay.

Provenance: binary `682149ab`, `llvm-cxxfilt` from `/usr/lib/llvm/23`,
sampler loop as in `LLM/tools/profile_sample.sh` but with the brief's
`-uDpvN` flags and out dir `/var/tmp/s2-measure` (scratch, deleted
after the evidence was copied here).
