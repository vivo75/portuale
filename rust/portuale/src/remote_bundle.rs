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
    /// Server-side `_pkgsplit` values, re-exported by the phase driver:
    /// the binary-branch load filter strips `CATEGORY/PVR/PF/PN/PR/PV/P`
    /// from the saved environment (see `split_pf`), so -- like local
    /// `phase_env_vars` -- these travel outside the file.
    pub eapi: String,
    pub category: String,
    pub pn: String,
    pub pv: String,
    pub pr: String,
    pub pvr: String,
    pub p: String,
    pub pf: String,
    /// Byte count the client must observe (`wc -c` equality gate).
    pub byte_count: u64,
    /// The manifest shipped inside (and parsed back here for the
    /// server-side report line).
    pub manifest: BundleManifest,
    /// Phases the client may run, in order: `pretend`/`setup`/`preinst`
    /// intersected with the binpkg's own `DEFINED_PHASES`, and only when
    /// both the ebuild file and the hook environment shipped (the local
    /// `merge_binpkg` degrade, mirrored).
    pub phases: Vec<String>,
}

/// Split `package-version` (`PF` without category) into `(PN, PVR)`
/// the way real `_pkgsplit` does: the version is the longest trailing
/// `-`-separated suffix that `ververify` accepts, so a package name may
/// itself contain digit-led words (`foo-1bar-2.0` -> `foo-1bar`).
/// Needed because the binary-branch load filter strips
/// `CATEGORY/PVR/PF/PN/PR/PV/P` from the saved environment (package
/// renames must not leak across), so the driver re-exports them from
/// server-side values -- mirroring local `phase_env_vars`.
pub fn split_pf(pf: &str) -> Option<(String, String)> {
    let words: Vec<&str> = pf.split('-').collect();
    for i in 1..words.len() {
        let candidate = words[i..].join("-");
        // A `-r<digits>` revision belongs to the version, not the name --
        // but only when the rest still verifies (else `foo-r1-2.0` would
        // mis-split; real `_pkgsplit` has the same shape).
        if portage_versions::ververify(&candidate) {
            return Some((words[..i].join("-"), candidate));
        }
    }
    None
}

/// Split `PVR` into `(PV, PR)`: trailing `-r<digits>` is the revision,
/// else `PR` is real portage's own `"r0"` default.
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
/// `EAPI=` line of the ebuild itself (PMS 7.3.1 rule). `None` when
/// neither exists (fail-early: real binpkgs always carry it).
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

/// Which of the slice-3 client phases (`pretend`/`setup`/`preinst`) a
/// binpkg's `DEFINED_PHASES` actually defines -- and only when the ebuild
/// file plus the hook environment are present to run them from (same
/// gate as the local merge's `extracted_ebuild` + `phase_defined`).
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

    // The ebuild runtime the client phases run under (`bash bin/ebuild.sh
    // <phase>`, same files the local merge drives). Per unit for now; a
    // per-session ship is the obvious later optimization (note the ~500K
    // in the trial logs if it ever matters).
    let bin_dir = crate::ebuild_phases::bin_dir();
    let status = std::process::Command::new("cp")
        .args(["-a"])
        .arg(bin_dir)
        .arg(unit.join("bin"))
        .status()
        .map_err(|e| format!("failed to spawn cp: {e}"))?;
    if !status.success() {
        return Err(format!("cp -a {} failed ({status})", bin_dir.display()));
    }

    let ebuild_file = build_info.join(format!("{pf}.ebuild"));
    let phases = select_phases(
        &std::fs::read_to_string(build_info.join("DEFINED_PHASES")).unwrap_or_default(),
        ebuild_file.is_file(),
        has_environment,
    );

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
    let (pn, pvr) =
        split_pf(&pf).ok_or_else(|| format!("{}: cannot split package name from version", pf))?;
    let (pv, pr) = split_pvr(&pvr);
    let p = format!("{pn}-{pv}");
    let ebuild_file = build_info.join(format!("{pf}.ebuild"));
    let eapi = read_eapi(&build_info, &ebuild_file)
        .ok_or_else(|| format!("{}: no EAPI in build-info nor ebuild", pf))?;
    Ok(StagedBundle {
        tarball,
        byte_count,
        manifest,
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
        // Missing ebuild or environment degrades to no phases (the local
        // merge's extracted_ebuild gate, mirrored).
        assert!(select_phases("pretend setup preinst", false, true).is_empty());
        assert!(select_phases("pretend setup preinst", true, false).is_empty());
        assert!(select_phases("-", true, true).is_empty());
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
