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
