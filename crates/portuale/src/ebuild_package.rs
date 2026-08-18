//! Binary-package building: `ebuild <file> package` runs the `install` chain,
//! then `bin/misc-functions.sh __dyn_package` to produce a genuine
//! `${PKGDIR}/${CATEGORY}/${PF}.tbz2` (xpak) or `.gpkg.tar` (gpkg), plus a
//! `<PKGDIR>/Packages` index entry so the package is immediately visible
//! to `emerge --pretend --usepkg`.
//!
//! Scope cuts a caller must know: `BINPKG_FORMAT` accepts only `gpkg` (the
//! default) and `xpak`; `USE` in the index entry is empty for standalone
//! builds (no resolved graph reaches this deep); `SLOT`/`KEYWORDS`/deps in
//! the index entry come from the repo's `metadata/md5-cache` when the ebuild
//! is inside a repo checkout, else empty; no `packdebug`/`splitdebug` or RPM
//! handling. Details in notes.
// Portage: doebuild.py, __dyn_package; bintree.py, EbuildBinpkg

use crate::binpkg;
use crate::ebuild_merge;
use crate::ebuild_phases;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Options for `run_package`, bundled into a struct rather than more
/// positional parameters. `pkgdir` is chain-resolved at the construction
/// sites (calling env over `make.conf`/profile/`make.globals`); `Default`
/// matches `make.globals`' `PKGDIR="/var/cache/binpkgs"`.
#[derive(Clone)]
pub struct PackageOptions {
    pub debug: bool,
    pub pkgdir: PathBuf,
    pub distdir: PathBuf,
    pub shell: ebuild_phases::ShellBackend,
    /// `BINPKG_COMPRESS` (default `"zstd"`); see `resolve_compression_command`.
    pub binpkg_compress: String,
    /// `BINPKG_COMPRESS_FLAGS_<NAME>` (per-compressor override) if set, else
    /// `BINPKG_COMPRESS_FLAGS` -- already resolved to the single value to use
    /// at the CLI boundary. Default is empty.
    pub binpkg_compress_flags: String,
    /// `PORTAGE_BZIP2_COMMAND` (default `"bzip2"`); only substituted when
    /// `binpkg_compress == "bzip2"`.
    pub portage_bzip2_command: String,
    /// `BINPKG_FORMAT` (default `"gpkg"`; `"xpak"` is the only other accepted
    /// value -- anything else is `Err("Unknown BINPKG_FORMAT ...")`).
    pub binpkg_format: String,
    /// `PORTAGE_CONFIGROOT` (mirrors `ebuild_merge::MergeOptions::config_root`).
    pub config_root: PathBuf,
    /// `FEATURES=buildpkg-live`: whether a `PROPERTIES=live` build also gets
    /// packaged when `buildpkg` is on. Default `true`; consulted by
    /// `emerge_build::entry_buildpkg_wanted`, not by this module itself.
    pub buildpkg_live: bool,
    /// `FEATURES=binpkg-multi-instance`, opt-in (default `false` here, a
    /// deliberate conservatism). Both formats build identical archive bytes;
    /// only the path convention differs: `<pkgdir>/<cat>/<pn>/<pf>-
    /// <build_id>.<suffix>`, suffix `xpak` for xpak, `gpkg.tar` for gpkg.
    pub binpkg_multi_instance: bool,
    /// `FEATURES=binpkg-signing`: the `__dyn_package` helper signs the gpkg
    /// members (detached `.sig` sidecars) and clear-signs the `Manifest`.
    /// Inert unless `binpkg_format == "gpkg"` (xpak has no signature mechanism).
    pub binpkg_signing: bool,
    /// `BINPKG_GPG_SIGNING_BASE_COMMAND`. Only exported to the helper when
    /// non-empty; empty means "not set", falling back to the helper's own
    /// default exactly as an unset `make.conf` would.
    pub binpkg_gpg_signing_base_command: String,
    /// `BINPKG_GPG_SIGNING_DIGEST` (default `"SHA512"`).
    pub binpkg_gpg_signing_digest: String,
    /// `BINPKG_GPG_SIGNING_GPG_HOME` (unset by default; `run_package` refuses
    /// to build with `binpkg-signing` when this or the key below is unset:
    /// `!!! {var} is not set`).
    pub binpkg_gpg_signing_gpg_home: String,
    /// `BINPKG_GPG_SIGNING_KEY` (likewise unset by default, same pre-check).
    pub binpkg_gpg_signing_key: String,
    /// The `<pkgdir>/Packages` header key set, resolved from the run's own
    /// config (`None` on the config-less standalone path, which keeps the
    /// minimal `TIMESTAMP`/`VERSION` header). `parent` serves the
    /// `--buildpkg` inject, `child` the `quickpkg` re-fold.
    // Portage: dbapi/bintree.py, _update_pkgindex_header
    pub pkgindex_header: Option<portage_profile::PkgIndexHeader>,
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
            binpkg_format: "gpkg".to_string(),
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
            buildpkg_live: true,
            binpkg_multi_instance: false,
            binpkg_signing: false,
            binpkg_gpg_signing_base_command: String::new(),
            binpkg_gpg_signing_digest: String::new(),
            binpkg_gpg_signing_gpg_home: String::new(),
            binpkg_gpg_signing_key: String::new(),
            pkgindex_header: None,
        }
    }
}

impl PackageOptions {
    /// Re-derives the binpkg-affecting `FEATURES` tokens from a resolved,
    /// already-folded `FEATURES` list by plain presence (a negated token has
    /// already been folded away, so presence alone is correct). The layout
    /// bit `binpkg_multi_instance` is deliberately not re-derived: the binpkg
    /// layout is run-wide-only, so it keeps its construction value.
    /// Config scalars are untouched.
    pub fn set_resolved_features(&mut self, features: &str) {
        let has = |token: &str| features.split_whitespace().any(|t| t == token);
        self.buildpkg_live = has("buildpkg-live");
        self.binpkg_signing = has("binpkg-signing");
    }
}

/// The `"compress"` half of Portage's `_compressors` table (this module only
/// ever builds a binpkg, never installs from one). `{JOBS}` is substituted
/// before `${...}` placeholders by `resolve_compression_command`.
// Portage: lib/portage/util/compression_probe.py
pub(crate) fn compress_template(name: &str) -> Option<&'static str> {
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

/// The first `PATH` entry containing an executable file named `name`.
// Portage: lib/portage/process.py, find_binary
pub(crate) fn find_binary(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(name))
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

/// Resolves `PORTAGE_COMPRESSION_COMMAND`: looks up `binpkg_compress` in the
/// compressor table, substitutes `{JOBS}` (host CPU count) and
/// `${PORTAGE_BZIP2_COMMAND}`/`${BINPKG_COMPRESS_FLAGS}` (plain `${VAR}`
/// substitution, not full shell expansion), whitespace-splits, and confirms
/// the binary is on `PATH`.
///
/// Returns `None` -- omitting the variable from the exported environment --
/// for an unknown compressor or a missing binary, matching Portage exactly.
/// Portage: doebuild.py
fn resolve_compression_command(
    binpkg_compress: &str,
    binpkg_compress_flags: &str,
    portage_bzip2_command: &str,
) -> Option<String> {
    resolve_compression_command_jobs(
        binpkg_compress,
        binpkg_compress_flags,
        portage_bzip2_command,
        &makeopts_to_job_count(""),
    )
}

/// The digits after the *last* `j` (or `--jobs=<whitespace>`) followed by
/// optional whitespace and a number, else the CPU count. Returned as text.
// Portage: util/cpuinfo.py, makeopts_to_job_count
pub(crate) fn makeopts_to_job_count(makeopts: &str) -> String {
    let cpu_count = || {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .to_string()
    };
    let bytes = makeopts.as_bytes();
    let digits_after = |mut i: usize| -> Option<String> {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        (i > start).then(|| makeopts[start..i].to_string())
    };
    for i in (0..bytes.len()).rev() {
        let hit = if bytes[i] == b'j' {
            digits_after(i + 1)
        } else if makeopts[i..].starts_with("--jobs=")
            && bytes.get(i + 7).is_some_and(|b| b.is_ascii_whitespace())
        {
            digits_after(i + 8)
        } else {
            None
        };
        if let Some(jobs) = hit {
            return jobs;
        }
    }
    cpu_count()
}

fn resolve_compression_command_jobs(
    binpkg_compress: &str,
    binpkg_compress_flags: &str,
    portage_bzip2_command: &str,
    jobs: &str,
) -> Option<String> {
    let template = compress_template(binpkg_compress)?;
    let expanded = template
        .replace("{JOBS}", jobs)
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

/// The `<pkgdir>/Packages` header block. Portage discards a header-less index
/// wholesale (`_pkgindex_version_supported`), so the `VERSION` stamp is what
/// makes portuale-built archives visible to it; `PACKAGES`/fingerprint fields
/// stay omitted (Portage only consumes them when it rewrites the index itself).
/// A resolved header snapshot replaces this with the full settings-derived
/// key set (see `write_packages_index_entry`).
// Portage: bintree.py, _load_pkgindex
fn packages_index_header(now: u64) -> String {
    format!("TIMESTAMP: {now}\nVERSION: 0\n")
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

/// Portage's `PackageIndex.write` drops a stanza value equal to its default
/// for that key; only `SLOT: "0"` and `EAPI: "0"` can reach a stanza non-empty
/// (every other default is `""`, already skipped), so those two are the whole
/// rule. Only the exact `"0"` is default.
// Portage: getbinpkg.py, PackageIndex.write; bintree.py, _pkgindex_default_pkg_data
fn omit_stanza_default<'a>(key: &'a str, value: &'a str) -> &'a str {
    match (key, value) {
        ("SLOT", "0") | ("EAPI", "0") => "",
        _ => value,
    }
}

/// Portage's `PackageIndex.write` key order: the internal names sorted, with
/// the translated pair landing last as `MTIME` before `REPO`. Sorting by this
/// internal-name mapping is byte-identical to Portage's order for every key
/// either stanza writer emits. Both writers route through here.
// Portage: getbinpkg.py, PackageIndex.write
fn sort_stanza_fields_for_write(fields: &mut Vec<(&'static str, &str)>) {
    fn internal(key: &str) -> &str {
        match key {
            "MTIME" => "_mtime_",
            "REPO" => "repository",
            _ => key,
        }
    }
    fields.sort_by(|a, b| internal(a.0).cmp(internal(b.0)));
}

/// One freshly built binary package's `<pkgdir>/Packages` stanza fields, in
/// Portage's `keys.sort()` order (the translated `MTIME`/`REPO` land last).
/// Default-valued and empty values are omitted, like Portage's write path.
/// `REPO_REVISIONS` has no default entry, so only the falsy guard applies.
// Portage: bintree.py, _pkgindex_entry
struct BuiltBinpkgStanza<'a> {
    bdepend: &'a str,
    build_id: &'a str,
    build_time: &'a str,
    cpv: &'a str,
    defined_phases: &'a str,
    depend: &'a str,
    eapi: &'a str,
    idepend: &'a str,
    iuse: &'a str,
    keywords: &'a str,
    license: &'a str,
    md5: &'a str,
    path: &'a str,
    pdepend: &'a str,
    properties: &'a str,
    provides: &'a str,
    rdepend: &'a str,
    repo_revisions: &'a str,
    requires: &'a str,
    restrict: &'a str,
    sha1: &'a str,
    size: &'a str,
    slot: &'a str,
    use_flags: &'a str,
    mtime: &'a str,
    repository: &'a str,
}

fn built_binpkg_stanza_fields<'a>(
    stanza: &'a BuiltBinpkgStanza<'a>,
) -> Vec<(&'static str, &'a str)> {
    // Hand-sorted into Portage's `keys.sort()` order; the sort call below is a
    // no-op guard so a future field lands ordered too.
    let mut fields = vec![
        ("BDEPEND", stanza.bdepend),
        ("BUILD_ID", stanza.build_id),
        ("BUILD_TIME", stanza.build_time),
        ("CPV", stanza.cpv),
        ("DEFINED_PHASES", stanza.defined_phases),
        ("DEPEND", stanza.depend),
        ("EAPI", omit_stanza_default("EAPI", stanza.eapi)),
        ("IDEPEND", stanza.idepend),
        ("IUSE", stanza.iuse),
        ("KEYWORDS", stanza.keywords),
        ("LICENSE", stanza.license),
        ("MD5", stanza.md5),
        ("PATH", stanza.path),
        ("PDEPEND", stanza.pdepend),
        ("PROPERTIES", stanza.properties),
        ("PROVIDES", stanza.provides),
        ("RDEPEND", stanza.rdepend),
        ("REPO_REVISIONS", stanza.repo_revisions),
        ("REQUIRES", stanza.requires),
        ("RESTRICT", stanza.restrict),
        ("SHA1", stanza.sha1),
        ("SIZE", stanza.size),
        ("SLOT", omit_stanza_default("SLOT", stanza.slot)),
        ("USE", stanza.use_flags),
        ("MTIME", stanza.mtime),
        ("REPO", stanza.repository),
    ];
    sort_stanza_fields_for_write(&mut fields);
    fields
}

/// Portage's `_pkgindex_write` gzip half: the same bytes as `Packages`,
/// compressed at level 9 with no original-filename field and the header
/// mtime set to the index `TIMESTAMP`. Written through a same-directory
/// temp file renamed into place, like Portage's `atomic_ofstream`.
// Portage: bintree.py, _pkgindex_write
fn write_packages_gz(pkgdir: &Path, contents: &[u8], timestamp: u64) -> Result<(), String> {
    use std::io::Write as _;
    let gz_path = pkgdir.join("Packages.gz");
    let tmp_path = gz_path.with_extension(format!("gz.tmp{}", std::process::id()));
    let result = (|| {
        let file =
            std::fs::File::create(&tmp_path).map_err(|e| format!("{}: {e}", tmp_path.display()))?;
        let mut encoder = flate2::GzBuilder::new()
            .mtime(timestamp as u32)
            .write(file, flate2::Compression::new(9));
        encoder
            .write_all(contents)
            .map_err(|e| format!("{}: {e}", tmp_path.display()))?;
        encoder
            .finish()
            .map_err(|e| format!("{}: {e}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &gz_path)
            .map_err(|e| format!("{} -> {}: {e}", tmp_path.display(), gz_path.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

/// With `FEATURES=binpkg-signing` on a gpkg build, the signing keyring and key
/// must be configured, else `!!! {var} is not set`. Checked by `run_package`
/// before the `install` chain and again by `package_after_install` for the
/// `FEATURES=buildpkg` side-effect path.
// Portage: _emerge/actions.py
fn require_signing_config(options: &PackageOptions) -> Result<(), String> {
    if options.binpkg_signing && options.binpkg_format == "gpkg" {
        for (var, value) in [
            (
                "BINPKG_GPG_SIGNING_GPG_HOME",
                options.binpkg_gpg_signing_gpg_home.as_str(),
            ),
            (
                "BINPKG_GPG_SIGNING_KEY",
                options.binpkg_gpg_signing_key.as_str(),
            ),
        ] {
            if value.is_empty() {
                return Err(format!("!!! {var} is not set"));
            }
        }
    }
    Ok(())
}

pub struct RunPackageArgs<'a> {
    pub ebuild_path: &'a Path,
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub options: &'a PackageOptions,
    pub build_env: &'a [(String, String)],
    pub use_flags: &'a str,
}

/// Rejected here rather than letting the bash hit `die "Unknown BINPKG_FORMAT"`,
/// so the caller gets a clean `Err` instead of a phase-script failure.
fn binpkg_extension(binpkg_format: &str) -> Result<&'static str, String> {
    match binpkg_format {
        "xpak" => Ok("tbz2"),
        "gpkg" => Ok("gpkg.tar"),
        other => Err(format!("Unknown BINPKG_FORMAT {other}")),
    }
}

/// The multi-instance filename suffix: `xpak` for xpak (not `tbz2` -- a
/// multi-instance xpak file is `<pf>-<build_id>.xpak`, byte-identical to a
/// `.tbz2` but named for the layout), `gpkg.tar` for gpkg.
// Portage: bintree.py, _allocate_filename_multi
fn multi_instance_binpkg_suffix(binpkg_format: &str) -> Result<&'static str, String> {
    match binpkg_format {
        "xpak" => Ok("xpak"),
        "gpkg" => Ok("gpkg.tar"),
        other => Err(format!("Unknown BINPKG_FORMAT {other}")),
    }
}

/// Allocates the next `BUILD_ID` for this `cp/pf`: one past the highest id in
/// `<pkgdir>/<cat>/<pn>/<pf>-*.<suffix>`, re-derived from the filesystem each
/// call (no long-lived index to consult), then incremented past any occupied
/// name. Single-process: existence alone is enough, no placeholder-file dance.
// Portage: bintree.py, _allocate_filename_multi
fn allocate_binpkg_build_id(
    pkgdir: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    suffix: &str,
) -> u64 {
    let dir = pkgdir.join(category).join(pn);
    let prefix = format!("{pf}-");
    let dot_suffix = format!(".{suffix}");
    let mut max_existing: u64 = 0;
    if let Ok(entries) = portage_util::read_dir_entries(&dir) {
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Some(rest) = name.strip_prefix(&prefix) else {
                continue;
            };
            let Some(id_str) = rest.strip_suffix(&dot_suffix) else {
                continue;
            };
            if let Ok(id) = id_str.parse::<u64>() {
                max_existing = max_existing.max(id);
            }
        }
    }
    let mut build_id = max_existing + 1;
    while dir.join(format!("{pf}-{build_id}.{suffix}")).exists() {
        build_id += 1;
    }
    build_id
}

/// The whole binpkg file's `MD5` and `SHA1` for the `Packages` entry, in a
/// single read. Without digests a self-built binpkg would silently skip
/// integrity checks for anyone fetching it as a remote binhost.
// Portage: bintree.py, _pkgindex_entry
fn binpkg_checksums(path: &Path) -> Option<(String, String)> {
    // `md5::Digest` and `sha1::Digest` are the same trait re-exported.
    use md5::Digest as _;
    let bytes = std::fs::read(path).ok()?;
    let md5: String = md5::Md5::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let sha1: String = sha1::Sha1::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Some((md5, sha1))
}

struct InvokeDynPackageArgs<'a> {
    ebuild_path: &'a Path,
    portage_tmpdir: &'a Path,
    root: &'a Path,
    options: &'a PackageOptions,
    binpkg_path: &'a Path,
    build_id: Option<u64>,
}
