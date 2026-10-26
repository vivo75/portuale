# crates/portuale/src/binpkg/digest.rs
Digest checks of a `$PKGDIR` binpkg against the index, and checksum-failure renames.

## fn file_mtime
- Portage `os.lstat(...)[stat.ST_MTIME]` -- whole seconds, the same integer Portage's own `_mtime_` index field records (`bintree.py:2306`'s `d["_mtime_"] = str(st[stat.ST_MTIME])`).

## struct BinpkgDigestMismatch
- A merge-time binpkg size/digest mismatch: Portage `_emerge/BinpkgVerifier._digest_exception`'s own `(name, value, expected)` triple (`BinpkgVerifier.py`), where `name` is `"size"` or a hash name (`"MD5"`, `"SHA1"`, ...), `got` the live file's value and `expected` the `<pkgdir>/Packages` index's claim.
- `name`: `"size"` or the hash name, exactly as Portage reports it.

## fn verify_binpkg_against_index
- Portage `_emerge/BinpkgVerifier` + `bintree._get_digests` (backlog #174, #578): verify a binpkg at merge time against the index record that vouches for it -- shared by the local `$PKGDIR` merge path and the remote download path (`download_and_verify`, which used to check MD5/SHA1 only). The size (`SIZE`, via the shared `parse_index_size`) is checked first via `stat(2)`; then every digest the record carries that survives `PORTAGE_CHECKSUM_FILTER` (`None` disables the filter) is hashed over the file and compared, in ASCII-sorted order (`VERIFIED_DIGESTS`, matching `checksum.verify_all`'s `sorted()`; Portage's own remote path walks frozenset order, so no order is canonical there). A record with no parseable `SIZE` verifies nothing (Portage's own `if "size" not in digests: return OK` in `BinpkgVerifier._start`). Ports Portage's `_emerge/BinpkgVerifier` + `bintree._get_digests`.
- `RMD160`/`WHIRLPOOL` records verify the size alone: no hasher vendored here (`file_hex_digest` returns `None`, skipped). Portage verifies them when its host provides them (hashlib/bundled); foreign index stanzas carrying only those keys are the residual gap (S0: `LLM/evidence/2026-10-25-review/t4/`). Portage's own local `--regen` writes only `MD5`/`SHA1` (Portage's `_pkgindex_hashes`), so the gap only bites on foreign stanzas.
- The local merge path passes the configured `PORTAGE_CHECKSUM_FILTER` (previously unfiltered); Portage applies the filter to local packages too (`BinpkgVerifier._start:41-44` has no remote gate).
- Mismatch order note: `size` first, then the surviving hashes in `VERIFIED_DIGESTS` order; only the first mismatch is returned.

## fn parse_index_size (#578)
- The one `SIZE` parse used by both verifiers and the remote guard: `str.trim()` (Unicode whitespace, like Python `int()`), one optional `+`/`-` sign, then ASCII digits. A negative (or out-of-`u64`-range) value parses in Portage too (`int("-1") == -1`) but matches no file, so it maps to `Some(u64::MAX)` and both paths fail the size check exactly as Portage's `st_size != -1` does. Garbage (`"x"`, `""`, `"++1"`) is `None`: Portage drops `size` from its digests then (local: verify nothing; remote: missing-digests refusal). The underscore quirk (`int("46_18") == 4618`) is observed in S0 and recorded in `LLM/portage-quirks.md`, not copied.

## fn index_size_expected (#578)
- The `Expected:` value for a size mismatch: the parsed number, except a sentinel parse (negative/out-of-range `SIZE`) prints the index's own trimmed text (leading `+` stripped) the way Portage prints its parsed `int` (`-1`, not the sentinel).

## fn apply_checksum_filter
- Ports `portage.checksum._hash_filter` + `_apply_hash_filter`: filters the uppercase hash names against `PORTAGE_CHECKSUM_FILTER`, always preserving at least one hash (backlog #436d). Moved here from `emerge_getbinpkg.rs` (#578) so both verifiers share it. Portage keeps one survivor by frozenset order (nondeterministic 6/6 over 12 processes in S0); this keeps the last in sorted order -- one of Portage's two possible outcomes.

## const VERIFIED_DIGESTS (#578)
- Every digest the verifier hashes: `BLAKE2B BLAKE2S MD5 SHA1 SHA256 SHA3_256 SHA3_512 SHA512`, ASCII-sorted like `verify_all`'s `sorted(verifiable_hash_types)`. `RMD160`/`WHIRLPOOL` omitted (no vendored hasher); `size` is handled separately, never filtered.

## fn file_hex_digest
- The same whole-file readers Portage's `perform_multiple_checksums` runs for a `Packages` entry: `MD5`/`SHA1`/`SHA256`/`SHA512`, plus `BLAKE2B`/`BLAKE2S` (`blake2` crate) and `SHA3_256`/`SHA3_512` (`sha3` crate, #578 -- S0 shows Portage verifies them via hashlib on the reference host). `RMD160`/`WHIRLPOOL` have no vendored hasher (`None`, skipped by the verifier).

## fn bytes_hex_digest
- The in-memory half of `file_hex_digest` (same match, over `&[u8]`; `file_hex_digest` reads the file then calls it): the global-updates index writer hashes its already-read file bytes through it (`MD5`/`SHA1`, the `_pkgindex_hashes` pair) instead of keeping its own hasher pair.

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
