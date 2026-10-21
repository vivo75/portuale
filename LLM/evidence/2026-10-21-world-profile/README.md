# `emerge -puDN @world` frozen benchmark + S0 profile, 2026-10-21 (backlog #455)

Method of `LLM/evidence/2026-10-20-world-profile/README.md`:
`PORTUALE_PROFILE=1` phase timers (`portage_util::profile`, stderr only)
plus `eu-stack` samples every 0.4 s aggregated with
`LLM/tools/profile_sample.sh` + `LLM/tools/stack_agg.py`
(symbols demangled with `llvm-cxxfilt`).

Binary: `target/release/portuale` built from `c59a1098` + the one-line
`refresh_binhost_indexes` span below (uncommitted, stderr-only),
mtime 2026-10-21 08:34 UTC. All runs `rc 1` with the same autounmask
report as the 2026-10-20 evidence (the abort path, not the owner's
194 s completed plan).

## Step 1 note: the frozen input (written before any timer ran)

(a) Invocation (same flags as the 2026-10-20 S0 evidence, so the numbers
are comparable):

```
PORTUALE_PROFILE=1 target/release/portuale emerge -puDN @world >/var/tmp/b455/world.out 2>/var/tmp/b455/world.err
```

(b) Tree+VDB fingerprint (content hashes taken 2026-10-21 ~08:35 UTC,
before the first profiled run; re-hash to verify before reusing):

- Gentoo tree `/.gentoo/repos/gentoo` (= `/var/db/repos/gentoo`, bind
  mount): `metadata/timestamp` = `Sun, 18 Oct 2026 23:15:53 +0000`,
  byte-identical to the bed pin
  (`../pmtest/differential-test-bed/repo-pins.txt`:
  `gentoo_timestamp_chk=Sun, 18 Oct 2026 23:15:53 +0000`), so the tree is
  reproducible from the bed image + pins. Content digest over the
  resolver's metadata inputs (`metadata/` + `profiles/` + `eclass/`,
  sorted per-file md5s): `c93f61c9db95d694bed765674e48d027`
  (41,411 files, 85,213,885 bytes). Ebuild bodies are covered
  indirectly: a body change without an md5-cache update fails `_md5_`
  validation (backlog #46) and aborts the run loudly, so it cannot
  silently change the workload.
- Overlay `/.gentoo/repos/buildovl`: git `cd571fc637a805ebde0e8497eab6f124b41c8f96`
  (clean), byte-identical to the bed pin (`buildovl_head=cd571fc...`).
  Active profile: `buildovl/profiles/vivo/desktop`
  (`make.defaults` md5 `6bfd6dcc307c4aad55ee12b0b5757072`).
- VDB `/var/db/pkg` (live state, not bed-pinned): `counter` 23355,
  2146 installed packages, content digest over all files (sorted
  per-file md5s): `d6e2df4805929c94e8fd16ec1829e494`
  (143,179,190 bytes). Verified unchanged (`counter`, package count)
  after the runs.
- Config: `world` (209 entries) md5 `4b46e0543e8a4708ea619dd010f24188`,
  `world_sets` empty (`d41d8cd98f00b204e9800998ecf8427e`); `make.conf`
  md5 `743f8ddad5e1bf8ea8171597225bf767`; `binrepos.conf/{f1r,gentoo}`
  md5 `702cc3a5aaaaab5d983fb3c9e6f7ec12`,
  `0acf5a76c2db4417f415727faad6c338`.
- Binhost (the benchmark uses `PORTAGE_BINHOST` **unset** — verified
  empty in the run environment): binrepos `[f1r]` (priority 42,
  `https://gpkg.f1r.eu/seed-desk/`) + `[gentoo]` (priority 9999,
  `https://distfiles.gentoo.org/releases/amd64/binpackages/23.0/x86-64/`),
  `--getbinpkg=y` from `make.defaults`
  (`EMERGE_DEFAULT_OPTS="--usepkg=n --getbinpkg=y
  --binpkg-respect-use=y --ask-enter-invalid"`). Cached indexes at run
  time: f1r `TIMESTAMP 1792496582`, no `TTL` (always re-fetched);
  gentoo `TIMESTAMP 1792492417`, `TTL 3600` (expired — re-fetched).
  Every profiled run re-downloads both indexes, so the refresh cost is
  a stable, measured part of the benchmark, not cache luck. One extra
  run with `PORTAGE_BINHOST` set documents the delta (see §Binhost).

(c) Why frozen: the content digests pin the exact bytes profiled —
any tree/VDB/config drift is detectable by re-hashing, and the tree
half is reproducible from the bed image via `repo-pins.txt`. No
`emerge --sync` or merge ran during the profiling window. Caveat:
the VDB/config half is live-host state, not a committed fixture; a
fully reproducible fixture input is an open question for the
coordinator (see report).

Change since the 2026-10-20 evidence: one new span,
`refresh_binhost_indexes`, around the binhost refresh in
`pretend/resolve_plan.rs` (isolates the fixed network cost inside
`plan_resolve`; stderr-only, no behaviour change).

## Phase timers (median of 3 runs, `PORTAGE_BINHOST` unset)

stdout byte-identical across all 3 runs (`cmp`); non-`[profile]`
stderr byte-identical too (fetch notes stable: both indexes
re-downloaded every run).

| span | run 1 | run 2 | run 3 | median |
|---|---|---|---|---|
| expand_targets | 0.151 | 0.125 | 0.125 | **0.125** |
| refresh_binhost_indexes | 9.770 | 9.793 | 9.758 | **9.770** |
| plan_resolve | 9.802 | 9.824 | 9.788 | **9.802** |
| ResolveCtx::new (2 builds: initial + post-restart, total) | 0.135 | 0.118 | 0.118 | **0.118** |
| run_pass ×22 (first=true once, then 21× first=false ~1.21–1.26) | 2.663 first | 2.624 first | 2.620 first | **2.624 first** |
| collect_pass_blockers ×23 (~0.19 each, ~4.4 total) | — | — | — | **~0.19 each** |
| resolve_once complete=false | 30.365 | 30.464 | 30.290 | **30.365** |
| run_resolve_phases | 30.367 | 30.466 | 30.292 | **30.367** |
| serialize_merge_order select_nodes | 0.021 | 0.023 | 0.021 | **0.021** |
| display_and_post | 0.004 | 0.004 | 0.004 | **0.004** |

Wall clock ≈ 40.3 s (faster than the 2026-10-20 44.5 s: the
#457 `binary_deps_changed` memo and the #454 `Arc` installed list
landed since). The refresh is 99.7% of `plan_resolve` and 24% of wall.

## Inclusive stack samples (293 samples over the 3 runs, `stack_agg.py`)

| frame | share |
|---|---|
| backtracking_resolve / run_resolve_phases | 74% |
| run_pass | 69% |
| step_queue_item | 57% |
| resolve_pretend (via resolve_queue_atom / initial_outcome) | 45% |
| refresh_binhost_indexes (wget download) | 25% |
| collect_binary_candidates (remote pool) | 22% |
| collect_pass_blockers | 10% |
| list_remote_binary_candidates | 9% |
| parse_atom | 8% |
| installed_matches_argument | 8% |
| binary_candidates_from_index / match_atom_matches_candidate / vdb_aux_get / schedule_graph | 7% each |
| is_visible / binary_deps_changed / build_digraph | 6% each |
| read_vdb_flag_set | 5% |
| find_hard_cycles | 4% |
| installed_candidates_by_cp | 3% |
| reinstall_flags_for_use_change | 3% |

Raw samples: `stacks.txt` (293 `===`-separated `eu-stack -n 60`
captures, undemangled); full stderr of run 1: `stderr.txt`.

## S1 decision (which review §2 multiplier dominates)

None alone — the profile is diffuse (same verdict as the strong
review §3). Against review §2's list: (A) the triple-BFS
`ResolveCtx::new` is 0.12 s total (0.3%, not dominant — the S1a `Arc`
work collapsed it); (B) the `-D` re-walk branch costs one extra
`ResolveCtx::new` + 1 of the 22 passes (~1.2 s, 4%); (C) the per-pass
VDB scans survive as `collect_pass_blockers` 10% (~4.4 s) plus
`vdb_aux_get` 7% / `read_vdb_flag_set` 5%; (D) backtracking itself is
the bulk (`step_queue_item` 57%, `resolve_pretend` 45%); (E) the
`PassState`/merge-order linear scans are `schedule_graph` 7% /
`build_digraph` 6%. Outside the review's list, two items are now
visible that S1 must price: `collect_binary_candidates` 22% (remote
pool over two binhosts) and `parse_atom` 8% (cf. #457's atom-parse
cache note). The refresh (24%) is network-fixed; only TTL/cache
policy can move it. `find_hard_cycles` (35% in the strong review's
24-sample run) is down to 4% — do not chase it first.

## Binhost: `PORTAGE_BINHOST` set vs unset

Benchmark = unset (above). One extra run with
`PORTAGE_BINHOST=https://distfiles.gentoo.org/releases/amd64/binpackages/23.0/x86-64-v3/`
(a real third binhost, so one more index fetch plus new remote
candidates): `refresh_binhost_indexes` 9.77 → 16.52 s (+6.75),
`resolve_once` 30.37 → 37.26 s (+6.9), `rc` still 1 but stdout
**differs** (the extra remote candidates move the abort report: the
`dev-libs/libbpf` conflict block renders differently). Non-`[profile]`
stderr is unchanged. Conclusion: the binhost set is part of the
fingerprint — a set-vs-unset comparison is a different workload, and
the frozen benchmark pins unset.
