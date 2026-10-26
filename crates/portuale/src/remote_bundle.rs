//! Remote-merge wire bundles: one binpkg prepared for stdin streaming.
//!
//! Layout inside the uncompressed tar:
//!
//! ```text
//! <pf>/image/...           the merge image (${D} on the client)
//! <pf>/build-info/...      CONTENTS, ebuild, DEFINED_PHASES, *DEPEND, ...
//! <pf>/environment         decompressed hook env (absent when the binpkg
//!                          carries no environment.bz2)
//! <pf>/remote-manifest     FORMAT=1 + CPV/SLOT/REPO/HAS_ENVIRONMENT
//! <pf>/bin/...             the ebuild runtime (`bash bin/ebuild.sh`)
//! ```
//!
//! The byte count travels beside the stream, not inside it, so the client
//! can reject a truncated transfer before touching anything.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parsed `remote-manifest` (client- and server-side shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleManifest {
    /// Manifest format version; only `1` exists.
    pub format: u64,
    /// `category/package-version`.
    pub cpv: String,
    /// Full `slot/sub_slot` (`SLOT` metadata verbatim).
    pub slot: String,
    /// Binpkg `repository`/`REPO` metadata (`__unknown__` fallback, same
    /// as the local merge).
    pub repo: String,
    /// Whether `environment` is in the bundle.
    pub has_environment: bool,
}

impl BundleManifest {
    /// Render `KEY=VALUE` lines (the client greps these; no spaces in
    /// keys, values are single-line metadata).
    pub fn render(&self) -> String {
        format!(
            "FORMAT={}\nCPV={}\nSLOT={}\nREPO={}\nHAS_ENVIRONMENT={}\n",
            self.format,
            self.cpv,
            self.slot,
            self.repo,
            if self.has_environment { "yes" } else { "no" },
        )
    }

    /// Parse back; `None` on missing keys, bad `FORMAT`, or multiline
    /// values (fail-closed: a forged manifest never parses half-way).
    pub fn parse(text: &str) -> Option<Self> {
        let mut map = HashMap::new();
        for line in text.lines() {
            let (key, value) = line.split_once('=')?;
            if !key.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                || value.contains('\n')
                || value.contains('\r')
            {
                return None;
            }
            map.insert(key.to_string(), value.to_string());
        }
        let format: u64 = map.get("FORMAT")?.parse().ok()?;
        if format != 1 {
            return None;
        }
        Some(Self {
            format,
            cpv: map.get("CPV")?.clone(),
            slot: map.get("SLOT")?.clone(),
            repo: map.get("REPO")?.clone(),
            has_environment: match map.get("HAS_ENVIRONMENT")?.as_str() {
                "yes" => true,
                "no" => false,
                _ => return None,
            },
        })
    }
}

/// A staged bundle: the tarball path plus its parsed manifest and the
/// `category/package-version` the client unpacks as.
pub struct StagedBundle {
    /// `bundle.tar` bytes, ready for stdin streaming.
    pub tarball: PathBuf,
    /// Server-side `_pkgsplit` values, re-exported by the phase driver
    /// (they travel outside the saved environment file).
    pub eapi: String,
    pub category: String,
    pub pn: String,
    pub pv: String,
    pub pr: String,
    pub pvr: String,
    pub p: String,
    pub pf: String,
    /// Byte count the client must observe.
    pub byte_count: u64,
    /// The manifest shipped inside.
    pub manifest: BundleManifest,
    /// Phases the client may run, in order: `pretend`/`setup`/`preinst`
    /// intersected with the binpkg's own `DEFINED_PHASES`, and only when
    /// both the ebuild file and the hook environment shipped.
    pub phases: Vec<String>,
    /// Whether the client may run `postinst` after the merge (same gate
    /// as `phases`, checked against the `postinst` word).
    pub postinst_defined: bool,
}

/// The client's `${PORTAGE_BZIP2_COMMAND}` stand-in for the merge-time
/// environment-regeneration postinst run: ignores its arguments and copies
/// stdin to stdout. Shipped in the runtime `bin` dir; only ever invoked
/// through Portage's `PORTAGE_UPDATE_ENV` block.
pub const BZIP2_PASSTHROUGH: &str = "#!/bin/sh\n# Backlog #171: PORTAGE_BZIP2_COMMAND stand-in -- ignore flags, pass stdin through.\nexec cat\n";

/// Copy the ebuild runtime into the bundle: always the files, never
/// an overlay's symlinks (`cp -aL` dereferences them).
pub(crate) fn copy_bin_runtime(bin_dir: &Path, dest: &Path) -> Result<(), String> {
    let status = std::process::Command::new("cp")
        .args(["-aL"])
        .arg(bin_dir)
        .arg(dest)
        .status()
        .map_err(|e| format!("failed to spawn cp: {e}"))?;
    if !status.success() {
        return Err(format!("cp -aL {} failed ({status})", bin_dir.display()));
    }
    Ok(())
}
/// Split `PVR` into `(PV, PR)`: trailing `-r<digits>` is the revision,
/// else `PR` is `"r0"`.
pub fn split_pvr(pvr: &str) -> (String, String) {
    if let Some((stem, rev)) = pvr.rsplit_once('-')
        && rev.starts_with('r')
        && rev[1..].chars().all(|c| c.is_ascii_digit())
        && !rev[1..].is_empty()
        && !stem.is_empty()
    {
        return (stem.to_string(), rev.to_string());
    }
    (pvr.to_string(), "r0".to_string())
}

/// EAPI for the driver exports: `build-info/EAPI` first, else the
/// `EAPI=` line of the ebuild itself. `None` when neither exists.
pub fn read_eapi(build_info: &Path, ebuild_file: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(build_info.join("EAPI")) {
        let value = text.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    let text = std::fs::read_to_string(ebuild_file).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("EAPI=") {
            let value = value.trim().trim_matches('"').trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// One image entry for the client's merge: type, content hash (files
/// only), source mtime, and `/`-rooted relative path.
pub struct FileMeta {
    /// `obj`, `dir` or `sym`.
    pub kind: &'static str,
    /// Hex MD5 of file bytes (`obj`), `-` otherwise.
    pub md5: String,
    /// Source mtime seconds, recorded verbatim into CONTENTS.
    pub mtime: i64,
    /// Path relative to the image root, `/`-separated, no leading `/`.
    pub rel: String,
    /// Symlink target (`sym` only).
    pub target: Option<String>,
}

/// Render `filemeta` lines: `<kind> <md5|-> <mtime> <relpath>[ <target>]`.
/// Paths with whitespace or newlines are rejected at build time.
pub fn render_filemeta(entries: &[FileMeta]) -> Result<String, String> {
    let mut out = String::new();
    for entry in entries {
        if entry.rel.chars().any(|c| c.is_whitespace()) {
            return Err(format!("image path {:?} has whitespace", entry.rel));
        }
        out.push_str(&format!(
            "{} {} {} {}",
            entry.kind, entry.md5, entry.mtime, entry.rel
        ));
        if let Some(target) = &entry.target {
            if target.chars().any(|c| c.is_whitespace()) {
                return Err(format!("symlink target {:?} has whitespace", target));
            }
            out.push_str(&format!(" {target}"));
        }
        out.push('\n');
    }
    Ok(out)
}

/// Walk `image/` (sorted, like the local `merge_tree`) recording type, MD5
/// and mtime per entry. Anything but a file, directory or symlink is an
/// error.
pub fn collect_filemeta(image: &Path) -> Result<Vec<FileMeta>, String> {
    use std::os::unix::fs::MetadataExt as _;
    fn md5_file(path: &Path) -> Result<String, String> {
        use md5::Digest as _;
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(portage_util::hex_lower(&md5::Md5::digest(&bytes)))
    }
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative_dir) = stack.pop() {
        let src_dir = image.join(&relative_dir);
        let children: Vec<PathBuf> = portage_util::read_dir_entries(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .into_iter()
            .map(|e| relative_dir.join(e.file_name()))
            .collect();
        for relative_path in children {
            let src = image.join(&relative_path);
            let meta =
                std::fs::symlink_metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            let file_type = meta.file_type();
            let rel = relative_path.to_string_lossy().replace('\\', "/");
            let mtime = meta.mtime();
            if file_type.is_symlink() {
                let target = std::fs::read_link(&src)
                    .map_err(|e| format!("{}: {e}", src.display()))?
                    .to_string_lossy()
                    .to_string();
                out.push(FileMeta {
                    kind: "sym",
                    md5: "-".to_string(),
                    mtime,
                    rel,
                    target: Some(target),
                });
            } else if file_type.is_dir() {
                stack.push(relative_path);
                out.push(FileMeta {
                    kind: "dir",
                    md5: "-".to_string(),
                    mtime,
                    rel,
                    target: None,
                });
            } else if file_type.is_file() {
                out.push(FileMeta {
                    kind: "obj",
                    md5: md5_file(&src)?,
                    mtime,
                    rel,
                    target: None,
                });
            } else {
                return Err(format!(
                    "{}: special file in image (only obj/dir/sym supported)",
                    src.display()
                ));
            }
        }
    }
    Ok(out)
}

/// Which of the client phases (`pretend`/`setup`/`preinst`) a
/// binpkg's `DEFINED_PHASES` actually defines -- and only when the ebuild
/// file plus the hook environment are present to run them from.
pub fn select_phases(defined_phases: &str, has_ebuild: bool, has_environment: bool) -> Vec<String> {
    if !(has_ebuild && has_environment) {
        return Vec::new();
    }
    ["pretend", "setup", "preinst"]
        .into_iter()
        .filter(|phase| defined_phases.split_whitespace().any(|word| word == *phase))
        .map(String::from)
        .collect()
}

/// Arguments of [`build_bundle`].
pub struct BuildBundleArgs<'a> {
    pub binpkg_path: &'a Path,
    pub staging_tmp: &'a Path,
    pub repo_override: Option<&'a str>,
    pub gpg: &'a crate::binpkg::GpgVerify,
    pub install_mask: &'a str,
    pub install_mask_prunes_usr_share: bool,
}

/// Stage one binpkg file (`.gpkg.tar` or `.tbz2`) into a wire bundle
/// under `staging_tmp` (a fresh temp dir per call; caller removes it).
/// `repo_override` fills the manifest only when the binpkg's own metadata
/// carries no `repository`/`REPO` key. `install_mask` /
/// `install_mask_prunes_usr_share` prune the staged image before
/// `collect_filemeta`, like the local merge. `gpg` is the same
/// merge-time signature policy the local merge runs.
pub fn build_bundle(args: BuildBundleArgs<'_>) -> Result<StagedBundle, String> {
    let BuildBundleArgs {
        binpkg_path,
        staging_tmp,
        repo_override,
        gpg,
        install_mask,
        install_mask_prunes_usr_share,
    } = args;
    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let is_gpkg = name.ends_with(".gpkg.tar");
    if !is_gpkg && !name.ends_with(".tbz2") {
        return Err(format!(
            "{}: not a binary package (need .gpkg.tar or .tbz2)",
            binpkg_path.display()
        ));
    }
    let meta = if is_gpkg {
        crate::binpkg::read_gpkg_metadata(binpkg_path)?
    } else {
        crate::binpkg::read_xpak_metadata(binpkg_path)?
    };
    let meta_get = |key: &str| -> Option<String> {
        meta.get(key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let category = meta_get("CATEGORY")
        .ok_or_else(|| format!("{}: binpkg has no CATEGORY", binpkg_path.display()))?;
    let pf =
        meta_get("PF").ok_or_else(|| format!("{}: binpkg has no PF", binpkg_path.display()))?;
    let slot = meta_get("SLOT").unwrap_or_else(|| "0".to_string());
    let repo = meta_get("repository")
        .or_else(|| meta_get("REPO"))
        .or_else(|| {
            repo_override
                .map(str::to_string)
                .filter(|s| !s.is_empty() && s != "__unknown__")
        })
        .unwrap_or_else(|| "__unknown__".to_string());

    // Same extraction the local merge runs: image/ + build-info/ land
    // staged exactly as merge_binpkg would see them.
    let unit = staging_tmp.join(&pf);
    let image = unit.join("image");
    let build_info = unit.join("build-info");
    // The xattr restore covers the local merge only.
    crate::binpkg::extract_binpkg(
        binpkg_path,
        &image,
        &build_info,
        gpg,
        &crate::ebuild_merge::XattrPolicy::default(),
    )?;

    // The same BINPKGMD5 digest the local merge records (whole binpkg
    // file), written into the staged `build-info/` so the client's
    // verbatim vdb copy lands it.
    // Portage: _emerge/Binpkg.py, _start_task
    let binpkg_md5 = crate::ebuild_merge::md5_hex(binpkg_path)?;
    std::fs::write(build_info.join("BINPKGMD5"), format!("{binpkg_md5}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("BINPKGMD5").display()))?;

    // The same `apply_install_mask` the local merge runs prunes the staged
    // image here, before `collect_filemeta` below.
    // Portage: vartree.py, treewalk (mask before collision-protect)
    let mask_options = crate::ebuild_merge::MergeOptions {
        install_mask: install_mask.to_string(),
        install_mask_prunes_usr_share,
        ..crate::ebuild_merge::MergeOptions::default()
    };
    crate::ebuild_merge::apply_install_mask(&image, &build_info, &mask_options)?;

    // Server-side `bzip2 -dc`: the client never needs bzip2.
    let saved_env = build_info.join("environment.bz2");
    let has_environment = if saved_env.is_file() {
        let dest_env = unit.join("environment");
        let out =
            std::fs::File::create(&dest_env).map_err(|e| format!("{}: {e}", dest_env.display()))?;
        let status = std::process::Command::new("bzip2")
            .args(["-d", "-c", "--"])
            .arg(&saved_env)
            .stdout(std::process::Stdio::from(out))
            .status()
            .map_err(|e| format!("failed to spawn bzip2: {e}"))?;
        if !status.success() {
            return Err(format!(
                "bzip2 failed to decompress {} ({status})",
                saved_env.display()
            ));
        }
        true
    } else {
        false
    };

    let manifest = BundleManifest {
        format: 1,
        cpv: format!("{category}/{pf}"),
        slot,
        repo,
        has_environment,
    };
    let manifest_text = manifest.render();
    // Prove the manifest parses back before it ever ships.
    if BundleManifest::parse(&manifest_text).is_none() {
        return Err("remote-manifest does not parse back".to_string());
    }
    std::fs::write(unit.join("remote-manifest"), manifest_text)
        .map_err(|e| format!("remote-manifest: {e}"))?;

    // The ebuild runtime the client phases run under (same files the
    // local merge drives).
    let bin_dir = crate::ebuild_phases::bin_dir()?;
    copy_bin_runtime(bin_dir, &unit.join("bin"))?;
    // The `PORTAGE_BZIP2_COMMAND` pass-through rides the shipped runtime
    // `bin` dir (see `BZIP2_PASSTHROUGH`).
    {
        use std::os::unix::fs::PermissionsExt as _;
        let passthrough = unit.join("bin").join("bzip2-passthrough");
        std::fs::write(&passthrough, BZIP2_PASSTHROUGH)
            .map_err(|e| format!("bzip2-passthrough: {e}"))?;
        std::fs::set_permissions(&passthrough, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("bzip2-passthrough: {e}"))?;
    }

    let ebuild_file = build_info.join(format!("{pf}.ebuild"));
    let defined = std::fs::read_to_string(build_info.join("DEFINED_PHASES")).unwrap_or_default();
    let phases = select_phases(&defined, ebuild_file.is_file(), has_environment);
    let postinst_defined = has_environment
        && ebuild_file.is_file()
        && defined.split_whitespace().any(|word| word == "postinst");

    // Per-file type/hash/mtime for the client's merge.
    let filemeta = render_filemeta(&collect_filemeta(&image)?)?;
    std::fs::write(unit.join("filemeta"), &filemeta).map_err(|e| format!("filemeta: {e}"))?;

    // Uncompressed tar of `<pf>/`.
    let tarball = staging_tmp.join("bundle.tar");
    let status = std::process::Command::new("tar")
        .args(["-cf"])
        .arg(&tarball)
        .args(["-C"])
        .arg(staging_tmp)
        .arg(&pf)
        .status()
        .map_err(|e| format!("failed to spawn tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar -cf {} failed ({status})", tarball.display()));
    }
    let byte_count = std::fs::metadata(&tarball)
        .map_err(|e| format!("{}: {e}", tarball.display()))?
        .len();
    let (pn, pvr) = portage_repo::split_pf(&pf)
        .ok_or_else(|| format!("{}: cannot split package name from version", pf))?;
    let (pv, pr) = split_pvr(&pvr);
    let p = format!("{pn}-{pv}");
    let ebuild_file = build_info.join(format!("{pf}.ebuild"));
    let eapi = read_eapi(&build_info, &ebuild_file)
        .ok_or_else(|| format!("{}: no EAPI in build-info nor ebuild", pf))?;
    Ok(StagedBundle {
        tarball,
        byte_count,
        manifest,
        postinst_defined,
        eapi,
        category,
        pn,
        pv,
        pr,
        pvr,
        p,
        pf,
        phases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        TempDir::new(&format!(
            "portuale-remote-bundle-{}-{}",
            std::process::id(),
            tag
        ))
        .keep()
    }

    #[test]
    fn manifest_round_trips_and_rejects_garbage() {
        let manifest = BundleManifest {
            format: 1,
            cpv: "dev-libs/packagepkg-1.0".to_string(),
            slot: "0".to_string(),
            repo: "testrepo".to_string(),
            has_environment: true,
        };
        assert_eq!(BundleManifest::parse(&manifest.render()), Some(manifest));
        assert_eq!(BundleManifest::parse("FORMAT=2\nCPV=x\n"), None);
        assert_eq!(BundleManifest::parse("CPV=x\n"), None);
        assert_eq!(
            BundleManifest::parse("FORMAT=1\nCPV=x\nSLOT=0\nREPO=r\nHAS_ENVIRONMENT=maybe\n"),
            None
        );
        assert_eq!(BundleManifest::parse("lowercase=1\n"), None);
    }

    #[test]
    fn select_phases_intersects_defined_words_in_order() {
        assert_eq!(
            select_phases("install postinst postrm preinst prerm setup", true, true),
            vec!["setup".to_string(), "preinst".to_string(),]
        );
        assert_eq!(
            select_phases("pretend setup preinst postinst", true, true),
            vec![
                "pretend".to_string(),
                "setup".to_string(),
                "preinst".to_string(),
            ]
        );
        // Missing ebuild or environment degrades to no phases.
        assert!(select_phases("pretend setup preinst", false, true).is_empty());
        assert!(select_phases("pretend setup preinst", true, false).is_empty());
        assert!(select_phases("-", true, true).is_empty());
    }

    #[test]
    fn bundle_stages_image_build_info_environment_and_manifest() {
        let tmp = tempdir("stage");
        let staged = build_bundle(BuildBundleArgs {
            binpkg_path: &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            staging_tmp: &tmp,
            repo_override: None,
            gpg: &crate::binpkg::GpgVerify::default(),
            install_mask: "",
            install_mask_prunes_usr_share: false,
        })
        .expect("fixture tbz2 stages");
        assert_eq!(staged.manifest.cpv, "dev-libs/packagepkg-1.0");
        assert!(staged.byte_count > 0);
        assert_eq!(
            std::fs::metadata(&staged.tarball).unwrap().len(),
            staged.byte_count
        );
        // The tar lists one top-level unit dir with the four members below.
        let listing = std::process::Command::new("tar")
            .args(["-tf"])
            .arg(&staged.tarball)
            .output()
            .expect("tar -tf runs");
        assert!(listing.status.success());
        // `environment` rides along because this fixture carries one.
        let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
        for member in [
            "packagepkg-1.0/image/",
            "packagepkg-1.0/build-info/",
            "packagepkg-1.0/remote-manifest",
            "packagepkg-1.0/environment",
        ] {
            assert!(listing.contains(member), "{member} missing:\n{listing}");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bundle_repo_falls_back_to_resolver_override() {
        // The fixture carries no `repository`/`REPO` key, so the
        // resolver-known repo fills the manifest.
        let tmp = tempdir("repo-override");
        let staged = build_bundle(BuildBundleArgs {
            binpkg_path: &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            staging_tmp: &tmp,
            repo_override: Some("testrepo"),
            gpg: &crate::binpkg::GpgVerify::default(),
            install_mask: "",
            install_mask_prunes_usr_share: false,
        })
        .expect("fixture tbz2 stages");
        assert_eq!(staged.manifest.repo, "testrepo");
        let _ = std::fs::remove_dir_all(&tmp);
        let tmp = tempdir("repo-unknown");
        let staged = build_bundle(BuildBundleArgs {
            binpkg_path: &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            staging_tmp: &tmp,
            repo_override: Some("__unknown__"),
            gpg: &crate::binpkg::GpgVerify::default(),
            install_mask: "",
            install_mask_prunes_usr_share: false,
        })
        .expect("fixture tbz2 stages");
        assert_eq!(staged.manifest.repo, "__unknown__");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The bundle carries `build-info/BINPKGMD5` into the vdb verbatim,
    /// and the tar carries it too.
    #[test]
    fn bundle_writes_binpkgmd5_of_the_shipped_binpkg() {
        use md5::Digest as _;
        let tmp = tempdir("binpkgmd5");
        let binpkg = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
        let staged = build_bundle(BuildBundleArgs {
            binpkg_path: &binpkg,
            staging_tmp: &tmp,
            repo_override: None,
            gpg: &crate::binpkg::GpgVerify::default(),
            install_mask: "",
            install_mask_prunes_usr_share: false,
        })
        .expect("fixture tbz2 stages");
        let bytes = std::fs::read(&binpkg).unwrap();
        let expected = format!("{}\n", portage_util::hex_lower(&md5::Md5::digest(&bytes)));
        assert_eq!(expected.len(), 33, "real BINPKGMD5 is 33 B");
        assert_eq!(
            std::fs::read_to_string(tmp.join("packagepkg-1.0/build-info/BINPKGMD5")).unwrap(),
            expected,
        );
        let listing = std::process::Command::new("tar")
            .args(["-tf"])
            .arg(&staged.tarball)
            .output()
            .expect("tar -tf runs");
        assert!(listing.status.success());
        let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
        assert!(
            listing.contains("packagepkg-1.0/build-info/BINPKGMD5"),
            "BINPKGMD5 must ride the bundle:\n{listing}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// With a non-empty mask the bundle prunes the staged image before
    /// `collect_filemeta` and ships `build-info/INSTALL_MASK`.
    #[test]
    fn bundle_applies_install_mask_before_filemeta() {
        let tmp = tempdir("mask");
        let staged = build_bundle(BuildBundleArgs {
            binpkg_path: &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            staging_tmp: &tmp,
            repo_override: None,
            gpg: &crate::binpkg::GpgVerify::default(),
            install_mask: "/usr/share/packagepkg",
            install_mask_prunes_usr_share: false,
        })
        .expect("fixture tbz2 stages");
        assert_eq!(
            std::fs::read_to_string(tmp.join("packagepkg-1.0/build-info/INSTALL_MASK")).unwrap(),
            "/usr/share/packagepkg\n",
        );
        assert!(
            !tmp.join("packagepkg-1.0/image/usr/share/packagepkg/hello.txt")
                .exists(),
            "masked file must be pruned from the staged image"
        );
        let filemeta = std::fs::read_to_string(tmp.join("packagepkg-1.0/filemeta")).unwrap();
        assert!(
            !filemeta.contains("hello.txt"),
            "masked file must not reach filemeta:\n{filemeta}"
        );
        let listing = std::process::Command::new("tar")
            .args(["-tf"])
            .arg(&staged.tarball)
            .output()
            .expect("tar -tf runs");
        assert!(listing.status.success());
        let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
        assert!(
            listing.contains("packagepkg-1.0/build-info/INSTALL_MASK"),
            "INSTALL_MASK must ride the bundle:\n{listing}"
        );
        assert!(
            !listing.contains("hello.txt"),
            "masked file must not ride the bundle:\n{listing}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// With an empty mask nothing is pruned and no `INSTALL_MASK` file ships.
    #[test]
    fn bundle_empty_mask_prunes_nothing_and_writes_no_file() {
        let tmp = tempdir("mask-empty");
        let _staged = build_bundle(BuildBundleArgs {
            binpkg_path: &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            staging_tmp: &tmp,
            repo_override: None,
            gpg: &crate::binpkg::GpgVerify::default(),
            install_mask: "",
            install_mask_prunes_usr_share: false,
        })
        .expect("fixture tbz2 stages");
        assert!(
            tmp.join("packagepkg-1.0/image/usr/share/packagepkg/hello.txt")
                .is_file(),
            "empty mask must leave the image alone"
        );
        let filemeta = std::fs::read_to_string(tmp.join("packagepkg-1.0/filemeta")).unwrap();
        assert!(
            filemeta.contains("hello.txt"),
            "unmasked file must reach filemeta:\n{filemeta}"
        );
        assert!(
            !tmp.join("packagepkg-1.0/build-info/INSTALL_MASK").exists(),
            "empty mask must write no INSTALL_MASK file"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The bundle ships an executable `bin/bzip2-passthrough` that ignores
    /// compression flags while passing stdin through.
    #[test]
    fn bundle_ships_bzip2_passthrough_for_regen() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempdir("passthrough");
        let _staged = build_bundle(BuildBundleArgs {
            binpkg_path: &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            staging_tmp: &tmp,
            repo_override: None,
            gpg: &crate::binpkg::GpgVerify::default(),
            install_mask: "",
            install_mask_prunes_usr_share: false,
        })
        .expect("fixture tbz2 stages");
        let passthrough = tmp.join("packagepkg-1.0/bin/bzip2-passthrough");
        assert_eq!(
            std::fs::read_to_string(&passthrough).unwrap(),
            BZIP2_PASSTHROUGH,
        );
        assert_eq!(
            std::fs::metadata(&passthrough)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755,
            "the client must execute it through ${{PORTAGE_BZIP2_COMMAND}}"
        );
        let listing = std::process::Command::new("tar")
            .args(["-tf"])
            .arg(tmp.join("bundle.tar"))
            .output()
            .expect("tar -tf runs");
        assert!(listing.status.success());
        let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
        assert!(
            listing.contains("packagepkg-1.0/bin/bzip2-passthrough"),
            "passthrough must ride the bundle:\n{listing}"
        );
        // Ignores Portage's `-c -f9`, copies stdin to stdout.
        let output = std::process::Command::new("sh")
            .arg(&passthrough)
            .args(["-c", "-f9"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(b"regen-bytes\n")?;
                child.wait_with_output()
            })
            .expect("passthrough runs");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"regen-bytes\n");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The bundle ships files, never an overlay's symlinks: an absolute
    /// symlink copies as the target's bytes.
    #[test]
    fn copy_bin_runtime_dereferences_symlinks() {
        let tmp = tempdir("dereference");
        let runtime = tmp.join("runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(runtime.join("ebuild.sh"), "#!/bin/bash\n").unwrap();
        let elsewhere = tmp.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("helper.sh"), "helper bytes\n").unwrap();
        std::os::unix::fs::symlink(elsewhere.join("helper.sh"), runtime.join("helper.sh")).unwrap();
        assert!(
            std::fs::symlink_metadata(runtime.join("helper.sh"))
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let dest = tmp.join("unit").join("bin");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        copy_bin_runtime(&runtime, &dest).expect("runtime copies");
        let copied = dest.join("helper.sh");
        assert!(
            !std::fs::symlink_metadata(&copied)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the bundle must carry the target's bytes, not the link"
        );
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "helper bytes\n");
        assert!(dest.join("ebuild.sh").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
