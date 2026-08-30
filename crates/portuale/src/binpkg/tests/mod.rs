use super::*;

use portage_util::TempDir;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// `usize -> u32` narrowing that panics instead of silently wrapping;
/// the fixture-making sizes are far below `u32::MAX`.
fn xpak_u32(len: usize) -> u32 {
    u32::try_from(len).expect("xpak length exceeds u32::MAX")
}

/// Build an XPAK segment and append it to prefix bytes, the way a
/// `.tbz2` is `[tarball][XPAK trailer]`.
// Portage: xpak.py, xpak_mem
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

// The in-process inner metadata.tar reader.

/// One inner-tar member for the unit tests, written through `tar::Builder`.
struct InnerTestEntry<'a> {
    name: &'a str,
    entry_type: tar::EntryType,
    link_name: &'a str,
    data: &'a [u8],
}

fn inner_entry<'a>(name: &'a str, data: &'a [u8]) -> InnerTestEntry<'a> {
    InnerTestEntry {
        name,
        entry_type: tar::EntryType::Regular,
        link_name: "",
        data,
    }
}
mod extract;
mod inner;
mod metadata;
