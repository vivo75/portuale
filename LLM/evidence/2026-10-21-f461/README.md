# Reproducible `@world`-shaped benchmark input (backlog #461), 2026-10-21

Pmtest-side deliverable for #461: the frozen #455 benchmark
(`LLM/evidence/2026-10-21-world-profile/README.md`) pins live-host
VDB/config state by content hash -- reproducible only on that machine,
and every run re-downloads two binhost indexes (~9.8 s network-fixed).
This input replaces the live state with a deterministic generator, so
any machine reproduces the workload with no network access.

## What was built (all pmtest-side, no product-code change)

- `pmtest/bench/gen_world_shaped.py` -- seeded generator (`--seed 461`).
  Writes a self-contained root: synthetic repo `benchmain` (~1200
  ebuilds with real-md5 `md5-cache` entries), pinned VDB (~4000
  installed), ~300-atom world file, `file://` binhost index, minimal
  config (repos.conf with relocatable `location = repo`,
  `EMERGE_DEFAULT_OPTS` = the host's own
  `--usepkg=n --getbinpkg=y --binpkg-respect-use=y`, `PORTAGE_BINHOST`
  scrubbed by the runner). Nothing committed; regenerated at run time.
- `pmtest/bench/world_shaped_benchmark.py` -- checks the tree
  fingerprint (refuses stale input, regenerates), resolves the binary
  through `managers/managers.yaml`, wipes `var/cache/` before every
  run (a run writes its regenerable binhost cache there -- leaving it
  would make run N+1 a cache-hit workload), does 1 unmeasured warmup,
  runs `emerge -puDN @world` (the #455 argv, byte-identical) 3 times,
  fails on rc != 0 or non-identical stdout, prints the median table.
- `pmtest/bench/WORLD_SHAPED.md` + a pointer item in
  `pmtest/USAGE.AGENTS.md` ("Valuta l'efficienza", item 5).

## Evidence run (this directory)

Canonical input at `/var/tmp/pmtest-world-bench` (21,707 files,
1,175,824 content bytes, ~76 MB on disk), tree digest
`3e5c307d…` (`fingerprint.json` here carries params + digest; the full
per-file map lives with the input). Binary
`target/release/portuale` from `f12241b5-dirty` (this worktree),
`release` profile. Three timed runs back-to-back after one warmup. Box note: four
sibling workers were active; a mid-day
invocation showed 2.3x slower medians with the same <3% within-run
spread -- compare only back-to-back pairs from one invocation.

| span | run 1 | run 2 | run 3 | median |
|---|---|---|---|---|
| expand_targets | 0.004 | 0.004 | 0.004 | **0.004** |
| refresh_binhost_indexes | 0.003 | 0.003 | 0.003 | **0.003** |
| plan_resolve | 0.004 | 0.003 | 0.003 | **0.003** |
| ResolveCtx::new (2 builds, total) | 0.053 | 0.056 | 0.056 | **0.056** |
| run_pass first=true ×2 | 0.920 | 0.910 | 0.911 | **0.911** |
| collect_pass_blockers ×2 | 0.267 | 0.266 | 0.263 | **0.266** |
| resolve_once complete=false | 1.202 | 1.193 | 1.193 | **1.193** |
| run_resolve_phases | 1.205 | 1.196 | 1.196 | **1.196** |
| serialize_merge_order select_nodes | 0.002 | 0.002 | 0.002 | **0.002** |
| display_and_post | 0.125 | 0.125 | 0.124 | **0.125** |
| wall | 1.354 | 1.343 | 1.342 | **1.343** |

stdout byte-identical across all runs (sha256
`92971776…`, 1188 merge rows: 1008 `[binary gU]`, 40 `[binary NSg]`,
140 `[ebuild N]`); rc 0. Raw: `stdout-run1.txt`,
`stderr-run1.txt` (full `[profile]` stderr), `results.json`
(`--json` output with pm/version/profile/tree digest).

## Comparison with the host numbers (#455: `resolve_once` 30.4 s)

Same span names, same 2-pass `-D` re-walk structure, same shape of
costs (run_pass dominates, blockers second, refresh negligible here
by design). Absolute times are ~25x smaller: this input is shaped,
not sized, like `@world` -- compare *relative* numbers
(before/after an optimisation on this input), never against the host
table. Deliberate cuts: no network (the 9.8 s refresh is out of
scope; the binary-candidate pool IS exercised via the hermetic
`file://` index), no Portage oracle (perf infra, not a parity cell;
the borrowed shapes -- satisfied `!=` blocks, slot atoms, newuse --
are grounded by #68/#361/#69).

## Reproduce

```
python3 bench/world_shaped_benchmark.py --skip-build   # reuses /var/tmp/pmtest-world-bench after a fingerprint check
python3 bench/world_shaped_benchmark.py --verify-only  # fingerprint only
```
