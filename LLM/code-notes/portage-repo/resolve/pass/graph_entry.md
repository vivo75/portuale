# crates/portage-repo/src/resolve/pass/graph_entry.rs
Adds the chosen candidate to the graph, with its autounmask suggestions.

## fn push_graph_entry
- Backlog #218: `resolved_depths` is still root-blind (one depth per slot); same-cp-both-roots depth arbitration is Slice C's dual-node modeling, not this slice's.
- Keyword arm: Portage `_pkg_visibility_check` under an `allow_unstable_keywords` level; `autounmask_unrestricted_atoms` defaults to `"n"`, so always the exact-version `=` form. The gate is per-category, not `keyword_masked_only`: a candidate masked by `~arch` *and* `LICENSE` records *both* changes.
- License arm: `check_if_latest(pkg)` without `check_visibility` selects the `>=` / `>=…:slot` / `=` form. The non-empty `missing` list *is* the "license needs a change" signal.
- Mask arm: `autounmask_unrestricted_atoms` `"n"` means always the exact-version form with no token; gate is `package.mask` matched with nothing unmasking it.
- `interactive`: `pkg.properties` is `PROPERTIES` after Portage USE-conditional evaluation (`_PackageMetadataWrapper`, gated on `"?" in v`); Portage `output.py:833` also requires `pkg.operation == "merge"`.
- Reinstall `myoldbest`: Portage's `vardb.cpv_exists(pkg.cpv)` arm (`output.py:721-731`) with `replace = True`; the `[oldver]` bracket shows on a slot/sub-slot move (e.g. a slot-operator cascade rebuild landing at its tree ebuild's new sub-slot) or a repo change. Same rule the old post-pass slot-op synthesiser applied. The quiet half needs no threading: `--quiet-repo-display` stays an unimplemented option (it exits 2), so the `not quiet_repo_display` disjunct reduces to the repo comparison (owner B5 reopened this cut, backlog #247).
