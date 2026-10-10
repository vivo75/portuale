# crates/portage-repo/src/candidates.rs
Ebuild candidates for a package.

## enum CandidateSource
- Portage's own `pkg.type_name` (`lib/_emerge/RootConfig.py`'s `pkg_tree_map`, exactly the strings `"ebuild"`/`"binary"`). See `GraphEntry::source` for the `--pretend` bracket words.

## struct Candidate
- `sub_slot`: read via `split_slot` alongside `slot`; folded into every candidate string for `portage_dep::match_from_list` as a `slot/sub_slot` suffix (see `portage_dep::Candidate`'s regex, which already parses this shape). Closed a previously-silent gap: any sub-slot-restricted atom could never match anything, since every candidate string omitted the sub-slot half no matter what the real `SLOT` metadata said.
- `repo_location`/`repo_priority`: `repos` iteration order comes from `find_repos`'s ascending `(priority, name)` sort. Needed once there's more than one repo (see `list_candidates`); ties break toward the higher-priority repo (`resolve_pretend`).
- `repo_name`: appended to every candidate string except the two paths in the module doc comment's `::reponame` bullet. Portage cross-checks against each repo's `profiles/repo_name` file too; portuale reuses the already-read `repos.conf` name rather than reading a second file.
- `license`: empty matches Portage `use_reduce("")`'s empty result; see `is_visible`'s license-masking check.
- `iuse`: most packages' LICENSE has no conditional, so most candidates never need this field.
- `properties`/`restrict`: see `properties_accepted`'s doc comment.
- `binary_use`: the only consumer of `list_binary_candidates`' output is `resolve_pretend`'s `--binpkg-respect-use` check, comparing this against what would currently be selected over the candidate's own `iuse` flags.
- `remote`: flows to `GraphEntry::remote_binary` -> the `g` bracket column (`PkgAttrDisplay.remote_binary`, `output_helpers.py:627-633`). Always `false` for ebuilds and local `$PKGDIR` binaries.
- `build_id`: Portage `output.py::_append_build_id` appends `-{build_id}` to the displayed version of a `type_name == "binary"` package (before `:slot` / `::repo`). From Portage binpkg-multi-instance / binhost `_pkg_str.build_id`.
- `build_time`: from Portage `_pkg_str.build_time`. Portage `dbapi._cmp_cpv` sorts same-version instances by `build_time`, and `_iter_match_pkgs` reverses that and `break`s on the first acceptable one -- so the highest `BUILD_TIME` passing the checks wins (`dedup_binary_instances`).

## fn strip_version_prefix
- Same disambiguation principle as the PMS package-name grammar ported in portage-dep's atom parser.

## fn split_pf
- Since #127 the single copy: the former `portuale::remote_bundle::split_pf` and this crate's private `split_installed_dir` were deleted for it. Needed by the md5-cache validator to find an entry's ebuild (`<repo>/<cat>/<pn>/<pf>.ebuild`); e.g. `foo-1bar-2.0` -> `foo-1bar`. Portage `lib/portage/versions.py`: a `-r<digits>` revision belongs to the version only when the rest still verifies (else `foo-r1-2.0` would mis-split).

## fn list_candidates
- Mirrors Portage `portdbapi.cp_list`, gathering from every configured repo, not just the first one with the package. V1 doesn't distinguish "stale cache" from "doesn't exist" -- both mean "not visible". The resolver calls this per cp per graph-walk visit (visibility, keyword, slot-operator, changed-deps and parent-scan checks all re-list the same cp); each build is a `read_dir` plus a dozen owned strings per version. Sound because nothing here ever writes an `.ebuild` (`--regen` only touches `metadata/md5-cache`, in a separate process) -- same reasoning as `read_md5_cache`. Keyed by a hash of the repo locations + `cp`.
