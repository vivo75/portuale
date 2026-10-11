# crates/portuale/src/vdb_view.rs
The read-only filesystem view of an installed-package database for `vdb mount`.

## module
- Origin: feat#157 / backlog #305, plan S7.2 and S7.3; design `docs/vdb_to_db.md` §12.
- Attributes ground in the S0 finding on Portage `_bump_mtime` (`vartree.py:578-583`): it touches the category directory and the VDB root, not the entry directory, so a category shows the latest of its entries and the root the latest category.
- The `metadata` Valid rewrite mirrors Portage `_read_metadata_file`, which accepts the file iff `#format=1` and `#dir_mtime=` equals the directory's `st_mtime_ns` (`vartree.py:176-185`).
- Stale stays stale (S0.3: a converter never repairs a stamp).

## enum ViewError::Perm
- A write the read-write layer (`vdb_rw`, #317) does not map.

## enum Node::Obj
- An object of the read-write layer (`vdb_rw`, #317): a volatile file, a staged directory or file. This view never serves it.

## enum Handle::File::generation
- A read no longer compares it (#316); only the tests ask.

## fn attr_of (the `Node::File` arm)
- `metadata` is served from its own rewritten bytes (#316). For ordinary field files the cached stat is re-read because on `files` an in-place `replace_file` moves neither the directory mtimes nor the generation; a database commit moves the generation instead, and this is then the same value.

## fn read (the `Pinned::Stat` arm)
- Checked on every read, not only once the generation has moved on (#316): on `files` an in-place rewrite does not move the generation. One stat (a query on a database) per read.

## tests: portage_reader_accepts (in `mod tests`)
- Portage `_read_metadata_file`'s acceptance rule (`vartree.py:157-185`): `#format=1` present, `#dir_mtime=<int>` present, equal to the directory's `st_mtime_ns`.

## tests: an_in_place_rewrite_on_files_is_seen_by_getattr_and_by_an_open_big_handle
- #316: on `files` an in-place `replace_file` moves no directory mtime, so the generation stays; the view must still report the new size of a rewritten file, and a big file's open handle must go stale instead of mixing the old and the new bytes.
