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
//   - Real `PORTAGE_COMPRESSION_COMMAND` resolution (real
//     `_compressors`/`BINPKG_COMPRESS_FLAGS_*`, `doebuild.py:697-750`) is
//     real now -- see `resolve_compression_command`'s own doc comment
//     for the exact real mechanics and v1 narrowing (no full shell
//     `varexpand`, real host CPU count for `{JOBS}`). `BINPKG_COMPRESS`/
//     `BINPKG_COMPRESS_FLAGS[_<NAME>]`/`PORTAGE_BZIP2_COMMAND` are env-
//     var-sourced at the `ebuild.rs` CLI boundary, the same "env var,
//     not full config resolution" shortcut `CONFIG_PROTECT` already
//     established; `Default` matches real `make.globals`'s own
//     `BINPKG_COMPRESS="zstd"` (**not** `"bzip2"` -- real portage's own
//     default changed at some point; this pilot's own previous hardcoded
//     `"bzip2 -c"` predated noticing that).
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
use std::os::unix::fs::PermissionsExt;
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
    /// Real `BINPKG_COMPRESS` (real `make.globals`'s own default:
    /// `"zstd"`) -- see `resolve_compression_command`'s own doc comment.
    pub binpkg_compress: String,
    /// Real `BINPKG_COMPRESS_FLAGS_<NAME>` (the per-compressor override,
    /// `<NAME>` = `binpkg_compress` uppercased) if set, else real
    /// `BINPKG_COMPRESS_FLAGS` -- already resolved to the single value
    /// to use, at the `ebuild.rs` CLI boundary, so this module itself
    /// doesn't need to know about the per-compressor override naming
    /// convention at all. Real `make.globals` sets neither, so
    /// `Default` is empty.
    pub binpkg_compress_flags: String,
    /// Real `PORTAGE_BZIP2_COMMAND` (real `make.globals`'s own default:
    /// `"bzip2"`) -- only actually substituted when `binpkg_compress ==
    /// "bzip2"`.
    pub portage_bzip2_command: String,
    /// Real `PORTAGE_CONFIGROOT` -- see `ebuild_merge::MergeOptions::
    /// config_root`'s own doc comment for the exact real default/`Default`
    /// split this mirrors (only consulted by `ebuild_phases::
    /// eclass_locations_value`'s own masters-chain resolution).
    pub config_root: PathBuf,
}

impl Default for PackageOptions {
    fn default() -> Self {
        Self {
            debug: false,
            pkgdir: PathBuf::from("/var/cache/binpkgs"),
            distdir: PathBuf::from("/var/cache/distfiles"),
            shell: ebuild_phases::ShellBackend::default(),
            binpkg_compress: "zstd".to_string(),
            binpkg_compress_flags: String::new(),
            portage_bzip2_command: "bzip2".to_string(),
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
        }
    }
}

/// Real `_compressors` (`lib/portage/util/compression_probe.py:10-53`),
/// narrowed to the `"compress"` half only -- this module only ever
/// *builds* a binpkg, never installs from one, so the real
/// `"decompress"`/`"decompress_alt"` fields (relevant only when
/// *installing* from a binpkg) have no equivalent here. `{JOBS}` is a
/// plain, non-`${...}`, pre-`varexpand` substitution (real
/// `doebuild.py:721-724`/`:740-743`); `${...}` placeholders are resolved
/// afterward by `resolve_compression_command`.
fn compress_template(name: &str) -> Option<&'static str> {
    Some(match name {
        "bzip2" => "${PORTAGE_BZIP2_COMMAND} ${BINPKG_COMPRESS_FLAGS}",
        "gzip" => "gzip ${BINPKG_COMPRESS_FLAGS}",
        "lz4" => "lz4 ${BINPKG_COMPRESS_FLAGS}",
        "lzip" => "lzip ${BINPKG_COMPRESS_FLAGS}",
        "lzop" => "lzop ${BINPKG_COMPRESS_FLAGS}",
        "xz" => "xz -T{JOBS} --memlimit-compress=50% -q ${BINPKG_COMPRESS_FLAGS}",
        "zstd" => "zstd -T{JOBS} ${BINPKG_COMPRESS_FLAGS}",
        _ => return None,
    })
}

/// Real `find_binary()` (`lib/portage/process.py`): the first `PATH`
/// entry containing an executable file named `name`.
fn find_binary(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(name))
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

/// Real `PORTAGE_COMPRESSION_COMMAND` resolution (`doebuild.py:697-750`):
/// looks up `binpkg_compress` in the real `_compressors` table,
/// substitutes `{JOBS}` (real host CPU count, matching real
/// `makeopts_to_job_count`'s own `get_cpu_count()` fallback path --
/// this pilot's own `MAKEOPTS` is always unset, the same path real code
/// takes whenever `MAKEOPTS` doesn't itself contain a `-j`/`--jobs=`
/// value) and `${PORTAGE_BZIP2_COMMAND}`/`${BINPKG_COMPRESS_FLAGS}` (a
/// plain, narrow `${VAR}` substitution -- not a full shell `varexpand`,
/// since none of the six real templates or realistic flag values need
/// anything beyond that), `shlex.split()`s the result (narrowed to
/// whitespace-splitting -- same reasoning, no real quoting need), and
/// confirms the resolved binary is real-`PATH`-findable (real
/// `find_binary()`).
///
/// Returns `None` -- the caller omits `PORTAGE_COMPRESSION_COMMAND` from
/// the exported environment entirely -- for an unknown `binpkg_compress`
/// name or a compressor whose binary isn't installed, matching real
/// behavior exactly: `mysettings["PORTAGE_COMPRESSION_COMMAND"]` is left
/// unset in both real cases too (only warned about, real `writemsg` --
/// not reproduced, this module's own real-execution path has no
/// message-printing output anywhere else either), so real, unmodified
/// `bin/misc-functions.sh` hits its own real `[[ -z
/// "${PORTAGE_COMPRESSION_COMMAND}" ]] && die "PORTAGE_COMPRESSION_
/// COMMAND is unset"` guard naturally.
fn resolve_compression_command(
    binpkg_compress: &str,
    binpkg_compress_flags: &str,
    portage_bzip2_command: &str,
) -> Option<String> {
    let template = compress_template(binpkg_compress)?;
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let expanded = template
        .replace("{JOBS}", &jobs.to_string())
        .replace("${PORTAGE_BZIP2_COMMAND}", portage_bzip2_command)
        .replace("${BINPKG_COMPRESS_FLAGS}", binpkg_compress_flags);
    let tokens: Vec<&str> = expanded.split_whitespace().collect();
    let binary = *tokens.first()?;
    if !find_binary(binary) {
        return None;
    }
    Some(tokens.join(" "))
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
        &options.config_root,
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
    let mut extra_env = vec![
        ("PKGDIR".to_string(), options.pkgdir.display().to_string()),
        (
            "PORTAGE_BINPKG_TMPFILE".to_string(),
            binpkg_path.display().to_string(),
        ),
        ("BINPKG_FORMAT".to_string(), "xpak".to_string()),
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
    if let Some(compression_command) = resolve_compression_command(
        &options.binpkg_compress,
        &options.binpkg_compress_flags,
        &options.portage_bzip2_command,
    ) {
        extra_env.push((
            "PORTAGE_COMPRESSION_COMMAND".to_string(),
            compression_command,
        ));
    }

    let package_status = ebuild_phases::run_misc_function(
        ebuild_path,
        portage_tmpdir,
        root,
        "package",
        "__dyn_package",
        &extra_env,
        options.debug,
        &options.config_root,
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

    #[test]
    fn compress_template_covers_exactly_the_real_six_compressors() {
        for name in ["bzip2", "gzip", "lz4", "lzip", "lzop", "xz", "zstd"] {
            assert!(
                compress_template(name).is_some(),
                "{name} should be a known real compressor"
            );
        }
        assert_eq!(compress_template("made-up-codec"), None);
    }

    #[test]
    fn find_binary_finds_a_real_path_entry_and_rejects_a_bogus_name() {
        assert!(find_binary("sh"), "sh should be on a real test PATH");
        assert!(!find_binary(
            "this-binary-definitely-does-not-exist-anywhere-xyz"
        ));
    }

    #[test]
    fn resolve_compression_command_substitutes_bzip2_var_and_flags() {
        // "bzip2" is used as both the compressor name and the
        // ${PORTAGE_BZIP2_COMMAND} value here so find_binary succeeds
        // without depending on any *other* binary actually being
        // installed on the test-running host.
        let cmd = resolve_compression_command("bzip2", "-9", "bzip2")
            .expect("bzip2 should be found on a real test PATH");
        assert_eq!(cmd, "bzip2 -9");
    }

    #[test]
    fn resolve_compression_command_substitutes_gzip_flags_with_no_bzip2_var() {
        let cmd = resolve_compression_command("gzip", "-9", "bzip2")
            .expect("gzip should be found on a real test PATH");
        assert_eq!(cmd, "gzip -9");
    }

    #[test]
    fn resolve_compression_command_substitutes_jobs_for_xz_and_zstd() {
        // {JOBS} is real host CPU count -- not pinned to a fixed value,
        // just proven to have actually been substituted (no literal
        // "{JOBS}" left, and a real positive integer follows "-T").
        for name in ["xz", "zstd"] {
            let cmd = resolve_compression_command(name, "", "bzip2")
                .unwrap_or_else(|| panic!("{name} should be found on a real test PATH"));
            assert!(!cmd.contains("{JOBS}"), "{cmd}");
            let jobs_token = cmd
                .split_whitespace()
                .find_map(|tok| tok.strip_prefix("-T"))
                .unwrap_or_else(|| panic!("{cmd} should contain a -T<jobs> token"));
            assert!(
                jobs_token.parse::<u32>().is_ok(),
                "-T should be followed by a real positive integer, got {jobs_token:?}"
            );
        }
    }

    #[test]
    fn resolve_compression_command_is_none_for_an_unknown_compressor() {
        assert_eq!(
            resolve_compression_command("made-up-codec", "", "bzip2"),
            None
        );
    }

    #[test]
    fn resolve_compression_command_is_none_when_the_bzip2_var_names_a_missing_binary() {
        // A real compressor name, but ${PORTAGE_BZIP2_COMMAND} resolves
        // to a binary that isn't actually installed -- real behavior:
        // PORTAGE_COMPRESSION_COMMAND is left unset (the caller omits it
        // from the exported environment), not a fabricated fallback.
        assert_eq!(
            resolve_compression_command(
                "bzip2",
                "",
                "this-binary-definitely-does-not-exist-anywhere-xyz"
            ),
            None
        );
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
            // Real xpak/tbz2 building is codec-agnostic (this pilot's
            // own `portage_repo` binpkg reader never parses a `.tbz2`'s
            // own content, only `Packages`) -- pinned to "bzip2"
            // explicitly here (rather than the real `Default`, "zstd")
            // so this test doesn't depend on the test-running host
            // actually having `zstd` installed; `bzip2` is a
            // near-universal base package.
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
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
        let index = portage_repo::BinaryIndex::from_pkgdir(&options.pkgdir);
        let candidates = portage_repo::list_binary_candidates(&index, "dev-libs", "packagepkg");
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.version, "1.0");
        assert_eq!(candidate.slot, "0");
        assert_eq!(candidate.keywords, vec!["amd64".to_string()]);

        // The real RDEPEND this fixture's own md5-cache entry declares
        // came through into the Packages index too.
        let metadata = portage_repo::read_binary_metadata(&index, "dev-libs", "packagepkg", "1.0")
            .expect("binary metadata entry exists");
        assert_eq!(
            metadata.get("RDEPEND").map(String::as_str),
            Some("dev-libs/samepkg")
        );
    }
}
