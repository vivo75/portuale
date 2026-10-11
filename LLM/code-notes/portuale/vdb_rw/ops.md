# crates/portuale/src/vdb_rw/ops.rs
Test harness for `vdb_rw`: replay operation scripts through an `RwView` and a `files` directory.

## module
- Built for backlog #317.
- `stamp` appends `#dir_mtime=<DIR's mtime ns>\n`, mirroring Portage's `_stamp_metadata_file`.
- File modes are set exactly (MODE / 0644), whatever the process umask (#308).
