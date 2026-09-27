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

/// Whether `command` is the package-building command this module implements
/// (`ebuild.rs` checks this alongside the phase/merge/unmerge routers).
pub fn is_package_command(command: &str) -> bool {
    command == "package"
}

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

/// `PORTAGE_COMPRESSION_COMMAND` from the resolved config (`lookup`: calling
/// env over `make.conf`/profile/`make.globals`): `BINPKG_COMPRESS` (default
/// `bzip2`; empty = `cat`), the `BINPKG_COMPRESS_FLAGS_<NAME>` override over
/// `BINPKG_COMPRESS_FLAGS`, `{JOBS}` from `MAKEOPTS`. `None` for an unknown
/// compressor or a missing binary, like Portage (left unset).
// Portage: doebuild.py, doebuild_environment
pub fn phase_compression_command(lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    let name = lookup("BINPKG_COMPRESS").unwrap_or_else(|| "bzip2".to_string());
    if name.is_empty() {
        return Some("cat".to_string());
    }
    let flags = lookup(&format!("BINPKG_COMPRESS_FLAGS_{}", name.to_uppercase()))
        .or_else(|| lookup("BINPKG_COMPRESS_FLAGS"))
        .unwrap_or_default();
    let bzip2 = lookup("PORTAGE_BZIP2_COMMAND").unwrap_or_else(|| "bzip2".to_string());
    let makeopts = lookup("MAKEOPTS").unwrap_or_else(|| "1".to_string());
    resolve_compression_command_jobs(&name, &flags, &bzip2, &makeopts_to_job_count(&makeopts))
}

/// `BINPKG_FORMAT` from the resolved config (`lookup`: calling env over
/// `make.conf`/profile/`make.globals`): the chain value when set anywhere,
/// else the default `"gpkg"`. An empty value stays an `Unknown BINPKG_FORMAT`
/// error downstream, never a fallback.
pub fn resolve_binpkg_format(lookup: impl Fn(&str) -> Option<String>) -> String {
    // An empty value stays an `Unknown BINPKG_FORMAT` error downstream:
    // a `make.conf` assignment overrides `make.globals` with `""` and
    // `__dyn_package` then dies the same way.
    lookup("BINPKG_FORMAT").unwrap_or_else(|| PackageOptions::default().binpkg_format.clone())
}

/// `PKGDIR` from the resolved config: the chain value when set anywhere, else
/// the default `/var/cache/binpkgs`. An empty value falls back (Portage has
/// no `PKGDIR`-empty die; an empty pkgdir only ever yields broken paths).
pub fn resolve_pkgdir(lookup: impl Fn(&str) -> Option<String>) -> std::path::PathBuf {
    lookup("PKGDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| PackageOptions::default().pkgdir.clone())
}

/// `BINPKG_COMPRESS` from the resolved config: the chain value when set
/// anywhere, else the default `"zstd"`. The caller picks the lookup per
/// format: the full chain for `xpak`, the files-only chain (no calling env)
/// for `gpkg`. An empty value is honored verbatim (disables compression);
/// only an *unset* chain falls back.
pub fn resolve_binpkg_compress(lookup: impl Fn(&str) -> Option<String>) -> String {
    lookup("BINPKG_COMPRESS").unwrap_or_else(|| PackageOptions::default().binpkg_compress.clone())
}

/// `BINPKG_COMPRESS_FLAGS_<NAME>` (per-compressor override) if set, else
/// `BINPKG_COMPRESS_FLAGS`, over the caller's `lookup` (full chain for
/// `xpak`, files-only for `gpkg`, as above). Default is empty.
pub fn resolve_binpkg_compress_flags(
    lookup: impl Fn(&str) -> Option<String>,
    binpkg_compress: &str,
) -> String {
    lookup(&format!(
        "BINPKG_COMPRESS_FLAGS_{}",
        binpkg_compress.to_uppercase()
    ))
    .or_else(|| lookup("BINPKG_COMPRESS_FLAGS"))
    .unwrap_or_default()
}

/// `PORTAGE_BZIP2_COMMAND` from the resolved config: the chain value when set
/// anywhere, else the default `"bzip2"`. Unlike the compression pair, the
/// calling env wins for both formats. Only substituted when
/// `binpkg_compress == "bzip2"`.
pub fn resolve_portage_bzip2_command(lookup: impl Fn(&str) -> Option<String>) -> String {
    lookup("PORTAGE_BZIP2_COMMAND")
        .unwrap_or_else(|| PackageOptions::default().portage_bzip2_command.clone())
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

/// Re-derives `PORTAGE_COMPRESSION_COMMAND` from an entry's already-layered
/// build env, replacing the run-wide pair in place. Portage computes the
/// command from the package's own settings, so a per-package
/// `BINPKG_COMPRESS=gzip` reaches the xpak tar pipe even when the run-wide
/// value differs -- but the gpkg helper reads the global settings, so a
/// per-entry value must never reach it.
///
/// The lookup is last-wins over the layered pairs, then the calling env, then
/// the resolved config's own file chain. An unmatched entry re-derives the
/// run-wide command byte-identically. A recompute of `None` (unknown
/// per-entry codec) removes the pair, matching Portage leaving the key unset.
/// `config` is `None` on the config-less paths.
pub(crate) fn refresh_entry_compression_command(
    build_env: &mut Vec<(String, String)>,
    config: Option<&portage_profile::Config>,
) {
    let lookup = |key: &str| {
        build_env
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .or_else(|| portage_profile::config_env_var(key))
            .or_else(|| config.and_then(|c| c.other_vars.get(key).cloned()))
    };
    let recomputed = phase_compression_command(lookup);
    if let Some(pos) = build_env
        .iter()
        .rposition(|(k, _)| k == "PORTAGE_COMPRESSION_COMMAND")
    {
        match recomputed {
            Some(cmd) => build_env[pos].1 = cmd,
            None => {
                build_env.remove(pos);
            }
        }
    } else if let Some(cmd) = recomputed {
        build_env.push(("PORTAGE_COMPRESSION_COMMAND".to_string(), cmd));
    }
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

/// Writes (creating the file and its header if necessary) or replaces `cpv`'s
/// entry in `<pkgdir>/Packages`. A pre-existing entry for the same `cpv` (a
/// rebuild) is replaced in place, not duplicated; every other entry is
/// preserved verbatim.
///
/// `header` is the resolved snapshot for this write's mode (`parent` for a
/// `--buildpkg` inject, `child` for a `quickpkg`): the stored header's keys
/// are refreshed from it, `TIMESTAMP`/`PACKAGES`/`VERSION` rewritten, and
/// the block emitted sorted, like Portage's `PackageIndex.write`. `None`
/// (the config-less standalone path) keeps the legacy minimal header and
/// preserves a stored one verbatim.
///
/// Like Portage's `bintree.inject`, the `Packages` lock is the
/// `wantnewlockfile` shape, so the `.Packages.portage_lockfile` sibling is
/// removed on release. A `Packages.gz` copy (same bytes, gzip level 9, no
/// original-filename field, header mtime = the index `TIMESTAMP`) is written
/// when `compress-index` is in the snapshot's resolved `FEATURES`, and any
/// stale copy is removed when it is not; `None` leaves it alone (no resolved
/// features to judge by).
///
/// The whole read-modify-write runs under that lock, so a concurrent write
/// to the same `Packages` file cannot interleave.
// Portage: bintree.py, bintree.inject/_pkgindex_write;
//   locks.py, lockfile/unlockfile; getbinpkg.py, PackageIndex.write
fn write_packages_index_entry(
    pkgdir: &Path,
    cpv: &str,
    fields: &[(&str, &str)],
    header: Option<&portage_profile::HeaderSnapshot>,
) -> Result<(), String> {
    let path = pkgdir.join("Packages");
    let now = now_unix_time()?;

    // Makedirs first, then the lock: the lock file is a sibling of `Packages`,
    // so its parent dir must exist first (Portage's order).
    std::fs::create_dir_all(pkgdir).map_err(|e| format!("{}: {e}", pkgdir.display()))?;
    let _lock = crate::portage_lock::PortageLockfile::acquire_new_lockfile(&path)?;

    let mut blocks: Vec<String> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for block in text.split("\n\n") {
            let block = block.trim();
            if !block.is_empty() {
                blocks.push(block.to_string());
            }
        }
    }
    let mut stored_header = String::new();
    if !blocks.is_empty() {
        stored_header = blocks.remove(0);
    }
    // Header is always blocks[0]; dedup like Portage's `_inject_file`: by
    // `PATH` when this entry carries one (a different `PATH` is a genuinely
    // different file and survives), else by `CPV` alone.
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

    let header_text = match header {
        Some(snapshot) => {
            let mut kept: std::collections::BTreeMap<String, String> = stored_header
                .lines()
                .filter_map(|l| l.split_once(": "))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            for (key, value) in snapshot {
                match value {
                    Some(v) => {
                        kept.insert(key.clone(), v.clone());
                    }
                    None => {
                        kept.remove(key);
                    }
                }
            }
            kept.insert("TIMESTAMP".to_string(), now.to_string());
            kept.insert("PACKAGES".to_string(), blocks.len().to_string());
            kept.insert("VERSION".to_string(), "0".to_string());
            kept.into_iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        None => {
            if stored_header.is_empty() {
                stored_header = packages_index_header(now).trim().to_string();
            }
            stored_header
        }
    };

    let mut text = header_text;
    for block in blocks {
        text.push_str("\n\n");
        text.push_str(&block);
    }
    // Every block (header and entries alike) ends with its own blank line,
    // like Portage's `_writepkgindex`, so the file ends with one too.
    // Portage: getbinpkg.py, PackageIndex._writepkgindex
    text.push_str("\n\n");

    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;

    // Portage's `_pkgindex_write` gzip half. The snapshot's `FEATURES` is
    // the resolved fold (`make.globals` carries `compress-index` by
    // default), so a config-less (`None`) write has nothing to judge by
    // and leaves any existing copy alone.
    let compress_index = header
        .and_then(|snapshot| {
            snapshot
                .iter()
                .find(|(key, _)| key == "FEATURES")
                .and_then(|(_, value)| value.as_ref())
        })
        .is_some_and(|features| features.split_whitespace().any(|t| t == "compress-index"));
    if compress_index {
        // The header just written carries `TIMESTAMP: {now}` (inserted
        // above), which is the mtime Portage stamps into the gzip header.
        write_packages_gz(pkgdir, text.as_bytes(), now)?;
    } else if header.is_some() {
        let gz_path = pkgdir.join("Packages.gz");
        match std::fs::remove_file(&gz_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", gz_path.display())),
        }
    }
    Ok(())
    // `_lock` drops here, releasing the flock and unlinking the lock file.
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

/// Runs the `install` chain, then the packaging tail.
/// `build_env`: the resolved phase environment for the `install` chain
/// (`--buildpkgonly` passes the full env plus the entry's `USE`;
/// standalone passes `&[]`). `use_flags` is `Package.use.enabled`, written
/// to the index `USE` field; `""` for standalone.
// Portage: doebuild.py, actionmap_deps["package"] == ["install"]
pub fn run_package(args: RunPackageArgs<'_>) -> Result<i32, String> {
    let RunPackageArgs {
        ebuild_path,
        root,
        portage_tmpdir,
        options,
        build_env,
        use_flags,
    } = args;
    require_signing_config(options)?;
    let status = ebuild_phases::run_commands(ebuild_phases::RunCommandsArgs {
        ebuild_path,
        commands: &["install"],
        root,
        portage_tmpdir,
        distdir: &options.distdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        build_env,
    })?;
    if status != 0 {
        return Ok(status);
    }
    // The per-entry multi-instance token comes from this run's own `build_env`,
    // falling back to the run-wide construction value for standalone runs.
    let per_entry_binpkg_multi_instance = build_env
        .iter()
        .rev()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.split_whitespace().any(|t| t == "binpkg-multi-instance"))
        .unwrap_or(options.binpkg_multi_instance);
    package_after_install(PackageAfterInstallArgs {
        ebuild_path,
        root,
        portage_tmpdir,
        options,
        use_flags,
        per_entry_binpkg_multi_instance,
    })
}

pub(crate) struct PackageAfterInstallArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a PackageOptions,
    pub(crate) use_flags: &'a str,
    pub(crate) per_entry_binpkg_multi_instance: bool,
}

/// The packaging tail of `run_package`, also run as a side effect of a source
/// merge (`FEATURES=buildpkg` / `--buildpkg`, after `src_install`, before the
/// vdb merge). Assumes a populated `${D}` from a prior `install` chain.
/// `use_flags` is `Package.use.enabled` (space-joined enabled-only, the shape
/// an index `USE` field carries); `""` for standalone builds (no resolved
/// graph reaches that deep). `per_entry_binpkg_multi_instance` is the entry's
/// own resolved token: the layout stays run-wide while the `BUILD_ID` export
/// and index entry follow this gate. Run-wide single + per-entry token writes
/// no `BUILD_ID` anywhere here, where Portage writes index `None` and vdb
/// `-1`: a documented divergence, not mirrored.
// Portage: _emerge/EbuildBinpkg
pub(crate) fn package_after_install(args: PackageAfterInstallArgs<'_>) -> Result<i32, String> {
    let PackageAfterInstallArgs {
        ebuild_path,
        root,
        portage_tmpdir,
        options,
        use_flags,
        per_entry_binpkg_multi_instance,
    } = args;
    // Repeated here so the `FEATURES=buildpkg` side-effect path through
    // `ebuild_merge::run_merge` hits the gate too.
    require_signing_config(options)?;
    let binpkg_extension = binpkg_extension(&options.binpkg_format)?;

    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;

    let build_time = now_unix_time()?;
    let build_info_dir = env.build_info();
    std::fs::create_dir_all(&build_info_dir)
        .map_err(|e| format!("{}: {e}", build_info_dir.display()))?;
    std::fs::write(build_info_dir.join("BUILD_TIME"), build_time.to_string())
        .map_err(|e| format!("{}: {e}", build_info_dir.join("BUILD_TIME").display()))?;

    // Multi-instance layout (see `PackageOptions::binpkg_multi_instance`):
    // `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.<suffix>`; the archive bytes are
    // the single-instance form either way.
    let multi_instance_suffix = multi_instance_binpkg_suffix(&options.binpkg_format)?;
    let build_id = options.binpkg_multi_instance.then(|| {
        allocate_binpkg_build_id(
            &options.pkgdir,
            &env.category,
            &env.split.pn,
            &env.split.pf,
            multi_instance_suffix,
        )
    });
    let binpkg_path = match build_id {
        Some(id) => options
            .pkgdir
            .join(&env.category)
            .join(&env.split.pn)
            .join(format!("{}-{id}.{multi_instance_suffix}", env.split.pf)),
        None => options
            .pkgdir
            .join(&env.category)
            .join(format!("{}.{binpkg_extension}", env.split.pf)),
    };
    if let Some(parent) = binpkg_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }

    // Temp-file-then-rename: `__dyn_package` writes a same-directory temp file,
    // renamed into place only after the phase exits success, so a mid-write
    // failure never leaves a corrupt archive at the final path.
    // Portage: _emerge/EbuildBinpkg
    let tmp_path = binpkg_path.with_extension(format!(
        "{}.tmp{}",
        binpkg_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default(),
        std::process::id()
    ));
    let package_status = invoke_dyn_package(InvokeDynPackageArgs {
        ebuild_path,
        portage_tmpdir,
        root,
        options,
        binpkg_path: &tmp_path,
        // The `BUILD_ID` export follows the entry's own token, while the
        // allocation above stays run-wide.
        build_id: build_id.filter(|_| per_entry_binpkg_multi_instance),
    })?;
    if package_status != 0 {
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(package_status);
    }
    std::fs::rename(&tmp_path, &binpkg_path)
        .map_err(|e| format!("{} -> {}: {e}", tmp_path.display(), binpkg_path.display()))?;

    let cpv = format!("{}/{}", env.category, env.split.pf);
    let metadata = ebuild_phases::repo_root_for(&env.pkg_dir)
        .and_then(|repo_root| {
            portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok()
        })
        .unwrap_or_default();
    let get = |key: &str| metadata.get(key).map(String::as_str).unwrap_or("");
    // `build-info` first (it carries what the install phase rewrote: `RDEPEND`,
    // `PROVIDES`/`REQUIRES`, `IUSE`, `SIZE`), `md5-cache` as fallback.
    let build_info = env.build_info();
    let build_info_value = |key: &str| -> Option<String> {
        std::fs::read_to_string(build_info.join(key))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let get_bi =
        |key: &str| -> String { build_info_value(key).unwrap_or_else(|| get(key).to_string()) };
    let build_time_str = build_time.to_string();
    // `PATH` is always written, in any format: a remote fetch falls back to a
    // bare `<pf>.<ext>` guess without it, and the local-pkgdir scan's
    // mtime-staleness fast path looks entries up by it. Derived from
    // `binpkg_path` relative to `$PKGDIR` so the two can never drift apart.
    // Portage: bintree.py, _pkgindex_entry
    let path_field = binpkg_path
        .strip_prefix(&options.pkgdir)
        .ok()
        .and_then(|rel| rel.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}/{}",
                env.category,
                binpkg_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
            )
        });
    // The index `BUILD_ID` is the archive's own metadata, so it follows the
    // same per-entry gate as the export above, not the allocation.
    let build_id_str = build_id
        .filter(|_| per_entry_binpkg_multi_instance)
        .map(|id| id.to_string())
        .unwrap_or_default();
    let size_str = std::fs::metadata(&binpkg_path)
        .map(|st| st.len().to_string())
        .unwrap_or_default();
    let mtime_str = std::fs::metadata(&binpkg_path)
        .ok()
        .map(|st| binpkg::file_mtime(&st).to_string())
        .unwrap_or_default();
    let (md5_str, sha1_str) = binpkg_checksums(&binpkg_path).unwrap_or_default();

    // The stanza fields are the archive metadata's values in Portage's
    // `keys.sort()` order; both digests are always computed (Portage's fixed
    // `_pkgindex_hashes`, unaffected by `PORTAGE_CHECKSUM_FILTER`).
    let defined_phases = get_bi("DEFINED_PHASES");
    let eapi = get_bi("EAPI");
    let slot = get_bi("SLOT");
    let iuse = get_bi("IUSE");
    let keywords = get_bi("KEYWORDS");
    let license = get_bi("LICENSE");
    let properties = get_bi("PROPERTIES");
    let restrict = get_bi("RESTRICT");
    let depend = get_bi("DEPEND");
    let rdepend = get_bi("RDEPEND");
    let bdepend = get_bi("BDEPEND");
    let pdepend = get_bi("PDEPEND");
    let idepend = get_bi("IDEPEND");
    let provides = get_bi("PROVIDES");
    let requires = get_bi("REQUIRES");
    let repo_revisions = get_bi("REPO_REVISIONS");
    let repository = build_info_value("repository").unwrap_or_default();
    let stanza = BuiltBinpkgStanza {
        bdepend: &bdepend,
        build_id: &build_id_str,
        build_time: &build_time_str,
        cpv: &cpv,
        defined_phases: &defined_phases,
        depend: &depend,
        eapi: &eapi,
        idepend: &idepend,
        iuse: &iuse,
        keywords: &keywords,
        license: &license,
        md5: &md5_str,
        path: &path_field,
        pdepend: &pdepend,
        properties: &properties,
        provides: &provides,
        rdepend: &rdepend,
        repo_revisions: &repo_revisions,
        requires: &requires,
        restrict: &restrict,
        sha1: &sha1_str,
        size: &size_str,
        slot: &slot,
        use_flags,
        mtime: &mtime_str,
        repository: &repository,
    };
    write_packages_index_entry(
        &options.pkgdir,
        &cpv,
        &built_binpkg_stanza_fields(&stanza),
        options.pkgindex_header.as_ref().map(|h| &h.parent),
    )?;

    // Record the fresh archive in `build-info`: `BINPKGMD5` always, `BUILD_ID`
    // when the archive carries one. The vdb merge copies `build-info`
    // wholesale, so a from-source merge records the binpkg it produced too.
    // Portage: _emerge/EbuildBuild._record_binpkg_info; doebuild.py, package
    if !md5_str.is_empty() {
        std::fs::write(build_info.join("BINPKGMD5"), format!("{md5_str}\n"))
            .map_err(|e| format!("{}: {e}", build_info.join("BINPKGMD5").display()))?;
    }
    if !build_id_str.is_empty() {
        std::fs::write(build_info.join("BUILD_ID"), format!("{build_id_str}\n"))
            .map_err(|e| format!("{}: {e}", build_info.join("BUILD_ID").display()))?;
    }

    Ok(0)
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

/// `MAKEOPTS` for the package step's native gpkg compressor: the ebuild's own
/// resolved chain, else the global chain (which serves `quickpkg_from_vdb`,
/// whose scratch vdb-ebuild copy has no repo above it).
fn package_makeopts(ebuild_path: &Path, config_root: &Path, root: &Path) -> Option<String> {
    ebuild_phases::resolve_standalone_chain_scalar(ebuild_path, config_root, root, "MAKEOPTS")
        .or_else(|| {
            ebuild_phases::resolve_unmerge_config(config_root, root)
                .and_then(|config| config.other_vars.get("MAKEOPTS").cloned())
        })
}

struct InvokeDynPackageArgs<'a> {
    ebuild_path: &'a Path,
    portage_tmpdir: &'a Path,
    root: &'a Path,
    options: &'a PackageOptions,
    binpkg_path: &'a Path,
    build_id: Option<u64>,
}

/// Runs `bin/misc-functions.sh __dyn_package` against `ebuild_path` with the
/// compressor / `PKGDIR` / `PORTAGE_BINPKG_TMPFILE` environment Portage sets.
/// Shared by `run_package` (fresh `src_install` image) and `quickpkg_from_vdb`
/// (installed-files image).
///
/// `build_id`: exported exactly when the caller allocated one, so
/// `__dyn_package` writes `build-info/BUILD_ID` into the archive itself (and
/// `__generate_packdebug` gets the name it needs under `FEATURES=packdebug`).
// Portage: _emerge/EbuildBinpkg._start
fn invoke_dyn_package(args: InvokeDynPackageArgs<'_>) -> Result<i32, String> {
    let InvokeDynPackageArgs {
        ebuild_path,
        portage_tmpdir,
        root,
        options,
        binpkg_path,
        build_id,
    } = args;
    let binpkg_format = options.binpkg_format.as_str();
    let mut extra_env = vec![
        ("PKGDIR".to_string(), options.pkgdir.display().to_string()),
        (
            "PORTAGE_BINPKG_TMPFILE".to_string(),
            binpkg_path.display().to_string(),
        ),
        ("BINPKG_FORMAT".to_string(), binpkg_format.to_string()),
    ];
    // The helper prefers `PORTAGE_PYTHONPATH` over `PORTAGE_PYM_PATH`: set it
    // directly so the helper imports `portage` from this checkout. Nothing is
    // pushed without a checkout.
    let checkout = ebuild_phases::portage_checkout();
    if let Some(pythonpath) = ebuild_phases::pythonpath_value(&checkout) {
        extra_env.push(("PORTAGE_PYTHONPATH".to_string(), pythonpath));
    }
    if let Some(id) = build_id {
        extra_env.push(("BUILD_ID".to_string(), id.to_string()));
    }
    if binpkg_format == "gpkg" {
        // The gpkg helper rebuilds its own settings in the subprocess and reads
        // the compressor from `BINPKG_COMPRESS`/`FLAGS` (not from
        // `PORTAGE_COMPRESSION_COMMAND`, which is xpak's tar-pipe only), so
        // the already-resolved values are exported for a deterministic build.
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
        // The helper takes `{JOBS}` from `MAKEOPTS` in its own settings; the
        // calling-env side already reaches it through the phase env, so only
        // the `make.conf`/profile side is exported here (left unset when
        // nothing resolves: the helper then counts CPUs, like Portage).
        if portage_profile::config_env_var("MAKEOPTS").is_none()
            && let Some(makeopts) = package_makeopts(ebuild_path, &options.config_root, root)
        {
            extra_env.push(("MAKEOPTS".to_string(), makeopts));
        }
        // Only non-empty signing values are exported, so an unset var falls back
        // to the helper's own default exactly as an unset `make.conf` would.
        // A signing request with no keyring/key never reaches here (rejected
        // by the pre-check).
        for (key, value) in [
            (
                "BINPKG_GPG_SIGNING_BASE_COMMAND",
                options.binpkg_gpg_signing_base_command.as_str(),
            ),
            (
                "BINPKG_GPG_SIGNING_DIGEST",
                options.binpkg_gpg_signing_digest.as_str(),
            ),
            (
                "BINPKG_GPG_SIGNING_GPG_HOME",
                options.binpkg_gpg_signing_gpg_home.as_str(),
            ),
            (
                "BINPKG_GPG_SIGNING_KEY",
                options.binpkg_gpg_signing_key.as_str(),
            ),
        ] {
            if !value.is_empty() {
                extra_env.push((key.to_string(), value.to_string()));
            }
        }
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

    ebuild_phases::run_misc_function(ebuild_phases::RunMiscFunctionArgs {
        ebuild_path,
        portage_tmpdir,
        root,
        ebuild_phase_value: "package",
        dyn_command: "__dyn_package",
        extra_env: &extra_env,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
    })
}

pub(crate) struct QuickpkgFromVdbArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) scratch_ebuild_dir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a PackageOptions,
    pub(crate) config_protect: &'a str,
    pub(crate) config_protect_mask: &'a str,
}

/// `FEATURES=unmerge-backup`'s pre-unmerge binpkg of the **installed** package,
/// built from its files under `${ROOT}` per `CONTENTS` rather than from a
/// fresh `src_install`. Stages the installed tree into the image dir and a
/// verbatim copy of the vdb dir into `build-info`, then runs the same
/// `__dyn_package` the `ebuild package` path uses. The `Packages` entry is
/// written from the vdb's own recorded metadata (not md5-cache -- the package
/// may no longer be in any repo).
///
/// A CONFIG_PROTECT'd (not -MASK'd) file is left out of the image
/// (`include_config=False`); fifo/device nodes are skipped. Returns `Ok(None)`
/// -- silently building no backup -- only when the target archive path already
/// exists (Portage overwrites; this is a conservative cut against clobbering a
/// user-placed binpkg). Portage's `BUILD_TIME` idempotency check is
/// deliberately not mirrored (see body).
///
/// Under `FEATURES=binpkg-multi-instance` the archive goes to
/// `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.<suffix>`; otherwise the bare
/// `<cat>/<pf>.<ext>`.
// Portage: vartree.py, dblink.quickpkg
pub(crate) fn quickpkg_from_vdb(args: QuickpkgFromVdbArgs<'_>) -> Result<Option<PathBuf>, String> {
    let QuickpkgFromVdbArgs {
        root,
        category,
        package,
        pf,
        scratch_ebuild_dir,
        portage_tmpdir,
        options,
        config_protect,
        config_protect_mask,
    } = args;
    let ext = binpkg_extension(&options.binpkg_format)?;
    // The entry is read through the root's installed-database backend: the entry
    // directory on `files`, a scratch materialized copy under
    // `scratch_ebuild_dir` on a database backend.
    let db = portage_vdb::for_root(root);
    let key = portage_vdb::EntryKey::new(category, pf);
    let vdb_dir = match db.entry_path(&key) {
        Some(dir) => dir,
        None => {
            let dir = scratch_ebuild_dir.join("vdb-entry").join(category).join(pf);
            if !portage_vdb::materialize_entry(db.as_ref(), &key, &dir)
                .map_err(|e| e.to_string())?
            {
                return Err(format!("{category}/{pf}: not installed"));
            }
            dir
        }
    };
    let vdb_build_time = ebuild_merge::read_entry_text(root, category, pf, "BUILD_TIME")
        .unwrap_or_default()
        .trim()
        .to_string();

    // No `BUILD_TIME` idempotency check: in every removal flow that reaches here
    // Portage's own bintree was never populated, so its check always rebuilds;
    // consulting the raw index here instead once skipped a backup whenever a
    // stale stanza shared the re-merged package's `BUILD_TIME`.
    let cpv = format!("{category}/{pf}");

    let multi_instance_suffix = multi_instance_binpkg_suffix(&options.binpkg_format)?;
    let build_id = options.binpkg_multi_instance.then(|| {
        allocate_binpkg_build_id(
            &options.pkgdir,
            category,
            package,
            pf,
            multi_instance_suffix,
        )
    });
    let binpkg_path = match build_id {
        Some(id) => options
            .pkgdir
            .join(category)
            .join(package)
            .join(format!("{pf}-{id}.{multi_instance_suffix}")),
        None => options.pkgdir.join(category).join(format!("{pf}.{ext}")),
    };
    if binpkg_path.exists() {
        return Ok(None);
    }

    // Error shapes mirror `read_to_string`: io error, ENOENT for missing, UTF-8
    // error text for non-UTF-8 CONTENTS.
    let contents_path = vdb_dir.join("CONTENTS");
    let contents_err = |e: &dyn std::fmt::Display| format!("{}: {e}", contents_path.display());
    let contents = match db.read_file(&key, "CONTENTS") {
        Ok(Some(bytes)) => String::from_utf8(bytes).map_err(|_| {
            contents_err(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            ))
        })?,
        Ok(None) => return Err(contents_err(&std::io::Error::from_raw_os_error(2))),
        Err(portage_vdb::Error::Io { source, .. }) => return Err(contents_err(&source)),
        Err(e) => return Err(contents_err(&e)),
    };
    let vdb_ebuild = vdb_dir.join(format!("{pf}.ebuild"));
    if !ebuild_merge::entry_file_is_regular(root, category, pf, &format!("{pf}.ebuild")) {
        return Err(format!(
            "{}: no vdb ebuild to package from (installed before portuale kept one?)",
            vdb_dir.display()
        ));
    }

    // The vdb layout is <cat>/<pf>/<pf>.ebuild; copy into a <cat>/<pn> layout
    // so `compute_environment`'s path parse works.
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

    // build-info/ is a verbatim copy of the vdb dir.
    let build_info = env.build_info();
    let _ = std::fs::remove_dir_all(&build_info);
    copy_dir_recursive(&vdb_dir, &build_info)?;

    // Same temp-file-then-rename as `package_after_install`, so a failed
    // compressor never leaves a corrupt archive at `binpkg_path`.
    let tmp_path = binpkg_path.with_extension(format!(
        "{}.tmp{}",
        binpkg_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default(),
        std::process::id()
    ));
    let status = invoke_dyn_package(InvokeDynPackageArgs {
        ebuild_path: &scratch_ebuild,
        portage_tmpdir,
        root,
        options,
        binpkg_path: &tmp_path,
        build_id: None,
    })?;
    if status != 0 {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("{category}/{pf}: __dyn_package failed ({status})"));
    }
    std::fs::rename(&tmp_path, &binpkg_path)
        .map_err(|e| format!("{} -> {}: {e}", tmp_path.display(), binpkg_path.display()))?;

    // `$PKGDIR/Packages` entry from the vdb's own build-info files.
    let bi = |k: &str| {
        ebuild_merge::read_entry_text(root, category, pf, k)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let build_time = vdb_build_time.as_str();
    // The stanza `BUILD_ID` is the installed entry's own, not the freshly
    // allocated filename suffix: a `quickpkg` archive carries no new
    // `BUILD_ID` (Portage never exports one on this path), so an entry for
    // a source-installed package omits it even in the multi-instance
    // layout. Same for the archive itself: no `BUILD_ID` export.
    // Portage: bin/quickpkg; bintree.py, _pkgindex_entry
    let build_id_str = bi("BUILD_ID");
    // `PATH` is always written (also needed by the local-pkgdir scan's
    // mtime-staleness fast path); derived from `binpkg_path` relative to
    // `$PKGDIR` so the multi-instance subdir path carries through.
    let path_field = binpkg_path
        .strip_prefix(&options.pkgdir)
        .ok()
        .and_then(|rel| rel.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{category}/{pf}.{ext}"));
    let size_str = std::fs::metadata(&binpkg_path)
        .map(|st| st.len().to_string())
        .unwrap_or_default();
    let mtime_str = std::fs::metadata(&binpkg_path)
        .ok()
        .map(|st| binpkg::file_mtime(&st).to_string())
        .unwrap_or_default();
    let (md5_str, sha1_str) = binpkg_checksums(&binpkg_path).unwrap_or_default();
    // The stanza is whatever of the vdb dir survives `PackageIndex.write`:
    // `EAPI` verbatim unless `"0"`, `REPO_REVISIONS` verbatim when carried,
    // `repository` verbatim as last key `REPO`, keys in sorted order, no
    // `PF`/`CATEGORY` lines. Empty values are skipped, like Portage's guard.
    let slot = omit_stanza_default("SLOT", bi("SLOT").as_str()).to_string();
    let eapi = omit_stanza_default("EAPI", bi("EAPI").as_str()).to_string();
    let keywords = bi("KEYWORDS");
    let use_flags = bi("USE");
    let license = bi("LICENSE");
    let iuse = bi("IUSE");
    let properties = bi("PROPERTIES");
    let restrict = bi("RESTRICT");
    let depend = bi("DEPEND");
    let rdepend = bi("RDEPEND");
    let bdepend = bi("BDEPEND");
    let pdepend = bi("PDEPEND");
    let idepend = bi("IDEPEND");
    let defined_phases = bi("DEFINED_PHASES");
    let repo_revisions = bi("REPO_REVISIONS");
    let repository = bi("repository");
    let mut fields: Vec<(&'static str, &str)> = vec![
        ("BDEPEND", &bdepend),
        ("BUILD_ID", &build_id_str),
        ("BUILD_TIME", build_time),
        ("CPV", &cpv),
        ("DEFINED_PHASES", &defined_phases),
        ("DEPEND", &depend),
        ("EAPI", &eapi),
        ("IDEPEND", &idepend),
        ("IUSE", &iuse),
        ("KEYWORDS", &keywords),
        ("LICENSE", &license),
        ("MD5", &md5_str),
        ("PATH", &path_field),
        ("PDEPEND", &pdepend),
        ("PROPERTIES", &properties),
        ("RDEPEND", &rdepend),
        ("REPO_REVISIONS", &repo_revisions),
        ("RESTRICT", &restrict),
        ("SHA1", &sha1_str),
        ("SIZE", &size_str),
        ("SLOT", &slot),
        ("USE", &use_flags),
        ("MTIME", &mtime_str),
        ("REPO", &repository),
    ];
    sort_stanza_fields_for_write(&mut fields);
    write_packages_index_entry(
        &options.pkgdir,
        &cpv,
        &fields,
        options.pkgindex_header.as_ref().map(|h| &h.child),
    )?;

    Ok(Some(binpkg_path))
}

/// Recursively copy `src` -> `dst` (files, symlinks-as-symlinks,
/// subdirs), creating `dst`. Used to stage the vdb dir as
/// `${PORTAGE_BUILDDIR}/build-info` for `quickpkg_from_vdb`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for entry in
        portage_util::read_dir_entries(src).map_err(|e| format!("{}: {e}", src.display()))?
    {
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
    use portage_util::TempDir;

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-ebuild-package-test").keep()
    }

    /// Expected value from Portage (container bed): `make.conf MAKEOPTS=-j5`
    /// must reach the gpkg compressor even outside a repo.
    #[test]
    fn package_makeopts_falls_back_to_the_global_chain_outside_a_repo() {
        let probe = tempdir();
        let repo = probe.join("repo");
        let prof = repo.join("profiles/default");
        let portage_dir = probe.join("etc/portage");
        std::fs::create_dir_all(&prof).unwrap();
        std::fs::create_dir_all(&portage_dir).unwrap();
        std::fs::write(prof.join("make.defaults"), "ARCH=\"amd64\"\n").unwrap();
        std::fs::write(portage_dir.join("make.conf"), "MAKEOPTS=\"-j5\"\n").unwrap();
        std::fs::write(
            portage_dir.join("repos.conf"),
            "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = repo\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(&prof, portage_dir.join("make.profile")).unwrap();
        // The scratch layout `quickpkg_from_vdb` uses: no repo ancestor.
        let scratch = probe.join("scratch/qp/q");
        std::fs::create_dir_all(&scratch).unwrap();
        let ebuild = scratch.join("q-1.ebuild");
        std::fs::write(&ebuild, "EAPI=8\nSLOT=0\n").unwrap();

        assert_eq!(
            ebuild_phases::resolve_standalone_chain_scalar(&ebuild, &probe, &probe, "MAKEOPTS"),
            None,
            "the ebuild-path chain resolves nothing outside a repo"
        );
        assert_eq!(
            package_makeopts(&ebuild, &probe, &probe).as_deref(),
            Some("-j5")
        );
        let _ = std::fs::remove_dir_all(&probe);
    }

    /// Re-derivation from a resolved list is by plain presence, and the layout
    /// bit stays run-wide.
    #[test]
    fn set_resolved_features_rederives_the_binpkg_affecting_tokens() {
        let mut options = PackageOptions {
            buildpkg_live: false,
            binpkg_multi_instance: false,
            binpkg_signing: false,
            ..PackageOptions::default()
        };
        options
            .set_resolved_features("binpkg-multi-instance buildpkg-live binpkg-signing splitdebug");
        assert!(options.buildpkg_live);
        assert!(options.binpkg_signing);
        // The layout bit is run-wide-only: a per-entry token doesn't flip it.
        assert!(!options.binpkg_multi_instance);

        // A folded list that no longer carries a token turns it back off
        // -- this is a full re-derivation, not an additive merge.
        options.set_resolved_features("splitdebug");
        assert!(!options.buildpkg_live);
        assert!(!options.binpkg_signing);

        // Config scalars are untouched.
        assert_eq!(
            options.binpkg_compress,
            PackageOptions::default().binpkg_compress
        );
        assert_eq!(
            options.binpkg_format,
            PackageOptions::default().binpkg_format
        );
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
        // "bzip2" doubles as the ${PORTAGE_BZIP2_COMMAND} value so find_binary
        // succeeds without depending on another installed binary.
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
        // {JOBS} is the host CPU count -- not pinned, just proven substituted
        // (no literal "{JOBS}" left, and a positive integer follows "-T").
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
    fn makeopts_to_job_count_matches_the_portage_greedy_regex() {
        assert_eq!(makeopts_to_job_count("-j1"), "1");
        assert_eq!(makeopts_to_job_count("-j4 -l5"), "4");
        assert_eq!(makeopts_to_job_count("-j 3 -j12"), "12");
        assert_eq!(makeopts_to_job_count("--jobs= 6"), "6");
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .to_string();
        // `--jobs=6` has no whitespace after `=`: Portage falls back to CPU count.
        assert_eq!(makeopts_to_job_count("--jobs=6"), cpus);
        assert_eq!(makeopts_to_job_count(""), cpus);
    }

    #[test]
    fn phase_compression_command_reads_the_resolved_config() {
        let cfg = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        // Flag-less templates collapse to the bare binary.
        assert_eq!(
            phase_compression_command(cfg(&[("BINPKG_COMPRESS", "gzip"), ("MAKEOPTS", "-j1")])),
            Some("gzip".to_string())
        );
        assert_eq!(
            phase_compression_command(cfg(&[
                ("BINPKG_COMPRESS", "gzip"),
                ("BINPKG_COMPRESS_FLAGS", "-1"),
                ("BINPKG_COMPRESS_FLAGS_GZIP", "-9"),
            ])),
            Some("gzip -9".to_string())
        );
        assert_eq!(
            phase_compression_command(cfg(&[("BINPKG_COMPRESS", "")])),
            Some("cat".to_string())
        );
        assert_eq!(
            phase_compression_command(cfg(&[("BINPKG_COMPRESS", "made-up-codec")])),
            None
        );
    }

    #[test]
    fn resolve_compression_command_is_none_for_an_unknown_compressor() {
        assert_eq!(
            resolve_compression_command("made-up-codec", "", "bzip2"),
            None
        );
    }

    #[test]
    fn resolve_binpkg_format_defaults_to_gpkg_and_obeys_the_chain() {
        // Expected values from Portage's `make.globals` defaults.
        let cfg = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(PackageOptions::default().binpkg_format, "gpkg");
        assert_eq!(resolve_binpkg_format(cfg(&[])), "gpkg");
        assert_eq!(
            resolve_binpkg_format(cfg(&[("BINPKG_FORMAT", "xpak")])),
            "xpak"
        );
        assert_eq!(
            resolve_binpkg_format(cfg(&[("BINPKG_FORMAT", "gpkg")])),
            "gpkg"
        );
    }

    #[test]
    fn refresh_entry_compression_command_rederives_the_command_from_the_layered_env() {
        let command_of = |env: &[(String, String)]| {
            env.iter()
                .rev()
                .find(|(k, _)| k == "PORTAGE_COMPRESSION_COMMAND")
                .map(|(_, v)| v.clone())
        };
        // Matched per-entry BINPKG_COMPRESS=gzip over a run-wide bzip2 pair:
        // the pair is replaced.
        let mut matched = vec![
            (
                "PORTAGE_COMPRESSION_COMMAND".to_string(),
                "bzip2".to_string(),
            ),
            ("BINPKG_COMPRESS".to_string(), "gzip".to_string()),
            ("MAKEOPTS".to_string(), "-j1".to_string()),
        ];
        refresh_entry_compression_command(&mut matched, None);
        assert_eq!(command_of(&matched), Some("gzip".to_string()));
        // Unmatched entry: recomputes byte-identically to the run-wide value.
        let mut unmatched = vec![(
            "PORTAGE_COMPRESSION_COMMAND".to_string(),
            "bzip2".to_string(),
        )];
        refresh_entry_compression_command(&mut unmatched, None);
        assert_eq!(command_of(&unmatched), Some("bzip2".to_string()));
        // Unknown per-entry codec: the pair is removed, matching Portage leaving
        // the key unset.
        let mut unknown = vec![
            (
                "PORTAGE_COMPRESSION_COMMAND".to_string(),
                "bzip2".to_string(),
            ),
            ("BINPKG_COMPRESS".to_string(), "made-up-codec".to_string()),
        ];
        refresh_entry_compression_command(&mut unknown, None);
        assert_eq!(command_of(&unknown), None);
        // A `make.conf` BINPKG_COMPRESS with no calling-env value and no
        // `package.env` match still reaches the pipe through the config tail.
        // Skipped when the ambient process env sets the compression keys
        // (the ambient value would rightly win).
        if std::env::var_os("BINPKG_COMPRESS").is_none()
            && std::env::var_os("BINPKG_COMPRESS_FLAGS").is_none()
            && std::env::var_os("PORTAGE_BZIP2_COMMAND").is_none()
        {
            let config = portage_profile::Config {
                other_vars: [("BINPKG_COMPRESS".to_string(), "gzip".to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            };
            let mut filed = vec![
                (
                    "PORTAGE_COMPRESSION_COMMAND".to_string(),
                    "bzip2".to_string(),
                ),
                ("MAKEOPTS".to_string(), "-j1".to_string()),
            ];
            refresh_entry_compression_command(&mut filed, Some(&config));
            assert_eq!(
                command_of(&filed),
                Some("gzip".to_string()),
                "the make.conf value must survive the re-derivation"
            );
        }
    }

    #[test]
    fn resolve_compression_command_is_none_when_the_bzip2_var_names_a_missing_binary() {
        // A genuine compressor name, but the bzip2 var names a missing binary:
        // the command is left unset, not fabricated.
        assert_eq!(
            resolve_compression_command(
                "bzip2",
                "",
                "this-binary-definitely-does-not-exist-anywhere-xyz"
            ),
            None
        );
    }

    // `repo_root_for` lives in, and is tested in, `ebuild_phases.rs`.

    #[test]
    fn write_packages_index_entry_creates_a_header_then_the_entry() {
        let tmp = tempdir();
        write_packages_index_entry(
            &tmp,
            "dev-libs/foo-1.0",
            &[("CPV", "dev-libs/foo-1.0"), ("SLOT", "0")],
            None,
        )
        .unwrap();
        let text = std::fs::read_to_string(tmp.join("Packages")).unwrap();
        let blocks: Vec<&str> = text.trim().split("\n\n").collect();
        assert_eq!(blocks.len(), 2, "header block + one entry: {text:?}");
        assert!(blocks[0].contains("TIMESTAMP:"));
        // Without VERSION Portage discards the index wholesale.
        assert!(
            blocks[0].contains("VERSION: 0"),
            "header must carry VERSION for real Portage: {text:?}"
        );
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
            None,
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
            None,
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
            None,
        )
        .unwrap();
        write_packages_index_entry(
            &tmp,
            "dev-libs/bar-2.0",
            &[("CPV", "dev-libs/bar-2.0"), ("SLOT", "0")],
            None,
        )
        .unwrap();

        let text = std::fs::read_to_string(tmp.join("Packages")).unwrap();
        assert!(text.contains("CPV: dev-libs/foo-1.0"));
        assert!(text.contains("CPV: dev-libs/bar-2.0"));
    }

    fn compress_index_snapshot(features: &str) -> portage_profile::HeaderSnapshot {
        vec![("FEATURES".to_string(), Some(features.to_string()))]
    }

    /// Without `compress-index` Portage unlinks a stale `Packages.gz`
    /// (`ENOENT` ignored); the lock file goes just the same.
    // Portage: bintree.py, _pkgindex_write; locks.py, unlockfile
    #[test]
    fn write_packages_index_entry_removes_a_stale_packages_gz_without_compress_index() {
        let tmp = tempdir();
        std::fs::write(tmp.join("Packages.gz"), b"stale").unwrap();
        write_packages_index_entry(
            &tmp,
            "dev-libs/foo-1.0",
            &[("CPV", "dev-libs/foo-1.0"), ("SLOT", "0")],
            Some(&compress_index_snapshot("sandbox -compress-index")),
        )
        .unwrap();
        assert!(
            !tmp.join("Packages.gz").exists(),
            "stale copy removed, like Portage's else branch"
        );
        assert!(
            !tmp.join(".Packages.portage_lockfile").exists(),
            "lock released as usual"
        );
    }

    /// A config-less (`None`) write has no resolved `FEATURES` to judge by,
    /// so it neither creates nor removes `Packages.gz` — but the lock file
    /// is still Portage's `wantnewlockfile` shape and goes.
    // Portage: bintree.py, _pkgindex_write; locks.py, unlockfile
    #[test]
    fn write_packages_index_entry_leaves_packages_gz_alone_without_a_snapshot() {
        let tmp = tempdir();
        write_packages_index_entry(
            &tmp,
            "dev-libs/foo-1.0",
            &[("CPV", "dev-libs/foo-1.0"), ("SLOT", "0")],
            None,
        )
        .unwrap();
        assert!(!tmp.join("Packages.gz").exists(), "no snapshot, no copy");
        std::fs::write(tmp.join("Packages.gz"), b"stale").unwrap();
        write_packages_index_entry(
            &tmp,
            "dev-libs/bar-2.0",
            &[("CPV", "dev-libs/bar-2.0"), ("SLOT", "0")],
            None,
        )
        .unwrap();
        assert_eq!(
            std::fs::read(tmp.join("Packages.gz")).unwrap(),
            b"stale",
            "a stale copy survives a snapshot-less write"
        );
        assert!(
            !tmp.join(".Packages.portage_lockfile").exists(),
            "lock released as usual"
        );
    }

    #[test]
    fn omit_stanza_default_omits_only_exact_slot_and_eapi_zero() {
        // Only the exact `"0"` is default; anything else passes through.
        assert_eq!(omit_stanza_default("SLOT", "0"), "");
        assert_eq!(omit_stanza_default("SLOT", ""), "");
        assert_eq!(omit_stanza_default("SLOT", "1"), "1");
        assert_eq!(omit_stanza_default("SLOT", "0/1"), "0/1");
        assert_eq!(omit_stanza_default("EAPI", "0"), "");
        assert_eq!(omit_stanza_default("EAPI", "8"), "8");
        assert_eq!(omit_stanza_default("USE", "0"), "0");
        assert_eq!(omit_stanza_default("SIZE", "0"), "0");
    }

    fn probe_shaped_stanza(slot: &str) -> BuiltBinpkgStanza<'_> {
        // Expected values from Portage (container bed): the probe's stanza
        // shape, with only `SLOT` varying.
        BuiltBinpkgStanza {
            bdepend: "",
            build_id: "1",
            build_time: "1790533433",
            cpv: "l32/dep-a-1.0",
            defined_phases: "install",
            depend: "",
            eapi: "8",
            idepend: "",
            iuse: "",
            keywords: "amd64",
            license: "GPL-2",
            md5: "45ee79b46260208582e042d087ad6fff",
            path: "l32/dep-a/dep-a-1.0-1.gpkg.tar",
            pdepend: "",
            properties: "",
            provides: "",
            rdepend: "",
            repo_revisions: "{}",
            requires: "",
            restrict: "",
            sha1: "dcb4b4b5cd72b7f1c8d839c82eb94ffb12374776",
            size: "10240",
            slot,
            use_flags: "abi_x86_64 amd64 elibc_glibc kernel_linux",
            mtime: "1790533434",
            repository: "l32",
        }
    }

    #[test]
    fn stanza_bytes_for_default_slot_omit_slot_and_keep_both_digests() {
        let stanza = probe_shaped_stanza("0");
        let text = format_packages_entry(&built_binpkg_stanza_fields(&stanza));
        assert_eq!(
            text,
            "BUILD_ID: 1\n\
             BUILD_TIME: 1790533433\n\
             CPV: l32/dep-a-1.0\n\
             DEFINED_PHASES: install\n\
             EAPI: 8\n\
             KEYWORDS: amd64\n\
             LICENSE: GPL-2\n\
             MD5: 45ee79b46260208582e042d087ad6fff\n\
             PATH: l32/dep-a/dep-a-1.0-1.gpkg.tar\n\
             REPO_REVISIONS: {}\n\
             SHA1: dcb4b4b5cd72b7f1c8d839c82eb94ffb12374776\n\
             SIZE: 10240\n\
             USE: abi_x86_64 amd64 elibc_glibc kernel_linux\n\
             MTIME: 1790533434\n\
             REPO: l32\n"
        );
    }

    #[test]
    fn stanza_bytes_for_nondefault_slot_write_it_between_size_and_use() {
        let stanza = probe_shaped_stanza("1");
        let text = format_packages_entry(&built_binpkg_stanza_fields(&stanza));
        assert!(text.contains("\nSIZE: 10240\nSLOT: 1\nUSE: "), "{text:?}");
        assert!(text.contains("MD5: 45ee79b46260208582e042d087ad6fff\n"));
        assert!(text.contains("SHA1: dcb4b4b5cd72b7f1c8d839c82eb94ffb12374776\n"));
    }

    #[test]
    fn quickpkg_from_vdb_writes_portage_ordered_stanza_with_eapi_and_repo_revisions() {
        // Expected values from Portage (container probe): vdb `EAPI`/`REPO_REVISIONS`
        // carried verbatim, `repository` as last key `REPO`, sorted key order,
        // no `PF`/`CATEGORY`/`SLOT` lines.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let scratch = tmp.join("scratch");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            binpkg_compress: "bzip2".to_string(),
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        std::fs::write(root.join("etc/probe.conf"), "probe\n").unwrap();

        let vdb_dir = root.join("var/db/pkg/dev-libs/probe-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        std::fs::copy(
            repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild"),
            vdb_dir.join("probe-1.0.ebuild"),
        )
        .unwrap();
        for (name, content) in [
            ("EAPI", "8\n"),
            ("SLOT", "0\n"),
            ("CATEGORY", "dev-libs\n"),
            ("PF", "probe-1.0\n"),
            ("KEYWORDS", "amd64\n"),
            ("LICENSE", "GPL-2\n"),
            ("USE", "amd64\n"),
            ("BUILD_TIME", "1790541998\n"),
            ("REPO_REVISIONS", "{}\n"),
            ("repository", "testrepo\n"),
            ("CONTENTS", "obj /etc/probe.conf\n"),
        ] {
            std::fs::write(vdb_dir.join(name), content).unwrap();
        }

        let built = quickpkg_from_vdb(QuickpkgFromVdbArgs {
            root: &root,
            category: "dev-libs",
            package: "probe",
            pf: "probe-1.0",
            scratch_ebuild_dir: &scratch,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            config_protect: "",
            config_protect_mask: "",
        })
        .expect("quickpkg_from_vdb succeeds")
        .expect("archive built");
        assert_eq!(built, options.pkgdir.join("dev-libs/probe-1.0.tbz2"));

        let packages = std::fs::read_to_string(options.pkgdir.join("Packages")).unwrap();
        let stanza: Vec<&str> = packages
            .split("\n\n")
            .map(str::trim)
            .find(|block| block.lines().any(|l| l == "CPV: dev-libs/probe-1.0"))
            .expect("quickpkg stanza present")
            .lines()
            .collect();
        let keys: Vec<&str> = stanza
            .iter()
            .map(|l| l.split_once(": ").map(|(k, _)| k).unwrap_or(""))
            .collect();
        assert_eq!(
            keys,
            vec![
                "BUILD_TIME",
                "CPV",
                "EAPI",
                "KEYWORDS",
                "LICENSE",
                "MD5",
                "PATH",
                "REPO_REVISIONS",
                "SHA1",
                "SIZE",
                "USE",
                "MTIME",
                "REPO",
            ],
            "quickpkg stanza keys must be real's sorted order: {stanza:?}"
        );
        assert!(
            stanza.contains(&"EAPI: 8"),
            "vdb EAPI must be carried like real's: {stanza:?}"
        );
        assert!(
            stanza.contains(&"REPO: testrepo"),
            "vdb repository must be carried as REPO last like real's: {stanza:?}"
        );
        assert!(
            stanza.contains(&"REPO_REVISIONS: {}"),
            "vdb REPO_REVISIONS must be carried verbatim: {stanza:?}"
        );
        assert!(
            stanza.contains(&"PATH: dev-libs/probe-1.0.tbz2"),
            "PATH must be relative to PKGDIR: {stanza:?}"
        );
        assert!(
            !stanza.iter().any(|l| l.starts_with("SLOT:")),
            "default SLOT must be omitted like real's: {stanza:?}"
        );
        assert!(
            !stanza
                .iter()
                .any(|l| l.starts_with("PF:") || l.starts_with("CATEGORY:")),
            "PF/CATEGORY are not stanza keys in real's: {stanza:?}"
        );
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
            // Pinned to "bzip2" (not the default "zstd") so this test doesn't
            // depend on the host having `zstd` installed.
            binpkg_compress: "bzip2".to_string(),
            // Pinned explicitly: the point of this test is the xpak path,
            // not the (now gpkg) default.
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        let status = run_package(RunPackageArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            build_env: &[],
            use_flags: "",
        })
        .expect("run_package succeeds");
        assert_eq!(status, 0);

        // A genuine bzip2+tar+XPAK file: Portage's `xpak.py` writes an
        // `XPAKPACK` magic marker right before the appended metadata blob.
        let binpkg_path = options.pkgdir.join("dev-libs/packagepkg-1.0.tbz2");
        let binpkg_bytes = std::fs::read(&binpkg_path)
            .unwrap_or_else(|e| panic!("{}: {e}", binpkg_path.display()));
        assert!(!binpkg_bytes.is_empty());
        assert!(
            binpkg_bytes.windows(8).any(|w| w == b"XPAKPACK"),
            "real XPAKPACK magic not found in {}",
            binpkg_path.display()
        );

        // The binpkg reader sees it immediately -- the point of writing a
        // Packages entry in the exact format that reader parses.
        let index = portage_repo::BinaryIndex::from_pkgdir(&options.pkgdir);
        let candidates = portage_repo::list_binary_candidates(&index, "dev-libs", "packagepkg");
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.version, "1.0");
        assert_eq!(candidate.slot, "0");
        assert_eq!(candidate.keywords, vec!["amd64".to_string()]);

        // The fixture's md5-cache RDEPEND came through into the index too.
        let metadata = portage_repo::read_binary_metadata(&index, "dev-libs", "packagepkg", "1.0")
            .expect("binary metadata entry exists");
        assert_eq!(
            metadata.get("RDEPEND").map(String::as_str),
            Some("dev-libs/samepkg")
        );

        // Both fixed digests are written; without them integrity checks would
        // be silently skipped for anyone fetching this file as a remote binhost.
        // (`md5::Digest` is the shared trait both hashers implement.)
        use md5::Digest as _;
        let expected_md5: String = md5::Md5::digest(&binpkg_bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let expected_sha1: String = sha1::Sha1::digest(&binpkg_bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            metadata.get("MD5").map(String::as_str),
            Some(expected_md5.as_str())
        );
        assert_eq!(
            metadata.get("SHA1").map(String::as_str),
            Some(expected_sha1.as_str())
        );
    }

    #[test]
    fn package_after_install_writes_the_resolved_use_flags_into_the_packages_index() {
        // Resolved `USE` flags pass straight through to the index `USE` field.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        let status = ebuild_phases::run_commands(ebuild_phases::RunCommandsArgs {
            ebuild_path: &ebuild,
            commands: &["install"],
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            distdir: &options.distdir,
            debug: options.debug,
            config_root: &options.config_root,
            shell: options.shell,
            build_env: &[],
        })
        .expect("install succeeds");
        assert_eq!(status, 0);

        let status = package_after_install(PackageAfterInstallArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            use_flags: "foo bar",
            per_entry_binpkg_multi_instance: options.binpkg_multi_instance,
        })
        .expect("package_after_install succeeds");
        assert_eq!(status, 0);

        let index = portage_repo::BinaryIndex::from_pkgdir(&options.pkgdir);
        let metadata = portage_repo::read_binary_metadata(&index, "dev-libs", "packagepkg", "1.0")
            .expect("binary metadata entry exists");
        assert_eq!(metadata.get("USE").map(String::as_str), Some("foo bar"));
    }

    #[test]
    fn package_after_install_stanza_omits_default_slot_and_writes_both_digests() {
        // Expected values from Portage (container probe): no `SLOT` line for the
        // default slot, both fixed digests, translated `MTIME` key.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        let status = ebuild_phases::run_commands(ebuild_phases::RunCommandsArgs {
            ebuild_path: &ebuild,
            commands: &["install"],
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            distdir: &options.distdir,
            debug: options.debug,
            config_root: &options.config_root,
            shell: options.shell,
            build_env: &[],
        })
        .expect("install succeeds");
        assert_eq!(status, 0);

        let status = package_after_install(PackageAfterInstallArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            use_flags: "",
            per_entry_binpkg_multi_instance: options.binpkg_multi_instance,
        })
        .expect("package_after_install succeeds");
        assert_eq!(status, 0);

        let packages = std::fs::read_to_string(options.pkgdir.join("Packages")).unwrap();
        assert!(
            !packages.lines().any(|l| l.starts_with("SLOT:")),
            "default SLOT must be omitted like real's: {packages:?}"
        );
        assert!(
            packages.lines().any(|l| l.starts_with("MD5: ")),
            "fixed MD5 digest missing: {packages:?}"
        );
        assert!(
            packages.lines().any(|l| l.starts_with("SHA1: ")),
            "fixed SHA1 digest missing: {packages:?}"
        );
        assert!(
            packages.lines().any(|l| l.starts_with("MTIME: ")),
            "translated MTIME key missing: {packages:?}"
        );
        assert!(
            !packages.lines().any(|l| l.starts_with("_mtime_:")),
            "internal _mtime_ key must never be written: {packages:?}"
        );
    }

    /// `FEATURES=unmerge-backup` / `quickpkg` with `BINPKG_FORMAT=gpkg`: the
    /// installed files and the vdb entry become a gpkg through the natively
    /// routed `gpkg-helper.py compress`.
    #[test]
    fn quickpkg_from_vdb_with_gpkg_format_builds_a_readable_gpkg() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let scratch = tmp.join("scratch");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            // Not the default zstd: the test must not need zstd installed.
            binpkg_compress: "bzip2".to_string(),
            binpkg_format: "gpkg".to_string(),
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        std::fs::write(root.join("etc/probe.conf"), "probe\n").unwrap();

        let vdb_dir = root.join("var/db/pkg/dev-libs/probe-1.0");
        std::fs::create_dir_all(&vdb_dir).unwrap();
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        std::fs::copy(
            repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild"),
            vdb_dir.join("probe-1.0.ebuild"),
        )
        .unwrap();
        for (name, content) in [
            ("EAPI", "8\n"),
            ("SLOT", "0\n"),
            ("CATEGORY", "dev-libs\n"),
            ("PF", "probe-1.0\n"),
            ("KEYWORDS", "amd64\n"),
            ("LICENSE", "GPL-2\n"),
            ("USE", "amd64\n"),
            ("BUILD_TIME", "1790541998\n"),
            ("REPO_REVISIONS", "{}\n"),
            ("repository", "testrepo\n"),
            ("CONTENTS", "obj /etc/probe.conf\n"),
        ] {
            std::fs::write(vdb_dir.join(name), content).unwrap();
        }

        let built = quickpkg_from_vdb(QuickpkgFromVdbArgs {
            root: &root,
            category: "dev-libs",
            package: "probe",
            pf: "probe-1.0",
            scratch_ebuild_dir: &scratch,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            config_protect: "",
            config_protect_mask: "",
        })
        .expect("quickpkg_from_vdb succeeds")
        .expect("archive built");
        assert_eq!(built, options.pkgdir.join("dev-libs/probe-1.0.gpkg.tar"));
        let meta = crate::binpkg::read_gpkg_metadata(&built)
            .expect("portuale's gpkg reader parses the native writer's output");
        assert_eq!(meta.get("PF").map(String::as_str), Some("probe-1.0"));
        assert_eq!(meta.get("CATEGORY").map(String::as_str), Some("dev-libs"));
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
            // Pinned to "bzip2" (not the default "zstd") so this test doesn't
            // depend on `zstd` being installed.
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        let status = run_package(RunPackageArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            build_env: &[],
            use_flags: "",
        })
        .expect("run_package succeeds");
        assert_eq!(status, 0);

        // A genuine `.gpkg.tar`: an outer tar of the `gpkg-1` marker, the
        // compressed `metadata.tar`/`image.tar`, and a `Manifest`.
        let binpkg_path = options.pkgdir.join("dev-libs/packagepkg-1.0.gpkg.tar");
        assert!(
            binpkg_path.is_file(),
            "{} should exist",
            binpkg_path.display()
        );

        // The round trip: portuale's own gpkg reader reads back what the helper wrote.
        let scalar = crate::binpkg::read_gpkg_metadata(&binpkg_path)
            .expect("portuale's gpkg reader parses the real writer's output");
        assert_eq!(scalar.get("SLOT").map(String::as_str), Some("0"));
        assert_eq!(scalar.get("CATEGORY").map(String::as_str), Some("dev-libs"));
        assert_eq!(scalar.get("PF").map(String::as_str), Some("packagepkg-1.0"));
        // The dep strings went from `build-info` into `metadata.tar`.
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

        // The directory scan reads the file directly: the just-written entry's
        // `_mtime_`/`SIZE` still agree with the file, so this hits the
        // mtime-staleness fast path rather than re-parsing the archive.
        let scanned =
            crate::binpkg::populate_local_pkgdir(&options.pkgdir, true).expect("scan succeeds");
        assert_eq!(scanned.len(), 1);
        assert_eq!(
            scanned[0].get("CPV").map(String::as_str),
            Some("dev-libs/packagepkg-1.0")
        );
    }

    #[test]
    fn real_package_with_binpkg_multi_instance_writes_the_cat_pn_subdir_layout() {
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

        // Build it twice -- multi-instance keeps both builds under distinct
        // `BUILD_ID`s instead of overwriting the same bare filename.
        let status = run_package(RunPackageArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            build_env: &[],
            use_flags: "",
        })
        .expect("run_package succeeds");
        assert_eq!(status, 0);
        let status = run_package(RunPackageArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            build_env: &[],
            use_flags: "",
        })
        .expect("run_package succeeds");
        assert_eq!(status, 0);

        // `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.gpkg.tar`, not a bare name.
        let first = options
            .pkgdir
            .join("dev-libs/packagepkg/packagepkg-1.0-1.gpkg.tar");
        let second = options
            .pkgdir
            .join("dev-libs/packagepkg/packagepkg-1.0-2.gpkg.tar");
        assert!(first.is_file(), "{} should exist", first.display());
        assert!(second.is_file(), "{} should exist", second.display());
        // Neither the bare single-instance name nor the old (wrong)
        // one-level multi-instance name was written.
        assert!(
            !options
                .pkgdir
                .join("dev-libs/packagepkg-1.0.gpkg.tar")
                .exists()
        );
        assert!(
            !options
                .pkgdir
                .join("dev-libs/packagepkg-1.0-1.gpkg.tar")
                .exists()
        );

        // Each archive's embedded `build-info/BUILD_ID` must match its own
        // filename suffix.
        let first_meta = crate::binpkg::read_gpkg_metadata(&first).expect("reads metadata");
        assert_eq!(first_meta.get("BUILD_ID").map(String::as_str), Some("1"));
        let second_meta = crate::binpkg::read_gpkg_metadata(&second).expect("reads metadata");
        assert_eq!(second_meta.get("BUILD_ID").map(String::as_str), Some("2"));

        // Both builds' entries survive: PATH-keyed dedup, not CPV-keyed.
        let packages = std::fs::read_to_string(options.pkgdir.join("Packages")).unwrap();
        assert!(packages.contains("PATH: dev-libs/packagepkg/packagepkg-1.0-1.gpkg.tar"));
        assert!(packages.contains("PATH: dev-libs/packagepkg/packagepkg-1.0-2.gpkg.tar"));
        assert!(packages.contains("BUILD_ID: 1"));
        assert!(packages.contains("BUILD_ID: 2"));

        // The scan walks the subdir, keeps each build's BUILD_ID, and CPV stays
        // the bare `cat/pf` (downstream splits `PF` from `CPV`, like Portage).
        let scanned =
            crate::binpkg::populate_local_pkgdir(&options.pkgdir, true).expect("scan succeeds");
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
        }
    }

    /// Expected values from Portage: after a package step `build-info` holds
    /// `BINPKGMD5` and, when the archive carries one, `BUILD_ID`.
    #[test]
    fn package_records_binpkgmd5_and_build_id_in_build_info_like_portage() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");
        let build_info = ebuild_phases::compute_environment(&ebuild, &portage_tmpdir)
            .unwrap()
            .build_info();

        for (multi, archive, build_id) in [
            (false, "dev-libs/packagepkg-1.0.gpkg.tar", None),
            (
                true,
                "dev-libs/packagepkg/packagepkg-1.0-1.gpkg.tar",
                Some("1\n"),
            ),
        ] {
            let options = PackageOptions {
                debug: false,
                pkgdir: tmp.join(format!("pkgdir-{multi}")),
                distdir: tmp.join("distdir"),
                shell: ebuild_phases::ShellBackend::default(),
                binpkg_format: "gpkg".to_string(),
                binpkg_compress: "bzip2".to_string(),
                binpkg_multi_instance: multi,
                ..PackageOptions::default()
            };
            let status = run_package(RunPackageArgs {
                ebuild_path: &ebuild,
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                options: &options,
                build_env: &[],
                use_flags: "",
            })
            .expect("run_package succeeds");
            assert_eq!(status, 0);
            let archive = options.pkgdir.join(archive);
            let md5 = crate::ebuild_merge::md5_hex(&archive).unwrap();
            assert_eq!(
                std::fs::read_to_string(build_info.join("BINPKGMD5")).unwrap(),
                format!("{md5}\n"),
                "multi={multi}"
            );
            let packages = std::fs::read_to_string(options.pkgdir.join("Packages")).unwrap();
            assert!(packages.contains(&format!("MD5: {md5}")), "{packages}");
            assert_eq!(
                std::fs::read_to_string(build_info.join("BUILD_ID"))
                    .ok()
                    .as_deref(),
                build_id,
                "multi={multi}"
            );
        }
    }

    #[test]
    fn real_multi_instance_xpak_uses_the_dot_xpak_extension_and_scans_back() {
        // An xpak multi-instance binpkg is `<pkgdir>/<cat>/<pn>/<pf>-<id>.xpak`:
        // the `.xpak` extension, byte-identical to a `.tbz2`. The index-less
        // scan must walk the subdir and carry `BUILD_ID` back.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            binpkg_format: "xpak".to_string(),
            binpkg_multi_instance: true,
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        let status = run_package(RunPackageArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            build_env: &[],
            use_flags: "",
        })
        .expect("run_package succeeds");
        assert_eq!(status, 0);

        let xpak = options
            .pkgdir
            .join("dev-libs/packagepkg/packagepkg-1.0-1.xpak");
        assert!(xpak.is_file(), "{} should exist", xpak.display());
        // Not a `.tbz2`, and not the wrong one-level name.
        assert!(!options.pkgdir.join("dev-libs/packagepkg-1.0.tbz2").exists());
        assert!(
            !options
                .pkgdir
                .join("dev-libs/packagepkg/packagepkg-1.0-1.tbz2")
                .exists()
        );

        // The `.xpak` file reads with the same reader as a `.tbz2`, and its
        // embedded `build-info/BUILD_ID` matches the filename suffix.
        let meta = crate::binpkg::read_xpak_metadata(&xpak).expect("reads xpak metadata");
        assert_eq!(meta.get("PF").map(String::as_str), Some("packagepkg-1.0"));
        assert_eq!(meta.get("BUILD_ID").map(String::as_str), Some("1"));

        // Drop the just-written index so the scan must parse the archive.
        let _ = std::fs::remove_file(options.pkgdir.join("Packages"));
        let scanned =
            crate::binpkg::populate_local_pkgdir(&options.pkgdir, true).expect("scan succeeds");
        assert_eq!(scanned.len(), 1);
        assert_eq!(
            scanned[0].get("CPV").map(String::as_str),
            Some("dev-libs/packagepkg-1.0")
        );
        assert_eq!(scanned[0].get("BUILD_ID").map(String::as_str), Some("1"));
        assert_eq!(
            scanned[0].get("PATH").map(String::as_str),
            Some("dev-libs/packagepkg/packagepkg-1.0-1.xpak")
        );
    }

    #[test]
    fn a_failed_packaging_phase_never_touches_the_final_binpkg_path() {
        // `__dyn_package` only ever writes a same-directory temp file, so a
        // mid-write failure can never leave a corrupt archive at the
        // discoverable index path.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        let options = PackageOptions {
            debug: false,
            pkgdir: tmp.join("pkgdir"),
            distdir: tmp.join("distdir"),
            shell: ebuild_phases::ShellBackend::default(),
            // An unresolvable compressor leaves PORTAGE_COMPRESSION_COMMAND
            // unset, so `__dyn_package` dies before ever opening the tar pipe:
            // a clean, deterministic failure that never touches the tmpfile.
            binpkg_compress: "made-up-codec".to_string(),
            // Pinned explicitly: the deterministic die this test needs is the
            // xpak tar pipe's guard, not the gpkg path.
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

        // Sentinel content at the final path, standing in for a prior
        // successful build -- a failed re-package must never clobber it.
        let binpkg_path = options.pkgdir.join("dev-libs/packagepkg-1.0.tbz2");
        std::fs::create_dir_all(binpkg_path.parent().unwrap()).unwrap();
        std::fs::write(&binpkg_path, b"OLD-CONTENT").unwrap();

        let status = run_package(RunPackageArgs {
            ebuild_path: &ebuild,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            build_env: &[],
            use_flags: "",
        })
        .expect("run_package returns Ok(status)");
        assert_ne!(
            status, 0,
            "packaging should fail with an unresolvable compressor"
        );

        assert_eq!(
            std::fs::read(&binpkg_path).unwrap(),
            b"OLD-CONTENT",
            "a failed packaging phase must not touch the final binpkg path"
        );
        // No stray temp file left behind either.
        let leftovers: Vec<_> = portage_util::read_dir_entries(binpkg_path.parent().unwrap())
            .unwrap()
            .into_iter()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
    }
}
