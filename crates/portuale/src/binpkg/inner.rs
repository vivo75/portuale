//! The gpkg inner metadata tar: limits, members, links.

use crate::binpkg::*;

/// Bound on the total inner-member bytes: Portage reads the whole tar into
/// memory too, and 64 MiB is far above any real `metadata.tar` while still
/// bounding a decompression bomb.
pub(crate) const MAX_INNER_METADATA_BYTES: u64 = 64 * 1024 * 1024;

/// A tar of millions of empty members is the same allocation attack with
/// no bytes to count.
pub(crate) const MAX_INNER_METADATA_ENTRIES: usize = 100_000;

/// Link-following stops at a fixed depth with an error naming the member
/// (Portage follows links until its recursion limit instead).
// Portage: `tarfile.extractfile`.
pub(crate) const MAX_INNER_LINK_DEPTH: usize = 40;

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

/// Collect every inner member in archive order, validating its name and
/// reading bytes only for regular/unknown types. A name outside
/// `metadata/`, an absolute name, or any `..` component is an error; the
/// zip-bomb guards cap the member count and the total bytes.
// Portage: `gpkg._strip_metadata_prefix`, `tar_safe_extract`.
pub(crate) fn collect_inner_members<R: Read>(
    gpkg: &Path,
    mut reader: R,
) -> Result<Vec<InnerMember>, BinpkgError> {
    let mut members: Vec<InnerMember> = Vec::new();
    let mut total_bytes: u64 = 0;
    {
        let mut archive = tar::Archive::new(&mut reader);
        // An inner tar that will not parse is Fatal.
        let entries = archive.entries().map_err(|e| {
            BinpkgError::Fatal(format!(
                "{}: reading inner metadata.tar: {e}",
                gpkg.display()
            ))
        })?;
        for entry in entries {
            let mut entry = entry.map_err(|e| {
                BinpkgError::Fatal(format!(
                    "{}: reading inner metadata.tar: {e}",
                    gpkg.display()
                ))
            })?;
            if members.len() >= MAX_INNER_METADATA_ENTRIES {
                return Err(BinpkgError::Invalid(format!(
                    "{}: inner metadata.tar has more than {MAX_INNER_METADATA_ENTRIES} members",
                    gpkg.display()
                )));
            }
            let raw = entry.path_bytes();
            let mut name = raw.to_vec();
            while name.last() == Some(&b'/') {
                name.pop();
            }
            let shown = String::from_utf8_lossy(&name);
            if name.starts_with(b"/")
                || name
                    .split(|byte| *byte == b'/')
                    .any(|component| component == b"..")
            {
                return Err(BinpkgError::Invalid(format!(
                    "{}: inner metadata member {shown:?} has an unsafe name",
                    gpkg.display()
                )));
            }
            let Some(key_bytes) = name.strip_prefix(b"metadata/") else {
                return Err(BinpkgError::Invalid(format!(
                    "{}: inner metadata member {shown:?} is outside metadata/",
                    gpkg.display()
                )));
            };
            let key = String::from_utf8_lossy(key_bytes).into_owned();
            let entry_type = entry.header().entry_type();
            let mode = entry.header().mode().unwrap_or(0o644) & 0o7777;
            let link_name = entry.link_name_bytes().map(|bytes| bytes.to_vec());
            let data = if entry_type.is_file()
                || entry_type.is_contiguous()
                || !known_inner_entry_type(entry_type)
            {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| {
                    BinpkgError::Fatal(format!(
                        "{}: reading inner metadata member {shown:?}: {e}",
                        gpkg.display()
                    ))
                })?;
                total_bytes = total_bytes.saturating_add(bytes.len() as u64);
                if total_bytes > MAX_INNER_METADATA_BYTES {
                    return Err(BinpkgError::Invalid(format!(
                        "{}: inner metadata.tar is larger than {MAX_INNER_METADATA_BYTES} bytes",
                        gpkg.display()
                    )));
                }
                Some(bytes)
            } else {
                None
            };
            members.push(InnerMember {
                name,
                key,
                entry_type,
                mode,
                link_name,
                data,
            });
        }
    }
    // The `tar` crate stops at the end-of-archive marker without reading
    // the trailing padding. When the reader is a decompressor's stdout
    // pipe, dropping it now would send SIGPIPE; drain to EOF instead.
    // A drain failure is `Invalid` (the skip class).
    std::io::copy(&mut reader, &mut std::io::sink()).map_err(|e| {
        BinpkgError::Invalid(format!(
            "{}: reading inner metadata.tar: {e}",
            gpkg.display()
        ))
    })?;
    Ok(members)
}

/// Resolve one member to bytes: a regular/unknown type yields its own
/// bytes; a symlink resolves `dirname(name)/linkname` normalized over the
/// whole archive (last match wins); a hardlink resolves `linkname` among
/// earlier members only; both recurse with a depth cap. A
/// directory/FIFO/device is an error naming the member and its type; an
/// unresolved link is an error naming the target. A link can therefore
/// only ever yield **another member's** bytes.
// Portage: `tarfile.extractfile`.
pub(crate) fn resolve_inner_member(
    gpkg: &Path,
    members: &[InnerMember],
    index: usize,
    depth: usize,
) -> Result<Vec<u8>, BinpkgError> {
    let member = &members[index];
    let shown = String::from_utf8_lossy(&member.name);
    let entry_type = member.entry_type;
    if entry_type.is_file() || entry_type.is_contiguous() || !known_inner_entry_type(entry_type) {
        return Ok(member.data.clone().unwrap_or_default());
    }
    // A link cycle is Fatal.
    if depth > MAX_INNER_LINK_DEPTH {
        return Err(BinpkgError::Fatal(format!(
            "{}: inner metadata member {shown:?} link chain is deeper than {MAX_INNER_LINK_DEPTH}",
            gpkg.display()
        )));
    }
    let link = member.link_name.clone().unwrap_or_default();
    let shown_link = String::from_utf8_lossy(&link);
    let (target, search_len) = if entry_type.is_symlink() {
        let name = String::from_utf8_lossy(&member.name);
        let dirname = name.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let joined = if dirname.is_empty() {
            shown_link.to_string()
        } else {
            format!("{dirname}/{shown_link}")
        };
        // The whole archive, last match wins.
        (normpath(&joined), members.len())
    } else if entry_type.is_hard_link() {
        // Only members *before* the link can be its target.
        (normpath(&shown_link), index)
    } else {
        // A directory/FIFO/device yields no bytes (Fatal).
        return Err(BinpkgError::Fatal(format!(
            "{}: inner metadata member {shown:?} is {}, not a regular file",
            gpkg.display(),
            inner_entry_type_phrase(entry_type)
        )));
    };
    let found = members[..search_len]
        .iter()
        .rposition(|candidate| normpath(&String::from_utf8_lossy(&candidate.name)) == target);
    let Some(target_index) = found else {
        // An unresolved link target is Fatal.
        return Err(BinpkgError::Fatal(format!(
            "{}: inner metadata member {shown:?} link target {shown_link:?} not in the archive",
            gpkg.display()
        )));
    };
    resolve_inner_member(gpkg, members, target_index, depth + 1)
}
