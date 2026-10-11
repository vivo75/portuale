# crates/portage-repo/src/resolve/pass/candidate_pool.rs
Re-derives the source (ebuild, PKGDIR or binhost binary) of the resolved version.

## fn candidate_pool
- The resolved version may have come from any of `repos`, PKGDIR (`--usepkg`/`--usepkgonly`) or a remote binhost (`--getbinpkg`); reuses `list_candidates`/`list_binary_candidates` rather than threading a repo location out of `PretendOutcome`, since more than one source can carry the identical version.
- Portage `depgraph.py:8492`: `_wrapped_select_pkg_highest_available_imp` returns `matched_packages[-1]` with `ebuild` built first in `dbs` order ("ebuild type is the last resort"), so at a version tie a `--usepkg`/`--getbinpkg` binary that passed `binpkg-respect-use` beats the ebuild. Mirrors `resolve_pretend`'s own pool construction (`--usepkgonly` excludes ebuilds entirely).
- The `--binpkg-respect-use` / `--binpkg-changed-deps` gates are the same ones `resolve_pretend` applied when it chose the version.
- `_equiv_ebuild_visible` (see `resolve_pretend`): `--use-ebuild-visibility` enforces the visible-ebuild requirement even under `--usepkgonly` / a `--useoldpkg-atoms` match.
- Backlog #212 (v2 `#24c`): only the `[binary]`-marked masked form filters here; plain (`!=cpv`) negatives are ignored.
