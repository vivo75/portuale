# crates/portage-repo/src/resolve/pretend.rs
Selecting one atom's candidate (`resolve_pretend`).

## fn dependency_avoid_update_candidate
- Portage's `lib/_emerge/depgraph.py` ~8440 (`if inst_pkg is not None and parent is not None and not self._want_update_pkg(parent, inst_pkg): return inst_pkg`); Portage's `_iter_match_pkgs` vardb-sourced USE-dep check for an installed package.
- The valid-flag domain follows Portage's `dbapi._iuse_implicit_cnstr` for a built package (recorded `IUSE` ∪ profile `IUSE_EFFECTIVE` ∪ the package's own recorded `USE`, bug #640318). On EAPI 5+ (all EAPIs here have `iuse_effective`): `_iuse_effective_match` (`elibc_*`/`kernel_*`/…) via `valid_iuse`, plus `_iuse_implicit_built`'s `flag in use` clause. Portage's `_match_use` recomputes the domain the same way rather than reading a persisted `IUSE_EFFECTIVE` file.
- `cat/pkg-1.0` installed only at slot 0 never satisfies a `cat/pkg:2` dependency even when the repo offers `cat/pkg-1.0` in slot 2.

## fn already_installed_or_reinstall
- The `BUILD_TIME` is of the local-or-remote binary candidate.

## fn slot_operator_binary_masked_version
- Backlog #212 (v2 `#24c`). `match_from_list` ignores USE-deps, so the marker would otherwise match the same-version ebuild too.

## fn is_binary_only_mask
- Backlog #212 (v2 `#24c`). Portage keys by package object.

## fn is_installed_only_mask
- Backlog #276. Portage's `_add_dep` missing-dependency mask (`depgraph.py:3456-3470`) keys by package object, so masking an installed parent hides only that instance -- the same-version ebuild stays visible for Portage's next backtracking try. `split_cpv` rejects the brackets.

## fn slot_operator_installed_masked_version
- Backlog #276.

## fn is_instance_only_mask
- Backlog #212/#276.

## struct SelectSource / SelectOptions
- 11 args trips `clippy::too_many_arguments`; same reasoning as `resolve_pretend_graph`'s identical allow. Backlog #123 for `local_binpkg`.

## fn resolve_pretend (pool)
- Portage's `dbs` construction (`if "--usepkgonly" not in myopts: dbs.append(("ebuild" ...)); if myopts.get("--usepkg") is True: dbs.append(("binary" ...))`); `is_visible` only consults source-independent fields (package.mask/license/keywords/properties/restrict), never anything ebuild-specific; `filter_usepkg_exclude_include` matches Portage's `break`-out-of-the-loop rejection.
- `--getbinpkg`: Portage's `bintree.isremote` -> not remote once in `_additional_pkgs`.
- `--binpkg-respect-use` (#69 S1): Portage's gate is `reinstall_use or (not installed and respect_use)` -- `--newuse`/`--reinstall` open it on their own, which is what makes a `newflag`-only IUSE difference reject the binary under `--newuse` even with `--binpkg-respect-use=n`.
- `--binpkg-changed-deps`: an explicit `--binpkg-changed-deps=y|n` forces the check on (even under `--usepkgonly`) or off (even for an ordinary `--getbinpkg`).
- `_equiv_ebuild_visible`: `dbs` order is ebuild-first, so Portage's `matched_packages` is non-empty by the time binaries are weighed. Portage's `(use_ebuild_visibility or matched_packages)` gate; `depgraph.py:8027`'s `not use_ebuild_visibility and (usepkgonly or useoldpkg)` guard.
- binpkg-multi-instance: Portage's `_iter_match_pkgs` yields instances newest-first and the selection loop `break`s on the first acceptable one.
- `slot_operator_mask_built` (`depgraph.py:2383`; `resolver/backtracking.py::_feedback_config:237-242` files it under `runtime_pkg_mask`; `_iter_similar_available` (`:3034`) skips masked packages on the re-pass). Portage masks the binary package object only.

## fn resolve_pretend (avoid_update)
- Confirmed live: `sys-fs/fuse`'s `sys-libs/liburing:=[abi_x86_64(-)?,...]` needs the early return -- the tree's only *visible* `liburing` candidate lacks the USE profile to satisfy the atom while the installed version satisfies it via its own vdb `USE`.
- #57: `dev-libs/paired` staying on installed 1.0 while a sibling's `>=paired-2.0` forces 2.0 reconciles on the backtracking retry instead of re-deriving the same conflict.
- #157: gating on `!empty` keeps portuale from reinstalling a keyword-masked installed dependency that Portage downgrades to the best visible stable.

## fn resolve_pretend (autounmask levels)
- Portage's `_AutounmaskLevel` is a single mutable object that's re-yielded (`allow_license_changes` sticks once set). Levels 3 & 6 (`missing keywords`) skipped -- no `**` suggestion. Portage bug #463394 for level 4.

## fn resolve_pretend (installed fallback)
- `dependency_avoid_update_candidate` only reaches atoms with *some* repo candidate (e.g. keyword-masked-but-present). `--usepkgonly` empties the ebuild pool wholesale, so without this an installed-only satisfier would wrongly go `NoVisibleCandidate` and land on the resume list ("no visible ebuild to merge") in a `-K` merge. Narrowed to the L1 test bed case (L1-a); the non-`--usepkg` path keeps reporting a tree-departed ebuild. Queue loop's #233 rewrite handles runtime-keyed deps.

## fn resolve_pretend (matched pipeline)
- `resolve_pretend_graph`'s unsolvable slot-conflict handler adds `!=cat/pkg-<ver>` entries to hide a specific version (of the conflicted package or of a pulling parent) and force a lower one on retry.

## fn resolve_pretend (USE filter)
- Same "can't tell, so exclude" precedent `reinstall_flags_for_use_change` sets. `use_masked_only` re-checks `is_visible` (redundant here) and the same "only-a-flip-away" test.

## fn resolve_pretend (useoldpkg)
- See `set_useoldpkg_atoms`.

## fn resolve_pretend (installed checks)
- `installed` (version-only, all slots) is what `dependency_avoid_update_candidate` consumes -- a separate concern from the slot-aware `installed_pairs`. Portage renders `[ebuild NS]`.

## fn resolve_pretend (installed-only USE selection)
- #74 S1, selection half. Portage's `_want_installed_pkg` keeps an installed package the user did not name as a nomerge selection. Host probe: `dev-lang/rust:1.94.1[llvm_slot_21]` (installed with the flag; the profile's `LLVM_SLOT` expand turns it off for the ebuild) stays `AlreadyInstalled` under `-puDvN`.

## fn resolve_pretend (update shortcut)
- Portage's `selective`/`is_top_level` semantics; the top-level `selective` arm is already false under `empty` (`selective = selective && !empty`).

## fn resolve_pretend (best pick)
- Portage's `_wrapped_select_pkg_highest_available_imp` builds `matched_packages` in `dbs` order. `--usepkgonly` has no ebuilds; without `--usepkg` no binaries -- moot either way.

## fn resolve_pretend (installed best)
- `policykit` (global flag `systemd`'s ebuild now defaults on) shows `[ebuild R] … policykit*` at rc 0; the child-flip block records an autounmask change only when the *effective* USE still can't satisfy the atom (`mime-types [nginx]` -> rc 1).
