//! The gpkg inner metadata tar: limits, members, links.

use crate::binpkg::*;

/// Bound on the total inner-member bytes: Portage reads the whole tar into
/// memory too, and 64 MiB is far above any real `metadata.tar` while still
/// bounding a decompression bomb.
pub(crate) const MAX_INNER_METADATA_BYTES: u64 = 64 * 1024 * 1024;

/// A tar of millions of empty members is the same allocation attack with
/// no bytes to count.
pub(crate) const MAX_INNER_METADATA_ENTRIES: usize = 100_000;

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

/// Whether `tarfile` calls the member regular (`TarInfo.isreg`): a plain,
/// legacy-plain, contiguous or GNU sparse file. Any other type byte is not.
fn is_regular_inner_member(entry_type: tar::EntryType) -> bool {
    entry_type.is_file() || entry_type.is_contiguous() || entry_type.as_byte() == b'S'
}

/// One inner `metadata.tar` member: its header fields plus bytes, read
/// before anything is written anywhere.
pub(crate) struct InnerMember {
    /// `name` with the `metadata/` prefix stripped, lossy-decoded.
    pub(crate) key: String,
    /// The member's own header mode.
    pub(crate) mode: u32,
    pub(crate) data: Vec<u8>,
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
/// reading the bytes of each. A member that is not a regular file (link,
/// directory, device, FIFO) is invalid, whatever it points at; so is a name
/// outside `metadata/`, an absolute name, or any `..` component; the
/// zip-bomb guards cap the member count and the total bytes.
// Portage: `gpkg._check_metadata_files`, `gpkg._strip_metadata_prefix`,
// `tar_safe_extract`.
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
            let entry_type = entry.header().entry_type();
            if !is_regular_inner_member(entry_type) {
                return Err(BinpkgError::Invalid(format!(
                    "{}: Metadata member is not a regular file: {shown}",
                    gpkg.display()
                )));
            }
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
            let mode = entry.header().mode().unwrap_or(0o644) & 0o7777;
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| {
                BinpkgError::Fatal(format!(
                    "{}: reading inner metadata member {shown:?}: {e}",
                    gpkg.display()
                ))
            })?;
            total_bytes = total_bytes.saturating_add(data.len() as u64);
            if total_bytes > MAX_INNER_METADATA_BYTES {
                return Err(BinpkgError::Invalid(format!(
                    "{}: inner metadata.tar is larger than {MAX_INNER_METADATA_BYTES} bytes",
                    gpkg.display()
                )));
            }
            members.push(InnerMember { key, mode, data });
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
    for InnerMember {
        key, mode, data, ..
    } in members
    {
        let bytes = data;
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
