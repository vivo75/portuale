# crates/portuale/src/merge_engines.rs
`MergeEngine` adapters executing the actual source/binary merges.

## module
- Grounding: Portage's `_emerge/MergeListItem.py::_start` dispatches on `pkg.type_name` -- a source package through the `EbuildBuild` chain, a `"binary"` one through `_emerge/Binpkg.py`. The `mrg-director` crate pins that kind routing (`SourceMergeEngine`/`BinaryMergeEngine`: own kind `Skipped`, other kind `Failed`); these adapters are the binary-crate halves that actually build and merge.
- Each engine carries the same context its underlying merge function already threads (repos, root, tmpdir, options, binhost config, `$PKGDIR`). The unit carries every merge-relevant field (version, kind, repo, slot/sub-slot and resolved USE for the build-phase env, `remote_binary`/`build_id` for binpkg location).
- Live paths: `emerge_getbinpkg::run_merge_plan` executes every entry's unit through one of these engines (mixed source+binary merge), and `emerge_build::run_source_merge`'s serial loop merges through `SourceEngine::merge_entry`. The `-jN` scheduler keeps calling its own build-then-merge split directly (build-half concurrency plus a serialized vdb merge, like Portage) -- the engine seam owns per-unit execution, not DAG scheduling.

## fn merge_unit_for_entry
- Version comes out of the outcome (`to` for an upgrade), kind out of `entry.source`, slot/USE/repo carried for the build-phase env.
- A blocker-removal task (backlog #72 B3) is not a merge unit: `AlreadyInstalled` (the merge functions' own early-return silent no-op) and `NoVisibleCandidate` map to `None`.

## fn rebuild_entry
- `New`/`Reinstall` share the `version` arm, `Upgrade`/`Downgrade` the `to` arm; display-only fields (blockers, oldbest, `required_by`, counters) are never consulted mid-merge.
- The `-rN` revision: `parse_candidate` splits it off `version` (Portage's `catpkgsplit`); the merge reads the full `package-version[-rN]`.

## struct SourceEngine / struct BinaryEngine
- `SourceEngine::merge_entry` merges one resolved entry from source (the `Source`-entry arm of the mixed dispatcher, and `run_source_merge`'s serial loop).
- The run's status display follows the blank/`>>>` rule for the status lines (backlog #197).
- `progress` is the entry's own `(curval, maxval)` snapshot (backlog #177).

## tests: engines_refuse_the_other_kind_and_a_bad_cpv
- The kind refusal is the same `type_name` routing Portage's `MergeListItem._start` performs. Live merging itself is covered by the existing `run_merge_plan` end-to-end tests.
