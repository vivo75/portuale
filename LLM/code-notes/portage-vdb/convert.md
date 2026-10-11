# crates/portage-vdb/src/convert.rs
Generic copy and comparison between two installed databases.

## module
- Plan S2.7, `docs/vdb_to_db.md` section 11. Design section 11 sketched converting back into a temporary `files` backend for comparison; `verify` instead compares directly because a second conversion's own bugs could mask the first one's.
- One SQLite transaction for a ~2,000-entry VDB (a few MB of rows, one WAL growth) is cheap, which is why one transaction for the whole copy is affordable.

## fn copy_all
- The skip-if-unchanged rule compares `entries` with `loaded`, so `libs.loaded` is set to the destination's own current registry, not the source's.
- Each `COUNTER` file travels inside its entry image; the counter itself is set last and never renumbered.
- Record import mark when copying from a files backend to a non-files backend (source generation + root path), used to detect stale conversions.
- A source entry that vanished between listing and reading (live source) is skipped.

## fn compare_images
- Permission bits only (`DIR_MODE_MASK`): a database converted before #307 holds the full `st_mode` (`0o40755`) a `files` reader used to report.
