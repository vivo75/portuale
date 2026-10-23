# crates/portuale/src/binpkg/digest.rs
Digest checks of a `$PKGDIR` binpkg against the index, and checksum-failure renames.

## fn file_mtime
- Portage `os.lstat(...)[stat.ST_MTIME]` -- whole seconds, the same integer Portage's own `_mtime_` index field records (`bintree.py:2306`'s `d["_mtime_"] = str(st[stat.ST_MTIME])`).

## struct BinpkgDigestMismatch
- A merge-time binpkg size/digest mismatch: Portage `_emerge/BinpkgVerifier._digest_exception`'s own `(name, value, expected)` triple (`BinpkgVerifier.py`), where `name` is `"size"` or a hash name (`"MD5"`, `"SHA1"`, ...), `got` the live file's value and `expected` the `<pkgdir>/Packages` index's claim.
- `name`: `"size"` or the hash name, exactly as Portage reports it.

## fn verify_binpkg_against_index
- Portage `_emerge/BinpkgVerifier` + `bintree._get_digests` (backlog #174): verify a local `$PKGDIR` binpkg at merge time against the `<pkgdir>/Packages` index record that vouches for it. The size (`SIZE`) is checked first via `stat(2)`; then every digest the record carries (`MD5`/`SHA1`, the two Portage `_pkgindex_entry` writes, plus `SHA256`/`SHA512` when present) is hashed over the file and compared.
- Ports Portage's `_emerge/BinpkgVerifier` + `bintree._get_digests`.
- Checks `MD5`/`SHA1` (the two Portage `_pkgindex_entry` writes) plus `SHA256`/`SHA512` when present, in that fixed order. A record with no `SIZE` verifies nothing (Portage's own `if "size" not in digests: return OK` in `BinpkgVerifier._start`).
- Fixed digest list, disclosed: Portage's `_get_digests` collects every key in `get_valid_checksum_keys()` (host-dependent; on the reference host: `BLAKE2B/BLAKE2S/MD5/RMD160/SHA1/SHA256/SHA3_256/SHA3_512/SHA512/WHIRLPOOL/size`) and checks them in frozenset order; portuale checks `MD5`/`SHA1`/`SHA256`/`SHA512` in that fixed order and skips the rest (`RMD160`/`WHIRLPOOL`/`BLAKE2B`/`BLAKE2S`/`SHA3_256`/`SHA3_512` need hashers portuale doesn't vendor). Portage's own local `--regen` writes only `MD5`/`SHA1` (Portage's `_pkgindex_hashes`), so the gap only bites on foreign index stanzas carrying the wider keys.

## fn file_hex_digest
- The same whole-file readers Portage's `perform_multiple_checksums` runs for a `Packages` entry.

## fn checksum_failure_rename
- Ports `portage.package.ebuild.fetch._checksum_failure_temp_file` (`fetch.py:293`); a same-size same-md5 duplicate is reused instead (the corrupt file is unlinked, Portage's own `os.unlink(filename)`).
- Match the `mkstemp` name shape exactly (`<rand>` is `[a-z0-9_]{8}`); the loop retries on the (absurd) collision instead of clobbering.

## fn checksum_failure_rand
- Ports Portage's `tempfile.mkstemp("", <basename> + "._checksum_failure_.", dir)` (`fetch.py:692-696`): the trailing random part is 8 characters from `tempfile._RandomNameSequence.characters` (`"abcdefghijklmnopqrstuvwxyz0123456789_"`), `os.urandom`-seeded.
- Portage `tempfile._RandomNameSequence`'s own 8-character name (`tempfile.py`: 8 `choices` over `"abcdefghijklmnopqrstuvwxyz0123456789_"`): 8 bytes from the OS mixed onto the same 37-character alphabet. The `attempt` salts the deterministic fallback (no OS entropy available) so retries still vary; the caller re-checks `exists()` either way.

## fn digest_failure_block
- Portage `_emerge/BinpkgVerifier._digest_exception`'s own failure block (`BinpkgVerifier.py`), byte-shaped: a leading blank line, three `!!!` lines (the path, the `Failed on <name> verification` reason, `Got`/`Expected`), then the bare `File renamed to '<path>'` line (no `!!!` prefix, Portage's own `f"File renamed to ..."`).
- Portage emits it through the scheduler (stdout + package log); portuale prints it to stdout.
- `renamed` is the `checksum_failure_rename` result -- when the rename itself failed the caller passes the original path (the file is still there), so the line always names a real location.

## fn index_has_size_and_checksum
- Ports the guard at the top of `BinpkgVerifier._start`: `self.pkg.remote and not ("size" in digests and digests.keys() - {"size"})`. `digests` is `bintree._get_digests`: every `get_valid_checksum_keys()` key with a non-empty value, plus `size` only when `int(SIZE)` parses. So SIZE alone, a checksum alone and neither all fail with the same text (verified on 3.0.83-r1, `LLM/evidence/2026-10-23-500-501/txt/s2-hb-*`); the guard runs before `PORTAGE_CHECKSUM_FILTER` is applied. Only remote records go through it: a local `$PKGDIR` record without `SIZE` still "verifies nothing" (`verify_binpkg_against_index`), as in Portage. Backlog #501.

## fn missing_digests_block
- `BinpkgVerifier._missing_digests_exception` text, byte for byte; unlike `_digest_exception` it renames nothing and has no `Got`/`Expected`/`File renamed` lines.
