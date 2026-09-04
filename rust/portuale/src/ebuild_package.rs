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
// KNOWN, DOCUMENTED GAPS (v1 scope, matching portuale's own
// "narrow v1, document the cut" pattern):
//   - `BINPKG_FORMAT` (`"xpak"` -- real portage's own first-listed
//     `SUPPORTED_GENTOO_BINPKG_FORMATS` default -- or `"gpkg"`) is real
//     now: for `"gpkg"`, real, unmodified `bin/misc-functions.sh
//     __dyn_package` shells out to real, unmodified `bin/gpkg-helper.py
//     compress` (real `portage.gpkg.gpkg().compress()`, no
//     reimplementation) exactly the way the `"xpak"` branch already
//     shells out to `bin/xpak-helper.py recompose`, producing a genuine
//     `${PKGDIR}/${CATEGORY}/${PF}.gpkg.tar` portuale's own
//     `binpkg::read_gpkg_metadata` reader round-trips. Anything other
//     than those two values is `Err("Unknown BINPKG_FORMAT ...")`, real
//     `__dyn_package`'s own `die`. Cut: no gpkg signing
//     (`FEATURES=binpkg-signing`/`binpkg-request-signature` -- the same
//     "portuale has no crypto" cut the reader's `Manifest`/`.sig`
//     verification already documents). `BUILD_ID` in the gpkg basename
//     IS real now, opt-in (`PackageOptions::binpkg_multi_instance`'s
//     own doc comment).
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
//     default changed at some point; portuale's own previous hardcoded
//     `"bzip2 -c"` predated noticing that).
//   - `USE` in the Packages index entry is empty for a standalone
//     `ebuild <file> package` (no resolved graph → no resolved USE, so
//     `""` is the honest value). An `emerge <atom> -b` build *does* now
//     run its phases with the resolved `USE`
//     (`MergeOptions::build_env`), so a package built that way carries
//     its real flags in build-info; propagating those into this index
//     entry is a small remaining follow-up.
//   - `SLOT`/`KEYWORDS`/`IUSE`/`LICENSE`/`PROPERTIES`/`RESTRICT`/the
//     `*DEPEND` family in the `Packages` *index* entry are read from the
//     ebuild's own repo's real `metadata/md5-cache` entry (via
//     `portage_repo::read_md5_cache`, the exact same source `emerge
//     --pretend`'s own dependency resolution already trusts) when the
//     ebuild's own containing repo can be found by walking up for a
//     `profiles/repo_name` file -- absent entirely (empty strings
//     throughout) for a standalone ebuild file outside any repo
//     checkout. The `.tbz2`'s own appended XPAK metadata gets the same
//     keys independently, from real `build-info` (`bin/phase-functions.
//     sh` + `ebuild_phases::write_post_install_metadata`) -- for a
//     USE-conditional dep string the two can differ (the index entry is
//     flat md5-cache, the XPAK is `use_reduce`'d against the empty
//     phase-side USE set); `--pretend` reads the index entry, so this is
//     cosmetic. Reading the index entry from `build-info` too, for a
//     single source of truth, is a documented follow-up.
//   - No `packdebug`/`splitdebug` handling, no RPM (`__dyn_rpm`) format.
//   - `PKGDIR`-index locking IS real now (`write_packages_index_entry`'s
//     own doc comment) -- the same real `flock(2)`-based
//     `PortageLockfile` `fetch.rs`'s own distlocks already use, wrapping
//     the whole read-modify-write sequence, matching real `bintree.
//     inject`/`update_pkgindex`/`remove`.
//   - Real `EbuildBinpkg`'s own separate `bindbapi.inject()` step (an
//     in-memory binary-package database update, distinct from the
//     on-disk `Packages` file write) has no equivalent here -- this
//     portuale has no long-lived `bindbapi` process at all, only ever
//     re-reading `Packages` fresh each invocation.

use crate::binpkg;
use crate::ebuild_merge;
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
    /// Real `BINPKG_FORMAT` (real `make.globals`'s own default: `"xpak"`,
    /// the first entry of `SUPPORTED_GENTOO_BINPKG_FORMATS`). `"gpkg"` is
    /// the only other accepted value; `run_package` rejects anything else
    /// with `Err("Unknown BINPKG_FORMAT ...")`, real `bin/misc-functions.
    /// sh __dyn_package`'s own `die`.
    pub binpkg_format: String,
    /// Real `PORTAGE_CONFIGROOT` -- see `ebuild_merge::MergeOptions::
    /// config_root`'s own doc comment for the exact real default/`Default`
    /// split this mirrors (only consulted by `ebuild_phases::
    /// eclass_locations_value`'s own masters-chain resolution).
    pub config_root: PathBuf,
    /// Real `FEATURES=buildpkg-live` (`_emerge/Package.py:621-637`'s own
    /// `binpkg_wanted`): whether a `PROPERTIES=live` build also gets
    /// packaged when `buildpkg` is otherwise on. Real default is `true`
    /// (`buildpkg-live` is one of real `make.globals`'s own default
    /// `FEATURES` tokens) -- explicit `FEATURES=-buildpkg-live` is the
    /// only way to skip packaging a live build. Consulted by
    /// `emerge_build::entry_buildpkg_wanted`, not by this module itself
    /// (a live-vs-not decision needs the resolved graph entry, which
    /// this module never sees -- it just builds whatever it's told to).
    pub buildpkg_live: bool,
    /// Real `FEATURES=binpkg-multi-instance` (`bintree.py:526-531`) --
    /// real default is `true` (one of real `make.globals`'s own default
    /// `FEATURES` tokens too), but this defaults to `false` here: real's
    /// multi-instance shape genuinely differs by format -- a `.gpkg.tar`
    /// is self-contained either way, so multi-instance there is purely
    /// a `{pf}-{build_id}.gpkg.tar` filename convention
    /// (`package_after_install`'s own `allocate_gpkg_build_id`), but a
    /// real multi-instance *xpak* binpkg is a bare `.xpak` metadata
    /// segment on a genuinely different on-disk layout (not the single-
    /// instance `.tbz2` archive shape `binpkg::read_xpak_metadata`
    /// already reads) that this doesn't attempt -- so matching real's
    /// actual default here would silently keep producing single-
    /// instance-shaped xpak binpkgs while claiming multi-instance is on.
    /// Opt in explicitly (`FEATURES=binpkg-multi-instance`) to get
    /// gpkg's own real multi-instance naming; xpak stays bare regardless.
    pub binpkg_multi_instance: bool,
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
            binpkg_format: "xpak".to_string(),
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
            buildpkg_live: true,
            binpkg_multi_instance: false,
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
/// portuale's own `MAKEOPTS` is always unset, the same path real code
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
/// real and portuale's own alike) -- a single real field
/// (`TIMESTAMP`) is enough to be an honest, non-empty header without
/// needing to replicate every real field (`VERSION`/`PACKAGES`/etc.)
/// portuale's own reader never looks at anyway.
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
///
/// Real `bintree.inject`/`update_pkgindex`/`remove` (`bintree.py:948`/
/// `:1999`/`:2059`) all wrap this exact "reread the index, in case
/// another process changed it, then update it" sequence in a real,
/// blocking `lockfile(self._pkgindex_file, wantnewlockfile=1)` --
/// `PortageLockfile::acquire`, the same real primitive `fetch.rs`'s own
/// distfile locking already uses. A genuinely concurrent `emerge -b`/
/// `--buildpkg` racing another write to the same `Packages` file could
/// otherwise interleave (portuale's own single-invocation-at-a-time CLI
/// usage rarely exercises this, but it's cheap and correct to hold the
/// same real lock real portage does regardless).
fn write_packages_index_entry(
    pkgdir: &Path,
    cpv: &str,
    fields: &[(&str, &str)],
) -> Result<(), String> {
    let path = pkgdir.join("Packages");
    let now = now_unix_time()?;

    // Real order: `os.makedirs(self.pkgdir, exist_ok=True)` *then*
    // acquire the lock (`bintree.py:2057-2059`) -- the lock file itself
    // is a sibling of `Packages`, so its own parent dir must exist first.
    std::fs::create_dir_all(pkgdir).map_err(|e| format!("{}: {e}", pkgdir.display()))?;
    let _lock = crate::portage_lock::PortageLockfile::acquire(&path)?;

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
    // Header is always blocks[0] -- keep it, then drop any existing
    // entry this fresh one replaces, matching real `_inject_file`'s own
    // dedup (`bintree.py:2237-2247`): primarily by `PATH` (a *different*
    // `PATH` -- e.g. a different `BUILD_ID` under multi-instance -- is a
    // genuinely different on-disk file and survives untouched, letting
    // multiple builds of the same `CPV` coexist), falling back to `CPV`
    // alone only when this entry carries no `PATH` at all.
    let header = blocks.remove(0);
    let path_value = fields
        .iter()
        .find(|(k, _)| *k == "PATH")
        .map(|(_, v)| *v)
        .unwrap_or("");
    if path_value.is_empty() {
        blocks.retain(|b| !b.lines().any(|l| l == format!("CPV: {cpv}")));
    } else {
        blocks.retain(|b| !b.lines().any(|l| l == format!("PATH: {path_value}")));
    }
    blocks.push(format_packages_entry(fields).trim().to_string());

    let mut text = header;
    for block in blocks {
        text.push_str("\n\n");
        text.push_str(&block);
    }
    text.push('\n');

    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    // `_lock` drops here, releasing the flock -- same real effect real
    // `unlockfile()` has.
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
        &[],
    )?;
    if status != 0 {
        return Ok(status);
    }
    package_after_install(ebuild_path, root, portage_tmpdir, options)
}

/// The packaging tail of `run_package`, split out so it can also run as a
/// side effect of a source merge (`FEATURES=buildpkg` / `--buildpkg`,
/// real `_emerge/EbuildBinpkg`: after `src_install`, before the vdb
/// merge -- `ebuild_merge::run_merge`'s own `buildpkg` param). Assumes a
/// populated `${D}` from a prior real `install` chain.
pub(crate) fn package_after_install(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &PackageOptions,
) -> Result<i32, String> {
    let binpkg_extension = binpkg_extension(&options.binpkg_format)?;

    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;

    let build_time = now_unix_time()?;
    let build_info_dir = env.build_info();
    std::fs::create_dir_all(&build_info_dir)
        .map_err(|e| format!("{}: {e}", build_info_dir.display()))?;
    std::fs::write(build_info_dir.join("BUILD_TIME"), build_time.to_string())
        .map_err(|e| format!("{}: {e}", build_info_dir.join("BUILD_TIME").display()))?;

    // Real FEATURES=binpkg-multi-instance (`bintree.py:529-531`'s own
    // `_allocate_filename_multi` swap-in, real default-on --
    // `PackageOptions::binpkg_multi_instance`'s own doc comment has the
    // full grounding) -- gpkg only for now: a `.gpkg.tar` is self-
    // contained regardless of multi-instance mode, so this is purely a
    // filename convention (`{pf}-{build_id}.gpkg.tar`); real xpak
    // multi-instance uses a genuinely different on-disk shape (a bare
    // `.xpak` metadata segment, not a `.tbz2` archive) this doesn't
    // attempt.
    let build_id = (options.binpkg_multi_instance && binpkg_format_is_gpkg(&options.binpkg_format))
        .then(|| allocate_gpkg_build_id(&options.pkgdir, &env.category, &env.split.pf));
    let binpkg_filename = match build_id {
        Some(id) => format!("{}-{id}.{binpkg_extension}", env.split.pf),
        None => format!("{}.{binpkg_extension}", env.split.pf),
    };
    let binpkg_path = options.pkgdir.join(&env.category).join(binpkg_filename);
    if let Some(parent) = binpkg_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }

    let package_status =
        invoke_dyn_package(ebuild_path, portage_tmpdir, root, options, &binpkg_path)?;
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
    // Real `_pkgindex_entry` (`bintree.py:2311`) always writes `PATH`,
    // regardless of format -- not gpkg-only. Needed for correctness (a
    // remote fetch falls back to a bare `<pf>.<ext>` guess without it,
    // real `gettbz2`'s own `if not rel_url: rel_url = pkgname + ".tbz2"`)
    // and now also for `binpkg::populate_local_pkgdir`'s own mtime-
    // staleness fast path, which looks up a cached entry by this same
    // `PATH`'s basename. Reuses `binpkg_path`'s own filename (bare, or
    // `-{build_id}`-suffixed under multi-instance) rather than
    // reconstructing it, so the two can never drift apart.
    let path_field = format!(
        "{}/{}",
        env.category,
        binpkg_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
    );
    let build_id_str = build_id.map(|id| id.to_string()).unwrap_or_default();
    let size_str = std::fs::metadata(&binpkg_path)
        .map(|st| st.len().to_string())
        .unwrap_or_default();
    let mtime_str = std::fs::metadata(&binpkg_path)
        .ok()
        .map(|st| binpkg::file_mtime(&st).to_string())
        .unwrap_or_default();
    write_packages_index_entry(
        &options.pkgdir,
        &cpv,
        &[
            ("CPV", &cpv),
            ("PF", &env.split.pf),
            ("CATEGORY", &env.category),
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
            ("PATH", &path_field),
            ("BUILD_TIME", &build_time_str),
            ("SIZE", &size_str),
            ("_mtime_", &mtime_str),
            ("BUILD_ID", &build_id_str),
        ],
    )?;

    Ok(0)
}

/// Real `bin/misc-functions.sh __dyn_package`'s own `die "Unknown
/// BINPKG_FORMAT ${BINPKG_FORMAT}"` -- rejected here rather than letting
/// the real bash hit it, so the caller gets a clean `Err` instead of a
/// phase-script failure exit code.
fn binpkg_extension(binpkg_format: &str) -> Result<&'static str, String> {
    match binpkg_format {
        "xpak" => Ok("tbz2"),
        "gpkg" => Ok("gpkg.tar"),
        other => Err(format!("Unknown BINPKG_FORMAT {other}")),
    }
}

fn binpkg_format_is_gpkg(binpkg_format: &str) -> bool {
    binpkg_format == "gpkg"
}

/// Real `_allocate_filename_multi`'s own build_id search
/// (`bintree.py:2607-2669`), narrowed to gpkg (see `PackageOptions::
/// binpkg_multi_instance`'s own doc comment for why xpak isn't
/// attempted): starts one past the highest `BUILD_ID` already used by
/// this `cp/pf` (scanning `{pkgdir}/{category}/{pf}-*.gpkg.tar`, real's
/// own `_max_build_id` equivalent -- portuale has no long-lived
/// `bindbapi` to consult instead, so this re-derives it from the
/// filesystem each call), then increments past any that's since become
/// occupied (real's own "avoid races" `while True` retry loop, narrowed
/// to a single-process CLI that never races itself: existence alone is
/// enough, no `open(..., "x")` placeholder-file dance needed).
fn allocate_gpkg_build_id(pkgdir: &Path, category: &str, pf: &str) -> u64 {
    let dir = pkgdir.join(category);
    let prefix = format!("{pf}-");
    let mut max_existing: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Some(rest) = name.strip_prefix(&prefix) else {
                continue;
            };
            let Some(id_str) = rest.strip_suffix(".gpkg.tar") else {
                continue;
            };
            if let Ok(id) = id_str.parse::<u64>() {
                max_existing = max_existing.max(id);
            }
        }
    }
    let mut build_id = max_existing + 1;
    while dir.join(format!("{pf}-{build_id}.gpkg.tar")).exists() {
        build_id += 1;
    }
    build_id
}

/// Runs the real, unmodified `bin/misc-functions.sh __dyn_package`
/// against `ebuild_path` (tars `${D}` + appends `${PORTAGE_BUILDDIR}/
/// build-info` as the xpak segment / hands both to `gpkg-helper.py`),
/// with the compressor / `PKGDIR` / `PORTAGE_BINPKG_TMPFILE` environment
/// real portage sets. Shared by `run_package` (fresh `src_install`
/// image) and `quickpkg_from_vdb` (installed-files image).
fn invoke_dyn_package(
    ebuild_path: &Path,
    portage_tmpdir: &Path,
    root: &Path,
    options: &PackageOptions,
    binpkg_path: &Path,
) -> Result<i32, String> {
    let binpkg_format = options.binpkg_format.as_str();
    let repo_lib_path = ebuild_phases::portage_checkout().join("lib");
    let mut extra_env = vec![
        ("PKGDIR".to_string(), options.pkgdir.display().to_string()),
        (
            "PORTAGE_BINPKG_TMPFILE".to_string(),
            binpkg_path.display().to_string(),
        ),
        ("BINPKG_FORMAT".to_string(), binpkg_format.to_string()),
        // Real `bin/misc-functions.sh`'s own `xpak-helper.py`/
        // `gpkg-helper.py` invocation prefers `PORTAGE_PYTHONPATH` over
        // `PORTAGE_PYM_PATH` (which portuale deliberately leaves unset
        // -- see `ebuild_phases`'s own module doc comment) -- set
        // directly so the real, unmodified helper subprocess imports
        // `portage` from *this* checkout, not whatever else might be
        // system-installed.
        (
            "PORTAGE_PYTHONPATH".to_string(),
            repo_lib_path.display().to_string(),
        ),
    ];
    if binpkg_format == "gpkg" {
        // Real `gpkg-helper.py` builds its own `portage.settings` inside
        // the subprocess and reads the compressor from it
        // (`BINPKG_COMPRESS`, `BINPKG_COMPRESS_FLAGS[_<NAME>]`, and
        // `${PORTAGE_BZIP2_COMMAND}` via `varexpand` -- real
        // `gpkg._get_binary_cmd`), NOT from `PORTAGE_COMPRESSION_COMMAND`
        // (that is xpak's `bin/misc-functions.sh` tar-pipe only). Export
        // the same values portuale already resolves at the `ebuild.rs`
        // CLI boundary so the gpkg build is deterministic rather than
        // inheriting the host's own `make.conf`.
        extra_env.push((
            "BINPKG_COMPRESS".to_string(),
            options.binpkg_compress.clone(),
        ));
        extra_env.push((
            format!(
                "BINPKG_COMPRESS_FLAGS_{}",
                options.binpkg_compress.to_uppercase()
            ),
            options.binpkg_compress_flags.clone(),
        ));
        extra_env.push((
            "BINPKG_COMPRESS_FLAGS".to_string(),
            options.binpkg_compress_flags.clone(),
        ));
        extra_env.push((
            "PORTAGE_BZIP2_COMMAND".to_string(),
            options.portage_bzip2_command.clone(),
        ));
    } else if let Some(compression_command) = resolve_compression_command(
        &options.binpkg_compress,
        &options.binpkg_compress_flags,
        &options.portage_bzip2_command,
    ) {
        extra_env.push((
            "PORTAGE_COMPRESSION_COMMAND".to_string(),
            compression_command,
        ));
    }

    ebuild_phases::run_misc_function(
        ebuild_path,
        portage_tmpdir,
        root,
        "package",
        "__dyn_package",
        &extra_env,
        options.debug,
        &options.config_root,
        options.shell,
    )
}

/// Real `dblink.quickpkg` + `_quickpkg_dblink` (`vartree.py:2307` /
/// `:6296`) -- `FEATURES=unmerge-backup`'s pre-unmerge binpkg of the
/// **installed** package, built from its files under `${ROOT}` per
/// `CONTENTS` rather than from a fresh `src_install`. Stages the
/// installed tree into `${PORTAGE_BUILDDIR}/image` and a verbatim copy
/// of the vdb dir into `${PORTAGE_BUILDDIR}/build-info`, then runs the
/// same real, unmodified `bin/misc-functions.sh __dyn_package` the
/// `ebuild <file> package` / `--buildpkgonly` paths already use -- which
/// tars `${D}` and appends `build-info/` as the xpak segment, byte-shape
/// identical to real quickpkg's own `tar_contents(...)` +
/// `xpak.tbz2(...).recompose_mem(xpak.xpak(dbdir))`. A `$PKGDIR/Packages`
/// entry is written from the vdb's own recorded dependency metadata (not
/// md5-cache -- the package may no longer be in any repo).
///
/// Real `include_config=False`: a CONFIG_PROTECT'd (not -MASK'd) file is
/// left out of the image. fifo/device `CONTENTS` nodes are skipped -- a
/// documented cut, the same `CAP_MKNOD` limitation the merge side's own
/// `create_special_node` has. Returns `Ok(None)` when `$PKGDIR` already
/// holds a binpkg for this cpv (real portage's own `BUILD_TIME`
/// idempotency check, narrowed here to file existence).
#[allow(clippy::too_many_arguments)]
pub(crate) fn quickpkg_from_vdb(
    root: &Path,
    category: &str,
    package: &str,
    pf: &str,
    scratch_ebuild_dir: &Path,
    portage_tmpdir: &Path,
    options: &PackageOptions,
    config_protect: &str,
    config_protect_mask: &str,
) -> Result<Option<PathBuf>, String> {
    let ext = binpkg_extension(&options.binpkg_format)?;
    let binpkg_path = options.pkgdir.join(category).join(format!("{pf}.{ext}"));
    if binpkg_path.exists() {
        return Ok(None);
    }

    let vdb_dir = root.join("var/db/pkg").join(category).join(pf);
    let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS"))
        .map_err(|e| format!("{}: {e}", vdb_dir.join("CONTENTS").display()))?;
    let vdb_ebuild = vdb_dir.join(format!("{pf}.ebuild"));
    if !vdb_ebuild.is_file() {
        return Err(format!(
            "{}: no vdb ebuild to package from (installed before portuale kept one?)",
            vdb_dir.display()
        ));
    }

    // Copy the vdb ebuild into a <cat>/<pn>/<pf>.ebuild layout so
    // `compute_environment`'s path parse works (the vdb layout is
    // <cat>/<pf>/<pf>.ebuild).
    let src_dir = scratch_ebuild_dir.join(category).join(package);
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("{}: {e}", src_dir.display()))?;
    let scratch_ebuild = src_dir.join(format!("{pf}.ebuild"));
    std::fs::copy(&vdb_ebuild, &scratch_ebuild)
        .map_err(|e| format!("{}: {e}", vdb_ebuild.display()))?;

    let env = ebuild_phases::compute_environment(&scratch_ebuild, portage_tmpdir)?;

    // Fresh image dir, then stage every CONTENTS entry from ${ROOT}.
    let image = env.d();
    let _ = std::fs::remove_dir_all(&image);
    std::fs::create_dir_all(&image).map_err(|e| format!("{}: {e}", image.display()))?;
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let (Some(kind), Some(abs)) = (fields.next(), fields.next()) else {
            continue;
        };
        let src = root.join(abs.trim_start_matches('/'));
        let dst = image.join(abs.trim_start_matches('/'));
        match kind {
            "dir" => {
                std::fs::create_dir_all(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
            }
            "obj" => {
                if ebuild_merge::is_protected(root, config_protect, config_protect_mask, &src) {
                    continue;
                }
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::copy(&src, &dst)
                    .map_err(|e| format!("quickpkg: {}: {e}", src.display()))?;
            }
            "sym" => {
                let target = std::fs::read_link(&src)
                    .map_err(|e| format!("quickpkg: {}: {e}", src.display()))?;
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::os::unix::fs::symlink(&target, &dst)
                    .map_err(|e| format!("quickpkg: {}: {e}", dst.display()))?;
            }
            // "fif"/"dev": documented cut (needs CAP_MKNOD).
            _ => {}
        }
    }

    // build-info/ = a verbatim copy of the vdb dir (real `xpak(dbdir)`).
    let build_info = env.build_info();
    let _ = std::fs::remove_dir_all(&build_info);
    copy_dir_recursive(&vdb_dir, &build_info)?;

    let status = invoke_dyn_package(&scratch_ebuild, portage_tmpdir, root, options, &binpkg_path)?;
    if status != 0 {
        return Err(format!("{category}/{pf}: __dyn_package failed ({status})"));
    }

    // `$PKGDIR/Packages` entry from the vdb's own build-info files.
    let bi = |k: &str| {
        std::fs::read_to_string(vdb_dir.join(k))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let cpv = format!("{category}/{pf}");
    let build_time = bi("BUILD_TIME");
    // Real `_pkgindex_entry` always writes `PATH` -- see
    // `package_after_install`'s own identical fix for the full real
    // grounding (also needed for `populate_local_pkgdir`'s mtime-
    // staleness fast path to find this entry at all).
    let path_field = format!("{category}/{pf}.{ext}");
    let size_str = std::fs::metadata(&binpkg_path)
        .map(|st| st.len().to_string())
        .unwrap_or_default();
    let mtime_str = std::fs::metadata(&binpkg_path)
        .ok()
        .map(|st| binpkg::file_mtime(&st).to_string())
        .unwrap_or_default();
    write_packages_index_entry(
        &options.pkgdir,
        &cpv,
        &[
            ("CPV", &cpv),
            ("PF", pf),
            ("CATEGORY", category),
            ("SLOT", &bi("SLOT")),
            ("KEYWORDS", &bi("KEYWORDS")),
            ("USE", &bi("USE")),
            ("LICENSE", &bi("LICENSE")),
            ("IUSE", &bi("IUSE")),
            ("PROPERTIES", &bi("PROPERTIES")),
            ("RESTRICT", &bi("RESTRICT")),
            ("DEPEND", &bi("DEPEND")),
            ("RDEPEND", &bi("RDEPEND")),
            ("BDEPEND", &bi("BDEPEND")),
            ("PDEPEND", &bi("PDEPEND")),
            ("IDEPEND", &bi("IDEPEND")),
            ("PATH", &path_field),
            ("BUILD_TIME", &build_time),
            ("SIZE", &size_str),
            ("_mtime_", &mtime_str),
        ],
    )?;

    Ok(Some(binpkg_path))
}

/// Recursively copy `src` -> `dst` (files, symlinks-as-symlinks,
/// subdirs), creating `dst`. Used to stage the vdb dir as
/// `${PORTAGE_BUILDDIR}/build-info` for `quickpkg_from_vdb`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", src.display()))?;
        let ft = entry.file_type().map_err(|e| format!("{e}"))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_symlink() {
            let target =
                std::fs::read_link(&from).map_err(|e| format!("{}: {e}", from.display()))?;
            std::os::unix::fs::symlink(&target, &to)
                .map_err(|e| format!("{}: {e}", to.display()))?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        }
    }
    Ok(())
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
            // Real xpak/tbz2 building is codec-agnostic (portuale's
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

        // A real file, real bzip2+tar+XPAK content -- not portuale's
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

    #[test]
    fn real_package_with_gpkg_format_builds_a_real_gpkg_tar_this_pilots_reader_round_trips() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            binpkg_format: "gpkg".to_string(),
            // Pinned to "bzip2" (not the real `Default`, "zstd") so this
            // test doesn't depend on `zstd` being installed -- real
            // `gpkg-helper.py` reads this from the environment we export.
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

        // A real `.gpkg.tar` -- real, unmodified `bin/gpkg-helper.py
        // compress` (real `portage.gpkg.gpkg().compress()`) built it: an
        // outer tar whose members are the `gpkg-1` version marker, the
        // compressed `metadata.tar.<comp>`, the compressed
        // `image.tar.<comp>`, and a `Manifest`.
        let binpkg_path = options.pkgdir.join("dev-libs/packagepkg-1.0.gpkg.tar");
        assert!(
            binpkg_path.is_file(),
            "{} should exist",
            binpkg_path.display()
        );

        // The round trip: portuale's OWN gpkg reader (the `$PKGDIR`
        // directory-scan buildout's `binpkg::read_gpkg_metadata`) reads
        // back exactly what the real writer put in.
        let scalar = crate::binpkg::read_gpkg_metadata(&binpkg_path)
            .expect("portuale's gpkg reader parses the real writer's output");
        assert_eq!(scalar.get("SLOT").map(String::as_str), Some("0"));
        assert_eq!(scalar.get("CATEGORY").map(String::as_str), Some("dev-libs"));
        assert_eq!(scalar.get("PF").map(String::as_str), Some("packagepkg-1.0"));
        // `write_post_install_metadata` (real
        // `_post_src_install_write_metadata`) put the dep strings into
        // `build-info`, and real `gpkg._generate_metadata_from_dir`
        // carried them into `metadata.tar`.
        assert_eq!(
            scalar.get("RDEPEND").map(|s| s.trim()),
            Some("dev-libs/samepkg")
        );

        // A `Packages` entry was written with the gpkg `PATH` field, so
        // `--pretend --usepkg` resolves it the same as an xpak binpkg.
        let packages = std::fs::read_to_string(options.pkgdir.join("Packages")).unwrap();
        assert!(
            packages.contains("PATH: dev-libs/packagepkg-1.0.gpkg.tar"),
            "gpkg entry needs a PATH field: {packages:?}"
        );
        let index = portage_repo::BinaryIndex::from_pkgdir(&options.pkgdir);
        let candidates = portage_repo::list_binary_candidates(&index, "dev-libs", "packagepkg");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].version, "1.0");

        // And the directory scan (real `bintree._populate_local`) reads
        // the file directly -- the just-written index entry's own
        // `_mtime_`/`SIZE` still agree with the real file (nothing else
        // touched it since), so this hits the mtime-staleness fast path
        // rather than re-parsing the gpkg archive.
        let scanned = crate::binpkg::populate_local_pkgdir(&options.pkgdir).expect("scan succeeds");
        assert_eq!(scanned.len(), 1);
        assert_eq!(
            scanned[0].get("CPV").map(String::as_str),
            Some("dev-libs/packagepkg-1.0")
        );
    }

    #[test]
    fn real_package_with_binpkg_multi_instance_names_the_gpkg_with_a_build_id() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            binpkg_format: "gpkg".to_string(),
            binpkg_compress: "bzip2".to_string(),
            binpkg_multi_instance: true,
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        // Build it twice -- real `FEATURES=binpkg-multi-instance` keeps
        // both builds around under distinct `BUILD_ID`s, rather than the
        // single-instance default of overwriting the same bare filename.
        let status =
            run_package(&ebuild, &root, &portage_tmpdir, &options).expect("run_package succeeds");
        assert_eq!(status, 0);
        let status =
            run_package(&ebuild, &root, &portage_tmpdir, &options).expect("run_package succeeds");
        assert_eq!(status, 0);

        let first = options.pkgdir.join("dev-libs/packagepkg-1.0-1.gpkg.tar");
        let second = options.pkgdir.join("dev-libs/packagepkg-1.0-2.gpkg.tar");
        assert!(first.is_file(), "{} should exist", first.display());
        assert!(second.is_file(), "{} should exist", second.display());
        // The old bare (non-multi-instance) filename was never written.
        assert!(!options
            .pkgdir
            .join("dev-libs/packagepkg-1.0.gpkg.tar")
            .exists());

        // Both builds' own Packages entries survive -- real `_inject_file`'s
        // own PATH-keyed dedup (not CPV-keyed), which
        // `write_packages_index_entry`'s own doc comment mirrors.
        let packages = std::fs::read_to_string(options.pkgdir.join("Packages")).unwrap();
        assert!(packages.contains("PATH: dev-libs/packagepkg-1.0-1.gpkg.tar"));
        assert!(packages.contains("PATH: dev-libs/packagepkg-1.0-2.gpkg.tar"));
        assert!(packages.contains("BUILD_ID: 1"));
        assert!(packages.contains("BUILD_ID: 2"));

        // The directory scan re-derives BUILD_ID from the real archive's
        // own embedded PF (not a filename-parsing guess) for each build,
        // and CPV stays the bare `cat/pf` either way.
        let scanned = crate::binpkg::populate_local_pkgdir(&options.pkgdir).expect("scan succeeds");
        assert_eq!(scanned.len(), 2);
        let build_ids: std::collections::HashSet<&str> = scanned
            .iter()
            .map(|e| e.get("BUILD_ID").map(String::as_str).unwrap_or(""))
            .collect();
        assert_eq!(build_ids, std::collections::HashSet::from(["1", "2"]));
        for entry in &scanned {
            assert_eq!(
                entry.get("CPV").map(String::as_str),
                Some("dev-libs/packagepkg-1.0")
            );
            assert_eq!(entry.get("PF").map(String::as_str), Some("packagepkg-1.0"));
        }
    }
}
