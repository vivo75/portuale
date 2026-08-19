// Real binary-package building (task #54's own natural sibling to
// `merge`): `ebuild <file> package` mirrors real `doebuild()`'s own
// `"package"` action -- runs the real `install` chain
// (`actionmap_deps["package"] == ["install"]`), then really invokes
// `bin/misc-functions.sh`'s own `__dyn_package` (real, unmodified bash --
// `ebuild_phases::run_misc_functions`'s own doc comment explains why this
// is a *separate* script invocation, not a `bin/ebuild.sh` phase), which
// itself shells out to the real, unmodified `bin/xpak-helper.py
// recompose` (real Python, no reimplementation needed at all) to tar
// `${D}` and append real XPAK metadata, producing a genuine
// `${PKGDIR}/${CATEGORY}/${PF}.tbz2`. `portage_repo`'s own binary-package
// reader (task #53/#63) never parses a `.tbz2`/XPAK file's own content at
// all -- only `<PKGDIR>/Packages`, a plain-text index -- so this module
// also writes/updates a real entry there, in the exact format that
// reader already parses, closing the loop: a package built this way is
// immediately visible to `emerge --pretend --usepkg`.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching this whole pilot's own
// "narrow v1, document the cut" pattern):
//   - `BINPKG_FORMAT` is always `"xpak"` (real portage's own first-listed
//     `SUPPORTED_GENTOO_BINPKG_FORMATS` default) -- the newer `"gpkg"`
//     format (`bin/gpkg-helper.py`) is a real, separately-scoped
//     alternative this slice doesn't attempt.
//   - `PORTAGE_COMPRESSION_COMMAND` is hardcoded to `"bzip2 -c"` (real
//     `make.globals`'s own `PORTAGE_COMPRESS="bzip2"` default) rather
//     than resolved through real `_compressors`/`BINPKG_COMPRESS_FLAGS_*`
//     substitution -- this pilot has no `make.conf` resolution path into
//     `ebuild.rs` at all yet, the same "env var/hardcoded default, not
//     full config resolution" shortcut `CONFIG_PROTECT` already
//     established.
//   - `USE` is always empty in the Packages index entry, matching this
//     pilot's own phase environment (`ebuild_phases`'s own setup block
//     always exports `USE=""`, a pre-existing, separately-documented v1
//     cut -- nothing was actually built with any USE flags enabled, so
//     recording an empty set is the *honest* value, not an
//     approximation).
//   - `SLOT`/`KEYWORDS`/`IUSE`/`LICENSE`/`PROPERTIES`/`RESTRICT`/the
//     `*DEPEND` family are read from the ebuild's own repo's real
//     `metadata/md5-cache` entry (via `portage_repo::read_md5_cache`,
//     the exact same source `emerge --pretend`'s own dependency
//     resolution already trusts) when the ebuild's own containing repo
//     can be found by walking up for a `profiles/repo_name` file --
//     absent entirely (empty strings throughout) for a standalone
//     ebuild file outside any repo checkout, the same tolerance
//     `ebuild_merge::repository_name_for` already established.
//   - No `BUILD_ID` support, no `packdebug`/`splitdebug` handling, no
//     RPM (`__dyn_rpm`) format.
//   - No `PKGDIR`-index locking -- a genuinely concurrent build racing
//     another write to the same `Packages` file could interleave; this
//     pilot's own single-invocation-at-a-time CLI usage never exercises
//     that.
//   - Real `EbuildBinpkg`'s own separate `bindbapi.inject()` step (an
//     in-memory binary-package database update, distinct from the
//     on-disk `Packages` file write) has no equivalent here -- this
//     pilot has no long-lived `bindbapi` process at all, only ever
//     re-reading `Packages` fresh each invocation.

use crate::ebuild_phases;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Whether `command` is the one real package-building command this
/// module implements -- `ebuild.rs` checks this alongside
/// `ebuild_phases::is_real_phase_command`/`ebuild_merge::
/// is_real_merge_command`/`ebuild_unmerge::is_real_unmerge_command`
/// before routing to real execution.
pub fn is_real_package_command(command: &str) -> bool {
    command == "package"
}

/// Options for `run_package`, bundled into a struct rather than more
/// positional parameters (the same "positional-parameter pain" lesson
/// `ebuild_merge::MergeOptions` already applied). `pkgdir` is env-var-
/// sourced at the `ebuild.rs` CLI boundary; `Default` matches real
/// `make.globals`'s own `PKGDIR="/var/cache/binpkgs"` exactly.
pub struct PackageOptions {
    pub debug: bool,
    pub pkgdir: PathBuf,
    pub distdir: PathBuf,
    pub shell: ebuild_phases::ShellBackend,
}

impl Default for PackageOptions {
    fn default() -> Self {
        Self {
            debug: false,
            pkgdir: PathBuf::from("/var/cache/binpkgs"),
            distdir: PathBuf::from("/var/cache/distfiles"),
            shell: ebuild_phases::ShellBackend::default(),
        }
    }
}

fn now_unix_time() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| format!("system clock before epoch: {e}"))
}

/// Real `<pkgdir>/Packages`'s own header block (see `portage_repo::
/// read_packages_index`'s own doc comment: always the first, blank-
/// line-terminated block, unconditionally skipped by every reader,
/// real and this pilot's own alike) -- a single real field
/// (`TIMESTAMP`) is enough to be an honest, non-empty header without
/// needing to replicate every real field (`VERSION`/`PACKAGES`/etc.)
/// this pilot's own reader never looks at anyway.
fn packages_index_header(now: u64) -> String {
    format!("TIMESTAMP: {now}\n")
}

fn format_packages_entry(fields: &[(&str, &str)]) -> String {
    let mut block = String::new();
    for (key, value) in fields {
        if !value.is_empty() {
            block.push_str(&format!("{key}: {value}\n"));
        }
    }
    block
}

/// Writes (creating the file, and its own header block, if necessary)
/// or replaces `cpv`'s own entry in `<pkgdir>/Packages` -- real
/// portage's own index format (`portage_repo::read_packages_index`'s
/// own doc comment: `KEY: value` lines, blank-line-separated blocks,
/// first block a header). A pre-existing entry for the *same* `cpv`
/// (a rebuild) is replaced in place, not duplicated -- every other
/// entry (including other versions of the same package) is preserved
/// verbatim.
fn write_packages_index_entry(
    pkgdir: &Path,
    cpv: &str,
    fields: &[(&str, &str)],
) -> Result<(), String> {
    let path = pkgdir.join("Packages");
    let now = now_unix_time()?;

    let mut blocks: Vec<String> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for block in text.split("\n\n") {
            let block = block.trim();
            if !block.is_empty() {
                blocks.push(block.to_string());
            }
        }
    }
    if blocks.is_empty() {
        blocks.push(packages_index_header(now).trim().to_string());
    }
    // Header is always blocks[0] -- keep it, drop any existing entry for
    // this exact CPV among the rest, then append the fresh one.
    let header = blocks.remove(0);
    blocks.retain(|b| !b.lines().any(|l| l == format!("CPV: {cpv}")));
    blocks.push(format_packages_entry(fields).trim().to_string());

    let mut text = header;
    for block in blocks {
        text.push_str("\n\n");
        text.push_str(&block);
    }
    text.push('\n');

    std::fs::create_dir_all(pkgdir).map_err(|e| format!("{}: {e}", pkgdir.display()))?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Real `doebuild()`'s own first step for `"package"` is always the real
/// `install` phase chain having already completed (`actionmap_deps
/// ["package"] == ["install"]`) -- run here directly, the same
/// "run it myself, don't require the caller to" shape `ebuild_merge::
/// run_merge` already established for its own `["install"]`
/// prerequisite.
pub fn run_package(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &PackageOptions,
) -> Result<i32, String> {
    let status = ebuild_phases::run_commands(
        ebuild_path,
        &["install"],
        root,
        portage_tmpdir,
        &options.distdir,
        options.debug,
        options.shell,
    )?;
    if status != 0 {
        return Ok(status);
    }

    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;

    let build_time = now_unix_time()?;
    let build_info_dir = env.build_info();
    std::fs::create_dir_all(&build_info_dir)
        .map_err(|e| format!("{}: {e}", build_info_dir.display()))?;
    std::fs::write(build_info_dir.join("BUILD_TIME"), build_time.to_string())
        .map_err(|e| format!("{}: {e}", build_info_dir.join("BUILD_TIME").display()))?;

    let binpkg_path = options
        .pkgdir
        .join(&env.category)
        .join(format!("{}.tbz2", env.split.pf));
    if let Some(parent) = binpkg_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }

    let repo_lib_path = ebuild_phases::repo_root().join("lib");
    let extra_env = vec![
        ("PKGDIR".to_string(), options.pkgdir.display().to_string()),
        (
            "PORTAGE_BINPKG_TMPFILE".to_string(),
            binpkg_path.display().to_string(),
        ),
        ("BINPKG_FORMAT".to_string(), "xpak".to_string()),
        (
            "PORTAGE_COMPRESSION_COMMAND".to_string(),
            "bzip2 -c".to_string(),
        ),
        // Real `bin/misc-functions.sh`'s own `xpak-helper.py` invocation
        // prefers `PORTAGE_PYTHONPATH` over `PORTAGE_PYM_PATH` (which
        // this pilot deliberately leaves unset -- see
        // `ebuild_phases`'s own module doc comment) -- set directly so
        // the real, unmodified `xpak-helper.py` subprocess imports
        // `portage` from *this* checkout, not whatever else might be
        // system-installed.
        (
            "PORTAGE_PYTHONPATH".to_string(),
            repo_lib_path.display().to_string(),
        ),
    ];

    let package_status = ebuild_phases::run_misc_function(
        ebuild_path,
        portage_tmpdir,
        root,
        "package",
        "__dyn_package",
        &extra_env,
        options.debug,
        options.shell,
    )?;
    if package_status != 0 {
        return Ok(package_status);
    }

    let cpv = format!("{}/{}", env.category, env.split.pf);
    let metadata: HashMap<String, String> = ebuild_phases::repo_root_for(&env.pkg_dir)
        .and_then(|repo_root| {
            portage_repo::read_md5_cache(&repo_root, &env.category, &env.split.pf).ok()
        })
        .unwrap_or_default();
    let get = |key: &str| metadata.get(key).map(String::as_str).unwrap_or("");
    let build_time_str = build_time.to_string();
    write_packages_index_entry(
        &options.pkgdir,
        &cpv,
        &[
            ("CPV", &cpv),
            ("SLOT", get("SLOT")),
            ("KEYWORDS", get("KEYWORDS")),
            ("USE", ""),
            ("LICENSE", get("LICENSE")),
            ("IUSE", get("IUSE")),
            ("PROPERTIES", get("PROPERTIES")),
            ("RESTRICT", get("RESTRICT")),
            ("DEPEND", get("DEPEND")),
            ("RDEPEND", get("RDEPEND")),
            ("BDEPEND", get("BDEPEND")),
            ("PDEPEND", get("PDEPEND")),
            ("IDEPEND", get("IDEPEND")),
            ("BUILD_TIME", &build_time_str),
        ],
    )?;

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale-ebuild-package-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn is_real_package_command_covers_exactly_package() {
        assert!(is_real_package_command("package"));
        assert!(!is_real_package_command("qmerge"));
        assert!(!is_real_package_command("merge"));
        assert!(!is_real_package_command("install"));
    }

    // `repo_root_for` itself now lives in, and is tested in,
    // `ebuild_phases.rs` (shared with `fetch_sources`'s own repo lookup
    // -- see that module's own doc comment on why it moved).

    #[test]
    fn write_packages_index_entry_creates_a_header_then_the_entry() {
        let tmp = tempdir();
        write_packages_index_entry(
            &tmp,
            "dev-libs/foo-1.0",
            &[("CPV", "dev-libs/foo-1.0"), ("SLOT", "0")],
        )
        .unwrap();
        let text = std::fs::read_to_string(tmp.join("Packages")).unwrap();
        let blocks: Vec<&str> = text.trim().split("\n\n").collect();
        assert_eq!(blocks.len(), 2, "header block + one entry: {text:?}");
        assert!(blocks[0].contains("TIMESTAMP:"));
        assert!(blocks[1].contains("CPV: dev-libs/foo-1.0"));
        assert!(blocks[1].contains("SLOT: 0"));
    }

    #[test]
    fn write_packages_index_entry_replaces_an_existing_entry_for_a_rebuild() {
        let tmp = tempdir();
        write_packages_index_entry(
            &tmp,
            "dev-libs/foo-1.0",
            &[
                ("CPV", "dev-libs/foo-1.0"),
                ("SLOT", "0"),
                ("BUILD_TIME", "100"),
            ],
        )
        .unwrap();
        write_packages_index_entry(
            &tmp,
            "dev-libs/foo-1.0",
            &[
                ("CPV", "dev-libs/foo-1.0"),
                ("SLOT", "0"),
                ("BUILD_TIME", "200"),
            ],
        )
        .unwrap();

        let text = std::fs::read_to_string(tmp.join("Packages")).unwrap();
        let blocks: Vec<&str> = text.trim().split("\n\n").collect();
        assert_eq!(blocks.len(), 2, "still header + a single entry: {text:?}");
        assert!(blocks[1].contains("BUILD_TIME: 200"));
        assert!(!text.contains("BUILD_TIME: 100"));
    }

    #[test]
    fn write_packages_index_entry_preserves_other_entries() {
        let tmp = tempdir();
        write_packages_index_entry(
            &tmp,
            "dev-libs/foo-1.0",
            &[("CPV", "dev-libs/foo-1.0"), ("SLOT", "0")],
        )
        .unwrap();
        write_packages_index_entry(
            &tmp,
            "dev-libs/bar-2.0",
            &[("CPV", "dev-libs/bar-2.0"), ("SLOT", "0")],
        )
        .unwrap();

        let text = std::fs::read_to_string(tmp.join("Packages")).unwrap();
        assert!(text.contains("CPV: dev-libs/foo-1.0"));
        assert!(text.contains("CPV: dev-libs/bar-2.0"));
    }

    #[test]
    fn real_package_builds_a_real_xpak_tbz2_and_a_real_packages_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        let status =
            run_package(&ebuild, &root, &portage_tmpdir, &options).expect("run_package succeeds");
        assert_eq!(status, 0);

        // A real file, real bzip2+tar+XPAK content -- not this pilot's
        // own invention: real portage's own `xpak.py` writes a real
        // `XPAKPACK` magic marker (`lib/portage/xpak.py`'s own
        // `xpak()`) right before the metadata blob it appends, via the
        // real, unmodified `xpak-helper.py recompose` subprocess.
        let binpkg_path = options.pkgdir.join("dev-libs/packagepkg-1.0.tbz2");
        let binpkg_bytes = std::fs::read(&binpkg_path)
            .unwrap_or_else(|e| panic!("{}: {e}", binpkg_path.display()));
        assert!(!binpkg_bytes.is_empty());
        assert!(
            binpkg_bytes.windows(8).any(|w| w == b"XPAKPACK"),
            "real XPAKPACK magic not found in {}",
            binpkg_path.display()
        );

        // Real, unmodified portage_repo::list_binary_candidates (task
        // #53/#63's own reader) sees it immediately -- the whole point
        // of writing a real Packages entry in the exact format that
        // reader already parses.
        let candidates =
            portage_repo::list_binary_candidates(&options.pkgdir, "dev-libs", "packagepkg");
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.version, "1.0");
        assert_eq!(candidate.slot, "0");
        assert_eq!(candidate.keywords, vec!["amd64".to_string()]);

        // The real RDEPEND this fixture's own md5-cache entry declares
        // came through into the Packages index too.
        let metadata =
            portage_repo::read_binary_metadata(&options.pkgdir, "dev-libs", "packagepkg", "1.0")
                .expect("binary metadata entry exists");
        assert_eq!(
            metadata.get("RDEPEND").map(String::as_str),
            Some("dev-libs/samepkg")
        );
    }
}
