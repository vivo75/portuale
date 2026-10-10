# crates/portuale/src/emerge_build/mod.rs
`--buildpkgonly` execution without `--pretend`: builds a binary package per entry the pretend gate proved safe.

## module
- Real `emerge --buildpkgonly` execution, WITHOUT `--pretend`: actually builds a binary package for every entry pretend.rs's own dry-run gate already proved is safe to build.
- The pretend gate (`pretend.rs`) checks `GraphResult::buildpkgonly_deps_unsatisfied == false` before calling anything here: no needs-building entry's own `required_by` set includes another needs-building entry, i.e. none of them depend on each other at all. That is Portage's `--buildpkgonly` whole point (every dependency must already be satisfied by something installed), so there is no cross-entry build ordering to compute here, unlike a merge's own topological sort.
- Reuses `ebuild_package::run_package` (task #105-#109) as-is: `package` is the unmodified `doebuild()` action Portage's `--buildpkgonly` itself is built on. Portage's `--buildpkgonly` is a resolution-time depgraph check, not a distinct execution mode of its own.
- `run_package`'s own `install` chain fetches a nonempty `SRC_URI` too (see `ebuild_phases::fetch_sources` / `crate::fetch`'s own module doc comments). This module used to refuse any entry with a `SRC_URI` outright (no fetch machinery existed yet); that refusal is gone now that fetching works, and a fetch/digest failure surfaces as an ordinary `run_package` error like any other build failure.
- `GraphEntry` deliberately does not carry the winning candidate's repo location (not threaded through the graph-resolution pair, which has no real-execution need for it). `locate_candidate` re-derives it via `portage_repo::list_candidates`, the same repo/version lookup `resolve_pretend_graph` already did to pick the winning version.
- Known, documented gaps (same "narrow v1, document the cut" pattern as every other real-execution slice): a `CandidateSource::Binary` entry (only via `--usepkg`) is skipped outright, already a binary with nothing to build; builds run strictly in `entries` order, which `resolve_pretend_graph` returns in Portage's dependency-first merge order (`topological_merge_order`), so even a relaxed gate would still build a dep before its dependent; a build failure aborts immediately unless Portage `--keep-going` is given (see `run_buildpkgonly`'s own doc comment), with no cleanup of already-built packages either way (portuale's single-invocation-at-a-time CLI usage never needs partial-build cleanup).

## fn entry_version
- `NoVisibleCandidate` cannot reach here: it aborts the whole resolution before any `GraphEntry` exists for it.
- A removal builds nothing (backlog #72 B3).

## fn locate_candidate
- Same repo/version lookup `resolve_pretend_graph` already did to pick the winning version, just not retained on `GraphEntry`. When more than one repo has the exact version (a rare overlay-shadows-main-repo case), prefers the highest-priority repo, the same tie-break resolving uses.

## fn ebuild_path
- Ports Portage's `doebuild()` `<repo>/<category>/<package>/<package>-<version>.ebuild` path convention.

## fn entry_matches_any
- Ports Portage's `--buildpkg-exclude` `InternalPackageSet.findAtomForPackage` check; each atom is an ordinary package atom matched against the entry's resolved cpv (+ slot).

## fn entry_is_live
- Ports Portage's `Package.binpkg_wanted` `"live" not in self.properties` half (`_emerge/Package.py:621-637`).
- The candidate's evaluated `PROPERTIES` is USE-conditional-reduced against the *build*, not the resolve-time, USE set; `entry.use_flags_display`'s enabled subset is the same set `entry_build_env`'s `USE=` export already uses.
- `PROPERTIES` has no `||`-group semantics (same reasoning as `portage_repo::evaluated_metadata_tokens`), so a flat `use_reduce` is faithful.

## fn entry_buildpkg_wanted
- Ports Portage's `Package.binpkg_wanted(exclude)`, narrowed to the `buildpkg` (not `buildsyspkg`) half portuale's `--buildpkg`/`FEATURES=buildpkg` models.
- `--buildpkg-exclude` (`entry_matches_any`) always wins outright; otherwise a `PROPERTIES=live` build is skipped unless `FEATURES=buildpkg-live` (Portage default, `buildpkg_live`, is on). Source: `Package.binpkg_wanted(exclude)`, `_emerge/Package.py:621-637`.
- A candidate this cannot even locate falls through to `true` (not live, by construction): the build path a moment later raises its own clear "could not locate its own ebuild file" error instead of this filter silently swallowing it.
