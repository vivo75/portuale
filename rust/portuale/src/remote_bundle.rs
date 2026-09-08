//! Remote-merge wire bundles (slice 2, `docs/remote-merge.md` §13).
//!
//! A bundle is one binpkg prepared for stdin streaming: the extracted
//! `image/` + `build-info/` (via `binpkg::extract_binpkg`, the same
//! extraction the local merge runs), a server-decompressed `environment`
//! file (the `bzip2 -dc` step `run_phase_from_saved_env` runs locally --
//! the client never needs `bzip2`), and a `remote-manifest` the client
//! sanity-checks before unpacking. Layout inside the tar:
//!
//! ```text
//! <pf>/image/...           the merge image (${D} on the client)
//! <pf>/build-info/...      CONTENTS, ebuild, DEFINED_PHASES, *DEPEND, ...
//! <pf>/environment         decompressed hook env (absent when the binpkg
//!                          carries no environment.bz2)
//! <pf>/remote-manifest     FORMAT=1 + CPV/SLOT/REPO/HAS_ENVIRONMENT
//! ```
//!
//! Tarred uncompressed (`tar -cf`, system tar like `binpkg::run_tar` --
//! wire compression is a future slice); the byte count travels beside the
//! stream, not inside it, so the client can reject a truncated transfer
//! before touching anything.

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
    /// Byte count the client must observe (`wc -c` equality gate).
    pub byte_count: u64,
    /// The manifest shipped inside (and parsed back here for the
    /// server-side report line).
    pub manifest: BundleManifest,
}

/// Stage one binpkg file (`.gpkg.tar` or `.tbz2`) into a wire bundle
/// under `staging_tmp` (a fresh temp dir per call; caller removes it).
pub fn build_bundle(binpkg_path: &Path, staging_tmp: &Path) -> Result<StagedBundle, String> {
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
        .unwrap_or_else(|| "__unknown__".to_string());

    // Same extraction the local merge runs: image/ + build-info/ land
    // staged exactly as merge_binpkg would see them.
    let unit = staging_tmp.join(&pf);
    let image = unit.join("image");
    let build_info = unit.join("build-info");
    crate::binpkg::extract_binpkg(binpkg_path, &image, &build_info)?;

    // Server-side `bzip2 -dc`: the client never needs bzip2 (plan §6).
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
    // Fail-early: prove the manifest parses back before it ever ships
    // (the client greps the same shape).
    if BundleManifest::parse(&manifest_text).is_none() {
        return Err("remote-manifest does not parse back".to_string());
    }
    std::fs::write(unit.join("remote-manifest"), manifest_text)
        .map_err(|e| format!("remote-manifest: {e}"))?;

    // Uncompressed tar of `<pf>/` (wire compression is a future slice).
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
    Ok(StagedBundle {
        tarball,
        byte_count,
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale-remote-bundle-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
    fn bundle_stages_image_build_info_environment_and_manifest() {
        let tmp = tempdir("stage");
        let staged = build_bundle(&fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"), &tmp)
            .expect("fixture tbz2 stages");
        assert_eq!(staged.manifest.cpv, "dev-libs/packagepkg-1.0");
        assert!(staged.byte_count > 0);
        assert_eq!(
            std::fs::metadata(&staged.tarball).unwrap().len(),
            staged.byte_count
        );
        // The tar lists exactly one top-level unit dir with the four
        // expected members.
        let listing = std::process::Command::new("tar")
            .args(["-tf"])
            .arg(&staged.tarball)
            .output()
            .expect("tar -tf runs");
        assert!(listing.status.success());
        // The tar lists exactly one top-level unit dir with the
        // guaranteed members (image/, build-info/, remote-manifest;
        // environment because this fixture carries environment.bz2).
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
}
