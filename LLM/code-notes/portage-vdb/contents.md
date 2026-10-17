# crates/portage-vdb/src/contents.rs
One shared `CONTENTS` line parser (#357); every backend and caller uses it.

## module
- Portage `_contents_re` (`vartree.py:1951-1958`): `obj (.+) (\S+) (\d+)`, `sym (.+) -> (.+) (\d+)` with the first group greedy, `(dev|dir|fif) (.+)`. A non-matching line is reported and skipped (`:2250-2253`); paths get `normalize_path` when they look unnormalized (`:2278-2282`). Portage writes the path raw (`write_contents`, `:6500-6516`).
- `portage-vdb` is the home because `portage-repo` and `portuale` both depend on it, and `mrg-director` reaches it through `portage-repo` (plan §0.3).
- The old tuple `sym` format is not parsed: the EAPI floor and portuale-written vdbs never have it.
- `bin` is kept although Portage's regex rejects it: portuale-written vdbs may hold it, and dropping those rows would silently un-claim paths.
- md5 is any non-space run (the plan's `(\S+)`); the unit tests pin `abc`-style md5s the conformance corpus also uses. mtime must be all digits (then `i64`; overflow rejects).
- A trailing `\r` is a line ending, never path content: Portage reads the file in text mode (universal newlines).

## fn parse_line
- `obj`: `rsplit_once(' ')` twice from the right (path keeps spaces, including a trailing one); empty path or md5 rejects.
- `sym`: strip ` <digits>`, then `rsplit_once(" -> ")` (the last arrow). A target holding ` -> ` mis-splits exactly like Portage's greedy regex (`sym /p -> x -> y 123` → path `/p -> x`, target `y`; S0 cell S3 pins the line, the unit test pins the split so nobody "fixes" it).
- `dir`/`dev`/`fif`/`bin`: the rest after the first space, verbatim (a leading space survives, as in Portage's `(.+)`); empty rejects.
- Anchored at the line start like the regex: a leading-space line does not parse (the old `split_whitespace` accepted it).

## fn paths
- `(kind, path)` per parsable line, in order; the collectors that were `split_whitespace().nth(1)` (`plib_merge`, `plib_unmerge`, `after_install`, `from_binpkg`, `being_unmerged`) map over this.

## fn migrate_contents_parser (sqlite/write.rs, redb_db/write.rs)
- Q4 default: `meta.contents_parser`, absent-or-below-2 → one `WriteTxn::rebuild_index` in one transaction with the stamp written in the same transaction (a crash leaves the old key, the next open retries). No `SCHEMA_VERSION` bump, so old databases open. Read-only opens never write: they warn once and serve the stale rows (a space path may be missed by `owners` until a writer opens the DB).
- New databases seed the key at creation (`sqlite::create`, redb `seed`), so the migration is a no-op for them. `portuale vdb rebuild-index` needs no change: `open` stamps before it runs.
- Deliberately out of scope (plan §1): the `\n`/`\r`-in-path merge refusal (#380), `_match_contents` inode matching and implicit parent `dir` entries (#381), `remote/merge_flow.sh` (#382).
- Deliberately not migrated (one-line `//` why at each site): config-memory lines (Portage `grabdict`/`writedict` are whitespace-split too, so spaces break there in Portage) and `remote/binpkg_flow.rs` FILEMETA lines (whitespace-free by bundle-build construction).
