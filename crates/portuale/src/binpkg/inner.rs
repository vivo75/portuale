//! The gpkg inner metadata tar: limits, members, links.

use crate::binpkg::*;

/// The `tar` entry types `tarfile` lists in `SUPPORTED_TYPES`; any other
/// byte is treated as a regular file, and must be here too.
pub(crate) fn known_inner_entry_type(entry_type: tar::EntryType) -> bool {
    matches!(
        entry_type.as_byte(),
        b'\0' | b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'L' | b'K' | b'S'
    )
}

/// The type phrase for a member that cannot yield bytes, for the error
/// message.
pub(crate) fn inner_entry_type_phrase(entry_type: tar::EntryType) -> &'static str {
    match entry_type.as_byte() {
        b'3' => "a character device",
        b'4' => "a block device",
        b'5' => "a directory",
        b'6' => "a FIFO",
        b'1' => "a hard link",
        b'2' => "a symbolic link",
        _ => "not a regular file",
    }
}

/// `normpath` for the archive-relative names link targets are compared
/// under: collapses `//`, drops `.`, resolves `..` lexically, keeps a
/// leading `/` (so an absolute target never matches a relative member).
// Portage: `os.path.normpath`, `gpkg._find_link_target`.
pub(crate) fn normpath(name: &str) -> String {
    let absolute = name.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for component in name.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// One inner `metadata.tar` member: its header fields plus bytes, read
/// before anything is written anywhere.
pub(crate) struct InnerMember {
    /// The raw member name with a trailing `/` stripped.
    pub(crate) name: Vec<u8>,
    /// `name` with the `metadata/` prefix stripped, lossy-decoded.
    pub(crate) key: String,
    pub(crate) entry_type: tar::EntryType,
    /// The member's own header mode.
    pub(crate) mode: u32,
    pub(crate) link_name: Option<Vec<u8>>,
    /// Present only for regular/unknown types; a link resolves to its
    /// target's bytes instead.
    pub(crate) data: Option<Vec<u8>>,
}

/// One resolved inner-metadata member: the key, the bytes (the target's
/// own bytes for a link), and the member's header mode.
#[derive(Debug)]
pub(crate) struct InnerEntry {
    pub(crate) key: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) mode: u32,
}
