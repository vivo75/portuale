//! The gpkg outer tar and its members.

use crate::binpkg::*;

/// One gpkg outer-container member as its tar header presents it.
/// `name` is the basename after the single prefix directory; `raw_name`
/// is the full member name bytes used to re-open and stream the member.
pub(crate) struct OuterMember {
    pub(crate) raw_name: Vec<u8>,
    pub(crate) name: String,
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
