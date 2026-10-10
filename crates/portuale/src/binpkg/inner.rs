//! The gpkg inner metadata tar: limits, members, links.

use crate::binpkg::*;

/// `#58` K0: the inner `metadata.tar` is read in process with the `tar`
/// crate. Real reads the whole tar into memory too (`unpack_metadata`'s
/// `io.BytesIO(metadata_reader.read())`), so the bound is on the total
/// member bytes: 64 MiB is far above any real `metadata.tar` (a few
/// KiB-MiB) and still bounds a decompression bomb.
pub(crate) const MAX_INNER_METADATA_BYTES: u64 = 64 * 1024 * 1024;

/// A tar of millions of empty members is the same allocation attack with
/// no bytes to count; real's writer emits ~15.
pub(crate) const MAX_INNER_METADATA_ENTRIES: usize = 100_000;

/// Real `tarfile.extractfile` follows links recursively until Python's
/// recursion limit (`RecursionError` on a cycle). Portuale stops at a
/// fixed depth with an error naming the member (`#58` K1).
pub(crate) const MAX_INNER_LINK_DEPTH: usize = 40;

/// The `tar` entry types Python's `tarfile` lists in `SUPPORTED_TYPES`
/// (`lib/python3.14/tarfile.py`); any other byte is treated as a regular
/// file by `extractfile` (`tarinfo.isreg() or tarinfo.type not in
/// SUPPORTED_TYPES`) and must be here too.
pub(crate) fn known_inner_entry_type(entry_type: tar::EntryType) -> bool {
    matches!(
        entry_type.as_byte(),
        b'\0' | b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'L' | b'K' | b'S'
    )
}

/// The type phrase for a member that cannot yield bytes (real's own
/// `None.read()` -> error cases), for the `#58` K6 error message.
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

/// `os.path.normpath` for the archive-relative names real's
/// `_find_link_target` compares: collapses `//`, drops `.` components,
/// resolves `..` lexically, keeps a leading `/` (an absolute link target
/// can then never match a relative member, exactly like real).
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
    /// The raw member name with a trailing `/` stripped -- Python
    /// `tarfile` strips it before real ever sees the name.
    pub(crate) name: Vec<u8>,
    /// `name` with the `metadata/` prefix stripped (real
    /// `_strip_metadata_prefix`), lossy-decoded for the value map.
    pub(crate) key: String,
    pub(crate) entry_type: tar::EntryType,
    /// The member's own header mode (`#58` K2: the merge path masks it
    /// to `0o7777` and strips setuid/setgid before writing).
    pub(crate) mode: u32,
    pub(crate) link_name: Option<Vec<u8>>,
    /// Present only for regular/unknown types; a link resolves to its
    /// target's bytes instead.
    pub(crate) data: Option<Vec<u8>>,
}

/// One resolved inner-metadata member for the caller: the key real's
/// `_strip_metadata_prefix` yields, the bytes `extractfile` would
/// return (the target's own bytes for a link), and the member's header
/// mode.
#[derive(Debug)]
pub(crate) struct InnerEntry {
    pub(crate) key: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) mode: u32,
}

/// `#58` S1: the resolved inner metadata. `entries` is first-appearance
/// order with last-wins values (real's own `dict` comprehension over
/// `getmembers()`, `gpkg.py:855-862`); `duplicate_keys` names the keys
/// seen more than once so the merge path (`#58` S3) can refuse an
/// archive real's own `tar_safe_extract` rejects as "Duplicate files
/// detected".
#[derive(Debug)]
pub(crate) struct InnerMetadata {
    pub(crate) entries: Vec<InnerEntry>,
    /// `#58` S3 refuses these on the merge path (the read path above
    /// last-wins like real); read only by S3 and the unit tests until
    /// then.
    #[allow(dead_code)]
    pub(crate) duplicate_keys: Vec<String>,
}

/// Collect every inner member in archive order, validating its name and
/// reading bytes only for regular/unknown types. `#58` K3/K5: a name
/// outside `metadata/`, an absolute name, or any `..` component is an
/// error (real `_strip_metadata_prefix` / its own `tar_safe_extract`);
/// the zip-bomb guards cap the member count and the total bytes.
pub(crate) fn collect_inner_members<R: Read>(
    gpkg: &Path,
    mut reader: R,
) -> Result<Vec<InnerMember>, BinpkgError> {
    let mut members: Vec<InnerMember> = Vec::new();
    let mut total_bytes: u64 = 0;
    {
        let mut archive = tar::Archive::new(&mut reader);
        // An inner tar that will not parse: real's `tarfile.open(mode=
        // "r:", fileobj=metadata_tar)` raises ReadError uncaught -> Fatal.
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
    // the trailing zero padding. When the reader is a decompressor's
    // stdout pipe (`#58` S5), dropping it now would send the child a
    // SIGPIPE; drain to EOF so it can exit cleanly.
    // Draining the decompressor's stream failed: real's whole-stream
    // `metadata_reader.read()` errors are wrapped as
    // `CompressorOperationFailed` (skip class).
    std::io::copy(&mut reader, &mut std::io::sink()).map_err(|e| {
        BinpkgError::Invalid(format!(
            "{}: reading inner metadata.tar: {e}",
            gpkg.display()
        ))
    })?;
    Ok(members)
}

/// Resolve one member to bytes exactly as real `tarfile.extractfile`
/// does (`#58` K1): a regular/unknown type yields its own bytes; a
/// symlink resolves `dirname(name)/linkname` normalized over the whole
/// archive (last match wins); a hardlink resolves `linkname` among
/// earlier members only; both recurse through further links with a
/// depth cap. A directory/FIFO/device is an error naming the member and
/// its type; an unresolved link is an error naming the target. A link
/// can therefore only ever yield **another member's** bytes.
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
    // real RecursionError -> Fatal.
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
        // The whole archive, last match wins (`_getmember` bottom-to-top).
        (normpath(&joined), members.len())
    } else if entry_type.is_hard_link() {
        // Only members *before* the link can be its target.
        (normpath(&shown_link), index)
    } else {
        // real `extractfile` returns None for a directory/FIFO/device,
        // and `None.read()` is an AttributeError -> Fatal.
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
        // real `_find_link_target` raises KeyError -> Fatal.
        return Err(BinpkgError::Fatal(format!(
            "{}: inner metadata member {shown:?} link target {shown_link:?} not in the archive",
            gpkg.display()
        )));
    };
    resolve_inner_member(gpkg, members, target_index, depth + 1)
}

/// `#58` S5: stream one outer member (`outer_member_read`) into `dest`,
/// decompressing when `comp` names a codec. The member stream is either
/// copied straight through (a plain `metadata.tar`/`image.tar`) or fed to
/// the `gpkg_compressions()` decompressor's stdin with `dest` as its
/// stdout -- no scratch copy of the compressed member, no second
/// decompression pass. `member_label` names the member in errors.
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
            // `dest` is a file, not a pipe, so the child never blocks on
            // stdout and the stdin copy below cannot deadlock.
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

/// `#58` S1: real `tarfile.extractfile` over the inner `metadata.tar`.
/// `member_bytes` is the outer container's `metadata.tar[.<comp>]`
/// member, already read (the compressed member is small; `#58` S5 reads
/// it in memory instead of a scratch file and caps it): read directly
/// when `comp` is `None`, else piped through the matching decompressor
/// (`gpkg_compressions`) straight into the `tar` crate -- no
/// `<scratch>/metadata.tar`, no second `tar` process. Returns every
/// resolved `(key, bytes)` in first-appearance order with last-wins
/// values.
pub(crate) fn read_inner_metadata(
    gpkg: &Path,
    member_label: &str,
    member_bytes: &[u8],
    comp: Option<&[&str]>,
) -> Result<InnerMetadata, BinpkgError> {
    let members = match comp {
        None => collect_inner_members(gpkg, member_bytes)?,
        Some(argv) => {
            // A missing decompressor is real's `CompressorNotFound`
            // (PortagePackageException) -> skip class.
            let mut child = Command::new(argv[0])
                .args(&argv[1..])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| BinpkgError::Invalid(format!("failed to spawn {}: {e}", argv[0])))?;
            let mut stdin = child.stdin.take().expect("stdin is piped");
            let stdout = child.stdout.take().expect("stdout is piped");
            // Feed the member bytes in on a scope thread: the child's
            // stdout (the tar stream) is being read here, so writing its
            // stdin inline could deadlock on a full pipe.
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
                // A decompressor that failed with a readable tar behind
                // it is real's `CompressorOperationFailed`
                // (PortagePackageException) -> skip class; a member-level
                // error (an unsafe name, a truncated stream) speaks for
                // itself.
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
