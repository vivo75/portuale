# crates/portuale/src/binpkg/digest.rs
Digest checks of a `$PKGDIR` binpkg against the index, and checksum-failure renames.

## fn file_mtime
- Whole seconds, the same integer Portage's own `_mtime_` index field records (`bintree.py:2306`'s `d["_mtime_"] = str(st[stat.ST_MTIME])`).

## fn verify_binpkg_against_index
- Ports Portage's `_emerge/BinpkgVerifier` + `bintree._get_digests`.
- Checks `MD5`/`SHA1` (the two Portage `_pkgindex_entry` writes) plus `SHA256`/`SHA512` when present, in that fixed order. A record with no `SIZE` verifies nothing (Portage's own `if "size" not in digests: return OK` in `BinpkgVerifier._start`).
- Fixed digest list, disclosed: Portage's `_get_digests` collects every key in `get_valid_checksum_keys()` (host-dependent; on the reference host: `BLAKE2B/BLAKE2S/MD5/RMD160/SHA1/SHA256/SHA3_256/SHA3_512/SHA512/WHIRLPOOL/size`) and checks them in frozenset order; portuale checks `MD5`/`SHA1`/`SHA256`/`SHA512` in that fixed order and skips the rest (`RMD160`/`WHIRLPOOL`/`BLAKE2B`/`BLAKE2S`/`SHA3_256`/`SHA3_512` need hashers portuale doesn't vendor). Portage's own local `--regen` writes only `MD5`/`SHA1` (Portage's `_pkgindex_hashes`), so the gap only bites on foreign index stanzas carrying the wider keys.

## fn file_hex_digest
- The same whole-file readers Portage's `perform_multiple_checksums` runs for a `Packages` entry.

## fn checksum_failure_rename
- Ports `portage.package.ebuild.fetch._checksum_failure_temp_file` (`fetch.py:293`); a same-size same-md5 duplicate is reused instead (the corrupt file is unlinked, Portage's own `os.unlink(filename)`).

## fn checksum_failure_rand
- Ports Portage's `tempfile.mkstemp("", <basename> + "._checksum_failure_.", dir)` (`fetch.py:692-696`): the trailing random part is 8 characters from `tempfile._RandomNameSequence.characters` (`"abcdefghijklmnopqrstuvwxyz0123456789_"`), `os.urandom`-seeded.

## fn digest_failure_block
- Portage emits it through the scheduler (stdout + package log); portuale prints it to stdout.
