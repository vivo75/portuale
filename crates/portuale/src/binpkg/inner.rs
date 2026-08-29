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

/// The resolved inner metadata. `entries` is first-appearance order with
/// last-wins values; `duplicate_keys` names the keys seen more than once
/// so the merge path can refuse them.
#[derive(Debug)]
pub(crate) struct InnerMetadata {
    pub(crate) entries: Vec<InnerEntry>,
    /// Refused on the merge path (the read path above is last-wins);
    /// read only by the merge path and the unit tests until then.
    #[allow(dead_code)]
    pub(crate) duplicate_keys: Vec<String>,
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

/// Stream one outer member into `dest`, decompressing when `comp` names a
/// codec: either copied straight through or fed to the decompressor's
/// stdin with `dest` as its stdout. No scratch copy, no second pass.
/// `member_label` names the member in errors.
pub(crate) fn decompress_member_to_file<R: Read>(
    member_label: &str,
    mut source: R,
    comp: Option<&[&str]>,
    dest: &Path,
) -> Result<(), String> {
    let mut out = fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    match comp {
        None => {
            std::io::copy(&mut source, &mut out).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
        Some(argv) => {
            // `dest` is a file, not a pipe, so the copy below cannot deadlock.
            let mut child = Command::new(argv[0])
                .args(&argv[1..])
                .stdin(Stdio::piped())
                .stdout(out)
                .spawn()
                .map_err(|e| format!("failed to spawn {}: {e}", argv[0]))?;
            let mut stdin = child.stdin.take().expect("stdin is piped");
            let copied = std::io::copy(&mut source, &mut stdin);
            drop(stdin);
            let status = child
                .wait()
                .map_err(|e| format!("waiting for {}: {e}", argv[0]))?;
            if !status.success() {
                return Err(format!(
                    "{} failed to decompress {member_label} ({status})",
                    argv[0]
                ));
            }
            copied.map_err(|e| format!("{member_label}: {e}"))?;
        }
    }
    Ok(())
}

/// Read the inner `metadata.tar` into resolved `(key, bytes)` members in
/// first-appearance order with last-wins values. `member_bytes` is the
/// outer container's `metadata.tar[.<comp>]` member, already read and
/// capped; when compressed it is piped through the matching decompressor
/// straight into the `tar` crate, with no scratch file and no second
/// process.
// Portage: `tarfile.extractfile`.
pub(crate) fn read_inner_metadata(
    gpkg: &Path,
    member_label: &str,
    member_bytes: &[u8],
    comp: Option<&[&str]>,
) -> Result<InnerMetadata, BinpkgError> {
    let members = match comp {
        None => collect_inner_members(gpkg, member_bytes)?,
        Some(argv) => {
            // A missing decompressor is the skip class.
            let mut child = Command::new(argv[0])
                .args(&argv[1..])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| BinpkgError::Invalid(format!("failed to spawn {}: {e}", argv[0])))?;
            let mut stdin = child.stdin.take().expect("stdin is piped");
            let stdout = child.stdout.take().expect("stdout is piped");
            // Feed the bytes on a scope thread: the child's stdout is read
            // here, so writing its stdin inline could deadlock on a pipe.
            let collected = std::thread::scope(|scope| {
                let pump = scope.spawn(move || {
                    let copied = std::io::copy(&mut { member_bytes }, &mut stdin);
                    drop(stdin);
                    copied
                });
                let collected = collect_inner_members(gpkg, stdout);
                let _ = pump.join();
                collected
            });
            let status = child
                .wait()
                .map_err(|e| BinpkgError::Fatal(format!("waiting for {}: {e}", argv[0])))?;
            match (collected, status.success()) {
                (Ok(members), true) => members,
                // A failed decompressor with a member-level error behind it
                // lets the member error speak for itself; otherwise the
                // decompress failure (skip class).
                (Ok(_), false) => {
                    return Err(BinpkgError::Invalid(format!(
                        "{} failed to decompress {member_label} ({status})",
                        argv[0]
                    )));
                }
                (Err(error), _) => return Err(error),
            }
        }
    };

    let mut entries: Vec<InnerEntry> = Vec::new();
    let mut duplicate_keys: Vec<String> = Vec::new();
    for index in 0..members.len() {
        let bytes = resolve_inner_member(gpkg, &members, index, 0)?;
        let key = members[index].key.clone();
        let mode = members[index].mode;
        match entries.iter_mut().find(|entry| entry.key == key) {
            Some(entry) => {
                entry.bytes = bytes;
                entry.mode = mode;
                if !duplicate_keys.contains(&key) {
                    duplicate_keys.push(key);
                }
            }
            None => entries.push(InnerEntry { key, bytes, mode }),
        }
    }
    Ok(InnerMetadata {
        entries,
        duplicate_keys,
    })
}
