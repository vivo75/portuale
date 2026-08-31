// The `$PKGDIR` directory-scan fallback -- real `bintree._populate_local`
// (see `PORTING/PROMPT-next.md`). Every other binary-package path in this
// pilot is `<pkgdir>/Packages`-index driven and format-agnostic, so a
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
//   - `scan_pkgdir`: walks `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}` and
//     synthesizes one `Packages`-style entry per file. Its output
//     becomes `portage_profile::Config::scanned_binpkgs` (NOT written
//     back to `Packages` -- this pilot recomputes each run, so
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
/// **v1 cuts, documented** (matching this pilot's own `Packages`-index
/// reader, which "trusts the index outright" -- real
/// `FEATURES=pkgdir-index-trusted`): NO `Manifest` digest verification
/// and NO GPG `.sig` signature check (real `gpkg._verify_binpkg`). Those
/// are gpkg's whole reason to exist, but this pilot has no crypto
/// anywhere and its `--pretend` binary path has never verified a
/// binpkg's integrity -- a real, separately-scoped follow-up. The
/// `gpkg-1` version marker's *presence* is still required (real
/// `_get_inner_tarinfo`'s own `InvalidBinaryPackageFormat` guard).
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
/// portage keeps both in the vdb, and the pilot now needs them: the
/// binpkg merge runs real `pkg_preinst`/`pkg_postinst` by `bunzip2`'ing
/// `environment.bz2` into `${T}/environment` (real `BinpkgEnvExtractor`
/// -> `bin/ebuild.sh`'s own saved-env source path).
pub fn extract_binpkg(
    binpkg_path: &Path,
    image_dest: &Path,
    build_info_dest: &Path,
) -> Result<(), String> {
    fs::create_dir_all(image_dest).map_err(|e| format!("{}: {e}", image_dest.display()))?;
    fs::create_dir_all(build_info_dest)
        .map_err(|e| format!("{}: {e}", build_info_dest.display()))?;

    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let is_gpkg = name.ends_with(".gpkg.tar");
    let metadata = if is_gpkg {
        extract_gpkg_member(binpkg_path, "image", image_dest)?;
        read_gpkg_metadata(binpkg_path)?
    } else {
        extract_xpak_image(binpkg_path, image_dest)?;
        read_xpak_metadata(binpkg_path)?
    };

    let mut non_scalar: Vec<String> = Vec::new();
    for (key, value) in &metadata {
        if key == "environment.bz2" || key.ends_with(".ebuild") {
            non_scalar.push(key.clone());
            continue;
        }
        let dest = build_info_dest.join(key);
        fs::write(&dest, format!("{}\n", value.trim()))
            .map_err(|e| format!("{}: {e}", dest.display()))?;
    }

    // The raw bytes of the two non-scalar members (verbatim, no trim / no
    // lossy UTF-8 round-trip). gpkg carries them as real files inside the
    // `metadata.tar`; xpak needs a targeted segment read.
    if !non_scalar.is_empty() {
        if is_gpkg {
            let md = ScratchDir::new("gpkg-nonscalar")?;
            extract_gpkg_member(binpkg_path, "metadata", md.path())?;
            for key in &non_scalar {
                let src = md.path().join("metadata").join(key);
                if src.is_file() {
                    fs::copy(&src, build_info_dest.join(key))
                        .map_err(|e| format!("{}: {e}", src.display()))?;
                }
            }
        } else {
            for key in &non_scalar {
                if let Some(bytes) = read_xpak_member_raw(binpkg_path, key)? {
                    fs::write(build_info_dest.join(key), bytes)
                        .map_err(|e| format!("{}: {e}", build_info_dest.join(key).display()))?;
                }
            }
        }
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
    run_tar(&["-xpf", &lossy(&inner_tar), "-C", &lossy(dest)])
}

/// Real `bintree._populate_local`, narrowed: walk `pkgdir` for binpkg
/// *files* and synthesize one `Packages`-style entry per file from its
/// own embedded metadata (`read_xpak_metadata` / `read_gpkg_metadata`).
/// The caller only runs this when `<pkgdir>/Packages` is absent (see
/// `pretend.rs`).
///
/// `$PKGDIR` layout is `<pkgdir>/<category>/<pf>.{tbz2,gpkg.tar}` (one
/// level deep). `CPV` is derived from the path (`<category>/<pf>` --
/// authoritative for a `$PKGDIR`), `SIZE` from the file's own byte size
/// (real `bintree`'s own `st_size`), `REPO` from the embedded
/// `repository`, `PATH` from the relative path. Entries are `CPV`-sorted
/// for a deterministic pool order.
///
/// v1 cuts: bare `.xpak` files (real `binpkg-multi-instance`
/// `<pkgdir>/<cat>/<pf>/<build_id>.xpak`) are skipped -- this pilot has
/// no multi-instance concept and a bare `.xpak` is a different on-disk
/// shape (just the segment, no `[tarball]…STOP` trailer); real
/// portage's own mtime-based `Packages` staleness revalidation is not
/// done (a present index is always trusted, this pilot's long-standing
/// stance); a file that fails to parse aborts the scan (rather than
/// real portage's own skip-and-warn) -- a `$PKGDIR` full of unreadable
/// binpkgs is a real problem worth surfacing, not silently resolving
/// against a partial pool.
pub fn scan_pkgdir(pkgdir: &Path) -> Result<Vec<HashMap<String, String>>, String> {
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
        for file in read_dir_sorted(&cat_path)? {
            let Some(name) = file.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let (pf, mut meta) = if let Some(pf) = name.strip_suffix(".gpkg.tar") {
                (pf.to_string(), read_gpkg_metadata(&file)?)
            } else if let Some(pf) = name.strip_suffix(".tbz2") {
                (pf.to_string(), read_xpak_metadata(&file)?)
            } else {
                continue;
            };
            meta.insert("CPV".to_string(), format!("{category}/{pf}"));
            meta.entry("CATEGORY".to_string())
                .or_insert_with(|| category.to_string());
            meta.entry("PF".to_string()).or_insert_with(|| pf.clone());
            if let Some(repo) = meta.remove("repository") {
                meta.entry("REPO".to_string()).or_insert(repo);
            }
            if let Ok(st) = fs::metadata(&file) {
                meta.insert("SIZE".to_string(), st.len().to_string());
            }
            meta.insert("PATH".to_string(), format!("{category}/{name}"));
            out.push(meta);
        }
    }
    out.sort_by(|a, b| a.get("CPV").cmp(&b.get("CPV")));
    Ok(out)
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

    /// Build a real XPAK segment (real `xpak.xpak_mem` layout) and append
    /// it to some prefix bytes, exactly the way a real `.tbz2` is
    /// `[tarball][XPAK trailer]`.
    fn make_xpak_binpkg(prefix: &[u8], entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut index = Vec::new();
        let mut data = Vec::new();
        for (name, value) in entries {
            index.extend_from_slice(&(name.len() as u32).to_be_bytes());
            index.extend_from_slice(name.as_bytes());
            index.extend_from_slice(&(data.len() as u32).to_be_bytes());
            index.extend_from_slice(&(value.len() as u32).to_be_bytes());
            data.extend_from_slice(value);
        }
        let mut segment = Vec::new();
        segment.extend_from_slice(b"XPAKPACK");
        segment.extend_from_slice(&(index.len() as u32).to_be_bytes());
        segment.extend_from_slice(&(data.len() as u32).to_be_bytes());
        segment.extend_from_slice(&index);
        segment.extend_from_slice(&data);
        segment.extend_from_slice(b"XPAKSTOP");

        let mut out = prefix.to_vec();
        out.extend_from_slice(&segment);
        out.extend_from_slice(&(segment.len() as u32).to_be_bytes());
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
    /// pilot's own `ebuild <file> package` on `dev-libs/packagepkg`
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
        assert!(m
            .get("packagepkg-1.0.ebuild")
            .is_some_and(|e| e.contains("EAPI=8")));
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
        extract_binpkg(&fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"), &image, &bi)
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
        assert!(fs::read_to_string(bi.join("packagepkg-1.0.ebuild"))
            .unwrap()
            .contains("EAPI=8"));
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
        )
        .expect("gpkg extract succeeds");
        // The gpkg fixture's image is non-empty and its metadata carries
        // the same scalar keys as any binpkg.
        assert!(image.is_dir());
        assert!(bi.join("SLOT").is_file());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_pkgdir_synthesizes_a_packages_style_entry_per_binpkg_file() {
        // fixtures/pkgdir/dev-libs/ holds both a real `.tbz2` and a real
        // `.gpkg.tar` (the increment-1/2 fixtures). `scan_pkgdir` itself
        // doesn't care whether a `Packages` file is also present -- the
        // caller (pretend.rs) makes that decision.
        let entries = scan_pkgdir(&fixture("pkgdir")).expect("scan succeeds");
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
    fn scan_pkgdir_of_a_missing_or_empty_dir_is_empty() {
        assert!(scan_pkgdir(Path::new("/nonexistent/pkgdir"))
            .unwrap()
            .is_empty());
        let scratch = ScratchDir::new("scan-empty").unwrap();
        assert!(scan_pkgdir(scratch.path()).unwrap().is_empty());
    }
}
