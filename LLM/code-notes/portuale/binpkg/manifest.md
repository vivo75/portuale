# crates/portuale/src/binpkg/manifest.rs
Verifying a gpkg `Manifest` at merge time.

## fn verify_gpkg_manifest
- Ports Portage's `portage.gpkg.gpkg._verify_binpkg` (`lib/portage/gpkg.py:1626`), checksum layer + GPG signature layer. The `DATA` lines come from Portage's `_record_checksum` / `_add_manifest` with `MANIFEST2_HASH_DEFAULTS = {BLAKE2B, SHA512}`.
- The regular-file discipline is the symlink-safety work: the prefix must be an actual directory and every trusted member a regular file (`symlink_metadata`, no follow), since Portage's own reader can never read through one either (see `TEST/findings/l2.md` "## #56 S0").
- A missing `Manifest` member is Portage's `MissingSignature`.
- GPG trigger (`gpkg.py:1682-1686`, `gpkg.py:1711-1712`): "if any signature exists, we assume all files have signature".
- Manifest-signature branch (`gpkg.py:1714-1731`): a failure is fatal only under Portage's own `if self.verify_signature: raise`; with `binpkg-ignore-signature` Portage falls back to the raw bytes.
- Per-file GPG branch (`gpkg.py:1764-1788`): Portage's loop verifies the `.sig` file bytes too; `f_signature` being `None` chooses plain over GPG verification, not skipping.
- Digest floor: Portage's "at least one supported checksum". Member/record set equality: Portage's `unverified_files` / `unverified_manifest` leftover checks.
- Deliberate cuts: no dropped-privilege (`nobody`/`nogroup`) `gpg` spawn when root (Portage `checksum_helper`'s `GPG_VERIFY_USER_DROP`; tests run unprivileged and production merges already run as root throughout).
- Streaming: members hash straight from the container stream (`verify_digests_reader`), so a large image member is a sequential read with no extraction to disk and no buffering.
- A truncated/corrupt member fails Portage's Manifest size+digest check on the metadata member first (`DigestException`, the skip class).
