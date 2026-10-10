//! Reading gpkg and xpak metadata.

use crate::binpkg::*;

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
/// `InvalidBinaryPackageFormat` guard) and, since `#56`, that every
/// trusted member name is a **regular member** ([`read_outer_members`],
/// since `#58` S5 from the tar header instead of an extracted
/// `symlink_metadata`): a symlinked marker/metadata/`Manifest` member is
/// an error naming the member and its type, never a follow (see
/// `TEST/findings/l2.md` "## #56 S0" -- before this, this path listed
/// the host `/etc` through a symlinked prefix and read a host file
/// through a symlinked `Manifest`). Since `#58`, the inner
/// `metadata.tar` is read in process with the `tar` crate
/// ([`read_inner_metadata`]), porting real `tarfile.extractfile`: a
/// symlink/hardlink resolves **inside the archive** (or errors), and a
/// directory/FIFO/device member or a name outside `metadata/` is an
/// error -- before this, the extracted tree was walked with
/// `Path::is_file`/`fs::read`, so an inner
/// `metadata/DESCRIPTION -> /etc/hostname` symlink returned the host
/// file's bytes (see `TEST/findings/l2.md` "## #58 S0").
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
pub fn read_gpkg_metadata(gpkg_path: &Path) -> Result<HashMap<String, String>, BinpkgError> {
    if !gpkg_path.is_file() {
        // real `_verify_binpkg`'s FileNotFound (PortagePackageException).
        return Err(BinpkgError::Invalid(format!(
            "{}: not a file",
            gpkg_path.display()
        )));
    }

    // 1. Read the outer container in process (`#58` K7/S5): the `gpkg-1`
    //    validity marker, the one-level structure and `#56`'s regular-
    //    member check all come from the tar headers, before anything is
    //    written or decompressed.
    let (members, gpkg_marker) = read_outer_members(gpkg_path)?;
    if !gpkg_marker {
        return Err(BinpkgError::Invalid(format!(
            "{}: not a gpkg container (no `gpkg-1` version marker)",
            gpkg_path.display()
        )));
    }
    let (metadata_member, comp) = members
        .iter()
        .find_map(|member| {
            classify_inner_member("metadata", &member.name).map(|comp| (member, comp))
        })
        .ok_or_else(|| {
            BinpkgError::Invalid(format!(
                "{}: no `metadata.tar` member in the gpkg",
                gpkg_path.display()
            ))
        })?;

    // 2. Read the inner `metadata.tar` in process (`#58` S1/K0): the
    //    member's stream goes straight into the `tar` crate (through the
    //    decompressor where needed) and every entry is resolved exactly
    //    like real `tarfile.extractfile`. Nothing is written to disk
    //    before it is accepted, so a symlink can only ever yield another
    //    member's bytes and no host path is ever read.
    let metadata = outer_member_read(gpkg_path, &metadata_member.raw_name, |entry| {
        let member_bytes =
            read_member_bytes_capped(entry, MAX_INNER_METADATA_BYTES, &metadata_member.name)?;
        read_inner_metadata(gpkg_path, &metadata_member.name, &member_bytes, comp)
    })?;

    // 3. The existing value shaping: UTF-8 + trim; a member whose bytes
    //    aren't valid UTF-8 (e.g. `environment.bz2`) is skipped, not an
    //    error.
    let mut out = HashMap::new();
    for entry in metadata.entries {
        let Ok(text) = String::from_utf8(entry.bytes) else {
            continue; // e.g. environment.bz2 -- not a scalar value
        };
        out.insert(entry.key, text.trim().to_string());
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
pub fn read_xpak_metadata(binpkg_path: &Path) -> Result<HashMap<String, String>, BinpkgError> {
    let seg = read_xpak_segment(binpkg_path)?;
    Ok(parse_xpak_members(&seg)?
        .into_iter()
        .map(|(key, bytes)| (key, String::from_utf8_lossy(bytes).trim().to_string()))
        .collect())
}

/// The `"XPAKPACK" … "XPAKSTOP"` segment bytes (real `tbz2.scan`): read
/// the last 16 bytes (`"XPAKSTOP" be32(infosize) "STOP"`), then the
/// `infosize + 8` byte segment they point back to. Only this bounded
/// tail of the file is ever touched.
pub(crate) fn read_xpak_segment(binpkg_path: &Path) -> Result<Vec<u8>, BinpkgError> {
    use std::io::{Read, Seek, SeekFrom};

    // Every xpak read error is Fatal: real's `tbz2.scan()` swallows the
    // failure and returns falsy, `get_data()` then returns None, and the
    // caller raises AttributeError on `None.get` -- real *aborts*, it
    // never skips a broken `.tbz2`/`.xpak`.
    let fatal = |e: String| BinpkgError::Fatal(e);
    let mut f = fs::File::open(binpkg_path)
        .map_err(|e| fatal(format!("{}: {e}", binpkg_path.display())))?;
    let file_len = f
        .seek(SeekFrom::End(0))
        .map_err(|e| fatal(format!("{}: {e}", binpkg_path.display())))?;
    if file_len < 16 {
        return Err(fatal(format!(
            "{}: too small to be an xpak binpkg",
            binpkg_path.display()
        )));
    }

    let mut trailer = [0u8; 16];
    f.seek(SeekFrom::End(-16))
        .and_then(|_| f.read_exact(&mut trailer))
        .map_err(|e| fatal(format!("{}: {e}", binpkg_path.display())))?;
    if &trailer[12..16] != b"STOP" || &trailer[0..8] != b"XPAKSTOP" {
        return Err(fatal(format!(
            "{}: not an xpak binary package (no XPAKSTOP trailer)",
            binpkg_path.display()
        )));
    }
    let infosize = be32(&trailer[8..12]) as u64;
    let xpaksize = infosize + 8;
    if xpaksize > file_len {
        return Err(fatal(format!(
            "{}: xpak trailer size exceeds the file",
            binpkg_path.display()
        )));
    }

    let mut seg = vec![0u8; xpaksize as usize];
    f.seek(SeekFrom::End(-(xpaksize as i64)))
        .and_then(|_| f.read_exact(&mut seg))
        .map_err(|e| fatal(format!("{}: {e}", binpkg_path.display())))?;
    Ok(seg)
}

/// Walk an xpak segment's index (real `getindex_mem`/`searchindex`:
/// `"XPAKPACK" be32(indexsize) be32(datasize) <index> <data>`, then
/// `while startpos + 8 < len` over `be32(namelen) name be32(datapos)
/// be32(datalen)` records into `<data>`). Returns every member as
/// `(name, &data bytes)`, borrowing from `seg`.
pub(crate) fn parse_xpak_members(seg: &[u8]) -> Result<Vec<(String, &[u8])>, BinpkgError> {
    if seg.len() < 16 || &seg[0..8] != b"XPAKPACK" {
        return Err(BinpkgError::Fatal(
            "not an xpak binary package (no XPAKPACK header)".to_string(),
        ));
    }
    let indexsize = be32(&seg[8..12]) as usize;
    let datasize = be32(&seg[12..16]) as usize;
    let index_start = 16;
    let data_start = index_start + indexsize;
    if data_start + datasize > seg.len() {
        return Err(BinpkgError::Fatal(
            "xpak index/data segments overrun the file".to_string(),
        ));
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

pub(crate) fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
