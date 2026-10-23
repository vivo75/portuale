# crates/portage-repo/src/user_patches.rs

## fn binary_user_patches_ok
- Selection's `--binpkg-respect-user-patches` retain (backlog #509): a binary survives iff its recorded `USER_PATCHES` equals the local digest for its `(cat, pkg, version, slot)`. Default on in every mode (also `--usepkgonly`); `=n` skips the `retain` outright. Portage: `depgraph.py` (`binpkg_respect_user_patches and pkg.user_patches != pkgsettings.userPatchDigest(pkg)`, bug #917047). Wired at both selection sites (`collect_binary_candidates` in `resolve/pretend.rs`, `candidate_pool` in `resolve/pass/candidate_pool.rs`) and through `pool_memo`'s flags; the masked-row re-derivation (`masked_deps.rs`) uses the same predicate, so report and selection cannot drift.
- Modeling note: Portage checks `--usepkg-exclude/--include` first and `break`s out of the built loop, so an excluded pool never reaches the digest check. Portuale's retains are independent (same as the respect-use retain precedent): a binary both excluded and digest-mismatched is dropped either way. No oracle shape covers the combination; upstream's own test says exclude/include "do not change this behaviour".

## struct IgnoredUserPatchBinary
- One dropped binary for the ignored-binaries block: `cpv[:slot][-build_id]` row plus the local patch files. `build_id` comes from the stanza (or the multi-instance filename parse); the slot suffix shows only when `!= "0"`, the build suffix only when non-empty (oracle shape 13: `app-misc/up-e-1.0:1-1`).

## fn render_ignored_user_patch_binaries
- Byte port of `_show_ignored_binaries_user_patches`: leading blank line, `!!! The following binary packages have been ignored due to user patches:`, rows `sorted()` (five-space indent, ` for <root>/` suffix when root is not `/`), the `--verbose`-only `!!! These user patches are triggering this warning:` tail (sorted, deduped), and the `=n`/`=y` NOTE. Oracle shapes 02/10/11 pin every byte (`LLM/evidence/2026-10-23-509/captures/`).

## fn eliminate_ignored
- Port of `_eliminate_ignored_binaries` (upstream `a7c346aa4`): a dropped binary is not reported when a more optimal alternative fills its slot — higher version selected, or an equal-version binary selected (`Package.__ge__` is true on equal versions; its build-time arm is unreachable dead code, copied as-is). The identical-installed-instance arm is subsumed by the merge-list filter (a kept installed instance never merges, so its cp is absent). Applied to the success-path recompute; the abort path passes no selected packages.

## fn ignored_user_patches_for_merging
- Success-path rows (backlog #509): for every merging `(cp, slot)`, the dropped binaries of the local pool plus the remote pools under `--getbinpkg`, minus `eliminate_ignored`. This recomputes Portage's `ignored_binaries["user_patches"]` accumulation filtered by the displayed list (`_serialized_tasks_cache`): selection's retains are pure, so recompute equals accumulation. Remote pools share the path (no separate remote oracle shape; same retain over the extended pool). Stored on `GraphResult::ignored_user_patch_binaries`, printed by `pretend/final_gates.rs` between the ABI rebuilds and the changed-deps report (Portage `display()` order), suppressed under `--quiet` and under an explicit `=y|n`.

## fn ignored_user_patches_for_atom, fn user_patches_abort_prefix
- Abort-path rows (backlog #509): the failing atom's dropped binaries (local, plus remote under `--getbinpkg`), prepended ahead of the `All binary packages` abort. Only the `--usepkgonly` aborts prepend: with eligible ebuilds (`-k` with a masked ebuild) Portage prints no block and no binary row (oracle probe 2026-10-23: `package.mask` + dropped binary shows only the ebuild row). Suppressed under an explicit `=y|n`; empty when the filter is off.

## struct SelectedPackage
- Minimal `(version, source, slot)` view of a merging entry for `eliminate_ignored`.
- The `verbose` flag for the abort-path tail is threaded `ResolveRequest.verbose -> ResolveCtx.verbose` (no other ctx consumer yet).
