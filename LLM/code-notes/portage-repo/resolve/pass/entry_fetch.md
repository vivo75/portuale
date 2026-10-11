# crates/portage-repo/src/resolve/pass/entry_fetch.rs
Records the entry's fetch restriction and the bytes left to download.

## fn record_entry_fetch
- Portage `output.py:633-641`: the `f`/`F` fetch-restrict column (`not pkg.built` maps to ebuild candidates only). `resolved.restrict` is the raw `RESTRICT`; `evaluated_metadata_tokens` resolves its USE conditionals the same way as for `interactive`'s `PROPERTIES`. `fetch_restrict_satisfied` is Portage's `not getfetchsizes(only_restricted=True)`.
- Portage `output.py:300-332`: `_calc_size` feeds `counters.totalsize` (the `-v` `Total:` line's `Size of downloads`). Runs for every merge-bound ebuild entry, not just fetch-restricted ones.
- Remote branch: Portage `_calc_size` via `bindbapi.getfetchsizes` returns `{<cpv>: SIZE}` from the binhost `Packages` index for a package not yet downloaded (`bintree.isremote`). Feeds both the `-v` per-line ` N KiB` suffix and the `Size of downloads:` counter.
