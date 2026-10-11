# crates/portage-fetch/src/layout.rs
Portage's distfiles mirror layouts: the path math and `layout.conf` parsing.

## module
- Ported from `lib/portage/package/ebuild/fetch.py` (`FlatLayout`, `FilenameHashLayout`, `ContentHashLayout`, `MirrorLayoutConfig`).
- The Gentoo mirrors (`distfiles.gentoo.org` and the mirrors Portage has cached in `.mirror-cache.json`) publish only `0=filename-hash BLAKE2B 8` -- the flat `<mirror>/distfiles/<file>` path 404s there, the hashed `<mirror>/distfiles/<2 hex>/<file>` one is served.
- `filename-hash` divergence: portuale hashes with `BLAKE2B`, `BLAKE2S`, `MD5`, `SHA1`, `SHA256`, `SHA512`. Portage also hashes with `RMD160`/`SHA3_256`/`SHA3_512`/`WHIRLPOOL`; a `filename-hash` structure naming one of those is treated as unsupported here, so the next structure entry (ultimately flat) is used instead. (`content-hash` needs no hashing, so it accepts every Portage name.)
- `layout.conf` divergence: parsed by a small `configparser` subset (see `MirrorLayoutConfig::parse`), not the full INI grammar.

## fn cut_path
- Mirrors the `for c in self.cutoffs: ret += fnhash[:c//4] + "/"; fnhash = fnhash[c//4:]` loop. A negative or zero cutoff is accepted by Portage's `verify_args` and sliced as Python would.

## const PORTAGE_CHECKSUM_KEYS
- Portage version pinned: 3.0.82.2.

## fn MirrorLayoutConfig::parse
- Errors Portage's `configparser` raises (which Portage's `async_mirror_url` turns into "use flat, don't cache") are errors here too: an option before any section header, a duplicate section or option, and a non-blank line that is neither a header nor `key=value`.
- The `configparser` subset: blank lines and lines whose first non-blank character is `#` or `;` are skipped; `[name]` opens a section; `key = value` / `key: value` sets an option (key stripped and lower-cased, value stripped); an indented line after an option continues its value.

## fn MirrorLayoutConfig::layout_for
- The content-hash arm mirrors `ContentHashLayout.verify_args` exactly: lower-case names are never valid.

## mod tests
- `filename_hash_paths_match_portage`: expected values were captured from Portage's own output, not derived from reading the code; includes the `BLAKE2B 0` and negative-cutoff Python-slicing cases. Expected paths are Portage's own output (`FilenameHashLayout(algo, cutoffs).get_path(name)`, portage 3.0.82.2), not derived from reading the code.
- `validate_structure_matches_portage`: filename-less form.
