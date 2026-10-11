# crates/portuale/src/emerge_build/resume.rs
`--resume` entries and their repositories.

## fn resume_cpv
- Ports Portage's `_save_resume_list`, which only keeps `Package`s with `operation == "merge"` (`Scheduler.py:2414-2418`): an already-installed no-op, a blocker removal, or an entry with no visible candidate merges nothing. The up-front save and the per-merge shrink name every entry identically because the kind tag comes straight from the resolved source, the same tag `entries_not_merged` (`pretend.rs`) writes on the failure path.

## fn entry_repo
- `repo_name` is always `Some` for a merging entry (see `GraphEntry::repo_name`).

## fn resume_binary_repo
- Ports Portage's `depgraph.py::_loadResumeCommand`, which re-resolves a `["binary", root, cpv, "merge"]` resume item from its bintree (`pkg_type == "binary"` matches the binary tree only). The resumed `Package`, and the `(N of M) cpv::repo` line (`PackageMerge._make_msg`, `PackageMerge.py:25`, `pkg.cpv + _repo_separator + pkg.repo`), carries the binary's repository: the `Packages` index `REPO` field Portage's `bintree` surfaces as the package's `repository`.
- Pool order mirrors the resolver: the local `$PKGDIR` index first (Portage's `bintree.isremote` prefers a local file), then the configured remote binhosts in order (`find_remote_binpkg`). A record without `REPO` yields `"__unknown__"`, Portage's `portage.versions._unknown_repo`, exactly what `binary_candidates_from_index` (hence a fresh binary merge) reports for the same record.
- Backlog #186. `None` keeps the ebuild-repo fallback (the merge then reports it cannot locate the entry, same as a non-resume run).

## fn resume_entry
- `source` is the resumed mergelist entry's own recorded `ResumeEntryKind` (Portage's own `type` tag). `CandidateSource::Binary` always resolves from the local `$PKGDIR` (`remote_binary: false`); see `mtimedb.rs` for why re-deriving "was this fetched remotely" is not attempted.
- `repo_name` is re-derived because the merge progress lines print Portage's `cpv::repo` shape (backlog #177) and the saved resume list records only `cat/pkg-ver`. Same highest-priority tie-break `locate_candidate` uses. `None` when no repo carries the version anymore (the merge reports it cannot locate the ebuild, same as a non-resume run). For a resumed binary entry, `binary_repo` (`resume_binary_repo`, Portage's bintree re-resolution, backlog #186) wins; `None` keeps the ebuild lookup as the fallback.
- `run_merge_loop`'s `--keep-going` dependent-drop has nothing to key on (`--resume` does not pass `--keep-going`).

## fn resolved_features
- Ports Portage's `PORTAGE_LOG_FILE` (`PORTAGE_LOGDIR` unset means `${T}/build.log`, i.e. `${PORTAGE_BUILDDIR}/temp/build.log`; with `FEATURES=compress-build-logs`, `${T}/build.log.gz` -- `prepare_build_dirs.py`'s own `f"build.log{compress_log_ext}"`). Same precedence `ebuild_phases::features_string` uses for the phase-execution gates; `MergeOptions::features` when the caller resolved a config (#37 S2), else the raw process env (standalone `ebuild <file>` / tests).
- `pub(crate)` for the binary-merge digest-failure path (`emerge_getbinpkg`): Portage's `Binpkg` prepares the same builddir before `BinpkgVerifier` runs, so the failure log lives at this same path.
