//! The gpkg outer tar and its members.

use crate::binpkg::*;

/// The outer member names trusted by name: the `gpkg-1` version marker,
/// the two inner tars (any compression), their `.sig` sidecars, and
/// `Manifest`. A non-regular entry at one of these names is an error
/// wherever a container is read, so a crafted symlink can never be
/// skipped in favour of a later regular one.
pub(crate) fn trusted_outer_member_name(name: &str) -> bool {
    name == "gpkg-1"
        || name == "Manifest"
        || name.ends_with(".sig")
        || classify_inner_member("metadata", name).is_some()
        || classify_inner_member("image", name).is_some()
}

/// One gpkg outer-container member as its tar header presents it.
/// `name` is the basename after the single prefix directory; `raw_name`
/// is the full member name bytes used to re-open and stream the member.
pub(crate) struct OuterMember {
    pub(crate) raw_name: Vec<u8>,
    pub(crate) name: String,
}

/// Read the outer container in process with the `tar` crate. Every name
/// must be relative and exactly one level deep under one common prefix,
/// with no duplicates; a trusted name with a non-regular header is an
/// error naming the member and its type, and its data is never read
/// through it. Returns the members and whether `gpkg-1` is present.
// Portage: gpkg.py, _verify_binpkg
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
        // `tarfile` strips a trailing `/` first, so a directory member
        // is a depth error like any other non-one-level name.
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

/// Stream one outer-container member through `consume` without
/// materialising it (members can be hundreds of MiB). The container is
/// opened fresh per read, so `raw_name` comes from
/// [`read_outer_members`].
// Portage: gpkg.py, container.extractfile
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
