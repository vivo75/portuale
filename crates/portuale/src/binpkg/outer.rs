//! The gpkg outer tar and its members.

use crate::binpkg::*;

/// The outer member names `#56` trusts by name: the `gpkg-1` version
/// marker, the two inner tars (and any compression), their `.sig`
/// sidecars, and `Manifest`. A non-regular entry at one of these names is
/// an error wherever a container is read -- not only where the member is
/// selected -- so a crafted symlink can never be skipped in favour of a
/// later regular one.
pub(crate) fn trusted_outer_member_name(name: &str) -> bool {
    name == "gpkg-1"
        || name == "Manifest"
        || name.ends_with(".sig")
        || classify_inner_member("metadata", name).is_some()
        || classify_inner_member("image", name).is_some()
}

/// `#58` S5: one gpkg outer-container member as its tar header presents
/// it. `name` is the basename after the single prefix directory (the
/// name real's `Manifest` records and `#56` trusts); `raw_name` is the
/// full member name bytes the crate reports, used by
/// [`outer_member_read`] to re-open and stream the member.
pub(crate) struct OuterMember {
    pub(crate) raw_name: Vec<u8>,
    pub(crate) name: String,
}

/// `#58` S5: read the outer container in process with the `tar` crate.
/// Real `_verify_binpkg`'s structural rules become header checks --
/// every name relative, exactly one level deep under one common prefix
/// (`f.count("/") == 1` + `os.path.commonpath`), no duplicate names --
/// and `#56`'s trusted names must be a **regular member**, now from the
/// tar header instead of a `symlink_metadata` after `tar -xf`: a
/// symlink/FIFO/device/hardlink header at `gpkg-1`, `Manifest`, an inner
/// tar or a `.sig` sidecar is an error naming the member and its type,
/// and the member's data is never read through it. Returns the members
/// and whether the `gpkg-1` marker is present.
pub(crate) fn read_outer_members(gpkg: &Path) -> Result<(Vec<OuterMember>, bool), BinpkgError> {
    let file =
        fs::File::open(gpkg).map_err(|e| BinpkgError::Fatal(format!("{}: {e}", gpkg.display())))?;
    let mut archive = tar::Archive::new(file);
    let entries = archive.entries().map_err(|e| {
        BinpkgError::Invalid(format!("{}: reading gpkg container: {e}", gpkg.display()))
    })?;
    let mut members: Vec<OuterMember> = Vec::new();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut prefix: Option<Vec<u8>> = None;
    let mut gpkg_marker = false;
    for entry in entries {
        let entry = entry.map_err(|e| {
            BinpkgError::Invalid(format!("{}: reading gpkg container: {e}", gpkg.display()))
        })?;
        // Python `tarfile` strips a trailing `/` before real ever sees
        // the name (so a directory member is a depth error, exactly as
        // real's `f.count("/") != 1` check would say).
        let mut raw = entry.path_bytes().to_vec();
        while raw.last() == Some(&b'/') {
            raw.pop();
        }
        let shown = String::from_utf8_lossy(&raw);
        if raw.starts_with(b"/") || raw.iter().filter(|byte| **byte == b'/').count() != 1 {
            return Err(BinpkgError::Invalid(format!(
                "{}: gpkg file structure mismatch {shown:?}",
                gpkg.display()
            )));
        }
        let slash = raw
            .iter()
            .position(|byte| *byte == b'/')
            .expect("exactly one slash was checked above");
        let (this_prefix, name_bytes) = (&raw[..slash], &raw[slash + 1..]);
        match &prefix {
            Some(prefix) if prefix.as_slice() != this_prefix => {
                return Err(BinpkgError::Invalid(format!(
                    "{}: gpkg file structure mismatch {shown:?}, more than one prefix directory",
                    gpkg.display()
                )));
            }
            None => prefix = Some(this_prefix.to_vec()),
            _ => {}
        }
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        let entry_type = entry.header().entry_type();
        if trusted_outer_member_name(&name) && !entry_type.is_file() {
            return Err(BinpkgError::Invalid(format!(
                "{}: gpkg container member {name:?} is {}, not a regular file",
                gpkg.display(),
                inner_entry_type_phrase(entry_type)
            )));
        }
        if seen.contains(&raw) {
            return Err(BinpkgError::Invalid(format!(
                "{}: gpkg container member {shown:?} is a duplicate",
                gpkg.display()
            )));
        }
        seen.push(raw.clone());
        if name == "gpkg-1" {
            gpkg_marker = true;
        }
        members.push(OuterMember {
            raw_name: raw,
            name,
        });
    }
    Ok((members, gpkg_marker))
}

/// `#58` S5: stream one outer-container member through `consume`, without
/// materialising it (a `verify_gpkg_manifest` pass hashes members that
/// can be hundreds of MiB). The container is opened fresh per read --
/// real's `container.extractfile` does the same, and re-opening beats
/// buffering a member -- so `raw_name` comes from
/// [`read_outer_members`].
pub(crate) fn outer_member_read<T, E>(
    gpkg: &Path,
    raw_name: &[u8],
    mut consume: impl FnMut(tar::Entry<'_, fs::File>) -> Result<T, E>,
) -> Result<T, E>
where
    E: From<BinpkgError>,
{
    let file =
        fs::File::open(gpkg).map_err(|e| BinpkgError::Fatal(format!("{}: {e}", gpkg.display())))?;
    let mut archive = tar::Archive::new(file);
    let entries = archive.entries().map_err(|e| {
        BinpkgError::Invalid(format!("{}: reading gpkg container: {e}", gpkg.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            BinpkgError::Invalid(format!("{}: reading gpkg container: {e}", gpkg.display()))
        })?;
        let mut raw = entry.path_bytes().to_vec();
        while raw.last() == Some(&b'/') {
            raw.pop();
        }
        if raw == raw_name {
            return consume(entry);
        }
    }
    Err(BinpkgError::Invalid(format!(
        "{}: gpkg container member {:?} not found",
        gpkg.display(),
        String::from_utf8_lossy(raw_name)
    ))
    .into())
}
