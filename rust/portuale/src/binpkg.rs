// The `$PKGDIR` directory-scan fallback -- real `bintree._populate_local`
// (see `docs/agent-context.md`). Every other binary-package path in this
// portuale is `<pkgdir>/Packages`-index driven and format-agnostic, so a
// `gpkg`/`xpak` *listed in an index* already resolves for `--pretend`.
// What the index reader can't do is the "no trusted index" branch: when
// `$PKGDIR` holds binpkg *files* but no `Packages`, open each file, read
// its own embedded metadata, and build the pool from that. That needs a
// real per-format reader -- this module has both:
//
//   - `read_gpkg_metadata`: real `gpkg.get_metadata()` -- a `.gpkg.tar`
//     is a plain tar container; find/decompress the inner `metadata.tar`.
//     Shells out to `tar` + the matching decompressor rather than parsing
//     natively or adding a Rust tar/compression crate -- consistent with
//     every other real-execution path here (`wget`/`ldconfig`/`scanelf`/
//     `bash`/`brush`/the compressors `ebuild_package.rs` already runs),
//     and `tar` + these compressors are hard Gentoo requirements anyway.
//   - `read_xpak_metadata`: real `xpak.tbz2.scan` -- the self-describing
//     `XPAKPACK…XPAKSTOP…STOP` trailer appended after the image tarball.
//     Pure Rust, no subprocess, reads only the bounded file tail.
//   - `populate_local_pkgdir`: walks `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}`
//     and synthesizes one `Packages`-style entry per file, fast-pathed
//     against any already-parsed `<pkgdir>/Packages` entry whose own
//     `_mtime_`/`SIZE` still agree with the live file (real's own
//     mtime-staleness revalidation -- see its own doc comment). Its
//     output becomes `portage_profile::Config::scanned_binpkgs` (NOT
//     written back to `Packages` -- portuale recomputes each run, so
//     `--pretend` still writes nothing).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `.tar<ext>` suffixes real `gpkg.gpkg.ext_list`
/// (`lib/portage/gpkg.py:821-829`) maps to a compression method, paired
/// with that method's own real `_compressors` decompress argv
/// (`lib/portage/util/compression_probe.py:10-53`; `{JOBS}` -> `0` =
/// "all cores", real's own substitution).
fn gpkg_compressions() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        (".gz", &["gzip", "-dc"]),
        (".bz2", &["bzip2", "-dc"]),
        (".lz4", &["lz4", "-dc"]),
        (".lz", &["lzip", "-dc"]),
        (".lzo", &["lzop", "-dc"]),
        (".xz", &["xz", "-T0", "-dc"]),
        (".zst", &["zstd", "-dc", "--long=31"]),
    ]
}

/// Real `gpkg._extract_filename_compression` (`gpkg.py:2176`): given an
/// inner member's basename, return `Some(None)` if it is exactly
/// `<want>.tar`, `Some(Some(decompress_argv))` if it is
/// `<want>.tar<ext>` for a known `ext`, or `None` if it names something
/// else.
fn classify_inner_member(
    want: &str,
    member_basename: &str,
) -> Option<Option<&'static [&'static str]>> {
    let plain = format!("{want}.tar");
    if member_basename == plain {
        return Some(None);
    }
    for (ext, argv) in gpkg_compressions() {
        if member_basename == format!("{plain}{ext}") {
            return Some(Some(*argv));
        }
    }
    None
}

/// A best-effort temp directory, removed when the guard drops. The
/// `nanos` suffix keeps concurrent reads (e.g. a `$PKGDIR` scan over
/// many files) from colliding -- same shape `ebuild_phases`/`fetch`
/// already use for their own scratch dirs.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(tag: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!(
            "portuale-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self(dir))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Real `portage.gpkg.gpkg.get_metadata()` / `unpack_metadata(want=None)`
/// (`lib/portage/gpkg.py:838-870`), narrowed to the local metadata read:
/// a `.gpkg.tar` is a plain (uncompressed) tar whose members are
/// `<basename>/{gpkg-1, metadata.tar[.<comp>], image.tar[.<comp>],
/// Manifest}` (+ optional `.sig` files). Returns the `metadata/<KEY>` ->
/// value map -- real `_strip_metadata_prefix` over the inner
/// `metadata.tar`'s own members -- each value UTF-8 with surrounding
/// whitespace trimmed (the same shape a vdb aux file / `read_md5_cache`
/// entry already has). A member whose bytes aren't valid UTF-8
/// (`environment.bz2`) is skipped, not an error.
///
/// This reader is the "populate the pool" path (real
/// `bintree._populate_local` / `get_metadata`); it trusts the container
/// the same way the `Packages`-index reader "trusts the index outright"
/// (real `FEATURES=pkgdir-index-trusted`). The *merge* path
/// (`extract_binpkg`) runs the real `Manifest` digest check first --
/// see [`verify_gpkg_manifest`]. Still required here: the `gpkg-1`
/// version marker's *presence* (real `_get_inner_tarinfo`'s own
/// `InvalidBinaryPackageFormat` guard).
///
/// **Deliberate cut**: NO GPG `.sig` check on this populate path --
/// a container that carries `.sig` members still has its cleartext
/// `DATA` digests verified at *merge* time (real portage's own
/// `binpkg-ignore-signature` behaviour is what an unverified read
/// amounts to), and the merge (`extract_binpkg` ->
/// `verify_gpkg_manifest`) enforces the real `request_signature` /
/// `verify_signature` policy via the system `gpg` (see [`GpgVerify`]).
/// Real `unpack_metadata`/`get_metadata` verify
/// (`_verify_binpkg(metadata_only=True)` still checks the metadata
/// `.sig` + Manifest), so a corrupt-or-foreign-signed binpkg resolves
/// in portuale's pool but fails at merge; the resolve side stays
/// deterministic and `gpg`-free on purpose.
pub fn read_gpkg_metadata(gpkg_path: &Path) -> Result<HashMap<String, String>, String> {
    if !gpkg_path.is_file() {
        return Err(format!("{}: not a file", gpkg_path.display()));
    }
    let scratch = ScratchDir::new("gpkg")?;
    let outer = scratch.path().join("outer");
    fs::create_dir_all(&outer).map_err(|e| format!("{}: {e}", outer.display()))?;

    // 1. Unpack the outer container (plain tar).
    run_tar(&["-xf", &lossy(gpkg_path), "-C", &lossy(&outer)])?;

    // 2. Locate `<basename>/gpkg-1` (real validity guard) and the
    //    `metadata.tar[.<comp>]` member.
    let mut gpkg_marker = false;
    let mut metadata_member: Option<(PathBuf, Option<&'static [&'static str]>)> = None;
    for basename_dir in read_dir_sorted(&outer)? {
        if !basename_dir.is_dir() {
            if basename_dir.file_name().and_then(|n| n.to_str()) == Some("gpkg-1") {
                gpkg_marker = true;
            }
            continue;
        }
        for member in read_dir_sorted(&basename_dir)? {
            let Some(name) = member.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == "gpkg-1" {
                gpkg_marker = true;
            }
            if let Some(comp) = classify_inner_member("metadata", name) {
                metadata_member.get_or_insert((member.clone(), comp));
            }
        }
    }
    if !gpkg_marker {
        return Err(format!(
            "{}: not a gpkg container (no `gpkg-1` version marker)",
            gpkg_path.display()
        ));
    }
    let (metadata_member, comp) = metadata_member.ok_or_else(|| {
        format!(
            "{}: no `metadata.tar` member in the gpkg",
            gpkg_path.display()
        )
    })?;

    // 3. Reduce the inner member to a plain `metadata.tar`.
    let inner_tar = scratch.path().join("metadata.tar");
    match comp {
        None => {
            fs::copy(&metadata_member, &inner_tar)
                .map_err(|e| format!("{}: {e}", metadata_member.display()))?;
        }
        Some(argv) => {
            let out = fs::File::create(&inner_tar)
                .map_err(|e| format!("{}: {e}", inner_tar.display()))?;
            let status = Command::new(argv[0])
                .args(&argv[1..])
                .arg(&metadata_member)
                .stdout(out)
                .status()
                .map_err(|e| format!("failed to spawn {}: {e}", argv[0]))?;
            if !status.success() {
                return Err(format!(
                    "{} failed to decompress {} ({status})",
                    argv[0],
                    metadata_member.display()
                ));
            }
        }
    }

    // 4. Unpack the inner `metadata.tar` (members are `metadata/<KEY>`).
    let md = scratch.path().join("md");
    fs::create_dir_all(&md).map_err(|e| format!("{}: {e}", md.display()))?;
    run_tar(&["-xf", &lossy(&inner_tar), "-C", &lossy(&md)])?;

    // 5. Read every `metadata/<KEY>` scalar file.
    let metadata_dir = md.join("metadata");
    let mut out = HashMap::new();
    for f in read_dir_sorted(&metadata_dir)? {
        if !f.is_file() {
            continue;
        }
        let Some(key) = f.file_name().and_then(|n| n.to_str()).map(String::from) else {
            continue;
        };
        let Ok(bytes) = fs::read(&f) else { continue };
        let Ok(text) = String::from_utf8(bytes) else {
            continue; // e.g. environment.bz2 -- not a scalar value
        };
        out.insert(key, text.trim().to_string());
    }
    Ok(out)
}

/// Real `portage.xpak`'s own `.tbz2` reader (`tbz2.scan` +
/// `getindex_mem`/`searchindex`, `lib/portage/xpak.py:395-460` / `234-266`).
/// An xpak binary package is `[image tarball]` immediately followed by a
/// self-describing XPAK trailer:
///
/// ```text
///   "XPAKPACK"  be32(indexsize)  be32(datasize)  <index>  <data>  "XPAKSTOP"  be32(infosize)  "STOP"
/// ```
///
/// where `infosize` is the length of the `XPAKPACK`…`XPAKSTOP` segment
/// and `<index>` is a flat run of `be32(namelen) name be32(datapos)
/// be32(datalen)` records into `<data>`. Every metadata key (`DEPEND`,
/// `SLOT`, …) is one record. Returns the key -> value map, each value
/// UTF-8 (lossy-decoded, then trimmed -- values carry a trailing newline
/// like a vdb aux file). `CONTENTS` is never present in a *binary*
/// package's own xpak (real `xpak()` skips it -- it's generated at merge
/// time).
///
/// Only the bounded `infosize + 8` tail of the file is read; the image
/// tarball itself is never touched (this reader answers "what metadata
/// does this binpkg carry", the same narrow question `read_gpkg_metadata`
/// does for gpkg). Codec-agnostic: the trailer is raw, whatever
/// compressor produced the tarball.
pub fn read_xpak_metadata(binpkg_path: &Path) -> Result<HashMap<String, String>, String> {
    let seg = read_xpak_segment(binpkg_path)?;
    Ok(parse_xpak_members(&seg)?
        .into_iter()
        .map(|(key, bytes)| (key, String::from_utf8_lossy(bytes).trim().to_string()))
        .collect())
}

/// The raw bytes of one xpak-segment member (real `tbz2.getfile(name)`),
/// or `None` when the binpkg doesn't carry it. Used for the two
/// non-scalar members `read_xpak_metadata` can only return lossily: the
/// saved `environment.bz2` (needed verbatim so it can be `bunzip2`'d
/// into `${T}/environment` for a real `pkg_preinst`/`pkg_postinst`) and
/// the `<pf>.ebuild` source. Reads only the bounded `infosize + 8` tail.
fn read_xpak_member_raw(binpkg_path: &Path, want: &str) -> Result<Option<Vec<u8>>, String> {
    let seg = read_xpak_segment(binpkg_path)?;
    Ok(parse_xpak_members(&seg)?
        .into_iter()
        .find(|(key, _)| key == want)
        .map(|(_, bytes)| bytes.to_vec()))
}

/// The `"XPAKPACK" … "XPAKSTOP"` segment bytes (real `tbz2.scan`): read
/// the last 16 bytes (`"XPAKSTOP" be32(infosize) "STOP"`), then the
/// `infosize + 8` byte segment they point back to. Only this bounded
/// tail of the file is ever touched.
fn read_xpak_segment(binpkg_path: &Path) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f =
        fs::File::open(binpkg_path).map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    let file_len = f
        .seek(SeekFrom::End(0))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if file_len < 16 {
        return Err(format!(
            "{}: too small to be an xpak binpkg",
            binpkg_path.display()
        ));
    }

    let mut trailer = [0u8; 16];
    f.seek(SeekFrom::End(-16))
        .and_then(|_| f.read_exact(&mut trailer))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if &trailer[12..16] != b"STOP" || &trailer[0..8] != b"XPAKSTOP" {
        return Err(format!(
            "{}: not an xpak binary package (no XPAKSTOP trailer)",
            binpkg_path.display()
        ));
    }
    let infosize = be32(&trailer[8..12]) as u64;
    let xpaksize = infosize + 8;
    if xpaksize > file_len {
        return Err(format!(
            "{}: xpak trailer size exceeds the file",
            binpkg_path.display()
        ));
    }

    let mut seg = vec![0u8; xpaksize as usize];
    f.seek(SeekFrom::End(-(xpaksize as i64)))
        .and_then(|_| f.read_exact(&mut seg))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    Ok(seg)
}

/// Walk an xpak segment's index (real `getindex_mem`/`searchindex`:
/// `"XPAKPACK" be32(indexsize) be32(datasize) <index> <data>`, then
/// `while startpos + 8 < len` over `be32(namelen) name be32(datapos)
/// be32(datalen)` records into `<data>`). Returns every member as
/// `(name, &data bytes)`, borrowing from `seg`.
fn parse_xpak_members(seg: &[u8]) -> Result<Vec<(String, &[u8])>, String> {
    if seg.len() < 16 || &seg[0..8] != b"XPAKPACK" {
        return Err("not an xpak binary package (no XPAKPACK header)".to_string());
    }
    let indexsize = be32(&seg[8..12]) as usize;
    let datasize = be32(&seg[12..16]) as usize;
    let index_start = 16;
    let data_start = index_start + indexsize;
    if data_start + datasize > seg.len() {
        return Err("xpak index/data segments overrun the file".to_string());
    }
    let index = &seg[index_start..data_start];
    let data = &seg[data_start..data_start + datasize];

    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 8 < index.len() {
        let namelen = be32(&index[pos..pos + 4]) as usize;
        if pos + 4 + namelen + 8 > index.len() {
            break;
        }
        let name = &index[pos + 4..pos + 4 + namelen];
        let datapos = be32(&index[pos + 4 + namelen..pos + 8 + namelen]) as usize;
        let datalen = be32(&index[pos + 8 + namelen..pos + 12 + namelen]) as usize;
        if let (Ok(key), true) = (std::str::from_utf8(name), datapos + datalen <= data.len()) {
            out.push((key.to_string(), &data[datapos..datapos + datalen]));
        }
        pos += namelen + 12;
    }
    Ok(out)
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Real `portage.gpkg.gpkg._verify_binpkg` (`lib/portage/gpkg.py:1626`),
/// checksum layer + GPG signature layer. A `.gpkg.tar` is a plain
/// (outer) tar whose members are every one exactly one level deep under
/// a single shared prefix directory (real "gpkg file structure" guard);
/// the `<prefix>/Manifest` member records one
/// `DATA <basename> <size> BLAKE2B <hex> SHA512 <hex>` line per other
/// member (real `_record_checksum` / `_add_manifest`, and
/// `MANIFEST2_HASH_DEFAULTS = {BLAKE2B, SHA512}`). This checks:
///   - a `Manifest` member exists (real `MissingSignature` otherwise);
///   - the GPG layer (real `request_signature` / `signature_exist` /
///     `verify_signature`): when the container carries any `.sig`
///     member or an inline-signed Manifest -- or `gpg.request_signature`
///     (`FEATURES=binpkg-request-signature`) says signatures are
///     mandatory -- the Manifest is verified as a clear-signed message
///     and every other member against its detached `.sig` sidecar, via
///     the system `gpg` (see [`GpgVerify`]). A failed Manifest check is
///     fatal only when `gpg.verify_signature` (real's own
///     `if self.verify_signature: raise` -- `binpkg-ignore-signature`
///     falls back to the raw Manifest bytes); a missing sidecar is
///     always fatal under `request_signature` (real `MissingSignature`),
///     except for the never-signed `gpkg-1` version marker. `.sig`
///     members themselves are digest-checked against their own Manifest
///     records, exactly like every other member (real's loop verifies
///     the `.sig` file bytes too -- `f_signature` is only `None` for
///     choosing *plain* over *GPG* verification, not for skipping).
///   - every member has a `DATA` record whose `size` and *every*
///     recognised hash match -- reusing `portage_fetch::verify_digests`
///     (size first, then BLAKE2B/SHA512), with real's "at least one
///     supported checksum" floor;
///   - the member set and the record set match exactly (real's
///     `unverified_files` / `unverified_manifest` leftovers checks).
///
/// Deliberate cuts: no dropped-privilege (`nobody`/`nogroup`) `gpg`
/// spawn when root (real `checksum_helper`'s own `GPG_VERIFY_USER_DROP`;
/// tests run unprivileged and production merges already run as root
/// throughout); `[PORTAGE_CONFIG]`/`[SIGNATURE]` substitution is plain
/// whitespace splitting, not real's `shlex` + `varexpand`.
fn verify_gpkg_manifest(gpkg_path: &Path, gpg: &GpgVerify) -> Result<(), String> {
    if !gpkg_path.is_file() {
        return Err(format!("{}: not a file", gpkg_path.display()));
    }
    let scratch = ScratchDir::new("gpkg-verify")?;
    let outer = scratch.path().join("outer");
    fs::create_dir_all(&outer).map_err(|e| format!("{}: {e}", outer.display()))?;
    run_tar(&["-xf", &lossy(gpkg_path), "-C", &lossy(&outer)])?;

    // The single `<prefix>/` directory: real portage rejects a member
    // that is not exactly one level deep, or a container whose members
    // do not share one common prefix.
    let mut prefix_dir: Option<PathBuf> = None;
    for entry in read_dir_sorted(&outer)? {
        if entry.is_dir() {
            if prefix_dir.is_some() {
                return Err(format!(
                    "{}: gpkg container has more than one top-level directory",
                    gpkg_path.display()
                ));
            }
            prefix_dir = Some(entry);
        } else {
            return Err(format!(
                "{}: gpkg container member {:?} is not inside a directory",
                gpkg_path.display(),
                entry.file_name().unwrap_or_default()
            ));
        }
    }
    let prefix_dir =
        prefix_dir.ok_or_else(|| format!("{}: empty gpkg container", gpkg_path.display()))?;

    let manifest_path = prefix_dir.join("Manifest");
    if !manifest_path.is_file() {
        return Err(format!(
            "{}: Manifest not found in the gpkg container",
            gpkg_path.display()
        ));
    }
    let manifest_bytes =
        fs::read(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest_text = String::from_utf8_lossy(&manifest_bytes);

    // Real `_verify_binpkg`'s own GPG trigger (`gpkg.py:1682-1686` +
    // `:1711-1712`): "if any signature exists, we assume all files have
    // signature" -- any `.sig` sidecar member, or an inline PGP block in
    // the Manifest itself.
    let member_names: Vec<String> = read_dir_sorted(&prefix_dir)?
        .iter()
        .filter_map(|m| m.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect();
    let signature_exist = member_names.iter().any(|n| n.ends_with(".sig"))
        || manifest_text.contains("-----BEGIN PGP SIGNATURE-----");

    // Real Manifest-signature branch (`gpkg.py:1714-1731`): verify the
    // clear-signed Manifest and parse the cleartext. A failure is fatal
    // only under `verify_signature` -- with `binpkg-ignore-signature`
    // real falls back to the raw bytes (and the armor-skipping parse
    // below reads the cleartext body straight through, as portuale
    // always used to).
    let manifest_text: std::borrow::Cow<'_, str> = if gpg.request_signature || signature_exist {
        match verify_clearsigned_manifest(&manifest_bytes, gpg, gpkg_path) {
            Ok(cleartext) => {
                std::borrow::Cow::Owned(String::from_utf8_lossy(&cleartext).into_owned())
            }
            Err(e) => {
                if gpg.verify_signature {
                    return Err(e);
                }
                std::borrow::Cow::Borrowed(manifest_text.as_ref())
            }
        }
    } else {
        std::borrow::Cow::Borrowed(manifest_text.as_ref())
    };

    // Parse the `DATA` lines. When signature checking is off
    // (`binpkg-ignore-signature`) a clear-signed Manifest is still
    // parsed raw, so PGP-armor lines are skipped and the cleartext body
    // read straight through; a verified cleartext has no armor left.
    let mut records: HashMap<String, portage_fetch::DistfileDigests> = HashMap::new();
    for line in manifest_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("-----") || line.starts_with("Hash:") {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("DATA") => {}
            // A PGP-armored Manifest wraps `DATA` lines in a signed
            // block; anything else on a non-blank line is malformed.
            Some(_) if manifest_text.contains("BEGIN PGP") => continue,
            _ => {
                return Err(format!(
                    "{}: invalid Manifest line {line:?}",
                    gpkg_path.display()
                ));
            }
        }
        let name = parts
            .next()
            .ok_or_else(|| format!("{}: Manifest DATA line missing a name", gpkg_path.display()))?;
        let size = parts
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| {
                format!(
                    "{}: Manifest DATA line for {name:?} has no valid size",
                    gpkg_path.display()
                )
            })?;
        let rest: Vec<&str> = parts.collect();
        let mut hashes = HashMap::new();
        let mut i = 0;
        while i + 1 < rest.len() {
            hashes.insert(rest[i].to_string(), rest[i + 1].to_string());
            i += 2;
        }
        if records
            .insert(
                name.to_string(),
                portage_fetch::DistfileDigests { size, hashes },
            )
            .is_some()
        {
            return Err(format!(
                "{}: Manifest lists {name:?} more than once",
                gpkg_path.display()
            ));
        }
    }

    let mut unmatched: std::collections::BTreeSet<String> = records.keys().cloned().collect();
    for name in &member_names {
        let member = prefix_dir.join(name);
        if name == "Manifest" {
            continue;
        }
        let record = records.get(name).ok_or_else(|| {
            format!(
                "{}: container member {name:?} is not listed in the Manifest",
                gpkg_path.display()
            )
        })?;
        if !record
            .hashes
            .keys()
            .any(|h| h == "BLAKE2B" || h == "SHA512")
        {
            return Err(format!(
                "{}: Manifest record for {name:?} carries no supported checksum",
                gpkg_path.display()
            ));
        }
        if !name.ends_with(".sig")
            && (gpg.request_signature || signature_exist)
            && gpg.verify_signature
        {
            // Real per-file GPG branch (`gpkg.py:1764-1788`): a member
            // whose `.sig` sidecar exists is verified detached before
            // its digests are compared; the never-signed `gpkg-1`
            // version marker is digest-only; anything else is
            // `MissingSignature`.
            let sidecar = format!("{name}.sig");
            if member_names.iter().any(|n| n == &sidecar) {
                let member_bytes =
                    fs::read(&member).map_err(|e| format!("{}: {e}", member.display()))?;
                let sig_bytes = fs::read(prefix_dir.join(&sidecar))
                    .map_err(|e| format!("{}: {e}", prefix_dir.join(&sidecar).display()))?;
                verify_detached_signature(
                    &member_bytes,
                    &sig_bytes,
                    gpg,
                    &format!("{}: container member {name:?}", gpkg_path.display()),
                )?;
            } else if name != "gpkg-1" {
                return Err(format!(
                    "{}: container member {name:?} signature not found in the gpkg container",
                    gpkg_path.display()
                ));
            }
        }
        portage_fetch::verify_digests(&member, record).map_err(|e| {
            format!(
                "{}: gpkg Manifest verification failed: {e}",
                gpkg_path.display()
            )
        })?;
        unmatched.remove(name);
    }

    if !unmatched.is_empty() {
        return Err(format!(
            "{}: Manifest lists files not present in the container: {}",
            gpkg_path.display(),
            unmatched.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

/// Real binary-package unpack (`portage.xpak.tbz2.decompose` /
/// `portage.gpkg.gpkg.decompress` + `_generate_metadata_from_dir` in
/// reverse): write a binpkg's *image* -- the built filesystem tree --
/// into `image_dest`, and its scalar metadata (one `<KEY>` file each,
/// real `build-info` shape) into `build_info_dest`.
///
/// xpak (`.tbz2`): `[image tarball][XPAK trailer]`; the image is the
/// leading `file_len - (infosize + 8)` bytes -- a compressed tar whose
/// codec `tar` auto-detects. gpkg (`.gpkg.tar`): the outer tar's
/// `<basename>/image.tar[.<comp>]` member.
///
/// The scalar metadata is `read_{xpak,gpkg}_metadata`'s own map; the two
/// non-scalar members (`environment.bz2` -- the package's saved build-
/// time bash environment -- and `<pf>.ebuild`) are written back
/// **verbatim** as raw bytes, not through the lossy scalar path. Real
/// portage keeps both in the vdb, and portuale now needs them: the
/// binpkg merge runs real `pkg_preinst`/`pkg_postinst` by `bunzip2`'ing
/// `environment.bz2` into `${T}/environment` (real `BinpkgEnvExtractor`
/// -> `bin/ebuild.sh`'s own saved-env source path).
///
/// `gpg` is the merge-time signature policy (see [`GpgVerify`]) -- real
/// `_verify_binpkg` runs before anything is unpacked for the merge.
pub fn extract_binpkg(
    binpkg_path: &Path,
    image_dest: &Path,
    build_info_dest: &Path,
    gpg: &GpgVerify,
) -> Result<(), String> {
    fs::create_dir_all(image_dest).map_err(|e| format!("{}: {e}", image_dest.display()))?;
    fs::create_dir_all(build_info_dest)
        .map_err(|e| format!("{}: {e}", build_info_dest.display()))?;

    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.ends_with(".gpkg.tar") {
        // Real `_verify_binpkg`: the Manifest digest + signature checks
        // run before anything is unpacked for the merge.
        verify_gpkg_manifest(binpkg_path, gpg)?;
        extract_gpkg_member(binpkg_path, "image", image_dest)?;
        // Real `bintree.dbapi.unpack_metadata` -> `gpkg().unpack_metadata`:
        // every `metadata/` member extracted **verbatim** into build-info.
        // Not a scalar parse + reserialize -- that lost `environment.bz2`
        // entirely (`read_gpkg_metadata` skips a non-UTF-8 member) and
        // turned every empty field (`DEBUGBUILD`) into a stray `"\n"`.
        extract_gpkg_member(binpkg_path, "metadata", build_info_dest)?;
        return Ok(());
    }

    // xpak (`.tbz2`/`.xpak`): the image tarball prefix, then the XPAK
    // trailer's scalar segments + the two raw members.
    extract_xpak_image(binpkg_path, image_dest)?;
    let metadata = read_xpak_metadata(binpkg_path)?;
    for (key, value) in &metadata {
        if key == "environment.bz2" || key.ends_with(".ebuild") {
            if let Some(bytes) = read_xpak_member_raw(binpkg_path, key)? {
                fs::write(build_info_dest.join(key), bytes)
                    .map_err(|e| format!("{}: {e}", build_info_dest.join(key).display()))?;
            }
            continue;
        }
        let dest = build_info_dest.join(key);
        fs::write(&dest, format!("{}\n", value.trim()))
            .map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(())
}

/// The xpak `[image tarball]` prefix -> `dest`. Real
/// `xpak.tbz2.decompose`: the image is everything before the
/// `XPAKPACK…STOP` trailer.
fn extract_xpak_image(binpkg_path: &Path, dest: &Path) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f =
        fs::File::open(binpkg_path).map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    let file_len = f
        .seek(SeekFrom::End(0))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if file_len < 16 {
        return Err(format!(
            "{}: too small for an xpak binpkg",
            binpkg_path.display()
        ));
    }
    let mut trailer = [0u8; 16];
    f.seek(SeekFrom::End(-16))
        .and_then(|_| f.read_exact(&mut trailer))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if &trailer[0..8] != b"XPAKSTOP" || &trailer[12..16] != b"STOP" {
        return Err(format!("{}: no XPAKSTOP trailer", binpkg_path.display()));
    }
    let infosize = be32(&trailer[8..12]) as u64;
    let image_len = file_len.checked_sub(infosize + 8).ok_or_else(|| {
        format!(
            "{}: xpak trailer larger than the file",
            binpkg_path.display()
        )
    })?;

    let scratch = ScratchDir::new("xpak-image")?;
    let image_tar = scratch.path().join("image.tar");
    f.seek(SeekFrom::Start(0))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    let mut out =
        fs::File::create(&image_tar).map_err(|e| format!("{}: {e}", image_tar.display()))?;
    std::io::copy(&mut f.take(image_len), &mut out)
        .map_err(|e| format!("{}: {e}", image_tar.display()))?;
    drop(out);
    // `tar -x` auto-detects gzip/bzip2/xz/zstd/... on read.
    run_tar(&["-xpf", &lossy(&image_tar), "-C", &lossy(dest)])
}

/// Locate `<basename>/<want>.tar[.<comp>]` in a gpkg's outer tar,
/// decompress it if needed, and extract it into `dest`. Shares the outer
/// unpack + `gpkg-1` validity guard with `read_gpkg_metadata`.
fn extract_gpkg_member(gpkg_path: &Path, want: &str, dest: &Path) -> Result<(), String> {
    if !gpkg_path.is_file() {
        return Err(format!("{}: not a file", gpkg_path.display()));
    }
    let scratch = ScratchDir::new("gpkg-member")?;
    let outer = scratch.path().join("outer");
    fs::create_dir_all(&outer).map_err(|e| format!("{}: {e}", outer.display()))?;
    run_tar(&["-xf", &lossy(gpkg_path), "-C", &lossy(&outer)])?;

    let mut gpkg_marker = false;
    let mut member: Option<(PathBuf, Option<&'static [&'static str]>)> = None;
    for basename_dir in read_dir_sorted(&outer)? {
        if !basename_dir.is_dir() {
            if basename_dir.file_name().and_then(|n| n.to_str()) == Some("gpkg-1") {
                gpkg_marker = true;
            }
            continue;
        }
        for m in read_dir_sorted(&basename_dir)? {
            let Some(n) = m.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if n == "gpkg-1" {
                gpkg_marker = true;
            }
            if let Some(comp) = classify_inner_member(want, n) {
                member.get_or_insert((m.clone(), comp));
            }
        }
    }
    if !gpkg_marker {
        return Err(format!(
            "{}: no `gpkg-1` version marker",
            gpkg_path.display()
        ));
    }
    let (member, comp) =
        member.ok_or_else(|| format!("{}: no `{want}.tar` member", gpkg_path.display()))?;

    let inner_tar = scratch.path().join(format!("{want}.tar"));
    match comp {
        None => {
            fs::copy(&member, &inner_tar).map_err(|e| format!("{}: {e}", member.display()))?;
        }
        Some(argv) => {
            let out = fs::File::create(&inner_tar)
                .map_err(|e| format!("{}: {e}", inner_tar.display()))?;
            let status = Command::new(argv[0])
                .args(&argv[1..])
                .arg(&member)
                .stdout(out)
                .status()
                .map_err(|e| format!("failed to spawn {}: {e}", argv[0]))?;
            if !status.success() {
                return Err(format!(
                    "{} failed to decompress {} ({status})",
                    argv[0],
                    member.display()
                ));
            }
        }
    }
    // Real `gpkg.tar_safe_extract.extractall(dest)`: the inner tarball's
    // members all live under a single `<want>/` top-level directory
    // (real `gpkg._add_data`: `image_tar.add(root_dir, "image",
    // recursive=True)`), and portage strips it -- it extracts to a temp
    // dir and moves `<want>/*` into `dest`. `--strip-components=1` is the
    // same strip: the format guarantees the one prefix component, so the
    // members become `usr/...` / `environment.bz2` directly. Without it
    // every path in the merged package's vdb `CONTENTS` was recorded as
    // `/image/...`, which breaks unmerge (portuale's and real portage's).
    run_tar(&[
        "-xpf",
        &lossy(&inner_tar),
        "-C",
        &lossy(dest),
        "--strip-components=1",
    ])
}

/// Real `bintree._populate_local`'s own default (non-`FEATURES=
/// pkgdir-index-trusted`) behavior: walk `pkgdir` for binpkg *files* and
/// synthesize one `Packages`-style entry per file from its own embedded
/// metadata (`read_xpak_metadata` / `read_gpkg_metadata`), fast-pathed
/// against any already-parsed `<pkgdir>/Packages` entries first -- real
/// `bintree.py:1108-1136`'s own "Validate data from the package index
/// and try to avoid reading the xpak if possible" comment names this
/// exact optimization. A candidate entry (matched by basename) whose
/// own `_mtime_`/`SIZE` still agree with the live file's `stat(2)` (and
/// which carries at least `CPV`/`SLOT`, real's own `minimum_keys`) is
/// reused verbatim -- its `PATH` is refreshed in case the file moved,
/// matching real's own `if oldpath != mypath: d["PATH"] = mypath`.
/// Anything else (a changed file, a brand-new one, an absent index
/// entirely, or a stale match) is freshly parsed via
/// `read_xpak_metadata`/`read_gpkg_metadata`, and gets a fresh
/// `_mtime_`/`SIZE` recorded on the returned entry so a *later* call can
/// hit the fast path. This real revalidation replaces portuale's own
/// former "present `Packages` is always trusted outright, no scan at
/// all" stance (`portage_repo::read_packages_index`'s own doc comment
/// used to describe that as portuale's long-standing default) -- the
/// caller (`pretend.rs`'s own `--usepkg` CLI-boundary setup) now uses
/// this function unconditionally, `Packages` present or not.
///
/// `$PKGDIR` layout is `<pkgdir>/<category>/<pf>.{tbz2,gpkg.tar}` for a
/// single instance, and -- under real `FEATURES=binpkg-multi-instance`
/// -- `<pkgdir>/<category>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}` (real
/// `bintree._allocate_filename_multi`: a `<cat>/<pn>` subdir, a
/// `-<build_id>` suffix, and the `.xpak` extension for the xpak format).
/// A `.xpak` file is byte-format-identical to a `.tbz2`
/// (`[image tarball][XPAK …STOP]` -- real `bin/misc-functions.sh` builds
/// the same archive either way), so `read_xpak_metadata` reads it
/// unchanged; the multi-instance `BUILD_ID` comes from the filename,
/// validated against the archive's own embedded `PF`. `CPV` is
/// `<category>/<pf>` (the `<pf>` from the embedded metadata for a
/// multi-instance file, from the filename otherwise), `SIZE` from the
/// file's own byte size (real `bintree`'s own `st_size`), `REPO` from
/// the embedded `repository`, `PATH` from the relative path. Entries are
/// `CPV`-sorted for a deterministic pool order.
///
/// v1 cuts: the old flat `<pkgdir>/All/<pf>.tbz2` layout
/// (real's own `mydir != "All"` fallback) is not walked; a file that
/// fails to parse aborts the scan (rather than real portage's own
/// skip-and-warn) -- a `$PKGDIR` full of unreadable binpkgs is a real
/// problem worth surfacing, not silently resolving against a partial
/// pool -- but a *misnamed* multi-instance file (one whose `<pf>-<id>`
/// stem disagrees with its embedded `PF`, or whose subdir isn't `<pn>`)
/// is skipped, matching real's own `invalid_name`/`name_split`
/// `continue`.
pub fn populate_local_pkgdir(pkgdir: &Path) -> Result<Vec<HashMap<String, String>>, String> {
    let existing = portage_repo::read_packages_index(pkgdir);
    let mut by_basename: HashMap<&str, Vec<&HashMap<String, String>>> = HashMap::new();
    for e in &existing {
        if let Some(path) = e.get("PATH")
            && let Some(basename) = Path::new(path).file_name().and_then(|n| n.to_str())
        {
            by_basename.entry(basename).or_default().push(e);
        }
    }

    let mut out: Vec<HashMap<String, String>> = Vec::new();
    let Ok(categories) = fs::read_dir(pkgdir) else {
        return Ok(out);
    };
    let mut cat_paths: Vec<PathBuf> = categories
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    cat_paths.sort();
    for cat_path in cat_paths {
        if !cat_path.is_dir() {
            continue;
        }
        let Some(category) = cat_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        for entry in read_dir_sorted(&cat_path)? {
            let Some(name) = entry.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if entry.is_dir() {
                // A `<cat>/<pn>` multi-instance package dir (real
                // `bintree._allocate_filename_multi`): its files are
                // `<pf>-<build_id>.{xpak,gpkg.tar}`.
                for mfile in read_dir_sorted(&entry)? {
                    let Some(mname) = mfile.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    let path_field = format!("{category}/{name}/{mname}");
                    let Some(e) =
                        scan_binpkg_file(&mfile, mname, category, path_field, &by_basename, true)?
                    else {
                        continue;
                    };
                    // Real's `tuple(catsplit(mydir)) == name_split[:2]`:
                    // the containing subdir must be this package's own
                    // `<pn>` (from its now-known `CPV`).
                    let pn_ok = e
                        .get("CPV")
                        .and_then(|cpv| portage_dep::parse_candidate(cpv))
                        .is_some_and(|c| c.package == name);
                    if pn_ok {
                        out.push(e);
                    }
                }
                continue;
            }
            let path_field = format!("{category}/{name}");
            if let Some(e) =
                scan_binpkg_file(&entry, name, category, path_field, &by_basename, false)?
            {
                out.push(e);
            }
        }
    }
    out.sort_by(|a, b| a.get("CPV").cmp(&b.get("CPV")));
    Ok(out)
}

/// One `$PKGDIR` binpkg file -> its synthesized `Packages`-style entry,
/// or `None` if it isn't a binpkg or is a misnamed multi-instance file
/// (real's own `invalid_name`/`name_split` `continue`). Reuses an
/// unchanged `<pkgdir>/Packages` entry (matched by basename, with
/// `_mtime_` and `SIZE` still agreeing -- real's "avoid reading the
/// xpak if possible") when one exists, else parses the archive's own
/// embedded metadata. `multi_instance` selects the naming contract: when set, the
/// file is `<pf>-<build_id>.{xpak,gpkg.tar}` with `PF` taken from the
/// archive and `BUILD_ID` from the filename; when clear, it is
/// `<pf>.{tbz2,gpkg.tar}`.
fn scan_binpkg_file(
    file: &Path,
    basename: &str,
    category: &str,
    path_field: String,
    by_basename: &HashMap<&str, Vec<&HashMap<String, String>>>,
    multi_instance: bool,
) -> Result<Option<HashMap<String, String>>, String> {
    let is_gpkg = basename.ends_with(".gpkg.tar");
    let ext = if is_gpkg {
        ".gpkg.tar"
    } else if basename.ends_with(".xpak") {
        ".xpak"
    } else if basename.ends_with(".tbz2") {
        ".tbz2"
    } else {
        return Ok(None);
    };
    let Ok(st) = fs::metadata(file) else {
        return Ok(None);
    };
    let mtime = file_mtime(&st);
    let size = st.len();

    if let Some(candidates) = by_basename.get(basename)
        && let Some(&hit) = candidates.iter().find(|d| {
            d.get("_mtime_").and_then(|m| m.parse::<i64>().ok()) == Some(mtime)
                && d.get("SIZE").and_then(|s| s.parse::<u64>().ok()) == Some(size)
                && d.contains_key("CPV")
                && d.contains_key("SLOT")
        })
    {
        let mut entry = hit.clone();
        entry.insert("PATH".to_string(), path_field);
        return Ok(Some(entry));
    }

    // Stale, moved, or unindexed -- re-derive from the file itself. A
    // `.xpak` file is byte-format-identical to a `.tbz2`, so
    // `read_xpak_metadata` reads both.
    let mut meta = if is_gpkg {
        read_gpkg_metadata(file)?
    } else {
        read_xpak_metadata(file)?
    };
    let stem = basename.strip_suffix(ext).unwrap();
    let embedded_pf = meta.get("PF").filter(|p| !p.is_empty()).cloned();

    let pf = if multi_instance {
        // The archive's own embedded `PF` is the ground truth; the
        // filename must be exactly `<PF>-<build_id>` (real
        // `_parse_build_id` + `myfile != f"{mypf}-{build_id}.<ext>"`).
        let Some(real_pf) = embedded_pf else {
            return Ok(None);
        };
        let Some(build_id) = stem
            .strip_prefix(&real_pf)
            .and_then(|s| s.strip_prefix('-'))
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        else {
            return Ok(None);
        };
        meta.insert("BUILD_ID".to_string(), build_id.to_string());
        real_pf
    } else {
        // Real's `myfile != mypf + ".<ext>"` -> `invalid_name`: a
        // category-level file whose stem disagrees with the archive's
        // own `PF` (e.g. a misplaced `-<build_id>` file) is skipped.
        if let Some(real_pf) = &embedded_pf
            && real_pf != stem
        {
            return Ok(None);
        }
        stem.to_string()
    };

    meta.insert("CPV".to_string(), format!("{category}/{pf}"));
    meta.entry("CATEGORY".to_string())
        .or_insert_with(|| category.to_string());
    meta.entry("PF".to_string()).or_insert_with(|| pf.clone());
    if let Some(repo) = meta.remove("repository") {
        meta.entry("REPO".to_string()).or_insert(repo);
    }
    meta.insert("SIZE".to_string(), size.to_string());
    meta.insert("PATH".to_string(), path_field);
    meta.insert("_mtime_".to_string(), mtime.to_string());
    Ok(Some(meta))
}

/// Real `os.lstat(...)[stat.ST_MTIME]` -- whole seconds, the same
/// integer real portage's own `_mtime_` index field records
/// (`bintree.py:2306`'s own `d["_mtime_"] = str(st[stat.ST_MTIME])`).
pub(crate) fn file_mtime(st: &fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    st.mtime()
}

fn run_tar(args: &[&str]) -> Result<(), String> {
    let status = Command::new("tar")
        .args(args)
        .status()
        .map_err(|e| format!("failed to spawn tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar {args:?} failed ({status})"));
    }
    Ok(())
}

fn lossy(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Real GPG binpkg signature policy + verification (real
/// `lib/portage/gpkg.py`'s `checksum_helper(VERIFY)` and
/// `gpkg._verify_binpkg`'s own GPG layer), via the system `gpg`
/// subprocess -- the same "shell out to the real tool" stance `tar` /
/// the compressors / `wget` already take, which keeps the musl-static
/// story (zero linked crypto) intact.
///
/// Real `BINPKG_GPG_VERIFY_BASE_COMMAND` (`cnf/make.globals:53`) is a
/// template: `[PORTAGE_CONFIG]` -> `--homedir <BINPKG_GPG_VERIFY_GPG_HOME>`
/// and `[SIGNATURE]` -> `<detached-.sig-file> -` for a detached `.sig`
/// member, or `--output - -` for an inline clear-signed `Manifest`. The
/// verified bytes go on `gpg`'s stdin; for clear-sign its stdout is the
/// cleartext (what real parses as the Manifest afterwards).
///
/// Policy mirrors real `gpkg.__init__` (`gpkg.py:792-819`) with no
/// per-binrepo override (that override is display/parse-only in
/// portuale -- see `GpgVerify::from_env`): `binpkg-request-signature`
/// forces both flags on (it beats `binpkg-ignore-signature`, real's own
/// `if`/`elif` order), `binpkg-ignore-signature` forces both off,
/// otherwise signatures are verified when present but not required.
#[derive(Clone, Debug)]
pub struct GpgVerify {
    /// Real `gpkg.verify_signature`: a failed or missing cryptographic
    /// check is fatal.
    pub verify_signature: bool,
    /// Real `gpkg.request_signature`: signature files are mandatory --
    /// a member without its `.sig` sidecar (other than the unsigned
    /// `gpkg-1` version marker) is rejected even if its digests match.
    pub request_signature: bool,
    /// Real `BINPKG_GPG_VERIFY_BASE_COMMAND`.
    pub base_command: String,
    /// Real `BINPKG_GPG_VERIFY_GPG_HOME`.
    pub gpg_home: String,
}

/// Real `cnf/make.globals:53`'s own default verify command.
pub const DEFAULT_GPG_VERIFY_BASE_COMMAND: &str = "/usr/bin/gpg --verify --batch --no-tty --yes --no-auto-check-trustdb --status-fd 2 [PORTAGE_CONFIG] [SIGNATURE]";

/// Real `cnf/make.globals:56`'s own default verify keyring.
pub const DEFAULT_GPG_VERIFY_GPG_HOME: &str = "/etc/portage/gnupg";

impl Default for GpgVerify {
    fn default() -> Self {
        let (verify_signature, request_signature) =
            gpg_policy_for_features(&std::env::var("FEATURES").unwrap_or_default());
        Self {
            verify_signature,
            request_signature,
            base_command: std::env::var("BINPKG_GPG_VERIFY_BASE_COMMAND")
                .unwrap_or_else(|_| DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string()),
            gpg_home: std::env::var("BINPKG_GPG_VERIFY_GPG_HOME")
                .unwrap_or_else(|_| DEFAULT_GPG_VERIFY_GPG_HOME.to_string()),
        }
    }
}

impl GpgVerify {
    /// Real portage's own `settings`-derived verify configuration, via
    /// the same "read the env var, fall back to `make.globals`'s own
    /// default" shortcut every other real-execution CLI boundary in this
    /// portuale already takes: `FEATURES` (`binpkg-request-signature` /
    /// `binpkg-ignore-signature`), `BINPKG_GPG_VERIFY_BASE_COMMAND`,
    /// `BINPKG_GPG_VERIFY_GPG_HOME`. A per-binrepo
    /// `verify-signature = false` (`binrepos.conf`) is a deliberate cut
    /// here -- portuale parses and displays it (see `BinRepo`), but the
    /// merge path has no binrepo at hand for a local `$PKGDIR` file, so
    /// `FEATURES` alone decides. Observable divergence is narrow: an
    /// outright-bad signature from such a repo fails here where real
    /// would skip the check; unsigned packages merge identically either
    /// way.
    pub fn from_env() -> Self {
        Self::default()
    }
}

/// Real `gpkg.__init__`'s own `request_signature` / `verify_signature`
/// derivation (`gpkg.py:798-819`), as a pure function of the `FEATURES`
/// string so it can be unit-tested without mutating the process
/// environment. Returns `(verify_signature, request_signature)`.
pub fn gpg_policy_for_features(features: &str) -> (bool, bool) {
    let has = |tok: &str| features.split_whitespace().any(|t| t == tok);
    // Real's own `if`/`elif` order: a request beats an ignore.
    if has("binpkg-request-signature") {
        (true, true)
    } else if has("binpkg-ignore-signature") {
        (false, false)
    } else {
        (true, false)
    }
}

/// Fill real `BINPKG_GPG_VERIFY_BASE_COMMAND`'s own two placeholders
/// (`gpkg.py:500-525`): `[PORTAGE_CONFIG]` -> `--homedir <gpg_home>`,
/// `[SIGNATURE]` -> `signature_arg` (either `<sig-file> -` for a
/// detached `.sig`, or `--output - -` for a clear-signed Manifest).
/// Split on whitespace -- a deliberate narrowing of real's
/// `shlex.split` + `varexpand` (real's own default template and every
/// test command split cleanly; a path with spaces would not).
fn gpg_verify_argv(template: &str, gpg_home: &str, signature_arg: &str) -> Vec<String> {
    template
        .replace("[PORTAGE_CONFIG]", &format!("--homedir {gpg_home} "))
        .replace("[SIGNATURE]", signature_arg)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Run one `gpg --verify` argv with `stdin_data` on its stdin, mirroring
/// real `checksum_helper.finish` + `_check_gpg_status`
/// (`gpkg.py:624-659` + `:590-612`): exit-status 0 is not enough --
/// GnuPG returns OK even for an untrusted signer, so the `--status-fd`
/// lines must carry both `GOODSIG` and `TRUST_ULTIMATE`/`TRUST_FULLY`,
/// else `InvalidSignature`. Returns gpg's stdout (the cleartext for a
/// clear-signed Manifest; empty for a detached `.sig`).
fn run_gpg_verify(argv: &[String], stdin_data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning {}: {e}", argv[0]))?;
    // A write failure here (EPIPE -- gpg already exited on bad input)
    // carries no signal of its own; gpg's exit status + status lines
    // below are the verdict, so it is deliberately ignored.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_data);
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("waiting for {}: {e}", argv[0]))?;
    let status_lines = String::from_utf8_lossy(&output.stderr);
    let good_sig = status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] GOODSIG"));
    let trusted = status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] TRUST_ULTIMATE") || l.starts_with("[GNUPG:] TRUST_FULLY"));
    if output.status.success() && good_sig && trusted {
        return Ok(output.stdout);
    }
    // Real `show_gpg_error`'s own single-cause summaries
    // (`gpkg.py:563-579`): only when exactly one cause matches, else
    // "(none available)" -- a malformed signature must not get a
    // confident-sounding diagnosis.
    let mut causes = 0;
    let mut summary = "(none available)";
    if status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] NODATA"))
    {
        causes += 1;
        summary = "binpkg appears unsigned (missing any signature)";
    }
    if status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] NO_PUBKEY"))
    {
        causes += 1;
        summary = "binpkg signed with at least one unknown key.";
    }
    if status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] TRUST_UNDEFINED"))
    {
        causes += 1;
        summary = "binpkg signed with a known key of undefined trust.";
    }
    if causes != 1 {
        summary = "(none available)";
    }
    Err(format!(
        "GnuPG verification failed: {summary}\n{}",
        status_lines.trim_end()
    ))
}

/// Real per-file detached verification (`_verify_binpkg`'s own
/// `f_signature` branch, `gpkg.py:1764-1788`): the `.sig` sidecar goes
/// to a temp file named in `[SIGNATURE]`, the member bytes on stdin.
/// `what` names the member for the error (real reports per-file).
fn verify_detached_signature(
    member_bytes: &[u8],
    sig_bytes: &[u8],
    gpg: &GpgVerify,
    what: &str,
) -> Result<(), String> {
    let sig_path = std::env::temp_dir().join(format!(
        "portuale-sign-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::write(&sig_path, sig_bytes).map_err(|e| format!("{}: {e}", sig_path.display()))?;
    let argv = gpg_verify_argv(
        &gpg.base_command,
        &gpg.gpg_home,
        &format!("{} -", sig_path.display()),
    );
    let result = run_gpg_verify(&argv, member_bytes).map(|_| ());
    let _ = fs::remove_file(&sig_path);
    result.map_err(|e| format!("{what}: {e}"))
}

/// Real clear-signed-`Manifest` verification (`_verify_binpkg`'s own
/// Manifest branch, `gpkg.py:1714-1731`): `[SIGNATURE]` -> `--output -
/// -`, the whole signed Manifest on stdin, gpg's stdout (the
/// cleartext `DATA` lines) back for Manifest parsing.
fn verify_clearsigned_manifest(
    signed_manifest: &[u8],
    gpg: &GpgVerify,
    gpkg_path: &Path,
) -> Result<Vec<u8>, String> {
    let argv = gpg_verify_argv(&gpg.base_command, &gpg.gpg_home, "--output - -");
    run_gpg_verify(&argv, signed_manifest)
        .map_err(|e| format!("{}: Manifest {e}", gpkg_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    #[test]
    fn classify_inner_member_matches_real_extract_filename_compression() {
        assert!(matches!(
            classify_inner_member("metadata", "metadata.tar"),
            Some(None)
        ));
        assert!(matches!(
            classify_inner_member("metadata", "metadata.tar.zst"),
            Some(Some(_))
        ));
        assert!(matches!(
            classify_inner_member("metadata", "metadata.tar.xz"),
            Some(Some(_))
        ));
        assert!(classify_inner_member("metadata", "metadata.tar.zst.sig").is_none());
        assert!(classify_inner_member("metadata", "image.tar.zst").is_none());
        assert!(classify_inner_member("metadata", "Manifest").is_none());
    }

    #[test]
    fn read_gpkg_metadata_extracts_the_embedded_scalar_metadata() {
        // A real, hand-built `.gpkg.tar` (real `tar` + real `zstd`) --
        // outer plain-tar container, `gpkg-1` marker, zstd-compressed
        // inner `metadata.tar` with `metadata/<KEY>` files.
        let m = read_gpkg_metadata(&fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"))
            .expect("the fixture gpkg reads");
        assert_eq!(m.get("EAPI").map(String::as_str), Some("8"));
        assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
        assert_eq!(m.get("KEYWORDS").map(String::as_str), Some("amd64"));
        assert_eq!(m.get("IUSE").map(String::as_str), Some("grfoo"));
        assert_eq!(m.get("USE").map(String::as_str), Some(""));
        assert_eq!(m.get("DEPEND").map(String::as_str), Some("dev-libs/newpkg"));
        assert_eq!(
            m.get("RDEPEND").map(String::as_str),
            Some("dev-libs/newpkg")
        );
        assert_eq!(m.get("CATEGORY").map(String::as_str), Some("dev-libs"));
        assert_eq!(m.get("PF").map(String::as_str), Some("gpkgreadpkg-1.0"));
        assert_eq!(m.get("repository").map(String::as_str), Some("gentoo"));
    }

    #[test]
    fn read_gpkg_metadata_rejects_a_non_gpkg_tar() {
        // A plain tar with no `gpkg-1` marker anywhere.
        let scratch = ScratchDir::new("gpkg-negtest").unwrap();
        let junk = scratch.path().join("a.txt");
        fs::write(&junk, b"x").unwrap();
        let not_gpkg = scratch.path().join("plain.tar");
        run_tar(&[
            "-cf",
            &lossy(&not_gpkg),
            "-C",
            &lossy(scratch.path()),
            "a.txt",
        ])
        .unwrap();
        let err = read_gpkg_metadata(&not_gpkg).unwrap_err();
        assert!(err.contains("gpkg-1"), "{err}");
    }

    /// `usize -> u32` narrowing that panics (like Python's
    /// `struct.pack(">I", n)` raising) instead of silently wrapping; the
    /// fixture-making sizes are far below `u32::MAX`, so this only fires
    /// when a test is corrupted by mistake.
    fn xpak_u32(len: usize) -> u32 {
        u32::try_from(len).expect("xpak length exceeds u32::MAX")
    }

    /// Build a real XPAK segment (real `xpak.xpak_mem` layout) and append
    /// it to some prefix bytes, exactly the way a real `.tbz2` is
    /// `[tarball][XPAK trailer]`.
    fn make_xpak_binpkg(prefix: &[u8], entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut index = Vec::new();
        let mut data = Vec::new();
        for (name, value) in entries {
            index.extend_from_slice(&xpak_u32(name.len()).to_be_bytes());
            index.extend_from_slice(name.as_bytes());
            index.extend_from_slice(&xpak_u32(data.len()).to_be_bytes());
            index.extend_from_slice(&xpak_u32(value.len()).to_be_bytes());
            data.extend_from_slice(value);
        }
        let mut segment = Vec::new();
        segment.extend_from_slice(b"XPAKPACK");
        segment.extend_from_slice(&xpak_u32(index.len()).to_be_bytes());
        segment.extend_from_slice(&xpak_u32(data.len()).to_be_bytes());
        segment.extend_from_slice(&index);
        segment.extend_from_slice(&data);
        segment.extend_from_slice(b"XPAKSTOP");

        let mut out = prefix.to_vec();
        out.extend_from_slice(&segment);
        out.extend_from_slice(&xpak_u32(segment.len()).to_be_bytes());
        out.extend_from_slice(b"STOP");
        out
    }

    #[test]
    fn read_xpak_metadata_walks_the_index_and_returns_every_key() {
        let scratch = ScratchDir::new("xpak-test").unwrap();
        let path = scratch.path().join("dev-libs:foo-1.0.tbz2");
        let bytes = make_xpak_binpkg(
            b"pretend this is a bzip2'd tarball, arbitrary length ......",
            &[
                ("EAPI", b"8\n"),
                ("SLOT", b"0\n"),
                ("KEYWORDS", b"amd64\n"),
                ("IUSE", b"xfoo xbar\n"),
                ("USE", b"\n"),
                ("RDEPEND", b"dev-libs/samepkg dev-libs/newpkg\n"),
                ("repository", b"gentoo\n"),
            ],
        );
        fs::write(&path, &bytes).unwrap();

        let m = read_xpak_metadata(&path).expect("reads");
        assert_eq!(m.get("EAPI").map(String::as_str), Some("8"));
        assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
        assert_eq!(m.get("KEYWORDS").map(String::as_str), Some("amd64"));
        assert_eq!(m.get("IUSE").map(String::as_str), Some("xfoo xbar"));
        assert_eq!(m.get("USE").map(String::as_str), Some(""));
        assert_eq!(
            m.get("RDEPEND").map(String::as_str),
            Some("dev-libs/samepkg dev-libs/newpkg")
        );
        assert_eq!(m.get("repository").map(String::as_str), Some("gentoo"));
    }

    #[test]
    fn read_xpak_metadata_rejects_a_file_with_no_xpak_trailer() {
        let scratch = ScratchDir::new("xpak-negtest").unwrap();
        let path = scratch.path().join("not-a-binpkg");
        fs::write(
            &path,
            b"just some bytes, definitely no XPAKSTOP here at all",
        )
        .unwrap();
        let err = read_xpak_metadata(&path).unwrap_err();
        assert!(err.contains("XPAKSTOP"), "{err}");
    }

    /// Reads a **genuine** `.tbz2` -- checked in at
    /// `fixtures/pkgdir/dev-libs/packagepkg-1.0.tbz2`, built once by the
    /// portuale's own `ebuild <file> package` on `dev-libs/packagepkg`
    /// (real `bin/misc-functions.sh` -> unmodified `xpak-helper.py
    /// recompose` -> real `xpak.py`). Kept as a committed fixture rather
    /// than rebuilt per-test on purpose: the read side doesn't need
    /// reproducible bytes, and driving `run_package` (the full brush
    /// phase chain) here would add real parallel-load pressure to the
    /// suite's brush-heavy tests for no reader-coverage gain.
    #[test]
    fn read_xpak_metadata_reads_a_real_ebuild_package_tbz2() {
        let tbz2 = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
        let m = read_xpak_metadata(&tbz2).expect("the real .tbz2 reads");
        assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
        assert_eq!(m.get("EAPI").map(String::as_str), Some("8"));
        assert_eq!(m.get("CATEGORY").map(String::as_str), Some("dev-libs"));
        assert_eq!(m.get("PF").map(String::as_str), Some("packagepkg-1.0"));
        assert_eq!(m.get("KEYWORDS").map(String::as_str), Some("amd64"));
        // The fixture ebuild's `RDEPEND="dev-libs/samepkg"` came through
        // via real `build-info` (`ebuild_phases::write_post_install_
        // metadata`), no `Packages` index involved.
        assert_eq!(
            m.get("RDEPEND").map(String::as_str),
            Some("dev-libs/samepkg")
        );
        // The bundled `<pf>.ebuild` source is a real member too.
        assert!(
            m.get("packagepkg-1.0.ebuild")
                .is_some_and(|e| e.contains("EAPI=8"))
        );
        // A binary package's own xpak never carries CONTENTS (real
        // `xpak()` skips it -- generated at merge time).
        assert!(!m.contains_key("CONTENTS"));
        // `environment.bz2` is binary -> lossy-decoded but present as a
        // key; a scan consumer only ever looks up scalar keys.
        assert!(m.contains_key("environment.bz2"));
    }

    #[test]
    fn extract_binpkg_unpacks_an_xpak_image_and_build_info() {
        let tmp = std::env::temp_dir().join(format!("binpkg-xpak-{}", std::process::id()));
        let image = tmp.join("image");
        let bi = tmp.join("build-info");
        extract_binpkg(
            &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &image,
            &bi,
            &GpgVerify::default(),
        )
        .expect("extract succeeds");

        let hello = image.join("usr/share/packagepkg/hello.txt");
        assert!(hello.is_file(), "the image tarball was unpacked");
        assert!(fs::read_to_string(&hello).unwrap().contains("hello"));

        assert_eq!(fs::read_to_string(bi.join("SLOT")).unwrap().trim(), "0");
        assert_eq!(
            fs::read_to_string(bi.join("RDEPEND")).unwrap().trim(),
            "dev-libs/samepkg"
        );
        // The two non-scalar members are kept verbatim: a real bzip2
        // stream (magic `BZh`) and the real ebuild source.
        let env_bz2 = fs::read(bi.join("environment.bz2")).expect("environment.bz2 kept");
        assert_eq!(&env_bz2[..3], b"BZh", "a real bzip2 stream, byte-exact");
        assert!(
            fs::read_to_string(bi.join("packagepkg-1.0.ebuild"))
                .unwrap()
                .contains("EAPI=8")
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extract_binpkg_unpacks_a_real_gpkg_image_and_build_info() {
        let tmp = std::env::temp_dir().join(format!("binpkg-gpkg-{}", std::process::id()));
        let image = tmp.join("image");
        let bi = tmp.join("build-info");
        extract_binpkg(
            &fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
            &image,
            &bi,
            &GpgVerify::default(),
        )
        .expect("gpkg extract succeeds");
        // The inner `image/` and `metadata/` top-level dirs real's
        // `tar_safe_extract` strips are stripped here too -- members land
        // directly, not one level deep. A missing strip is what made the
        // merged vdb `CONTENTS` record every path as `/image/...`.
        assert!(image.join("hello.txt").is_file(), "image prefix stripped");
        assert!(!image.join("image").exists(), "no leftover image/ dir");
        assert!(bi.join("SLOT").is_file(), "metadata prefix stripped");
        assert!(!bi.join("metadata").exists(), "no leftover metadata/ dir");
        let _ = fs::remove_dir_all(&tmp);
    }

    /// Assemble a real (plain outer tar) gpkg container from a set of
    /// `<prefix>/<name>` members plus a `Manifest` body, in real gpkg
    /// member order.
    fn build_gpkg(prefix: &str, members: &[(&str, &[u8])], manifest: Option<&str>) -> PathBuf {
        let scratch = ScratchDir::new("gpkg-build").unwrap();
        // leak the scratch dir for the caller's test lifetime
        let root = scratch.path().to_path_buf();
        std::mem::forget(scratch);
        let pkgdir = root.join(prefix);
        fs::create_dir_all(&pkgdir).unwrap();
        let mut argv: Vec<String> = vec![
            "-cf".into(),
            lossy(&root.join("out.gpkg.tar")),
            "-C".into(),
            lossy(&root),
        ];
        for (name, bytes) in members {
            fs::write(pkgdir.join(name), bytes).unwrap();
            argv.push(format!("{prefix}/{name}"));
        }
        if let Some(body) = manifest {
            fs::write(pkgdir.join("Manifest"), body).unwrap();
            argv.push(format!("{prefix}/Manifest"));
        }
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        run_tar(&refs).unwrap();
        root.join("out.gpkg.tar")
    }

    fn blake2b_hex(bytes: &[u8]) -> String {
        use blake2::Digest as _;
        blake2::Blake2b512::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    fn sha512_hex(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        sha2::Sha512::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    fn data_line(name: &str, bytes: &[u8]) -> String {
        format!(
            "DATA {name} {} BLAKE2B {} SHA512 {}\n",
            bytes.len(),
            blake2b_hex(bytes),
            sha512_hex(bytes)
        )
    }

    #[test]
    fn verify_gpkg_manifest_accepts_the_real_fixture() {
        verify_gpkg_manifest(
            &fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
            &GpgVerify::default(),
        )
        .expect("the committed fixture's Manifest verifies");
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_missing_manifest() {
        let g = build_gpkg("foo-1.0", &[("gpkg-1", b""), ("image.tar", b"img")], None);
        let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
        assert!(err.contains("Manifest not found"), "{err}");
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_size_mismatch_without_hashing() {
        let img: &[u8] = b"the real image bytes";
        let manifest = format!(
            "{}{}",
            data_line("gpkg-1", b""),
            // deliberately wrong size -- caught before any hashing
            "DATA image.tar 999999 BLAKE2B dead SHA512 beef\n",
        );
        let g = build_gpkg(
            "foo-1.0",
            &[("gpkg-1", b""), ("image.tar", img)],
            Some(&manifest),
        );
        let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
        assert!(err.contains("size mismatch"), "{err}");
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_tampered_member() {
        let img: &[u8] = b"the real image bytes";
        // Manifest records the digests of *different* bytes.
        let manifest = format!(
            "{}{}",
            data_line("gpkg-1", b""),
            data_line("image.tar", b"some other bytes entirely"),
        );
        let g = build_gpkg(
            "foo-1.0",
            &[("gpkg-1", b""), ("image.tar", img)],
            Some(&manifest),
        );
        let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
        assert!(err.contains("mismatch"), "{err}");
    }

    #[test]
    fn verify_gpkg_manifest_rejects_an_unlisted_member() {
        let manifest = data_line("gpkg-1", b"");
        let g = build_gpkg(
            "foo-1.0",
            &[("gpkg-1", b""), ("image.tar", b"img")],
            Some(&manifest),
        );
        let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
        assert!(err.contains("not listed in the Manifest"), "{err}");
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_manifest_only_file() {
        let manifest = format!(
            "{}{}",
            data_line("gpkg-1", b""),
            data_line("image.tar", b"img"),
        );
        let g = build_gpkg("foo-1.0", &[("gpkg-1", b"")], Some(&manifest));
        let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
        assert!(err.contains("not present in the container"), "{err}");
    }

    #[test]
    fn extract_binpkg_rejects_a_gpkg_with_a_bad_manifest() {
        let img: &[u8] = b"pretend image tar bytes";
        let manifest = format!(
            "{}{}",
            data_line("gpkg-1", b""),
            "DATA image.tar 3 BLAKE2B x SHA512 y\n",
        );
        let g = build_gpkg(
            "foo-1.0",
            &[("gpkg-1", b""), ("image.tar", img)],
            Some(&manifest),
        );
        let tmp = std::env::temp_dir().join(format!("binpkg-gpkg-bad-{}", std::process::id()));
        let err = extract_binpkg(
            &g,
            &tmp.join("image"),
            &tmp.join("build-info"),
            &GpgVerify::default(),
        )
        .unwrap_err();
        assert!(err.contains("size mismatch"), "{err}");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn populate_local_pkgdir_synthesizes_a_packages_style_entry_per_binpkg_file() {
        // fixtures/pkgdir/dev-libs/ holds a real `.tbz2` and a real
        // `.gpkg.tar` (among other loose binpkgs) plus an index-only
        // `Packages` file (fixtures/pkgdir/Packages) covering unrelated
        // CPVs. `populate_local_pkgdir` walks the directory unconditionally
        // (real `_populate_local`'s default `reindex=True`), so entries
        // for these two files are always freshly parsed from disk
        // regardless of what the index claims.
        let entries = populate_local_pkgdir(&fixture("pkgdir")).expect("scan succeeds");
        let by_cpv: HashMap<&str, &HashMap<String, String>> = entries
            .iter()
            .map(|e| (e.get("CPV").unwrap().as_str(), e))
            .collect();

        let tbz2 = by_cpv["dev-libs/packagepkg-1.0"];
        assert_eq!(tbz2.get("SLOT").map(String::as_str), Some("0"));
        assert_eq!(tbz2.get("EAPI").map(String::as_str), Some("8"));
        assert_eq!(tbz2.get("CATEGORY").map(String::as_str), Some("dev-libs"));
        assert_eq!(tbz2.get("PF").map(String::as_str), Some("packagepkg-1.0"));
        assert_eq!(
            tbz2.get("PATH").map(String::as_str),
            Some("dev-libs/packagepkg-1.0.tbz2")
        );
        assert!(tbz2.get("SIZE").is_some_and(|s| s.parse::<u64>().is_ok()));
        assert!(
            tbz2.get("_mtime_")
                .is_some_and(|m| m.parse::<i64>().is_ok())
        );

        let gpkg = by_cpv["dev-libs/gpkgreadpkg-1.0"];
        assert_eq!(gpkg.get("KEYWORDS").map(String::as_str), Some("amd64"));
        assert_eq!(
            gpkg.get("DEPEND").map(String::as_str),
            Some("dev-libs/newpkg")
        );
        // `repository` -> `REPO` (real `Packages` field name).
        assert_eq!(gpkg.get("REPO").map(String::as_str), Some("gentoo"));
        assert!(!gpkg.contains_key("repository"));

        // Entries are CPV-sorted for a deterministic candidate pool.
        let cpvs: Vec<&str> = entries
            .iter()
            .map(|e| e.get("CPV").unwrap().as_str())
            .collect();
        let mut sorted = cpvs.clone();
        sorted.sort_unstable();
        assert_eq!(cpvs, sorted);
    }

    #[test]
    fn populate_local_pkgdir_of_a_missing_or_empty_dir_is_empty() {
        assert!(
            populate_local_pkgdir(Path::new("/nonexistent/pkgdir"))
                .unwrap()
                .is_empty()
        );
        let scratch = ScratchDir::new("scan-empty").unwrap();
        assert!(populate_local_pkgdir(scratch.path()).unwrap().is_empty());
    }

    // ---- GPG binpkg signatures (`FEATURES=binpkg-signing`) ----

    /// Portage's own committed GnuPG test keyring
    /// (`3rdparty/portage/lib/portage/tests/.gnupg` -- the same keys
    /// real's own `test_gpkg_gpg.py` signs with): trusted
    /// `0x5D90EA06352177F6`, untrusted `0x8812797DDF1DD192`, both with
    /// passphrase `GentooTest`. Neither key expires, so a container
    /// signed with them verifies deterministically for the life of the
    /// fixture.
    const GPG_TRUSTED_KEY: &str = "0x5D90EA06352177F6";
    const GPG_UNTRUSTED_KEY: &str = "0x8812797DDF1DD192";
    const GPG_PASSPHRASE: &str = "GentooTest";

    /// A writable copy of the committed keyring under a fresh tempdir
    /// (`gpg` refuses a homedir it doesn't own outright, and signs /
    /// verifies with lock files inside it -- the committed tree itself
    /// must stay read-only). `chmod 700`, real's own requirement.
    fn test_gpg_home(tag: &str) -> PathBuf {
        fn copy_dir(src: &Path, dest: &Path) {
            fs::create_dir_all(dest).unwrap();
            for entry in fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let to = dest.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_dir(&entry.path(), &to);
                } else {
                    fs::copy(entry.path(), &to).unwrap();
                }
            }
        }
        let dest = std::env::temp_dir().join(format!(
            "portuale-gpg-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        copy_dir(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../3rdparty/portage/lib/portage/tests/.gnupg"),
            &dest,
        );
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o700)).unwrap();
        dest
    }

    /// Best-effort `gpg-agent` shutdown for a test homedir (a signing
    /// `gpg` daemonizes its agent, which would otherwise outlive the
    /// test session -- real portage's own `conftest.py` does the same).
    /// Verify-only tests never start an agent and don't need this.
    fn kill_gpg_agent(home: &Path) {
        let _ = Command::new("gpgconf")
            .args(["--homedir"])
            .arg(home)
            .args(["--kill", "all"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    fn test_gpg_verify(home: &Path, request: bool, verify: bool) -> GpgVerify {
        GpgVerify {
            verify_signature: verify,
            request_signature: request,
            base_command: DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string(),
            gpg_home: home.display().to_string(),
        }
    }

    /// Clear-sign `data` exactly the way real `checksum_helper(SIGNING,
    /// detached=False)` does for the `Manifest` (`gpkg.py:1552-1560`):
    /// `gpg --clearsign` over stdin. Detached-signs `data` the way real
    /// signs each member (`--detach-sig`, `gpkg.py:1031-1058`).
    fn gpg_sign(home: &Path, key: &str, data: &[u8], clearsign: bool) -> Vec<u8> {
        use std::io::Write;
        let mut cmd = Command::new("/usr/bin/gpg");
        cmd.args(["--homedir"])
            .arg(home)
            .args([
                "--batch",
                "--no-tty",
                "--yes",
                "--pinentry-mode",
                "loopback",
                "--passphrase",
                GPG_PASSPHRASE,
                "--digest-algo",
                "SHA512",
                "--local-user",
                key,
            ])
            .args(if clearsign {
                &["--clearsign"][..]
            } else {
                &["--armor", "--detach-sig"][..]
            })
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().expect("gpg signs");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(data)
            .expect("gpg stdin");
        let output = child.wait_with_output().expect("gpg runs");
        assert!(
            output.status.success(),
            "gpg sign failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    #[test]
    fn gpg_policy_for_features_matches_real_gpkg_init_precedence() {
        // Real `gpkg.__init__` (`gpkg.py:798-819`): default is
        // verify-when-present, request off.
        assert_eq!(gpg_policy_for_features(""), (true, false));
        assert_eq!(
            gpg_policy_for_features("sandbox binpkg-signing"),
            (true, false)
        );
        assert_eq!(
            gpg_policy_for_features("binpkg-request-signature"),
            (true, true)
        );
        assert_eq!(
            gpg_policy_for_features("binpkg-ignore-signature"),
            (false, false)
        );
        // Real's own `if`/`elif` order: a request beats an ignore.
        assert_eq!(
            gpg_policy_for_features("binpkg-request-signature binpkg-ignore-signature"),
            (true, true)
        );
    }

    #[test]
    fn gpg_verify_argv_substitutes_both_placeholders() {
        let argv = gpg_verify_argv(
            DEFAULT_GPG_VERIFY_BASE_COMMAND,
            "/tmp/test-home",
            "/tmp/x.sig -",
        );
        assert_eq!(
            argv,
            vec![
                "/usr/bin/gpg",
                "--verify",
                "--batch",
                "--no-tty",
                "--yes",
                "--no-auto-check-trustdb",
                "--status-fd",
                "2",
                "--homedir",
                "/tmp/test-home",
                "/tmp/x.sig",
                "-",
            ]
        );
        let argv = gpg_verify_argv(DEFAULT_GPG_VERIFY_BASE_COMMAND, "/tmp/h", "--output - -");
        assert!(argv.ends_with(&["--output".to_string(), "-".to_string(), "-".to_string()]));
    }

    #[test]
    fn verify_gpkg_manifest_accepts_a_real_signed_container() {
        let home = test_gpg_home("signed-ok");
        let g = fixture("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar");
        // The committed fixture was signed by real, unmodified
        // `bin/gpkg-helper.py compress` with the trusted test key: the
        // Manifest clear-sign + both detached `.sig` sidecars verify.
        verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true))
            .expect("signed fixture verifies");
        // `binpkg-request-signature` on a fully-signed container.
        verify_gpkg_manifest(&g, &test_gpg_verify(&home, true, true))
            .expect("signed fixture verifies under request-signature");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn verify_gpkg_manifest_skips_gpg_entirely_under_ignore_signature() {
        // `binpkg-ignore-signature`: even a garbage `gpg` binary path
        // succeeds -- proving no subprocess runs at all (the cleartext
        // `DATA` body is read straight through the armor, the standing
        // unsigned-container behaviour).
        let gpg = GpgVerify {
            verify_signature: false,
            request_signature: false,
            base_command: "/does/not/exist-gpg".to_string(),
            gpg_home: "/does/not/exist".to_string(),
        };
        verify_gpkg_manifest(&fixture("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar"), &gpg)
            .expect("ignore-signature never invokes gpg");
    }

    #[test]
    fn verify_gpkg_manifest_rejects_an_unsigned_container_under_request_signature() {
        let home = test_gpg_home("unsigned-request");
        // The unsigned fixture has no `.sig` members and a plain
        // Manifest: real `_verify_binpkg` tries the clear-sign verify
        // first and reports the NODATA cause (real `show_gpg_error`'s
        // own "binpkg appears unsigned" summary).
        let err = verify_gpkg_manifest(
            &fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
            &test_gpg_verify(&home, true, true),
        )
        .unwrap_err();
        assert!(err.contains("GnuPG verification failed"), "{err}");
        assert!(err.contains("binpkg appears unsigned"), "{err}");
        let _ = fs::remove_dir_all(&home);
    }

    /// Copy a `.gpkg.tar` fixture to a tempdir, unpack its outer tar,
    /// let `mutate` alter the unpacked `<prefix>/` tree, and repack --
    /// for tamper tests that must keep a parseable container (flipping
    /// bytes in the packed tar directly would just corrupt the tar
    /// framing instead of failing the signature).
    fn repack_gpkg_with_mutation(src: &Path, tag: &str, mutate: impl Fn(&Path)) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "portuale-gpg-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let outer = root.join("outer");
        fs::create_dir_all(&outer).unwrap();
        run_tar(&["-xf", &lossy(src), "-C", &lossy(&outer)]).unwrap();
        mutate(&outer);
        let mut argv: Vec<String> = vec![
            "-cf".into(),
            lossy(&root.join("out.gpkg.tar")),
            "-C".into(),
            lossy(&outer),
        ];
        for entry in read_dir_sorted(&outer).unwrap() {
            argv.push(entry.file_name().unwrap().to_string_lossy().into_owned());
        }
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        run_tar(&refs).unwrap();
        root.join("out.gpkg.tar")
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_tampered_signed_member() {
        let home = test_gpg_home("tampered");
        // Flip a byte of the signed `image.tar.gz`: the detached `.sig`
        // no longer matches, before digests are even compared.
        let g = repack_gpkg_with_mutation(
            &fixture("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar"),
            "tampered",
            |outer| {
                let member = outer.join("gpgsignedpkg-1.0/image.tar.gz");
                let mut bytes = fs::read(&member).unwrap();
                let mid = bytes.len() / 2;
                bytes[mid] ^= 0xff;
                fs::write(&member, bytes).unwrap();
            },
        );
        let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
        assert!(err.contains("GnuPG verification failed"), "{err}");
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(g.parent().unwrap());
    }

    /// Build a signed container around a fixed payload whose Manifest
    /// is clear-signed with `manifest_key`, and whose payload `.sig`
    /// sidecar is present only when `with_sidecar` (signed with
    /// `member_key`, defaulting to the manifest key).
    fn build_signed_gpkg(
        home: &Path,
        manifest_key: &str,
        with_sidecar: bool,
        member_key: Option<&str>,
    ) -> PathBuf {
        let payload: &[u8] = b"signed payload bytes";
        let manifest = format!(
            "{}{}",
            data_line("gpkg-1", b""),
            data_line("payload.bin", payload)
        );
        let signed_manifest = gpg_sign(home, manifest_key, manifest.as_bytes(), true);
        let mut members: Vec<(&str, Vec<u8>)> =
            vec![("gpkg-1", b"".to_vec()), ("payload.bin", payload.to_vec())];
        if with_sidecar {
            let sig = gpg_sign(home, member_key.unwrap_or(manifest_key), payload, false);
            members.push(("payload.bin.sig", sig));
        }
        let scratch = ScratchDir::new("gpkg-signed-build").unwrap();
        let root = scratch.path().to_path_buf();
        std::mem::forget(scratch);
        let prefix = "sigtest-1.0";
        let pkgdir = root.join(prefix);
        fs::create_dir_all(&pkgdir).unwrap();
        let mut argv: Vec<String> = vec![
            "-cf".into(),
            lossy(&root.join("out.gpkg.tar")),
            "-C".into(),
            lossy(&root),
        ];
        let member_refs: Vec<(String, Vec<u8>)> = members
            .into_iter()
            .map(|(n, b)| (format!("{prefix}/{n}"), b))
            .collect();
        for (name, bytes) in &member_refs {
            fs::write(root.join(name), bytes).unwrap();
            argv.push(name.clone());
        }
        fs::write(pkgdir.join("Manifest"), &signed_manifest).unwrap();
        argv.push(format!("{prefix}/Manifest"));
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        run_tar(&refs).unwrap();
        root.join("out.gpkg.tar")
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_member_without_its_sig_sidecar() {
        let home = test_gpg_home("missing-sidecar");
        // Validly clear-signed Manifest, but no `payload.bin.sig`:
        // real `MissingSignature` (`gpkg.py:1783-1786`).
        let g = build_signed_gpkg(&home, GPG_TRUSTED_KEY, false, None);
        let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
        assert!(err.contains("signature not found"), "{err}");
        kill_gpg_agent(&home);
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(g.parent().unwrap());
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_signature_from_an_untrusted_key() {
        let home = test_gpg_home("untrusted");
        // Signed end-to-end with the committed *untrusted* test key:
        // the cryptography is valid, but real `_check_gpg_status`
        // demands `TRUST_ULTIMATE`/`TRUST_FULLY`, so this is real's own
        // "signed with a known key of undefined trust" failure.
        let g = build_signed_gpkg(&home, GPG_UNTRUSTED_KEY, true, None);
        let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
        assert!(err.contains("GnuPG verification failed"), "{err}");
        assert!(err.contains("undefined trust"), "{err}");
        kill_gpg_agent(&home);
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(g.parent().unwrap());
    }

    #[test]
    fn verify_gpkg_manifest_rejects_a_sidecar_from_an_untrusted_key() {
        let home = test_gpg_home("untrusted-sidecar");
        // Manifest signed by the trusted key, member sidecar by the
        // untrusted one -- the Manifest passes, the member fails.
        let g = build_signed_gpkg(&home, GPG_TRUSTED_KEY, true, Some(GPG_UNTRUSTED_KEY));
        let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
        assert!(err.contains("GnuPG verification failed"), "{err}");
        assert!(err.contains("undefined trust"), "{err}");
        kill_gpg_agent(&home);
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(g.parent().unwrap());
    }

    #[test]
    fn populate_local_pkgdir_trusts_an_unchanged_index_entry_and_revalidates_a_stale_one() {
        // A stale `Packages` entry (wrong SIZE/_mtime_, real `bintree.
        // py:1108-1136`'s own "avoid reading the xpak if possible" fast
        // path failing its own check) is dropped in favor of the file's
        // own real embedded metadata -- proven here by a bogus SLOT the
        // index claims that the real `.tbz2` itself does not carry.
        let scratch = ScratchDir::new("mtime-staleness").unwrap();
        let pkgdir = scratch.path();
        let cat_dir = pkgdir.join("dev-libs");
        fs::create_dir_all(&cat_dir).unwrap();
        let real_tbz2 = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
        let dest = cat_dir.join("packagepkg-1.0.tbz2");
        fs::copy(&real_tbz2, &dest).unwrap();
        let st = fs::metadata(&dest).unwrap();
        let real_size = st.len();
        let real_mtime = file_mtime(&st);

        // A trustworthy entry (matching SIZE/_mtime_) is reused verbatim,
        // even though its SLOT is fabricated -- proving the fast path
        // really does skip re-parsing the file.
        let trusted = format!(
            "CPV: dev-libs/packagepkg-1.0\nSLOT: 99-not-the-real-slot\nSIZE: {real_size}\n_mtime_: {real_mtime}\nPATH: dev-libs/packagepkg-1.0.tbz2\n"
        );
        fs::write(
            pkgdir.join("Packages"),
            format!("TIMESTAMP: 0\n\n{trusted}"),
        )
        .unwrap();
        let entries = populate_local_pkgdir(pkgdir).expect("scan succeeds");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("SLOT").map(String::as_str),
            Some("99-not-the-real-slot"),
            "an unchanged index entry must be trusted, not re-derived"
        );

        // The same claim, but with a wrong _mtime_ -- now stale, so the
        // real SLOT ("0") is re-derived from the file itself instead.
        let stale = format!(
            "CPV: dev-libs/packagepkg-1.0\nSLOT: 99-not-the-real-slot\nSIZE: {real_size}\n_mtime_: {}\nPATH: dev-libs/packagepkg-1.0.tbz2\n",
            real_mtime + 1000
        );
        fs::write(pkgdir.join("Packages"), format!("TIMESTAMP: 0\n\n{stale}")).unwrap();
        let entries = populate_local_pkgdir(pkgdir).expect("scan succeeds");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("SLOT").map(String::as_str),
            Some("0"),
            "a stale index entry must be re-derived from the real file"
        );
    }
}
