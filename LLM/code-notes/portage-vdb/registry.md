# crates/portage-vdb/src/registry.rs
The process-wide backend registry: root path to `InstalledDb` backend.

## module
- Design: `docs/vdb_to_db.md` section 6.2.
- Same env-free process-global pattern as `portage_repo::set_resolver_debug`, with a map instead of a flag.
- `mrg` registers its chosen backend before `pretend::run`; `emerge` never registers, so every root falls back to a `FilesDb`.
- Root keys are the path as given, not canonicalised: no extra syscall, exactly like `root.join("var/db/pkg")` today.
