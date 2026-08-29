//! Reading gpkg and xpak metadata.

use crate::binpkg::*;

/// Read the `metadata/<KEY>` map from a `.gpkg.tar`: a plain tar whose
/// members are `<basename>/{gpkg-1, metadata.tar[.<comp>],
/// image.tar[.<comp>], Manifest}` (+ optional `.sig` files). Each value is
/// UTF-8 with surrounding whitespace trimmed; a member whose bytes aren't
/// valid UTF-8 (`environment.bz2`) is skipped, not an error.
///
/// This is the "populate the pool" path: it trusts the container the same
/// way the `Packages`-index reader trusts the index. The *merge* path
/// runs the `Manifest` digest check first (see [`verify_gpkg_manifest`]).
/// Still required here: the `gpkg-1` version marker's *presence*, and that
/// every trusted member name is a **regular member** (a symlinked
/// marker/metadata/`Manifest` is an error, never a follow).
///
/// **Deliberate cut**: no GPG `.sig` check here; the merge enforces the
/// signature policy via the system `gpg` (see [`GpgVerify`).
// Portage: `gpkg.get_metadata`, `gpkg.unpack_metadata`,
// `gpkg._strip_metadata_prefix`, `tarfile.extractfile`.
pub fn read_gpkg_metadata(gpkg_path: &Path) -> Result<HashMap<String, String>, BinpkgError> {
    if !gpkg_path.is_file() {
        // A missing file is `Invalid`, not `Fatal`.
        return Err(BinpkgError::Invalid(format!(
            "{}: not a file",
            gpkg_path.display()
        )));
    }

    // Read the outer container from its tar headers: marker, one-level
    // structure and regular-member checks, before anything is written or
    // decompressed.
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

    // Read the inner `metadata.tar` in process: symlinks resolve inside
    // the archive only, so no host path is ever read.
    let metadata = outer_member_read(gpkg_path, &metadata_member.raw_name, |entry| {
        let member_bytes =
            read_member_bytes_capped(entry, MAX_INNER_METADATA_BYTES, &metadata_member.name)?;
        read_inner_metadata(gpkg_path, &metadata_member.name, &member_bytes, comp)
    })?;

    // UTF-8 + trim; non-UTF-8 members are skipped, not an error.
    let mut out = HashMap::new();
    for entry in metadata.entries {
        let Ok(text) = String::from_utf8(entry.bytes) else {
            continue; // e.g. environment.bz2 -- not a scalar value
        };
        out.insert(entry.key, text.trim().to_string());
    }
    Ok(out)
}

/// The `"XPAKPACK" … "XPAKSTOP"` segment bytes: read the last 16 bytes
/// (`"XPAKSTOP" be32(infosize) "STOP"`), then the `infosize + 8` byte
/// segment they point back to.
pub(crate) fn read_xpak_segment(binpkg_path: &Path) -> Result<Vec<u8>, BinpkgError> {
    use std::io::{Read, Seek, SeekFrom};

    // Every xpak read error is Fatal: Portage swallows the scan failure
    // and then raises on `None`, so it aborts, never skips, a broken file.
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

/// Walk an xpak segment's index into `(name, &data bytes)` members,
/// borrowing from `seg`.
// Portage: `portage.xpak.getindex_mem`, `searchindex`.
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
