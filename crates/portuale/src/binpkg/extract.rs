//! Extracting a binary package's image and metadata.

use crate::binpkg::*;

/// Unpack a binpkg's *image* (the built filesystem tree) into `image_dest`,
/// and its scalar metadata (one `<KEY>` file each) into `build_info_dest`.
/// xpak (`.tbz2`): `[image tarball][XPAK trailer]`, the image a compressed
/// tar `tar` auto-detects. gpkg (`.gpkg.tar`): the outer tar's
/// `<basename>/image.tar[.<comp>]` member.
///
/// The two non-scalar members (`environment.bz2`, `<pf>.ebuild`) are written
/// back **verbatim** as raw bytes, not through the lossy scalar path: the
/// merge runs `pkg_preinst`/`pkg_postinst` from the saved `environment.bz2`.
///
/// The Manifest digest + signature checks run before anything is unpacked.
// Portage: `xpak.tbz2.decompose`, `gpkg.gpkg.decompress`,
// `BinpkgEnvExtractor`, `gpkg._verify_binpkg`.
pub fn extract_binpkg(
    binpkg_path: &Path,
    image_dest: &Path,
    build_info_dest: &Path,
    gpg: &GpgVerify,
    xattrs: &crate::ebuild_merge::XattrPolicy,
) -> Result<(), String> {
    fs::create_dir_all(image_dest).map_err(|e| format!("{}: {e}", image_dest.display()))?;
    fs::create_dir_all(build_info_dest)
        .map_err(|e| format!("{}: {e}", build_info_dest.display()))?;

    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.ends_with(".gpkg.tar") {
        // The Manifest checks run before anything is unpacked.
        verify_gpkg_manifest(binpkg_path, gpg)?;
        extract_gpkg_member(binpkg_path, "image", image_dest)?;
        // Every `metadata/` member extracted **verbatim**: a scalar parse +
        // reserialize lost `environment.bz2` and turned empty fields into
        // stray newlines.
        extract_gpkg_member(binpkg_path, "metadata", build_info_dest)?;
        return Ok(());
    }

    // xpak: every XPAK member written VERBATIM, not parsed + reserialized
    // (which turned an empty `DEBUGBUILD` into `"\n"` and trimmed the
    // trailing space kept on the last `NEEDED` line).
    // Portage: `bintree.dbapi.unpack_metadata`, `tbz2.unpackinfo`.
    extract_xpak_image(binpkg_path, image_dest, xattrs)?;
    let seg = read_xpak_segment(binpkg_path)?;
    for (name, bytes) in parse_xpak_members(&seg)? {
        let Some(dest) = xpak_member_dest(build_info_dest, &name) else {
            continue;
        };
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(())
}

/// `unpackinfo`'s target path for one member: the name with leading `/`
/// stripped, joined under `dest` and normalized. A name whose `..`
/// components climb out of `dest` is skipped (`None`).
// Portage: `xpak.tbz2.unpackinfo`.
pub(crate) fn xpak_member_dest(dest: &Path, name: &str) -> Option<PathBuf> {
    let mut parts: Vec<&str> = Vec::new();
    for comp in name.trim_start_matches('/').split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            c => parts.push(c),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.iter().fold(dest.to_path_buf(), |p, c| p.join(c)))
}

/// The metadata half of `extract_gpkg_member`: every inner `metadata/`
/// member is written as a **regular file** with its header mode (masked to
/// `0o7777`, setuid/setgid stripped). An in-archive symlink is never
/// extracted as one (later readers would follow it); duplicates and nested
/// keys are refused. `dest` exists by the time this runs.
// Portage: `bintree.dbapi.unpack_metadata`, `gpkg.unpack_metadata`.
pub(crate) fn extract_gpkg_metadata(
    gpkg: &Path,
    dest: &Path,
    metadata: InnerMetadata,
) -> Result<(), String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    if !metadata.duplicate_keys.is_empty() {
        return Err(format!(
            "{}: inner metadata.tar has duplicate members: {}",
            gpkg.display(),
            metadata.duplicate_keys.join(", ")
        ));
    }
    for entry in &metadata.entries {
        if entry.key.contains('/') {
            return Err(format!(
                "{}: inner metadata member {:?} is nested, which Portage's writer never emits",
                gpkg.display(),
                entry.key
            ));
        }
        let path = dest.join(&entry.key);
        let mode = entry.mode & 0o7777 & !0o6000;
        // `create_new`: never overwrite a file another member (or the
        // image side) already placed.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        std::io::Write::write_all(&mut file, &entry.bytes)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        drop(file);
        // `OpenOptions::mode` passes through the process umask, so set the
        // header mode explicitly afterwards for a umask-independent result.
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Pre-scan the decompressed scratch `image.tar` with the member rules
/// **before** GNU `tar` unpacks it, so nothing reaches the filesystem
/// until the whole member list is accepted. Check order: duplicate name,
/// absolute name, `..` traversal, device/FIFO, and a hard link whose
/// target is not an earlier member; symlinks are allowed.
///
/// One deliberate addition: a member whose path traverses an earlier
/// symlink member is refused (extracting through it would write outside
/// the destination; a genuine tree can never produce the shape, since a
/// symlink has no children on disk).
// Portage: `gpkg.tar_safe_extract`.
pub(crate) fn scan_image_tar(image_tar: &Path, gpkg: &Path) -> Result<(), String> {
    let file = fs::File::open(image_tar).map_err(|e| format!("{}: {e}", image_tar.display()))?;
    let mut archive = tar::Archive::new(file);
    let entries = archive
        .entries()
        .map_err(|e| format!("{}: reading image.tar: {e}", gpkg.display()))?;
    // One entry per accepted member (trailing `/` trimmed).
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut symlinks: Vec<Vec<u8>> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: reading image.tar: {e}", gpkg.display()))?;
        let mut name = entry.path_bytes().to_vec();
        while name.last() == Some(&b'/') {
            name.pop();
        }
        let shown = String::from_utf8_lossy(&name);
        let entry_type = entry.header().entry_type();
        let hard_link_target = entry.link_name_bytes().map(|bytes| bytes.to_vec());

        if names.iter().any(|seen| seen == &name) {
            return Err(format!(
                "{}: image.tar member {shown:?} is a duplicate",
                gpkg.display()
            ));
        }
        if name.starts_with(b"/") {
            return Err(format!(
                "{}: image.tar member {shown:?} has an absolute name",
                gpkg.display()
            ));
        }
        if name.starts_with(b"../") || name.windows(4).any(|window| window == b"/../") {
            return Err(format!(
                "{}: image.tar member {shown:?} has a path traversal",
                gpkg.display()
            ));
        }
        if entry_type.is_character_special()
            || entry_type.is_block_special()
            || entry_type.is_fifo()
        {
            return Err(format!(
                "{}: image.tar member {shown:?} is {}",
                gpkg.display(),
                inner_entry_type_phrase(entry_type)
            ));
        }
        if entry_type.is_hard_link() {
            let target = hard_link_target.clone().unwrap_or_default();
            if !names.iter().any(|seen| seen == &target) {
                return Err(format!(
                    "{}: image.tar member {shown:?} hard link target {:?} is not an earlier member",
                    gpkg.display(),
                    String::from_utf8_lossy(&target)
                ));
            }
        }
        if let Some(symlink) = symlinks.iter().find(|symlink| {
            name.starts_with(symlink.as_slice()) && name.get(symlink.len()) == Some(&b'/')
        }) {
            return Err(format!(
                "{}: image.tar member {shown:?} is inside the symlink member {:?}",
                gpkg.display(),
                String::from_utf8_lossy(symlink)
            ));
        }
        if entry_type.is_symlink() {
            symlinks.push(name.clone());
        }
        names.push(name);
    }
    Ok(())
}

/// The xpak `[image tarball]` prefix -> `dest`: the image is everything
/// before the `XPAKPACK…STOP` trailer.
// Portage: `xpak.tbz2.decompose`.
pub(crate) fn extract_xpak_image(
    binpkg_path: &Path,
    dest: &Path,
    xattrs: &crate::ebuild_merge::XattrPolicy,
) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f =
        fs::File::open(binpkg_path).map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    let file_len = f
        .seek(SeekFrom::End(0))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if file_len < 16 {
        return Err(format!(
            "{}: too small for an xpak binpkg",
            binpkg_path.display()
        ));
    }
    let mut trailer = [0u8; 16];
    f.seek(SeekFrom::End(-16))
        .and_then(|_| f.read_exact(&mut trailer))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if &trailer[0..8] != b"XPAKSTOP" || &trailer[12..16] != b"STOP" {
        return Err(format!("{}: no XPAKSTOP trailer", binpkg_path.display()));
    }
    let infosize = be32(&trailer[8..12]) as u64;
    let image_len = file_len.checked_sub(infosize + 8).ok_or_else(|| {
        format!(
            "{}: xpak trailer larger than the file",
            binpkg_path.display()
        )
    })?;

    let scratch = ScratchDir::new("xpak-image")?;
    let image_tar = scratch.path().join("image.tar");
    f.seek(SeekFrom::Start(0))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    let mut out =
        fs::File::create(&image_tar).map_err(|e| format!("{}: {e}", image_tar.display()))?;
    std::io::copy(&mut f.take(image_len), &mut out)
        .map_err(|e| format!("{}: {e}", image_tar.display()))?;
    drop(out);
    // `tar -x` auto-detects gzip/bzip2/xz/zstd/... on read.
    let (tar, dest) = (lossy(&image_tar), lossy(dest));
    let mut args = vec!["-xpf", tar.as_str(), "-C", dest.as_str()];
    let excludes = xpak_xattr_tar_options(xattrs, tar_supports_xattrs());
    args.extend(excludes.iter().map(String::as_str));
    run_tar(&args)
}

/// Under `FEATURES=xattr`, when tar knows `--xattrs`, extract with
/// `--xattrs --xattrs-include='*'` plus one `--xattrs-exclude=<pat>` per
/// `PORTAGE_XATTR_EXCLUDE` pattern. Only the xpak image takes these (gpkg
/// neither stores nor restores xattrs).
// Portage: `BinpkgExtractorAsync._xpak_start`.
pub(crate) fn xpak_xattr_tar_options(
    xattrs: &crate::ebuild_merge::XattrPolicy,
    tar_knows_xattrs: bool,
) -> Vec<String> {
    if !xattrs.enabled || !tar_knows_xattrs {
        return Vec::new();
    }
    let mut opts = vec!["--xattrs".to_string(), "--xattrs-include=*".to_string()];
    opts.extend(
        xattrs
            .exclude
            .split_whitespace()
            .map(|pat| format!("--xattrs-exclude={pat}")),
    );
    opts
}

/// The `b"--xattrs" in $(gtar --help)` probe, run once per process.
pub(crate) fn tar_supports_xattrs() -> bool {
    use std::sync::OnceLock;
    static KNOWS: OnceLock<bool> = OnceLock::new();
    *KNOWS.get_or_init(|| {
        Command::new("tar")
            .arg("--help")
            .output()
            .map(|o| o.stdout.windows(8).any(|w| w == b"--xattrs"))
            .unwrap_or(false)
    })
}

/// Locate `<basename>/<want>.tar[.<comp>]` in a gpkg's outer tar,
/// decompress it if needed, and extract it into `dest`. Shares the outer
/// unpack, the trusted-name regular-file walk and the `gpkg-1` validity
/// guard with `read_gpkg_metadata`.
///
/// The `image` half keeps GNU `tar` (`-xpf --strip-components=1`) for its
/// ownership/permission/xattr/sparse semantics, with the member rules
/// applied by a pre-scan first ([`scan_image_tar`]). The `metadata` half
/// is resolved in process ([`extract_gpkg_metadata`]).
pub(crate) fn extract_gpkg_member(gpkg_path: &Path, want: &str, dest: &Path) -> Result<(), String> {
    if !gpkg_path.is_file() {
        return Err(format!("{}: not a file", gpkg_path.display()));
    }

    // The outer container is read from its tar headers: regular-member
    // check, one-level structure and marker, before any bytes stream.
    let (members, gpkg_marker) = read_outer_members(gpkg_path)?;
    if !gpkg_marker {
        return Err(format!(
            "{}: no `gpkg-1` version marker",
            gpkg_path.display()
        ));
    }
    let (member, comp) = members
        .iter()
        .find_map(|member| classify_inner_member(want, &member.name).map(|comp| (member, comp)))
        .ok_or_else(|| format!("{}: no `{want}.tar` member", gpkg_path.display()))?;

    if want == "metadata" {
        // The inner metadata tar is resolved in process and written as
        // regular files only (no scratch copy, no `tar -xpf`).
        let metadata = outer_member_read(gpkg_path, &member.raw_name, |entry| {
            let member_bytes =
                read_member_bytes_capped(entry, MAX_INNER_METADATA_BYTES, &member.name)?;
            read_inner_metadata(gpkg_path, &member.name, &member_bytes, comp)
        })?;
        return extract_gpkg_metadata(gpkg_path, dest, metadata);
    }

    // Image: stream the member into a scratch `image.tar` (through the
    // decompressor where needed) for the GNU `tar` unpack below.
    let scratch = ScratchDir::new("gpkg-member")?;
    let inner_tar = scratch.path().join(format!("{want}.tar"));
    outer_member_read(gpkg_path, &member.raw_name, |source| {
        decompress_member_to_file(&member.name, source, comp, &inner_tar)
    })?;
    // Portage validates the image stream as it extracts; validate the whole
    // scratch `image.tar` first ([`scan_image_tar`]) so a rejected member
    // never reaches the filesystem, then let GNU `tar` unpack it.
    scan_image_tar(&inner_tar, gpkg_path)?;
    // The inner tarball's members all live under a single `<want>/`
    // top-level directory, stripped here; without the strip every merged
    // path would be recorded as `/image/...`, which breaks unmerge.
    run_tar(&[
        "-xpf",
        &lossy(&inner_tar),
        "-C",
        &lossy(dest),
        "--strip-components=1",
    ])
}
