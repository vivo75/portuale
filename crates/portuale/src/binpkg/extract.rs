//! Extracting a binary package's image and metadata.

use crate::binpkg::*;
use std::collections::{HashMap, HashSet};

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
        // Both halves are validated before either writes: a refused
        // metadata member leaves the image untouched, and the image scan
        // runs before `tar` (see `extract_gpkg_member`).
        let metadata = load_gpkg_metadata(binpkg_path)?;
        extract_gpkg_member(binpkg_path, "image", image_dest)?;
        // Every `metadata/` member extracted **verbatim**: a scalar parse +
        // reserialize lost `environment.bz2` and turned empty fields into
        // stray newlines.
        extract_gpkg_metadata(binpkg_path, build_info_dest, metadata)?;
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

/// Unpack a binpkg's *metadata only* (the scalar `<KEY>` files plus the
/// verbatim `environment.bz2` / `<pf>.ebuild`) into `build_info_dest`,
/// without touching the image. The up-front pretend pass needs exactly
/// what Portage's `bintree.dbapi.unpack_metadata` reads: metadata, never
/// the image (so a binpkg truncated inside the image region still pretends
/// when its metadata is intact, like Portage).
/// Deliberately no Manifest digest/signature check here either: that is the
/// merge-time verifier's job (`merge_one_binary_entry`), exactly as
/// Portage verifies inside `_run_pkg_pretend_one` only for entries the
/// pretend filter keeps.
// Portage: `bintree.dbapi.unpack_metadata`, `tbz2.unpackinfo`
pub(crate) fn extract_binpkg_metadata_only(
    binpkg_path: &Path,
    build_info_dest: &Path,
) -> Result<(), String> {
    fs::create_dir_all(build_info_dest)
        .map_err(|e| format!("{}: {e}", build_info_dest.display()))?;

    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.ends_with(".gpkg.tar") {
        // No manifest verification: see the doc comment above.
        let metadata = load_gpkg_metadata(binpkg_path)?;
        extract_gpkg_metadata(binpkg_path, build_info_dest, metadata)?;
        return Ok(());
    }

    // xpak: every XPAK member written VERBATIM, not parsed + reserialized
    // (same verbatim rule as `extract_binpkg`).
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

/// Refuse, before anything is written, duplicate keys and nested keys.
fn check_gpkg_metadata(gpkg: &Path, metadata: &InnerMetadata) -> Result<(), String> {
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
    }
    Ok(())
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

    check_gpkg_metadata(gpkg, &metadata)?;
    for entry in &metadata.entries {
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

/// Pre-scan the decompressed scratch `image.tar` with Portage's member
/// rules **before** GNU `tar` unpacks it, so nothing reaches the filesystem
/// until the whole member list is accepted. Per member, in Portage's order:
/// duplicate name (also as `./<name>`), absolute name, any `..` component,
/// device/FIFO, a hard link whose target is not an earlier member, then the
/// resolved-path containment of the member's parent and of the member
/// itself, and for a hard link its source and (when the source is a
/// symlink) the relocated symlink target.
///
/// Portage checks against the tree it has already extracted, rooted at
/// its temporary directory; here that tree is modelled by [`ImageLinks`]
/// (every symlink member, and every hard link to one, which `os.link`
/// turns into a symlink), rooted at the staging directory GNU `tar`
/// unpacks into (see [`extract_gpkg_member`]). A link that climbs exactly
/// to that root is accepted, like Portage; only a climb above it escapes.
// Portage: `gpkg.tar_safe_extract._check_member`, `_check_symlink_path`,
// `_check_hardlink`.
pub(crate) fn scan_image_tar(image_tar: &Path, gpkg: &Path) -> Result<(), String> {
    let file = fs::File::open(image_tar).map_err(|e| format!("{}: {e}", image_tar.display()))?;
    let mut archive = tar::Archive::new(file);
    let entries = archive
        .entries()
        .map_err(|e| format!("{}: reading image.tar: {e}", gpkg.display()))?;
    let mut names: HashSet<Vec<u8>> = HashSet::new();
    let mut links = ImageLinks::default();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: reading image.tar: {e}", gpkg.display()))?;
        let mut name = entry.path_bytes().to_vec();
        while name.last() == Some(&b'/') {
            name.pop();
        }
        let shown = String::from_utf8_lossy(&name).into_owned();
        let danger = |what: &str| format!("{}: Danger: {what} detected: {shown}", gpkg.display());
        let entry_type = entry.header().entry_type();
        let link_name = entry.link_name_bytes().map(|bytes| bytes.to_vec());

        let dotted = [b"./".as_slice(), name.as_slice()].concat();
        if names.contains(&name) || names.contains(&dotted) {
            return Err(danger("duplicate files"));
        }
        if name.starts_with(b"/") {
            return Err(danger("absolute path"));
        }
        if name.split(|byte| *byte == b'/').any(|part| part == b"..") {
            return Err(danger("path traversal"));
        }
        if entry_type.is_character_special()
            || entry_type.is_block_special()
            || entry_type.is_fifo()
        {
            return Err(danger("device file"));
        }
        let hard_target = if entry_type.is_hard_link() {
            let target = link_name.clone().unwrap_or_default();
            if !names.contains(&target) {
                return Err(danger("hardlink escape"));
            }
            Some(target)
        } else {
            None
        };

        // Portage names the path it was checking, not the member.
        let escape = |path: &[u8]| {
            format!(
                "{}: Danger: symlink escape detected: {}",
                gpkg.display(),
                String::from_utf8_lossy(path)
            )
        };
        let parent = match name.iter().rposition(|b| *b == b'/') {
            Some(slash) => &name[..slash],
            None => &[][..],
        };
        links.contained(parent, true).map_err(|_| escape(parent))?;
        links.contained(&name, true).map_err(|_| escape(&name))?;
        if let Some(target) = &hard_target {
            links.contained(target, true).map_err(|_| escape(target))?;
            if let Some(symlink) = links.symlink_target(target) {
                let relocated = join_relocated(parent, &symlink);
                links
                    .contained(&relocated, true)
                    .map_err(|_| escape(&relocated))?;
                links.add(&name, symlink);
            }
        } else if entry_type.is_symlink() {
            links.add(&name, link_name.unwrap_or_default());
        }
        names.insert(name);
    }
    // Portage's final `_check_symlink_path(temp_dir, prefix)`: the `image/`
    // top level itself must resolve inside the staging root (a symlink
    // member named `image` pointing outside fails its own member check
    // first, so this only re-states the invariant).
    links
        .contained(b"image", true)
        .map_err(|_| format!("{}: Danger: symlink escape detected: image", gpkg.display()))?;
    Ok(())
}

/// `os.path.join(dirname, target)` for the relocated hard-link check; an
/// absolute `target` stays absolute, which [`ImageLinks::resolve`] refuses.
fn join_relocated(dirname: &[u8], target: &[u8]) -> Vec<u8> {
    if target.starts_with(b"/") || dirname.is_empty() {
        return target.to_vec();
    }
    [dirname, b"/", target].concat()
}

/// The symlinks an `image.tar` has created so far, keyed by their resolved
/// location under the staging root (names keep their `image/` prefix, as
/// Portage checks them against its temporary directory).
#[derive(Default)]
struct ImageLinks {
    links: HashMap<Vec<Vec<u8>>, Vec<u8>>,
}

/// Longest symlink chain followed before a path counts as a loop (the
/// kernel's own `MAXSYMLINKS`).
const MAX_IMAGE_SYMLINKS: usize = 40;

/// A path that leaves the staging root (or loops).
struct Escape;

impl ImageLinks {
    /// `os.path.realpath` of `path` over the modelled tree, relative to the
    /// staging root; the final component is followed only when
    /// `follow_final`. A `..` above the root, an absolute target (it leaves
    /// the staging directory) or a chain of more than 40 links is an escape.
    fn resolve(&self, path: &[u8], follow_final: bool) -> Result<Vec<Vec<u8>>, Escape> {
        if path.starts_with(b"/") {
            return Err(Escape);
        }
        let mut todo: Vec<Vec<u8>> = path.split(|b| *b == b'/').map(<[u8]>::to_vec).collect();
        todo.reverse();
        let mut current: Vec<Vec<u8>> = Vec::new();
        let mut followed = 0;
        while let Some(part) = todo.pop() {
            if part.is_empty() || part == b"." {
                continue;
            }
            if part == b".." {
                current.pop().ok_or(Escape)?;
                continue;
            }
            current.push(part);
            let Some(target) = self.links.get(&current) else {
                continue;
            };
            if todo.is_empty() && !follow_final {
                continue;
            }
            followed += 1;
            if followed > MAX_IMAGE_SYMLINKS || target.starts_with(b"/") {
                return Err(Escape);
            }
            current.pop();
            todo.extend(target.split(|b| *b == b'/').rev().map(<[u8]>::to_vec));
        }
        Ok(current)
    }

    /// `_check_symlink_path`: `Err` when `path` resolves outside.
    fn contained(&self, path: &[u8], follow_final: bool) -> Result<(), Escape> {
        self.resolve(path, follow_final).map(drop)
    }

    /// `os.readlink` of `path` when its last component is a symlink
    /// (`os.path.islink`).
    fn symlink_target(&self, path: &[u8]) -> Option<Vec<u8>> {
        let resolved = self.resolve(path, false).ok()?;
        self.links.get(&resolved).cloned()
    }

    /// Record a symlink member at `name`.
    fn add(&mut self, name: &[u8], target: Vec<u8>) {
        if let Ok(resolved) = self.resolve(name, false)
            && !resolved.is_empty()
        {
            self.links.insert(resolved, target);
        }
    }
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

/// The outer-container checks shared by both halves, then the member
/// `<basename>/<want>.tar[.<comp>]` with its decompressor.
fn locate_gpkg_member(
    gpkg_path: &Path,
    want: &str,
) -> Result<(OuterMember, Option<&'static [&'static str]>), String> {
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
    members
        .into_iter()
        .find_map(|member| classify_inner_member(want, &member.name).map(|comp| (member, comp)))
        .ok_or_else(|| format!("{}: no `{want}.tar` member", gpkg_path.display()))
}

/// Read and validate the inner `metadata.tar` in process, writing nothing:
/// every member must be a regular file ([`read_inner_metadata`]), keys
/// unique and not nested ([`check_gpkg_metadata`]).
// Portage: `gpkg._check_metadata_files`, `gpkg.unpack_metadata`.
pub(crate) fn load_gpkg_metadata(gpkg_path: &Path) -> Result<InnerMetadata, String> {
    let (member, comp) = locate_gpkg_member(gpkg_path, "metadata")?;
    let metadata = outer_member_read(gpkg_path, &member.raw_name, |entry| {
        let member_bytes = read_member_bytes_capped(entry, MAX_INNER_METADATA_BYTES, &member.name)?;
        read_inner_metadata(gpkg_path, &member.name, &member_bytes, comp)
    })?;
    check_gpkg_metadata(gpkg_path, &metadata)?;
    Ok(metadata)
}

/// Locate `<basename>/<want>.tar[.<comp>]` in a gpkg's outer tar,
/// decompress it if needed, and extract it into `dest`. Shares the outer
/// unpack, the trusted-name regular-file walk and the `gpkg-1` validity
/// guard with `read_gpkg_metadata`.
///
/// The `image` half keeps GNU `tar` for its ownership/permission/xattr/
/// sparse semantics, with the member rules applied by a pre-scan first
/// ([`scan_image_tar`]): like Portage's `extractall`, `tar` unpacks into a
/// staging directory inside `dest` and `<stage>/image/*` is then moved into
/// `dest`. The `metadata` half is resolved in process
/// ([`extract_gpkg_metadata`]).
pub(crate) fn extract_gpkg_member(gpkg_path: &Path, want: &str, dest: &Path) -> Result<(), String> {
    if want == "metadata" {
        // Written as regular files only (no scratch copy, no `tar -xpf`).
        let metadata = load_gpkg_metadata(gpkg_path)?;
        return extract_gpkg_metadata(gpkg_path, dest, metadata);
    }
    let (member, comp) = locate_gpkg_member(gpkg_path, want)?;

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
    // The staging directory lives inside `dest` (same filesystem, so the
    // move below is a rename) and is removed with the guard however the
    // unpack goes: a refused or truncated archive leaves `dest` empty.
    // `--strip-components=1` can not express this split: a symlink the
    // scan accepts may still defeat GNU `tar`, and its
    // partial unpack must not reach `dest`.
    let stage = ScratchDir::new_in(dest, "gpkg-image")?;
    run_tar(&["-xpf", &lossy(&inner_tar), "-C", &lossy(stage.path())])?;
    for entry in read_dir_sorted(&stage.path().join("image"))? {
        let name = entry
            .file_name()
            .ok_or_else(|| format!("{}: bad image entry name", gpkg_path.display()))?;
        fs::rename(&entry, dest.join(name))
            .map_err(|e| format!("{}: {e}", dest.join(name).display()))?;
    }
    Ok(())
}
