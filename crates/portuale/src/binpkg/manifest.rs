//! Verifying a gpkg Manifest.

use crate::binpkg::*;

/// `read_member_bytes` with a zip-bomb cap: for the inner metadata member,
/// which the decompressed-size cap ([`MAX_INNER_METADATA_BYTES`]) would
/// otherwise not bound while compressed.
pub(crate) fn read_member_bytes_capped(
    mut reader: impl Read,
    cap: u64,
    label: &str,
) -> Result<Vec<u8>, BinpkgError> {
    let mut bytes = Vec::new();
    // A truncated/corrupt member fails the Manifest size+digest check first.
    reader
        .by_ref()
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| BinpkgError::Invalid(format!("{label}: {e}")))?;
    if bytes.len() as u64 > cap {
        return Err(BinpkgError::Invalid(format!(
            "{label}: larger than {cap} bytes"
        )));
    }
    Ok(bytes)
}
