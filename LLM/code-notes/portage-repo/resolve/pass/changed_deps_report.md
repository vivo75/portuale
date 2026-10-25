# crates/portage-repo/src/resolve/pass/changed_deps_report.rs
Record one installed version for the changed-deps report.

## fn record_changed_deps_report
- Portage's own `_changed_deps_pkgs` dict is discarded unread by `_changed_deps_report`'s early return. `deps_changed` is a vdb-vs-current-ebuild comparison for one installed version; Portage's `newuse`/`changed_use`/`changed_slot` triggers stay freely combinable.


## gate threading (#571)
- `DepsChangedArgs` and the version-lookup use `ctx.gate()` / `ctx.repo_candidates`.
