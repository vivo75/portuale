# crates/portuale/src/binpkg/mod.rs
The `$PKGDIR` directory-scan fallback: per-format binpkg readers and shared helpers.

## module
- Ports Portage's `bintree._populate_local` (see `LLM/agent-context.md`). Every other binary-package path in this portuale is `<pkgdir>/Packages`-index driven and format-agnostic, so a `gpkg`/`xpak` listed in an index already resolves for `--pretend`. What the index reader can't do is the "no trusted index" branch: when `$PKGDIR` holds binpkg files but no `Packages`, open each file, read its own embedded metadata, and build the pool from that. That needs a real per-format reader -- this module has both.
- `read_gpkg_metadata` ports `gpkg.get_metadata()`: a `.gpkg.tar` is a plain tar container; find/decompress the inner `metadata.tar` and read each `metadata/<KEY>` member. The inner `metadata.tar` is read in process with the `tar` crate (`#58`, K0). A member is judged from its own header (regular/unknown type yields bytes, symlink/hardlink resolves inside the archive, anything else errors), exactly Portage's `tarfile.extractfile`, so no symlink is followed and no FIFO/device is opened. The outer container and the `image.tar` unpack still shell out to `tar`; the compressors stay subprocesses, the same "shell out to the real tool" stance every other real-execution path here takes (`wget`/`ldconfig`/`scanelf`/`bash`/`brush`/the compressors `ebuild_package.rs` already runs); `tar` and the compressors are hard Gentoo requirements anyway.
- `read_xpak_metadata` ports `xpak.tbz2.scan`: the self-describing `XPAKPACK…XPAKSTOP…STOP` trailer appended after the image tarball. Pure Rust, no subprocess, reads only the bounded file tail.
- `populate_local_pkgdir` walks `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}` and synthesizes one `Packages`-style entry per file, fast-pathed against any already-parsed `<pkgdir>/Packages` entry whose `_mtime_`/`SIZE` still agree with the live file (Portage's own mtime-staleness revalidation). Output is recomputed each run, so `--pretend` still writes nothing.

## enum BinpkgError
- The two classes Portage's `bintree._populate_local` caller distinguishes (`bintree.py:1185`).
- `Invalid` is Portage's `(PortagePackageException, SignatureException)` catch: the `$PKGDIR` scan prints Portage's own `!!! Invalid binary package: '<path>', <msg>` and skips the file (`populate_local_pkgdir`).
- `Fatal` is what Portage lets propagate (a `KeyError`/`AttributeError`/`RecursionError` from `tarfile`, an inner `tarfile.ReadError`, an `OSError`); the scan aborts.
- The classification mirrors Portage's exception families site by site; the per-site table is `LLM/07.60-gpkg-populate-read-errors.opus.md` section 4. Every `.xpak` read error is `Fatal`: Portage's `tbz2.get_data()` returns `None` on any scan failure and the caller then raises `AttributeError` on `None.get`.

## fn gpkg_compressions
- The `.tar<ext>` suffixes Portage `gpkg.gpkg.ext_list` (`lib/portage/gpkg.py:821-829`) maps to a compression method, paired with that method's own Portage `_compressors` decompress argv (`lib/portage/util/compression_probe.py:10-53`; `{JOBS}` -> `0` = "all cores", Portage's own substitution).

## fn classify_inner_member
- Portage `gpkg._extract_filename_compression` (`gpkg.py:2176`): given an inner member's basename, return `Some(None)` if it is exactly `<want>.tar`, `Some(Some(decompress_argv))` if it is `<want>.tar<ext>` for a known `ext`, or `None` if it names something else.

## struct ScratchDir
- A best-effort temp directory, removed when the guard drops. The `nanos` suffix keeps concurrent reads (e.g. a `$PKGDIR` scan over many files) from colliding -- same shape `ebuild_phases`/`fetch` already use for their own scratch dirs.
