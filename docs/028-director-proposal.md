# 028 — `Director` as the production `action_build` entry? (H5, #28)

Status: proposal + close-out, 2026-09-15, `backlog/tier_2_e_5`. Slice H5 of
[`backlog_tier_5_and_2_sliced.opus.md`](backlog_tier_5_and_2_sliced.opus.md)
§7. Owner decision **D6** (2026-09-14) already fixes the end state of #28 as
**"slots only"**, so this note records the call graph after H1–H4, gives the
recommendation D6 asked for, and closes #28. It does not reopen D6.

## 1. Call graph after H1–H4 (verified on this branch)

Every `mrg_director` slot has a production caller. The `Director` struct
itself has none.

| slot (trait) | production implementation | production call site |
|---|---|---|
| solver (`Resolver`, re-export of `portage_repo::Resolver`) | `BacktrackingResolver` (+ `PubGrubResolver`/`ResolvoResolver` behind `--solver=`) | `rust/portuale/src/pretend.rs` `active_resolver_for(solver).resolve(&req)` |
| `SchedulerPolicy` | `UnlimitedPolicy`, `LoadAwarePolicy` | `rust/portuale/src/emerge_build.rs` (`-jN` DAG walk, `run_build_scheduler`) |
| `MergeEngine` | `SourceEngine`, `BinaryEngine` (`merge_engines.rs`) | `rust/portuale/src/emerge_getbinpkg.rs` `run_merge_plan` per-entry dispatch |
| `NewsSelector` | `FilesystemNews` | `rust/portuale/src/pretend.rs` `--check-news` |
| `Fetcher` (H1) | `WgetFetcher` | `rust/portuale/src/fetch.rs` `fetch_src_uri` candidate loop (`&dyn Fetcher`) |
| `PackagesDb` (H2) | `VdbReader` | `rust/portuale/src/ebuild_merge.rs` CONTENTS reads (`find_owners`/`owns_path`) |
| `BinpkgIndex` (H3) | `RemoteBinhostIndex` | `rust/portuale/src/emerge_getbinpkg.rs` remote `Packages` lookup |
| `RepoCache` (H4) | `Md5Cache` → `portage_repo::repo_aux_metadata` (cache → depcachedir → depend phase) | `rust/portuale/src/emerge_build.rs` `entry_metadata_env` |

Still test-only, by design:

- `Director<S, D, C, F, M, B, N, P>` (`rust/mrg-director/src/lib.rs`):
  its `plan()` and the per-slot forwarding methods are used only by
  the crate's own tests.
- `PackagesDb::reverse_dependents`: no production consumer (depclean keeps
  its own reverse-dependency walk).
- `PkgdirBinIndex`: the local `$PKGDIR` path still resolves by directory
  scan in `emerge_getbinpkg.rs`.
- `VolatileCache`: the in-memory `depcachedir` fallback is implemented
  inside the #41 provider rather than through this type.

So the orchestration shape real `actions.py::action_build` has (resolve,
then `Scheduler` walking fetch → build → merge per `MergeListItem`) exists
in portuale as `pretend::run` → `emerge_build`/`emerge_getbinpkg`. Each
algorithm inside that walk is now reached through a slot. The `Director`
value that would bundle the eight slots is never constructed in production.

## 2. Should `Director` become the production entry?

**Recommendation: no, not now (consistent with D6).** What promotion would
take, and why it doesn't pay yet:

1. **It is a refactor of `pretend::run`, not a wiring change.**
   `pretend::run` owns argument parsing, `--ask`/display, autounmask output,
   resume state (`mtimedb`), `--keep-going`, and the pretend/merge split. A
   `Director` entry would have to take a decomposed `action_build`
   (plan → confirm → fetch → build/merge → post) with those concerns as
   explicit stages. That is the "separate, later decision" D6's
   recommendation column names.
2. **There is no second implementation that needs runtime selection.**
   Every production slot has exactly one implementation per code path,
   except the solver, which already switches through `active_resolver_for`.
   The generic struct buys static composition, which no caller needs today.
3. **The risk sits on every merge path.** The L1/L2/L3 beds gate the
   current walk byte-for-byte. Moving the entry point touches all of them at
   once, for no behaviour change.

**When to revisit (the trigger list):**

- a second implementation lands in a non-solver slot and must be chosen at
  runtime (for example a content-addressed `MergeEngine` or an OCI
  `BinpkgIndex`);
- `mrg` (the applet) needs a non-`emerge` front end that should not go
  through `pretend::run`;
- the `action_build` stage split is wanted for its own sake (resume and
  `--keep-going` owned by one type).

If one of those fires, the path is: carve `pretend::run`'s merge half into
stage functions that take the slots as arguments, then have `Director`
own them. Construct `Director` in `pretend::run` behind no flag, and gate the
change on L1 porttest + L2 + L3 smoke staying at 0 unexplained.

## 3. Close-out

Per D6, #28 is **DONE**. All slots carry production traffic through
their traits. `Director` stays the composition type the crate's tests
exercise. The residues above (`reverse_dependents`, `PkgdirBinIndex`,
`VolatileCache`) are recorded as consumer-less by design, not as open work.
