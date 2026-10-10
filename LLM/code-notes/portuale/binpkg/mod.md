# crates/portuale/src/binpkg/mod.rs
The `$PKGDIR` directory-scan fallback: per-format binpkg readers and shared helpers.

## module
- Ports Portage's `bintree._populate_local`.
- `read_gpkg_metadata` ports `gpkg.get_metadata()`: a `.gpkg.tar` is a plain tar container; the inner `metadata.tar` is read in process with the `tar` crate. A member is judged from its own header (regular/unknown type yields bytes, symlink/hardlink resolves inside the archive, anything else errors), exactly Portage's `tarfile.extractfile`, so no symlink is followed and no FIFO/device is opened. The outer container and the `image.tar` unpack still shell out to `tar`; the compressors stay subprocesses, the same "shell out to the actual tool" stance every other execution path here takes (`wget`/`ldconfig`/`scanelf`/`bash`/`brush`/compressors); `tar` and the compressors are hard Gentoo requirements anyway.
- `read_xpak_metadata` ports `xpak.tbz2.scan`: the self-describing `XPAKPACK...XPAKSTOP...STOP` trailer appended after the image tarball. Pure Rust, no subprocess, reads only the bounded file tail.
- `populate_local_pkgdir` walks `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}` and synthesizes one `Packages`-style entry per file, fast-pathed against any already-parsed `<pkgdir>/Packages` entry whose `_mtime_`/`SIZE` still agree with the live file (Portage's own mtime-staleness revalidation). Output is recomputed each run, so `--pretend` still writes nothing.

## enum BinpkgError
- `Invalid` is Portage's `(PortagePackageException, SignatureException)` catch: the `$PKGDIR` scan prints Portage's own `!!! Invalid binary package: '<path>', <msg>` and skips the file (`populate_local_pkgdir`).
- `Fatal` is what Portage lets propagate (a `KeyError`/`AttributeError`/`RecursionError` from `tarfile`, an inner `tarfile.ReadError`, an `OSError`); the scan aborts.
- The classification mirrors Portage's exception families site by site; the per-site table is `LLM/07.60-gpkg-populate-read-errors.opus.md` section 4. Every `.xpak` read error is `Fatal`: Portage's `tbz2.get_data()` returns `None` on any scan failure and the caller then raises `AttributeError` on `None.get`.
