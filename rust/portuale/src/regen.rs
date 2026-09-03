//! `emerge --regen` (real `_emerge/actions.py::action_regen` +
//! `_emerge/MetadataRegen.py`): regenerate every repo's on-disk
//! `metadata/md5-cache/<cat>/<pf>` by running each ebuild's `depend`
//! phase and writing the result in real `portage.cache.flat_hash`
//! (`md5_database`) format -- `KEY=value` lines, keys sorted, empty
//! values omitted, `_md5_=<md5 of the ebuild file>` last.
//!
//! Real portage runs the `depend` phases through the scheduler (parallel,
//! `--jobs`); this v1 is sequential -- `--jobs` is accepted (the CLI
//! parses it) but not yet threaded through. Real `--regen` also drops a
//! stale cache entry whose ebuild has vanished (`MetadataRegen` iterates
//! `cp_all()` then diffs against the on-disk cache); this v1 only
//! (re)writes entries for ebuilds that exist -- a documented cut.
//!
//! Like every real filesystem-mutating `emerge` action, this rejects
//! `--pretend` (real `actions.py:4106-4111`) at the CLI layer before
//! reaching here.

use crate::ebuild_phases;
use md5::{Digest as _, Md5};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

/// Real `portage.dbapi.porttree`'s own md5-cache key set
/// (`portage.auxdbkeys`), plus the two synthetic keys
/// `portage.cache.flat_hash` always adds (`_eclasses_`, `_<chf>_` =
/// `_md5_`). Real `flat_hash.database.__init__` writes
/// `sorted(known_keys | {"_eclasses_", "_md5_"})` -- and `_` (0x5F) sorts
/// after every uppercase letter, so the two synthetic keys land last.
const WRITE_KEYS: &[&str] = &[
    "BDEPEND",
    "DEFINED_PHASES",
    "DEPEND",
    "DESCRIPTION",
    "EAPI",
    "HOMEPAGE",
    "IDEPEND",
    "INHERIT",
    "INHERITED",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "PDEPEND",
    "PROPERTIES",
    "RDEPEND",
    "REQUIRED_USE",
    "RESTRICT",
    "SLOT",
    "SRC_URI",
    "_eclasses_",
    "_md5_",
];

pub fn run(config_root: &Path, root: &Path) -> ExitCode {
    // `--debug`/`-d` is not wired through the pretend CLI path; the
    // `depend` phase always runs non-debug here.
    const DEBUG: bool = false;
    let repos = match portage_repo::find_repos(config_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp/portage"));

    // Real `MetadataRegen._iter_metadata_processes`: iterate `cp_all()`
    // and, per `cat/pkg`, every ebuild version. `Regenerating cache
    // entries...` then `Processing <cp>` per cp (stdout), `done!` at the
    // end (`action_regen`).
    println!("Regenerating cache entries...");

    let mut failures = 0u32;
    for cp in portage_repo::all_cp(&repos) {
        println!("Processing {cp}");
        let (category, package) = match cp.split_once('/') {
            Some(x) => x,
            None => continue,
        };
        for repo in &repos {
            let pkg_dir = repo.location.join(category).join(package);
            let Ok(entries) = std::fs::read_dir(&pkg_dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let Some(pf) = name.strip_suffix(".ebuild") else {
                    continue;
                };
                if let Err(e) = regen_one(
                    &entry.path(),
                    &repo.location,
                    category,
                    pf,
                    root,
                    config_root,
                    &portage_tmpdir,
                    DEBUG,
                ) {
                    eprintln!(" * {e}");
                    failures += 1;
                }
            }
        }
    }

    println!("done!");
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[allow(clippy::too_many_arguments)]
fn regen_one(
    ebuild_path: &Path,
    repo_location: &Path,
    category: &str,
    pf: &str,
    root: &Path,
    config_root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
) -> Result<(), String> {
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    let md = ebuild_phases::run_depend_phase(&env, root, config_root, debug)?;

    let ebuild_bytes =
        std::fs::read(ebuild_path).map_err(|e| format!("{}: {e}", ebuild_path.display()))?;
    let ebuild_md5 = format!("{:x}", Md5::digest(&ebuild_bytes));

    // Real `flat_hash._setitem`: `for k in self._write_keys: v =
    // values.get(k); if not v: continue; write f"{k}={v}\n"`. Sorted
    // keys, empty values skipped.
    let mut fields: BTreeMap<&str, String> = BTreeMap::new();
    for &key in WRITE_KEYS {
        if key == "_md5_" || key == "_eclasses_" {
            continue;
        }
        if let Some(v) = md.get(key) {
            if !v.is_empty() {
                fields.insert(key, v.clone());
            }
        }
    }
    if let Some(ec) = eclasses_field(&md, repo_location) {
        fields.insert("_eclasses_", ec);
    }
    fields.insert("_md5_", ebuild_md5);

    let mut out = String::new();
    for &key in WRITE_KEYS {
        if let Some(v) = fields.get(key) {
            out.push_str(key);
            out.push('=');
            out.push_str(v);
            out.push('\n');
        }
    }

    let cache_dir = repo_location.join("metadata/md5-cache").join(category);
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("{}: {e}", cache_dir.display()))?;
    let cache_file = cache_dir.join(pf);
    let tmp = cache_dir.join(format!(".{pf}.regen"));
    std::fs::write(&tmp, out.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &cache_file).map_err(|e| format!("{}: {e}", cache_file.display()))?;
    Ok(())
}

/// Real `metadata.database._setitem`: `_eclasses_` is serialized as
/// `name\tmd5\tname\tmd5...` (`serialize_eclasses(..., "md5")`). Built
/// from the `depend` phase's own `INHERITED` list -- for each eclass
/// name, md5 of `<repo>/eclass/<name>.eclass` (this repo only; a
/// masters-chain lookup is a documented cut, none of portuale's
/// fixture ebuilds inherit). `None` when nothing was inherited.
fn eclasses_field(
    md: &std::collections::HashMap<String, String>,
    repo_location: &Path,
) -> Option<String> {
    let names = md.get("INHERITED").map(String::as_str).unwrap_or("");
    let names: Vec<&str> = names.split_whitespace().collect();
    if names.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for name in names {
        let path = repo_location.join("eclass").join(format!("{name}.eclass"));
        let bytes = std::fs::read(&path).ok()?;
        parts.push(name.to_string());
        parts.push(format!("{:x}", Md5::digest(&bytes)));
    }
    Some(parts.join("\t"))
}
