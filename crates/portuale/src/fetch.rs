//! Network fetch: the second half of fetching a package's sources (the
//! offline half -- SRC_URI flattening, Manifest parsing, digest
//! verification -- lives in `portage_fetch`). Shells out to `wget` using
//! `make.globals`' default `FETCHCOMMAND`, and honors `FEATURES=distlocks`
//! with a blocking `flock` on a sibling lockfile per distfile.
//!
//! Scope cuts a caller must know: resume means any non-empty partial (no
//! size threshold); `mirror://` resolution, `RESTRICT=mirror/fetch/
//! primaryuri`, `force-mirror`, on-filesystem mirrors and `pkg_nofetch`
//! are modelled; third-party mirrors are never shuffled (deterministic);
//! no `FEATURES=verify-sig` (a `gpkg`/sync concept, not distfile-fetch).
use mrg_director::{FetchRequest, Fetcher, WgetFetcher};
use portage_fetch::{
    SrcUriEntry, flatten_src_uri, parse_manifest, parse_thirdpartymirrors,
    resolve_mirror_candidates, verify_digests,
};
use std::path::{Path, PathBuf};

use crate::portage_lock::PortageLockfile;

/// Default `GENTOO_MIRRORS` when the env var is unset/empty. A `FetchOptions`
/// field rather than an ambient read, so tests control it directly (most set
/// it to `vec![]`, so a deliberately-failing fetch never reaches the genuine
/// mirror as an unintended fallback).
pub fn gentoo_mirrors_from_env() -> Vec<String> {
    match std::env::var("GENTOO_MIRRORS") {
        Ok(value) if !value.trim().is_empty() => {
            value.split_whitespace().map(String::from).collect()
        }
        _ => vec!["http://distfiles.gentoo.org".to_string()],
    }
}

/// `distdir`/`config_root` are set at the CLI boundary, never read from the
/// ambient env inside library code (a genuine Gentoo host's `/etc/portage`
/// would otherwise leak into tests). `Default` matches `make.globals`
/// (`DISTDIR`, the default mirrors) with an impossible `config_root`
/// sentinel (empty `custommirrors` unless opted in) and `distlocks: true`
/// (a default `FEATURES` token).
pub struct FetchOptions {
    pub distdir: PathBuf,
    pub gentoo_mirrors: Vec<String>,
    pub config_root: PathBuf,
    pub distlocks: bool,
    /// Portage's `RESTRICT=mirror`: skip the public `GENTOO_MIRRORS` fallback
    /// for this package (`mirror://` expansions and explicit URIs are still
    /// tried). A `mirror+` prefix re-permits it for that one file.
    /// Sourced from the ebuild's `RESTRICT` field by `ebuild_phases::fetch_sources`.
    pub restrict_mirror: bool,
    /// Portage's `RESTRICT=fetch`: bar plain `SRC_URI` URIs and the public
    /// mirrors, so the file resolves only from `DISTDIR`/`custommirrors`/
    /// `mirror://` mirrors. A `fetch+`/`mirror+` prefix re-permits the URI.
    /// Sourced from the ebuild's `RESTRICT` field by `ebuild_phases::fetch_sources`.
    pub restrict_fetch: bool,
    /// Portage's `RESTRICT=primaryuri`: the file's literal URIs (plus the
    /// `mirror://` expansions) move to the front of its candidate list.
    /// Sourced from the ebuild's `RESTRICT` field by `ebuild_phases::fetch_sources`.
    pub restrict_primaryuri: bool,
    /// Portage's `FEATURES=force-mirror`: plain `SRC_URI` URIs are never
    /// candidates; mirrors only. Read from the process env by the caller.
    pub force_mirror: bool,
    /// The resolved `USE` (`PORTAGE_USE`) that `SRC_URI` `flag?` groups reduce
    /// against; empty when none was threaded through.
    pub use_flags: std::collections::HashSet<String>,
    /// Clock time for the `.mirror-cache.json` day-long freshness check;
    /// `None` reads the system clock. Tests pin it.
    pub mirror_cache_now: Option<f64>,
    /// Portage's `PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` (default 5): stop trying
    /// locations after this many digest failures.
    pub checksum_failure_max_tries: usize,
    /// The resolved `FETCHCOMMAND`/`RESUMECOMMAND` family; `None` keeps the
    /// `make.globals` defaults. Built via [`fetch_commands_from_config`].
    pub fetch_commands: Option<portage_fetch::FetchCommands>,
    /// Portage's `PORTAGE_RO_DISTDIRS` (existing dirs only): a verified file is
    /// symlinked into a writable DISTDIR, never copied.
    pub ro_distdirs: Vec<std::path::PathBuf>,
    /// Portage's `PORTAGE_SSH_OPTS` for the `FETCHCOMMAND_SSH`/`_SFTP`
    /// templates; `None` (unset) expands to empty. See [`portage_ssh_opts_from_config`].
    pub portage_ssh_opts: Option<String>,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            distdir: PathBuf::from("/var/cache/distfiles"),
            gentoo_mirrors: vec!["http://distfiles.gentoo.org".to_string()],
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
            distlocks: true,
            restrict_mirror: false,
            restrict_fetch: false,
            restrict_primaryuri: false,
            force_mirror: false,
            use_flags: std::collections::HashSet::new(),
            mirror_cache_now: None,
            checksum_failure_max_tries: 5,
            fetch_commands: None,
            ro_distdirs: Vec::new(),
            portage_ssh_opts: None,
        }
    }
}

/// The default-`FETCHCOMMAND` wget (the shared `portage_fetch` transport),
/// invoked as a genuine subprocess, not an in-process HTTP client. This names
/// the non-resume `FETCHCOMMAND` for index/`layout.conf` fetches, while
/// `fetch_src_uri` dispatches fresh-vs-resume through the `Fetcher` seam.
// Portage: cnf/make.globals, FETCHCOMMAND
pub(crate) fn wget_fetch(uri: &str, dest: &Path) -> Result<(), String> {
    portage_fetch::download_via_wget(uri, dest, false)
}

/// Output-captured sibling of [`wget_fetch`] for the binhost index refresh,
/// whose failure message is shaped from the transcript, never inherited.
pub(crate) fn wget_fetch_quiet(
    uri: &str,
    dest: &Path,
) -> Result<(), portage_fetch::QuietFetchError> {
    portage_fetch::download_via_wget_quiet(uri, dest)
}

/// The resolved `PORTAGE_SSH_OPTS` scalar, else `None` (absent expands to
/// empty, like Portage's missing settings key).
pub fn portage_ssh_opts_from_config(config: &portage_profile::Config) -> Option<String> {
    config.other_vars.get("PORTAGE_SSH_OPTS").cloned()
}

/// The `FETCHCOMMAND` family out of a resolved config: plain scalars plus
/// every `FETCHCOMMAND_<PROTO>`/`RESUMECOMMAND_<PROTO>` override. An absent
/// entry resolves to the shipped `make.globals` default template, never `None`.
pub fn fetch_commands_from_config(
    config: &portage_profile::Config,
) -> portage_fetch::FetchCommands {
    let mut commands = portage_fetch::FetchCommands::default();
    for (key, value) in &config.other_vars {
        match key.as_str() {
            "FETCHCOMMAND" => commands.fetchcommand = Some(value.clone()),
            "RESUMECOMMAND" => commands.resumecommand = Some(value.clone()),
            _ => {
                if let Some(proto) = key.strip_prefix("FETCHCOMMAND_") {
                    commands
                        .fetchcommand_proto
                        .insert(proto.to_string(), value.clone());
                } else if let Some(proto) = key.strip_prefix("RESUMECOMMAND_") {
                    commands
                        .resumecommand_proto
                        .insert(proto.to_string(), value.clone());
                }
            }
        }
    }
    commands
}

/// Per-file candidate order for one distfile's `group` of `SRC_URI` entries:
/// `custommirrors["local"]` roots, public `GENTOO_MIRRORS` roots (unless
/// mirror-restricted), inline `mirror://` expansions, then the primary-uri
/// group (literals in reverse `SRC_URI` order plus third-party expansions
/// again) -- appended last normally, prepended first under
/// `RESTRICT=primaryuri`. Steps 1-2 are mirror roots resolved lazily, so a
/// file fetched earlier never downloads a later mirror's `layout.conf`.
// Portage: lib/portage/package/ebuild/fetch.py, fetch
fn assemble_candidates(
    group: &[&SrcUriEntry],
    custommirrors: &std::collections::HashMap<String, Vec<String>>,
    thirdpartymirrors: &std::collections::HashMap<String, Vec<String>>,
    options: &FetchOptions,
) -> Vec<Candidate> {
    let Some(first) = group.first() else {
        return Vec::new();
    };
    // `local_mirrors` keep their spelling, `public_mirrors` are stripped; both
    // spellings are `.mirror-cache.json` keys, so it matters.
    let mut filedict: Vec<Candidate> = custommirrors
        .get("local")
        .into_iter()
        .flatten()
        .filter(|r| !r.starts_with('/'))
        .map(|r| Candidate::Mirror(r.clone()))
        .collect();
    // Mirror restriction is decided once, by the FIRST URI's `mirror+` prefix.
    let public_barred =
        (options.restrict_mirror || options.restrict_fetch) && !first.override_mirror;
    if !public_barred {
        filedict.extend(
            options
                .gentoo_mirrors
                .iter()
                // `/`-rooted entries are on-filesystem mirrors, never download candidates.
                .filter(|root| !root.starts_with('/'))
                .map(|root| Candidate::Mirror(root.trim_end_matches('/').to_string())),
        );
    }
    for entry in group {
        if entry.uri.starts_with("mirror://") {
            filedict.extend(
                resolve_mirror_candidates(&entry.uri, custommirrors, thirdpartymirrors)
                    .into_iter()
                    .map(Candidate::Uri),
            );
        }
    }
    let primary: Vec<Candidate> = primary_uris(group, thirdpartymirrors, options)
        .into_iter()
        .map(Candidate::Uri)
        .collect();
    if options.restrict_primaryuri {
        [primary, filedict].concat()
    } else {
        [filedict, primary].concat()
    }
}

/// The file's fetchable literals in reverse `SRC_URI` order (tried
/// last-listed first), then every third-party `mirror://` expansion.
// Portage: lib/portage/package/ebuild/fetch.py, fetch
fn primary_uris(
    group: &[&SrcUriEntry],
    thirdpartymirrors: &std::collections::HashMap<String, Vec<String>>,
    options: &FetchOptions,
) -> Vec<String> {
    let mut literals: Vec<String> = Vec::new();
    let mut thirdparty: Vec<String> = Vec::new();
    for entry in group {
        if entry.uri.starts_with("mirror://") {
            thirdparty.extend(resolve_mirror_candidates(
                &entry.uri,
                &std::collections::HashMap::new(),
                thirdpartymirrors,
            ));
        } else if !((options.restrict_fetch && !entry.override_fetch) || options.force_mirror) {
            literals.push(entry.uri.clone());
        }
    }
    literals.reverse();
    literals.extend(thirdparty);
    literals
}

/// Parse `PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` (default 5); a non-integer or
/// sub-1 value warns and uses the default.
pub fn checksum_failure_max_tries(value: Option<&str>) -> (usize, Vec<String>) {
    const DEFAULT: i64 = 5;
    let Some(value) = value else {
        return (DEFAULT as usize, Vec::new());
    };
    let fallback = format!("!!! Using PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS default value: {DEFAULT}");
    match value.trim().parse::<i64>() {
        Err(_) => (
            DEFAULT as usize,
            vec![
                format!(
                    "!!! Variable PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS contains non-integer value: '{value}'"
                ),
                fallback,
            ],
        ),
        Ok(v) if v < 1 => (
            DEFAULT as usize,
            vec![
                format!(
                    "!!! Variable PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS contains value less than 1: '{v}'"
                ),
                fallback,
            ],
        ),
        Ok(v) => (v as usize, Vec::new()),
    }
}

/// `SRC_URI` entries grouped by distfile in first-seen order, identical URIs
/// listed once.
// Portage: lib/portage/package/ebuild/porttree.py, _parse_uri_map
fn group_by_filename(entries: &[SrcUriEntry]) -> Vec<(&str, Vec<&SrcUriEntry>)> {
    let mut groups: Vec<(&str, Vec<&SrcUriEntry>)> = Vec::new();
    for entry in entries {
        match groups.iter_mut().find(|(name, _)| *name == entry.filename) {
            Some((_, group)) => {
                if !group.iter().any(|e| e == &entry) {
                    group.push(entry);
                }
            }
            None => groups.push((&entry.filename, vec![entry])),
        }
    }
    groups
}

/// One entry of a file's candidate list: a ready URI, or a mirror root
/// whose URL depends on the mirror's `layout.conf` (`mirror_url`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Candidate {
    Uri(String),
    Mirror(String),
}

/// Arguments of [`mirror_url`].
struct MirrorUrlArgs<'a> {
    mirror: &'a str,
    filename: &'a str,
    digests: &'a std::collections::HashMap<String, String>,
    distdir: &'a Path,
    cache_path: Option<&'a Path>,
    now: f64,
}

/// The URL (or local path, for a `/`-rooted mirror) of `filename` under
/// `mirror`, laid out per the mirror's own `layout.conf`. Layouts cache in
/// `.mirror-cache.json` for a day; only successful reads are cached, and a
/// failed/unparseable read silently means flat. `now` is injectable for tests.
// Portage: lib/portage/package/ebuild/fetch.py, async_mirror_url
fn mirror_url(args: MirrorUrlArgs<'_>) -> String {
    let MirrorUrlArgs {
        mirror,
        filename,
        digests,
        distdir,
        cache_path,
        now,
    } = args;
    use portage_fetch::layout::MirrorLayoutConfig;
    use portage_fetch::mirror_cache::{
        CacheEntry, MIRROR_CACHE_TTL_SECS, mirror_file_url, parse_mirror_cache,
        serialize_mirror_cache, upsert_mirror_cache, url_hostname,
    };

    let mut cache = cache_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| parse_mirror_cache(&text))
        .unwrap_or_default();
    let fresh = cache
        .iter()
        .find(|e| e.mirror_url == mirror)
        .filter(|e| e.timestamp >= now - MIRROR_CACHE_TTL_SECS)
        .map(|e| MirrorLayoutConfig {
            structure: e.structure.clone(),
        });
    let config = fresh.unwrap_or_else(|| {
        let read = if mirror.starts_with('/') {
            let text = std::fs::read(Path::new(mirror).join("layout.conf")).unwrap_or_default();
            MirrorLayoutConfig::parse(&String::from_utf8_lossy(&text)).ok()
        } else {
            let tmpfile = distdir.join(format!(".layout.conf.{}", url_hostname(mirror)));
            wget_fetch(&format!("{mirror}/distfiles/layout.conf"), &tmpfile)
                .ok()
                .and_then(|()| std::fs::read(&tmpfile).ok())
                .and_then(|text| MirrorLayoutConfig::parse(&String::from_utf8_lossy(&text)).ok())
        };
        let Some(config) = read else {
            return MirrorLayoutConfig::default();
        };
        if let Some(cache_path) = cache_path {
            upsert_mirror_cache(
                &mut cache,
                CacheEntry {
                    mirror_url: mirror.to_string(),
                    timestamp: now,
                    structure: config.structure.clone(),
                },
            );
            // Write a sibling, then rename over (Portage's `atomic_ofstream`).
            let tmp = cache_path.with_extension(format!("json.{}", std::process::id()));
            if std::fs::write(&tmp, serialize_mirror_cache(&cache)).is_ok() {
                let _ = std::fs::rename(&tmp, cache_path);
            } else {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        config
    });
    let layout = config.best_supported(Some(digests));
    let path = layout
        .get_path(filename, digests)
        .unwrap_or_else(|| filename.to_string());
    mirror_file_url(mirror, &path)
}

/// The `/`-rooted `custommirrors["local"]` entries (as written), then the
/// `/`-rooted `GENTOO_MIRRORS` entries (stripped).
fn fsmirrors(
    custommirrors: &std::collections::HashMap<String, Vec<String>>,
    gentoo_mirrors: &[String],
) -> Vec<String> {
    let local = custommirrors
        .get("local")
        .into_iter()
        .flatten()
        .filter(|root| root.starts_with('/'))
        .cloned();
    let public = gentoo_mirrors
        .iter()
        .filter(|root| root.starts_with('/'))
        .map(|root| root.trim_end_matches('/').to_string());
    local.chain(public).collect()
}

/// Copy the first on-filesystem mirror that has the file to `dest`, printing
/// `Local mirror has file: <file>`. A missing file moves on; any other copy
/// error is fatal. Returns whether a copy was made.
// Portage: lib/portage/package/ebuild/fetch.py, fetch
fn copy_from_fsmirrors(
    fsmirrors: &[String],
    filename: &str,
    digests: &std::collections::HashMap<String, String>,
    options: &FetchOptions,
    dest: &Path,
) -> Result<bool, String> {
    for dir in fsmirrors {
        let source = mirror_url(MirrorUrlArgs {
            mirror: dir,
            filename,
            digests,
            distdir: &options.distdir,
            cache_path: None,
            now: 0.0,
        });
        match std::fs::copy(&source, dest) {
            Ok(_) => {
                eprintln!("Local mirror has file: {filename}");
                return Ok(true);
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(libc::ESTALE) => {}
            Err(e) => return Err(format!("{source}: {e}")),
        }
    }
    Ok(false)
}

/// Whether `DISTDIR` is writable, which decides if `.mirror-cache.json` is
/// used at all.
fn distdir_writable(distdir: &Path) -> bool {
    let Ok(c) = std::ffi::CString::new(distdir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path that outlives the call; the
    // second argument is a plain access-mode flag.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Whether the filesystem holding `distdir` fits `bytes`, via `statvfs`.
fn has_enough_space(distdir: &Path, bytes: u64) -> bool {
    let Ok(c) = std::ffi::CString::new(distdir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path; `stat` is a valid mutable buffer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return false;
    }
    let available_bytes = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
    available_bytes >= bytes
}

/// Fetch every file `src_uri` names for the current USE set into
/// `options.distdir`, unless a Manifest-verified copy is already there.
/// `flag?` groups reduce against `options.use_flags`. Returns the filename
/// list the caller exports as `A`. A file with no `Manifest` entry is refused
/// outright: unverifiable content is worse than a loud failure.
pub fn fetch_src_uri(
    pkg_dir: &Path,
    src_uri: &str,
    options: &FetchOptions,
) -> Result<Vec<String>, String> {
    let manifest = parse_manifest(&pkg_dir.join("Manifest"))?;
    let entries = flatten_src_uri(src_uri, |negated, flag| {
        options.use_flags.contains(flag) != negated
    })
    .map_err(|e| format!("{}: {e}", pkg_dir.display()))?;

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    std::fs::create_dir_all(&options.distdir)
        .map_err(|e| format!("{}: {e}", options.distdir.display()))?;

    // `mirror://` resolution: the ebuild repo's `thirdpartymirrors` (empty
    // outside any repo checkout), `${config_root}/etc/portage/mirrors` as
    // `custommirrors` (same file format), plus the `GENTOO_MIRRORS` fallback.
    let thirdpartymirrors = crate::ebuild_phases::repo_root_for(pkg_dir)
        .map(|repo_root| parse_thirdpartymirrors(&repo_root.join("profiles/thirdpartymirrors")))
        .transpose()?
        .unwrap_or_default();
    // The default `config_root` is an impossible sentinel path, so ANY failure
    // here (not just `NotFound`) degrades to an empty `custommirrors` map.
    let custommirrors = parse_thirdpartymirrors(&options.config_root.join("etc/portage/mirrors"))
        .unwrap_or_default();

    let mut filenames = Vec::new();
    for (_, group) in group_by_filename(&entries) {
        // The first entry names the file (and any error message).
        let entry = group[0];
        let dest = options.distdir.join(&entry.filename);
        // `FEATURES=distlocks`: held for the whole per-file sequence, released
        // when `_lock` drops. Acquired after the no-Manifest-entry refusal below,
        // which needs no lock (nothing to fetch or protect when refusing outright).
        let digests = manifest.get(&entry.filename);
        let Some(digests) = digests else {
            return Err(format!(
                "{}: no Manifest entry, cannot verify -- refusing to fetch \
                 unverifiable content",
                entry.filename
            ));
        };
        let _lock = if options.distlocks {
            Some(PortageLockfile::acquire(&dest)?)
        } else {
            None
        };

        let mut already_verified = dest.is_file() && verify_digests(&dest, digests).is_ok();

        // `PORTAGE_RO_DISTDIRS`: a verified read-only copy is symlinked into a
        // writable DISTDIR -- before the fsmirrors copy and every download.
        if !already_verified
            && distdir_writable(&options.distdir)
            && !options.ro_distdirs.is_empty()
        {
            for dir in &options.ro_distdirs {
                if !dir.is_dir() {
                    continue;
                }
                let source = mirror_url(MirrorUrlArgs {
                    mirror: &dir.to_string_lossy(),
                    filename: &entry.filename,
                    digests: &digests.hashes,
                    distdir: &options.distdir,
                    cache_path: None,
                    now: 0.0,
                });
                let source = std::path::PathBuf::from(source);
                if verify_digests(&source, digests).is_ok() {
                    let _ = std::fs::remove_file(&dest);
                    if std::os::unix::fs::symlink(&source, &dest).is_ok() {
                        already_verified = true;
                        break;
                    }
                }
            }
        }

        // Before any download, a missing file is copied from the first
        // on-filesystem mirror that has it, given enough free space.
        if !already_verified && !dest.exists() && has_enough_space(&options.distdir, digests.size) {
            let fsmirrors = fsmirrors(&custommirrors, &options.gentoo_mirrors);
            if copy_from_fsmirrors(&fsmirrors, &entry.filename, &digests.hashes, options, &dest)? {
                match verify_digests(&dest, digests) {
                    Ok(()) => already_verified = true,
                    // A short copy is kept to resume from; a full-size corrupt
                    // one is replaced by the download.
                    Err(_) if std::fs::metadata(&dest).is_ok_and(|m| m.len() < digests.size) => {}
                    Err(_) => {
                        let _ = std::fs::remove_file(&dest);
                    }
                }
            }
        }

        if !already_verified {
            let candidates =
                assemble_candidates(&group, &custommirrors, &thirdpartymirrors, options);
            // A plain URI barred by `RESTRICT=fetch`/`force-mirror` never enters
            // the list; a `fetch+`/`mirror+` prefix re-permits it (except under
            // `force-mirror`). `mirror://` candidates come only from expansions.
            let plain_uri_barred_by_restrict_fetch =
                (options.restrict_fetch && !entry.override_fetch || options.force_mirror)
                    && !entry.uri.starts_with("mirror://");
            // The public mirror list is appended unless mirroring is restricted;
            // `mirror+` re-permits it for this file. `RESTRICT=fetch` implies it too.
            let public_mirrors_barred =
                (options.restrict_mirror || options.restrict_fetch) && !entry.override_mirror;
            if candidates.is_empty() {
                // A `RESTRICT=fetch` file missing from DISTDIR fails here; the
                // caller then runs the ebuild's own `pkg_nofetch` phase.
                let why = if plain_uri_barred_by_restrict_fetch {
                    format!(
                        "RESTRICT=fetch bars downloading it -- place a verified copy in {} \
                         by hand (see the pkg_nofetch output above)",
                        options.distdir.display()
                    )
                } else if public_mirrors_barred {
                    "unknown mirror name, and RESTRICT=mirror bars the GENTOO_MIRRORS fallback"
                        .to_string()
                } else {
                    "unknown mirror name, and GENTOO_MIRRORS is empty".to_string()
                };
                return Err(format!(
                    "{}: no working candidate mirror for {:?} ({why})",
                    entry.filename, entry.uri
                ));
            }

            let mut errors = Vec::new();
            let mut fetched = false;
            // The assembled list may repeat a location, but each is attempted
            // once per file.
            let mut tried = std::collections::HashSet::new();
            let cache_path = distdir_writable(&options.distdir)
                .then(|| options.distdir.join(".mirror-cache.json"));
            let now = options.mirror_cache_now.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_secs_f64())
            });
            // A stack popped from the end, so the primary-uri switch below jumps
            // ahead of what is left.
            let mut uri_list: Vec<Candidate> = candidates.iter().rev().cloned().collect();
            let mut checksum_failures = 0;
            // One resolved candidate URI -> `dest`, fresh or resumed, with no
            // Manifest context; digest verification stays here, which holds it.
            let digests_var = portage_fetch::digests_variable(digests);
            let fetcher: &dyn Fetcher = &WgetFetcher;
            while let Some(candidate) = uri_list.pop() {
                let candidate = match &candidate {
                    Candidate::Uri(uri) => uri.clone(),
                    Candidate::Mirror(root) => mirror_url(MirrorUrlArgs {
                        mirror: root,
                        filename: &entry.filename,
                        digests: &digests.hashes,
                        distdir: &options.distdir,
                        cache_path: cache_path.as_deref(),
                        now,
                    }),
                };
                if !tried.insert(candidate.clone()) {
                    continue;
                }
                let candidate = candidate.as_str();
                // A non-empty partial on disk switches from `FETCHCOMMAND` to
                // `RESUMECOMMAND` (`wget -c` continues it).
                let has_partial = std::fs::metadata(&dest)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false);
                let attempt = fetcher.fetch(&FetchRequest {
                    filename: entry.filename.as_str(),
                    uri: candidate,
                    dest: &dest,
                    resume: has_partial,
                    commands: options.fetch_commands.as_ref(),
                    vars: portage_fetch::FetchCommandVars {
                        digests: Some(&digests_var),
                        portage_ssh_opts: options.portage_ssh_opts.as_deref(),
                    },
                });
                match attempt {
                    Ok(()) => match verify_digests(&dest, digests) {
                        Ok(()) => {
                            fetched = true;
                            break;
                        }
                        Err(e) => {
                            errors.push(format!("{candidate}: digest verification failed: {e}"));
                            checksum_failures += 1;
                            // Rename the corrupt file aside with a deterministic
                            // counter suffix, preserving it as evidence.
                            let bad_file = format!(
                                "{}._checksum_failure_{}",
                                dest.display(),
                                checksum_failures
                            );
                            let _ = std::fs::rename(&dest, &bad_file);
                            eprintln!("Refetching... File renamed to '{bad_file}'");
                            // The second failure jumps to the primary URIs; the cap
                            // stops trying further locations at all.
                            if checksum_failures == 2 {
                                uri_list.extend(
                                    primary_uris(&group, &thirdpartymirrors, options)
                                        .into_iter()
                                        .rev()
                                        .map(Candidate::Uri),
                                );
                            }
                            if checksum_failures >= options.checksum_failure_max_tries {
                                break;
                            }
                        }
                    },
                    // A transport failure may have left a resumable partial for
                    // the next candidate.
                    Err(e) => errors.push(e),
                }
            }
            if !fetched {
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "{}: every candidate failed:\n{}",
                    entry.filename,
                    errors.join("\n")
                ));
            }
        }
        filenames.push(entry.filename.clone());
    }
    Ok(filenames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::fs;

    fn tempdir() -> std::path::PathBuf {
        let dir = TempDir::new("portuale_fetch_test").keep();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Fixed, known-good distfile payload ("hello world"; digests confirmed via
    // the system `b2sum`/`sha512sum` tools).
    const HELLO_BLAKE2B: &str = "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbcc05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0";
    const HELLO_SHA512: &str = "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f";

    fn write_manifest(pkg_dir: &Path, filename: &str, size: u64) {
        fs::write(
            pkg_dir.join("Manifest"),
            format!("DIST {filename} {size} BLAKE2B {HELLO_BLAKE2B} SHA512 {HELLO_SHA512}\n"),
        )
        .unwrap();
    }

    /// Serves `body` over loopback HTTP for exactly one connection, so tests
    /// exercise the genuine `wget` subprocess end to end without internet.
    fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}/file"), handle)
    }

    /// A mirror-shaped HTTP server: serves `routes` by request path (404
    /// otherwise), recording every requested path. A caller expecting fewer
    /// requests than `connections` stops the thread with `unblock_server` (an empty
    /// connection ends it).
    #[allow(clippy::type_complexity)]
    fn serve_mirror(
        routes: Vec<(&'static str, Vec<u8>)>,
        connections: usize,
    ) -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        std::thread::JoinHandle<()>,
    ) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requested = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = requested.clone();
        let handle = std::thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let Some(path) = request.split_whitespace().nth(1) else {
                    // `unblock_server`: stop serving.
                    return;
                };
                log.lock().unwrap().push(path.to_string());
                let response = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(body);
                        r
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), requested, handle)
    }

    /// Connects (sending nothing) so a parked `serve_mirror` thread can exit.
    fn unblock_server(root: &str) {
        let addr = root.trim_start_matches("http://");
        let _ = std::net::TcpStream::connect(addr);
    }

    #[test]
    fn distfile_lock_creates_a_real_sibling_lockfile() {
        let dir = tempdir();
        let dest = dir.join("foo-1.0.tar.gz");

        let _lock = PortageLockfile::acquire(&dest).expect("acquire succeeds");

        assert!(
            dir.join(".foo-1.0.tar.gz.portage_lockfile").is_file(),
            "real lockfile naming: '.' + basename + '.portage_lockfile'"
        );
    }

    #[test]
    fn distfile_lock_release_on_drop_lets_a_second_acquire_succeed_immediately() {
        let dir = tempdir();
        let dest = dir.join("foo-1.0.tar.gz");

        let lock1 = PortageLockfile::acquire(&dest).expect("first acquire succeeds");
        drop(lock1);

        // The lockfile persists after release, just unlocked: a second acquire
        // must succeed immediately, not block on itself.
        PortageLockfile::acquire(&dest).expect("second acquire succeeds once the first is dropped");
    }

    /// A second acquire from another thread genuinely blocks until the first
    /// lock is dropped.
    #[test]
    fn distfile_lock_blocks_a_second_acquire_until_released() {
        let dir = tempdir();
        let dest = dir.join("foo-1.0.tar.gz");

        let lock1 = PortageLockfile::acquire(&dest).expect("first acquire succeeds");

        let dest2 = dest.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _lock2 =
                PortageLockfile::acquire(&dest2).expect("second acquire succeeds once unblocked");
            tx.send(()).unwrap();
        });

        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "a second acquire must block while the first lock is still held"
        );

        drop(lock1);

        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("second acquire completes promptly once the first lock is released");
        handle.join().unwrap();
    }

    #[test]
    fn fetch_src_uri_is_empty_for_an_empty_src_uri() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        let result = fetch_src_uri(
            &pkg_dir,
            "",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert!(result.is_empty());
        assert!(
            !distdir.join("Manifest").exists(),
            "must not touch DISTDIR at all when there's nothing to fetch"
        );
    }

    #[test]
    fn fetch_src_uri_skips_a_real_already_verified_file_without_touching_the_network() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
        fs::write(distdir.join("hello-1.0.tar.gz"), b"hello world").unwrap();

        // Deliberately unreachable address: reaching the network at all would
        // hang/fail instead of returning immediately.
        let result = fetch_src_uri(
            &pkg_dir,
            "https://192.0.2.1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result, vec!["hello-1.0.tar.gz".to_string()]);
    }

    /// Expected values from Portage (`app-shells/bash-completion[eselect]`):
    /// an enabled conditional distfile is part of `A`; a `!flag?` one is not.
    #[test]
    fn fetch_src_uri_reduces_use_conditional_groups_against_the_resolved_use() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
        fs::write(distdir.join("hello-1.0.tar.gz"), b"hello world").unwrap();
        let src_uri = "eselect? ( https://192.0.2.1/hello-1.0.tar.gz ) \
                       !eselect? ( https://192.0.2.1/unlisted-1.0.tar.gz )";
        let with_use = |flags: &[&str]| FetchOptions {
            distdir: distdir.clone(),
            gentoo_mirrors: vec![],
            use_flags: flags.iter().map(|f| f.to_string()).collect(),
            ..FetchOptions::default()
        };
        assert_eq!(
            fetch_src_uri(&pkg_dir, src_uri, &with_use(&["eselect"])).unwrap(),
            vec!["hello-1.0.tar.gz".to_string()]
        );
        // Flag off: the negated group is selected (and refused -- no
        // Manifest entry), the enabled-only file is not.
        let err = fetch_src_uri(&pkg_dir, src_uri, &with_use(&[])).unwrap_err();
        assert!(err.contains("unlisted-1.0.tar.gz"), "{err}");
    }

    #[test]
    fn fetch_src_uri_runs_a_configured_fetchcommand() {
        // A configured FETCHCOMMAND is spawned directly, with substitutions.
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let uri = format!("{uri_base} -> hello-1.0.tar.gz");
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
        let marker = distdir.join("marker");
        let script = distdir.join("fetch.sh");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\ntouch {}\nwget -q -O \"$1\" \"$2\"\n",
                marker.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();

        let fetch_commands = portage_fetch::FetchCommands {
            fetchcommand: Some(format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            )),
            resumecommand: Some(format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            )),
            ..portage_fetch::FetchCommands::default()
        };
        let result = fetch_src_uri(
            &pkg_dir,
            &uri,
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                fetch_commands: Some(fetch_commands),
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result, vec!["hello-1.0.tar.gz".to_string()]);
        assert!(marker.is_file(), "the configured FETCHCOMMAND ran");
        assert_eq!(
            fs::read(distdir.join("hello-1.0.tar.gz")).unwrap(),
            b"hello world"
        );
        handle.join().unwrap();
    }

    #[test]
    fn fetch_commands_from_config_defaults_an_absent_fetchcommand() {
        // Without FETCHCOMMAND/RESUMECOMMAND the shipped defaults resolve,
        // never `None`.
        let commands = fetch_commands_from_config(&portage_profile::Config::default());
        assert_eq!(commands, portage_fetch::FetchCommands::default());
    }

    #[test]
    fn portage_ssh_opts_from_config_reads_the_scalar() {
        // Absent `PORTAGE_SSH_OPTS` stays None (expands to empty, like Portage).
        let mut config = portage_profile::Config::default();
        config.other_vars.insert(
            "PORTAGE_SSH_OPTS".to_string(),
            "-o User=portage".to_string(),
        );
        assert_eq!(
            portage_ssh_opts_from_config(&config).as_deref(),
            Some("-o User=portage")
        );
        assert_eq!(
            portage_ssh_opts_from_config(&portage_profile::Config::default()),
            None
        );
    }

    #[test]
    fn fetch_src_uri_uses_the_make_globals_default_without_a_config_value() {
        // A config root without make.globals still downloads via the default
        // template.
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let uri = format!("{uri_base} -> hello-1.0.tar.gz");
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let config_root = tempdir();
        let eroot = tempdir();
        let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
            config_root: &config_root,
            main_repo_location: &config_root,
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "gentoo",
            repo_masters: &std::collections::HashMap::new(),
            eroot: &eroot,
        })
        .unwrap();
        assert!(
            !config_root
                .join("usr/share/portage/config/make.globals")
                .exists(),
            "fixture root must have no make.globals"
        );
        let result = fetch_src_uri(
            &pkg_dir,
            &uri,
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                fetch_commands: Some(fetch_commands_from_config(&config)),
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result, vec!["hello-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read(distdir.join("hello-1.0.tar.gz")).unwrap(),
            b"hello world"
        );
        handle.join().unwrap();
    }

    #[test]
    fn fetch_src_uri_symlinks_a_verified_portage_ro_distdirs_file() {
        // A verified read-only copy is symlinked, never downloaded.
        let pkg_dir = tempdir();
        let distdir = tempdir();
        let ro = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
        fs::write(ro.join("hello-1.0.tar.gz"), b"hello world").unwrap();
        let result = fetch_src_uri(
            &pkg_dir,
            "https://192.0.2.1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ro_distdirs: vec![ro.clone()],
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result, vec!["hello-1.0.tar.gz".to_string()]);
        let dest = distdir.join("hello-1.0.tar.gz");
        assert!(
            fs::symlink_metadata(&dest)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the RO distdir copy is symlinked, not copied"
        );
        assert_eq!(fs::read(&dest).unwrap(), b"hello world");
        // A non-existent RO entry is skipped, not an error.
        let err = fetch_src_uri(
            &pkg_dir,
            "https://192.0.2.1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: tempdir(),
                gentoo_mirrors: vec![],
                ro_distdirs: vec![distdir.join("does-not-exist")],
                ..FetchOptions::default()
            },
        );
        assert!(err.is_err(), "no other candidate can serve the file");
    }

    #[test]
    fn fetch_src_uri_refuses_a_file_with_no_manifest_entry() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        // No Manifest written at all.
        let err = fetch_src_uri(
            &pkg_dir,
            "https://192.0.2.1/nowhere-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("no Manifest entry"), "{err}");
        assert!(!distdir.join("nowhere-1.0.tar.gz").exists());
    }

    #[test]
    fn fetch_src_uri_really_downloads_via_a_real_wget_subprocess_and_verifies_it() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let uri = format!("{uri_base} -> hello-1.0.tar.gz");

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let result = fetch_src_uri(
            &pkg_dir,
            &uri,
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result, vec!["hello-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read(distdir.join("hello-1.0.tar.gz")).unwrap(),
            b"hello world"
        );
        handle.join().unwrap();
    }

    /// Serves `body` in two halves: the first connection drops mid-transfer;
    /// the second answers `wget -c`'s `Range` request with the rest.
    fn serve_dropped_then_resumed(
        body: Vec<u8>,
        split: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let total = body.len();
        let handle = std::thread::spawn(move || {
            // Connection 1: header promises `total`, but only `split`
            // bytes are written before the connection drops.
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\n\
                     Connection: close\r\n\r\n"
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body[..split]);
                let _ = stream.flush();
            }
            // Connection 2: `wget -c` asks for `bytes=split-`.
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                assert!(
                    req.contains(&format!("Range: bytes={split}-")),
                    "wget -c must send a Range header, got:\n{req}"
                );
                let header = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\
                     Content-Range: bytes {split}-{}/{total}\r\nConnection: close\r\n\r\n",
                    total - split,
                    total - 1
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body[split..]);
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}/file"), handle)
    }

    #[test]
    fn fetch_src_uri_resumes_a_dropped_download_with_wget_c() {
        let (uri_base, handle) = serve_dropped_then_resumed(b"hello world".to_vec(), 5);
        let uri = format!("{uri_base} -> hello-1.0.tar.gz");

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let result = fetch_src_uri(
            &pkg_dir,
            &uri,
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .expect("the dropped download must be resumed and verified");
        assert_eq!(result, vec!["hello-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read(distdir.join("hello-1.0.tar.gz")).unwrap(),
            b"hello world"
        );
        handle.join().unwrap();
    }

    #[test]
    fn fetch_src_uri_rejects_a_real_download_that_fails_digest_verification() {
        // Serves content that mismatches the Manifest's digests/size.
        let (uri_base, handle) = serve_once(b"this is not hello world at all".to_vec());
        let uri = format!("{uri_base} -> wrong-1.0.tar.gz");

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "wrong-1.0.tar.gz", 11);

        let err = fetch_src_uri(
            &pkg_dir,
            &uri,
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("digest verification failed"), "{err}");
        assert!(
            !distdir.join("wrong-1.0.tar.gz").exists(),
            "a failed-verification download must not be left behind"
        );
        handle.join().unwrap();
    }

    /// End-to-end `mirror://` resolution through a fixture repo checkout's own
    /// `thirdpartymirrors`, fetched and digest-verified.
    #[test]
    fn fetch_src_uri_resolves_a_real_mirror_uri_via_thirdpartymirrors() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        // The stub handler ignores the request path, so appending a
        // `mirror://`-style extra segment is safe.
        let mirror_root = uri_base.trim_end_matches("/file");

        let repo_root = tempdir();
        fs::write(repo_root.join("profiles/repo_name"), "mirrortest\n").unwrap_or_else(|_| {
            fs::create_dir_all(repo_root.join("profiles")).unwrap();
            fs::write(repo_root.join("profiles/repo_name"), "mirrortest\n").unwrap();
        });
        fs::write(
            repo_root.join("profiles/thirdpartymirrors"),
            format!("testmirror {mirror_root}\n"),
        )
        .unwrap();
        let pkg_dir = repo_root.join("dev-libs/mirrorpkg");
        fs::create_dir_all(&pkg_dir).unwrap();
        write_manifest(&pkg_dir, "foo-1.0.tar.gz", 11);

        let distdir = tempdir();
        let filenames = fetch_src_uri(
            &pkg_dir,
            "mirror://testmirror/foo-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["foo-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read_to_string(distdir.join("foo-1.0.tar.gz")).unwrap(),
            "hello world"
        );
        handle.join().unwrap();
    }

    /// End-to-end `custommirrors` proof: resolves with no `thirdpartymirrors`
    /// entry for the name, proving independent consultation.
    #[test]
    fn fetch_src_uri_resolves_a_real_mirror_uri_via_custommirrors() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let mirror_root = uri_base.trim_end_matches("/file");

        let config_root = tempdir();
        fs::create_dir_all(config_root.join("etc/portage")).unwrap();
        fs::write(
            config_root.join("etc/portage/mirrors"),
            format!("testmirror {mirror_root}\n"),
        )
        .unwrap();

        let pkg_dir = tempdir();
        write_manifest(&pkg_dir, "foo-1.0.tar.gz", 11);

        let distdir = tempdir();
        let filenames = fetch_src_uri(
            &pkg_dir,
            "mirror://testmirror/foo-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                config_root: config_root.clone(),
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["foo-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read_to_string(distdir.join("foo-1.0.tar.gz")).unwrap(),
            "hello world"
        );
        handle.join().unwrap();
    }

    /// The default `config_root` sentinel fails with `ENOTDIR`, not `NotFound`:
    /// must still degrade to "no custommirrors" with a clean error.
    #[test]
    fn fetch_src_uri_degrades_gracefully_when_config_root_is_the_default_sentinel() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "foo-1.0.tar.gz", 11);

        let err = fetch_src_uri(
            &pkg_dir,
            "mirror://unknown-name/foo-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("no working candidate mirror"), "{err}");
    }

    /// End-to-end `GENTOO_MIRRORS` fallback: the literal URI is unreachable, so
    /// only the mirror can serve the file (flat path, nothing cached).
    #[test]
    fn fetch_src_uri_falls_back_to_gentoo_mirrors_when_the_primary_uri_is_unreachable() {
        let (mirror_root, requested, handle) = serve_mirror(
            vec![("/distfiles/hello-1.0.tar.gz", b"hello world".to_vec())],
            2,
        );

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let filenames = fetch_src_uri(
            &pkg_dir,
            "http://127.0.0.1:1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![mirror_root],
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["hello-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read_to_string(distdir.join("hello-1.0.tar.gz")).unwrap(),
            "hello world"
        );
        handle.join().unwrap();
        assert_eq!(
            *requested.lock().unwrap(),
            vec!["/distfiles/layout.conf", "/distfiles/hello-1.0.tar.gz"]
        );
        assert!(
            !distdir.join(".mirror-cache.json").exists(),
            "an unreadable layout.conf is not cached"
        );
    }

    /// A mirror's filename-hash layout: the file is fetched from the hashed
    /// path Portage computes, the downloaded
    /// `layout.conf` is left as `.layout.conf.<host>`, and the structure
    /// is cached in Portage's `.mirror-cache.json` shape.
    #[test]
    fn fetch_src_uri_follows_a_mirrors_filename_hash_layout_and_caches_it() {
        let (mirror_root, requested, handle) = serve_mirror(
            vec![
                (
                    "/distfiles/layout.conf",
                    b"[structure]\n0=filename-hash BLAKE2B 8\n".to_vec(),
                ),
                ("/distfiles/ce/hello-1.0.tar.gz", b"hello world".to_vec()),
            ],
            2,
        );
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        fetch_src_uri(
            &pkg_dir,
            "http://127.0.0.1:1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![mirror_root.clone()],
                mirror_cache_now: Some(1800000000.5),
                ..FetchOptions::default()
            },
        )
        .unwrap();
        handle.join().unwrap();
        assert_eq!(
            *requested.lock().unwrap(),
            vec!["/distfiles/layout.conf", "/distfiles/ce/hello-1.0.tar.gz"]
        );
        assert!(distdir.join(".layout.conf.127.0.0.1").is_file());
        assert_eq!(
            fs::read_to_string(distdir.join(".mirror-cache.json")).unwrap(),
            format!(r#"{{"{mirror_root}": [1800000000.5, [["filename-hash", "BLAKE2B", "8"]]]}}"#)
        );
    }

    /// A cache entry younger than a day is used as-is; an older one is
    /// refreshed and rewritten.
    #[test]
    fn fetch_src_uri_uses_a_fresh_mirror_cache_entry_and_refreshes_a_stale_one() {
        let now = 1800000000.0;
        for (age, expect_layout_request) in [(10.0, false), (90000.0, true)] {
            let hashed = "/distfiles/ce/hello-1.0.tar.gz";
            let mut routes = vec![(hashed, b"hello world".to_vec())];
            if expect_layout_request {
                routes.push((
                    "/distfiles/layout.conf",
                    b"[structure]\n0=filename-hash BLAKE2B 8\n".to_vec(),
                ));
            }
            let connections = routes.len();
            let (mirror_root, requested, handle) = serve_mirror(routes, connections);
            let pkg_dir = tempdir();
            let distdir = tempdir();
            write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
            fs::write(
                distdir.join(".mirror-cache.json"),
                format!(
                    r#"{{"{mirror_root}": [{}, [["filename-hash", "BLAKE2B", "8"]]]}}"#,
                    now - age
                ),
            )
            .unwrap();

            fetch_src_uri(
                &pkg_dir,
                "http://127.0.0.1:1/hello-1.0.tar.gz",
                &FetchOptions {
                    distdir: distdir.clone(),
                    gentoo_mirrors: vec![mirror_root.clone()],
                    mirror_cache_now: Some(now),
                    ..FetchOptions::default()
                },
            )
            .unwrap();
            handle.join().unwrap();
            let requested = requested.lock().unwrap().clone();
            if expect_layout_request {
                assert_eq!(requested, vec!["/distfiles/layout.conf", hashed], "stale");
                assert!(
                    fs::read_to_string(distdir.join(".mirror-cache.json"))
                        .unwrap()
                        .contains("[1800000000.0, "),
                    "stale entry rewritten with the new time"
                );
            } else {
                assert_eq!(requested, vec![hashed], "fresh");
            }
        }
    }

    /// A file fetched from an earlier candidate never asks a later mirror for
    /// its `layout.conf`.
    #[test]
    fn fetch_src_uri_never_contacts_a_mirror_it_does_not_reach() {
        let (literal_root, _, literal_handle) =
            serve_mirror(vec![("/hello-1.0.tar.gz", b"hello world".to_vec())], 1);
        let (mirror_root, mirror_requests, mirror_handle) = serve_mirror(vec![], 1);
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        fetch_src_uri(
            &pkg_dir,
            &format!("{literal_root}/hello-1.0.tar.gz"),
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![mirror_root.clone()],
                restrict_primaryuri: true,
                ..FetchOptions::default()
            },
        )
        .unwrap();
        literal_handle.join().unwrap();
        unblock_server(&mirror_root);
        mirror_handle.join().unwrap();
        assert!(mirror_requests.lock().unwrap().is_empty());
        assert!(!distdir.join(".mirror-cache.json").exists());
    }

    /// `RESTRICT=mirror`: the public fallback is not tried, so an unreachable
    /// primary URI fails the fetch and the mirror is never contacted.
    #[test]
    fn fetch_src_uri_restrict_mirror_skips_the_gentoo_mirrors_fallback() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let mirror_addr = uri_base
            .trim_start_matches("http://")
            .trim_end_matches("/file")
            .to_string();
        let mirror_root = format!("http://{mirror_addr}");

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let err = fetch_src_uri(
            &pkg_dir,
            "http://127.0.0.1:1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![mirror_root],
                restrict_mirror: true,
                ..FetchOptions::default()
            },
        )
        .unwrap_err();
        // Only the unreachable primary URI was tried.
        assert!(err.contains("127.0.0.1:1"), "{err}");
        assert!(!distdir.join("hello-1.0.tar.gz").exists());

        // Unblock the still-parked server thread so it can exit cleanly.
        let _ = std::net::TcpStream::connect(&mirror_addr);
        handle.join().unwrap();
    }

    /// A `mirror+` prefix re-permits the public fallback under
    /// `RESTRICT=mirror`, so the mirror rescues the fetch.
    #[test]
    fn fetch_src_uri_mirror_prefix_re_permits_the_gentoo_mirrors_fallback_under_restrict_mirror() {
        // No `layout.conf` on this mirror (404): flat path.
        let (mirror_root, _, handle) = serve_mirror(
            vec![("/distfiles/hello-1.0.tar.gz", b"hello world".to_vec())],
            2,
        );

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let filenames = fetch_src_uri(
            &pkg_dir,
            "mirror+http://127.0.0.1:1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![mirror_root],
                restrict_mirror: true,
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["hello-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read_to_string(distdir.join("hello-1.0.tar.gz")).unwrap(),
            "hello world"
        );
        handle.join().unwrap();
    }

    /// `RESTRICT=mirror` bars only the public list: a `mirror://` URI's own
    /// `custommirrors` expansion is still tried.
    #[test]
    fn fetch_src_uri_restrict_mirror_still_allows_a_custommirror() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let mirror_root = uri_base.trim_end_matches("/file");

        let config_root = tempdir();
        fs::create_dir_all(config_root.join("etc/portage")).unwrap();
        fs::write(
            config_root.join("etc/portage/mirrors"),
            format!("testmirror {mirror_root}\n"),
        )
        .unwrap();

        let pkg_dir = tempdir();
        write_manifest(&pkg_dir, "foo-1.0.tar.gz", 11);

        let distdir = tempdir();
        let filenames = fetch_src_uri(
            &pkg_dir,
            "mirror://testmirror/foo-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                config_root: config_root.clone(),
                restrict_mirror: true,
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["foo-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read_to_string(distdir.join("foo-1.0.tar.gz")).unwrap(),
            "hello world"
        );
        handle.join().unwrap();
    }

    /// A `/`-rooted `GENTOO_MIRRORS` entry is a local directory the missing
    /// file is copied from -- before any download, whatever the restrictions.
    #[test]
    fn fetch_src_uri_copies_a_missing_file_from_an_on_filesystem_mirror() {
        for (layout_conf, rel) in [
            (None, "hello-1.0.tar.gz"),
            (
                Some("[structure]\n0=filename-hash BLAKE2B 8\n"),
                "ce/hello-1.0.tar.gz",
            ),
        ] {
            for restrict_fetch in [false, true] {
                let mirror_dir = tempdir();
                if let Some(conf) = layout_conf {
                    fs::write(mirror_dir.join("layout.conf"), conf).unwrap();
                }
                fs::create_dir_all(mirror_dir.join(rel).parent().unwrap()).unwrap();
                fs::write(mirror_dir.join(rel), "hello world").unwrap();
                let pkg_dir = tempdir();
                let distdir = tempdir();
                write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

                let filenames = fetch_src_uri(
                    &pkg_dir,
                    "http://127.0.0.1:1/hello-1.0.tar.gz",
                    &FetchOptions {
                        distdir: distdir.clone(),
                        gentoo_mirrors: vec![format!("{}/", mirror_dir.display())],
                        restrict_fetch,
                        ..FetchOptions::default()
                    },
                )
                .unwrap_or_else(|e| panic!("{rel} restrict_fetch={restrict_fetch}: {e}"));
                assert_eq!(filenames, vec!["hello-1.0.tar.gz".to_string()]);
                assert_eq!(
                    fs::read_to_string(distdir.join("hello-1.0.tar.gz")).unwrap(),
                    "hello world"
                );
                assert!(!distdir.join(".mirror-cache.json").exists());
            }
        }
    }

    /// `custommirrors["local"]` `/` entries come before `/`-rooted
    /// `GENTOO_MIRRORS`; first mirror with a digest-satisfying copy wins.
    #[test]
    fn fetch_src_uri_tries_local_fsmirrors_before_gentoo_mirrors_dirs() {
        let empty_local = tempdir();
        let local = tempdir();
        fs::write(local.join("hello-1.0.tar.gz"), "hello world").unwrap();
        let public = tempdir();
        fs::write(public.join("hello-1.0.tar.gz"), "HELLO WORLD").unwrap();
        let config_root = tempdir();
        fs::create_dir_all(config_root.join("etc/portage")).unwrap();
        fs::write(
            config_root.join("etc/portage/mirrors"),
            format!("local {} {}\n", empty_local.display(), local.display()),
        )
        .unwrap();
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        fetch_src_uri(
            &pkg_dir,
            "http://127.0.0.1:1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![public.display().to_string()],
                config_root,
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(distdir.join("hello-1.0.tar.gz")).unwrap(),
            "hello world"
        );
    }

    /// A `/`-rooted entry is never handed to `wget`: only the download
    /// candidates are tried and reported.
    #[test]
    fn fetch_src_uri_never_downloads_from_a_slash_rooted_gentoo_mirror() {
        let mirror_dir = tempdir();
        let pkg_dir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
        let err = fetch_src_uri(
            &pkg_dir,
            "http://127.0.0.1:1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: tempdir(),
                gentoo_mirrors: vec![mirror_dir.display().to_string()],
                ..FetchOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("127.0.0.1:1"), "{err}");
        assert!(!err.contains(&mirror_dir.display().to_string()), "{err}");
    }

    #[test]
    fn has_enough_space_checks_free_disk_space() {
        let distdir = tempdir();
        // Typical systems have at least a few GB of free space;
        // test with a huge number that should fail.
        assert!(
            has_enough_space(&distdir, 100),
            "typical DISTDIR should have more than 100 bytes free"
        );
        // A ridiculously large number (larger than most disks)
        let impossible_size = u64::MAX / 2;
        assert!(
            !has_enough_space(&distdir, impossible_size),
            "should not have {impossible_size} bytes free"
        );
    }

    #[test]
    fn fetch_src_uri_fsmirrors_with_layout_conf_resolution() {
        let mirror = tempdir();
        // Create a file in the filesystem mirror using flat layout
        fs::write(mirror.join("hello-1.0.tar.gz"), "hello world").unwrap();
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        // Fetch from the filesystem mirror (flat layout)
        let result = fetch_src_uri(
            &pkg_dir,
            "http://127.0.0.1:999/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![mirror.display().to_string()],
                ..FetchOptions::default()
            },
        );

        assert!(
            result.is_ok(),
            "should successfully copy from filesystem mirror"
        );
        assert_eq!(
            fs::read_to_string(distdir.join("hello-1.0.tar.gz")).unwrap(),
            "hello world"
        );
    }

    /// `RESTRICT=fetch`: a missing distfile fails without ever contacting the
    /// URI or the mirror server.
    #[test]
    fn fetch_src_uri_restrict_fetch_bars_the_plain_uri_and_public_mirrors() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let mirror_addr = uri_base
            .trim_start_matches("http://")
            .trim_end_matches("/file")
            .to_string();
        let mirror_root = format!("http://{mirror_addr}");

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let err = fetch_src_uri(
            &pkg_dir,
            // A *reachable* server -- proving it's the RESTRICT=fetch
            // gate, not a connection failure, that stops the fetch.
            &format!("{uri_base} -> hello-1.0.tar.gz"),
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![mirror_root],
                restrict_fetch: true,
                ..FetchOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("RESTRICT=fetch"), "{err}");
        assert!(!distdir.join("hello-1.0.tar.gz").exists());

        // The server was never contacted -- unblock it so it exits.
        let _ = std::net::TcpStream::connect(&mirror_addr);
        handle.join().unwrap();
    }

    /// `RESTRICT=fetch` still accepts an already-verified `DISTDIR` copy.
    #[test]
    fn fetch_src_uri_restrict_fetch_uses_an_already_verified_distdir_copy() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
        fs::write(distdir.join("hello-1.0.tar.gz"), b"hello world").unwrap();

        let filenames = fetch_src_uri(
            &pkg_dir,
            "https://192.0.2.1/hello-1.0.tar.gz",
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                restrict_fetch: true,
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["hello-1.0.tar.gz".to_string()]);
    }

    /// A `fetch+` prefix re-permits the plain URI under `RESTRICT=fetch`.
    #[test]
    fn fetch_src_uri_fetch_prefix_re_permits_the_uri_under_restrict_fetch() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let uri = format!("fetch+{uri_base} -> hello-1.0.tar.gz");

        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let filenames = fetch_src_uri(
            &pkg_dir,
            &uri,
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![],
                restrict_fetch: true,
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["hello-1.0.tar.gz".to_string()]);
        assert_eq!(
            fs::read_to_string(distdir.join("hello-1.0.tar.gz")).unwrap(),
            "hello world"
        );
        handle.join().unwrap();
    }

    fn test_entry(uri: &str, filename: &str) -> SrcUriEntry {
        SrcUriEntry {
            uri: uri.to_string(),
            filename: filename.to_string(),
            override_mirror: false,
            override_fetch: false,
        }
    }

    fn test_options() -> FetchOptions {
        FetchOptions {
            distdir: tempdir(),
            gentoo_mirrors: vec![
                "https://public1.example.com".to_string(),
                "https://public2.example.com".to_string(),
            ],
            ..FetchOptions::default()
        }
    }

    /// A plain URI sorts after the mirror lists, last (Portage's `fetch`
    /// order).
    #[test]
    fn assemble_candidates_orders_plain_uri_after_the_mirror_lists() {
        use std::collections::HashMap;
        let mut custom: HashMap<String, Vec<String>> = HashMap::new();
        custom.insert(
            "local".to_string(),
            vec!["https://local-mirror.example.com".to_string()],
        );
        let options = test_options();
        assert_eq!(
            assemble_candidates(
                &[&test_entry(
                    "https://primary.example.com/f-1.0.tar.gz",
                    "f-1.0.tar.gz"
                )],
                &custom,
                &HashMap::new(),
                &options,
            ),
            vec![
                mirror("https://local-mirror.example.com"),
                mirror("https://public1.example.com"),
                mirror("https://public2.example.com"),
                uri("https://primary.example.com/f-1.0.tar.gz"),
            ]
        );
    }

    fn mirror(root: &str) -> Candidate {
        Candidate::Mirror(root.to_string())
    }

    fn uri(uri: &str) -> Candidate {
        Candidate::Uri(uri.to_string())
    }

    /// Under `RESTRICT=primaryuri` the literal (plus third-party expansions)
    /// moves ahead of the mirror lists.
    #[test]
    fn assemble_candidates_prepends_the_literal_under_restrict_primaryuri() {
        use std::collections::HashMap;
        let mut custom: HashMap<String, Vec<String>> = HashMap::new();
        custom.insert(
            "local".to_string(),
            vec!["https://local-mirror.example.com".to_string()],
        );
        let mut third: HashMap<String, Vec<String>> = HashMap::new();
        third.insert(
            "gentoo".to_string(),
            vec!["https://third.example.com/distfiles".to_string()],
        );
        let mut options = test_options();
        options.restrict_primaryuri = true;
        // Plain URI: literal first, then local, public (no expansions
        // for a non-mirror:// token, no third-party tail).
        assert_eq!(
            assemble_candidates(
                &[&test_entry(
                    "https://primary.example.com/f-1.0.tar.gz",
                    "f-1.0.tar.gz"
                )],
                &custom,
                &third,
                &options,
            )[..2],
            vec![
                uri("https://primary.example.com/f-1.0.tar.gz"),
                mirror("https://local-mirror.example.com"),
            ]
        );
        // mirror:// URI: literal group is empty, so the third-party
        // expansions lead (twice: the primary-uri group tail, then the
        // inline expansions after the mirror lists).
        let got = assemble_candidates(
            &[&test_entry("mirror://gentoo/f-1.0.tar.gz", "f-1.0.tar.gz")],
            &custom,
            &third,
            &options,
        );
        let third_url = uri("https://third.example.com/distfiles/f-1.0.tar.gz");
        assert_eq!(
            got,
            vec![
                third_url.clone(),
                mirror("https://local-mirror.example.com"),
                mirror("https://public1.example.com"),
                mirror("https://public2.example.com"),
                third_url,
            ]
        );
    }

    /// Expected values from Portage's `fetch(..., listonly=1)`: the public
    /// mirror once, inline expansions, then literals last-listed-first plus
    /// third-party expansions again -- or that group first under primaryuri.
    #[test]
    fn assemble_candidates_groups_a_files_uris_like_portage_listonly() {
        use std::collections::HashMap;
        let mut third: HashMap<String, Vec<String>> = HashMap::new();
        third.insert(
            "gnome".to_string(),
            vec!["https://download.gnome.org/".to_string()],
        );
        let mut options = test_options();
        options.gentoo_mirrors = vec!["http://127.0.0.1:1".to_string()];
        let public = mirror("http://127.0.0.1:1");
        let (a, b, c) = (
            test_entry("http://a.example/f-1.tar.gz", "f-1.tar.gz"),
            test_entry("http://b.example/f-1.tar.gz", "f-1.tar.gz"),
            test_entry("http://c.example/f-1.tar.gz", "f-1.tar.gz"),
        );
        let gnome = test_entry("mirror://gnome/x/f-1.tar.gz", "f-1.tar.gz");
        let gnome_url = uri("https://download.gnome.org/x/f-1.tar.gz");
        let (ua, ub, uc) = (
            uri("http://a.example/f-1.tar.gz"),
            uri("http://b.example/f-1.tar.gz"),
            uri("http://c.example/f-1.tar.gz"),
        );
        for (group, default, primaryuri) in [
            (
                vec![&a, &b, &c],
                vec![public.clone(), uc.clone(), ub.clone(), ua.clone()],
                vec![uc.clone(), ub.clone(), ua.clone(), public.clone()],
            ),
            (
                vec![&a, &gnome, &b],
                vec![
                    public.clone(),
                    gnome_url.clone(),
                    ub.clone(),
                    ua.clone(),
                    gnome_url.clone(),
                ],
                vec![
                    ub.clone(),
                    ua.clone(),
                    gnome_url.clone(),
                    public.clone(),
                    gnome_url.clone(),
                ],
            ),
        ] {
            options.restrict_primaryuri = false;
            assert_eq!(
                assemble_candidates(&group, &HashMap::new(), &third, &options),
                default
            );
            options.restrict_primaryuri = true;
            assert_eq!(
                assemble_candidates(&group, &HashMap::new(), &third, &options),
                primaryuri
            );
        }
    }

    /// The second digest failure jumps to the primary URIs (mirror 3 never
    /// contacted); with a cap of 2 nothing after the second failure is tried.
    #[test]
    fn fetch_src_uri_checksum_failures_switch_to_primary_uris_then_stop_at_the_cap() {
        for (cap, expect_success) in [(5, true), (2, false)] {
            let bad = || {
                serve_mirror(
                    vec![("/distfiles/hello-1.0.tar.gz", b"HELLO WORLD".to_vec())],
                    2,
                )
            };
            let (m1, _, h1) = bad();
            let (m2, _, h2) = bad();
            let (m3, m3_requests, h3) = bad();
            let (literal_root, literal_requests, hl) =
                serve_mirror(vec![("/hello-1.0.tar.gz", b"hello world".to_vec())], 1);
            let pkg_dir = tempdir();
            let distdir = tempdir();
            write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

            let result = fetch_src_uri(
                &pkg_dir,
                &format!("{literal_root}/hello-1.0.tar.gz"),
                &FetchOptions {
                    distdir: distdir.clone(),
                    gentoo_mirrors: vec![m1, m2, m3.clone()],
                    checksum_failure_max_tries: cap,
                    ..FetchOptions::default()
                },
            );
            h1.join().unwrap();
            h2.join().unwrap();
            unblock_server(&m3);
            h3.join().unwrap();
            if !expect_success {
                unblock_server(&literal_root);
            }
            hl.join().unwrap();
            assert!(
                m3_requests.lock().unwrap().is_empty(),
                "cap={cap}: mirror 3 never contacted"
            );
            if expect_success {
                result.unwrap_or_else(|e| panic!("cap={cap}: {e}"));
                assert_eq!(
                    fs::read_to_string(distdir.join("hello-1.0.tar.gz")).unwrap(),
                    "hello world"
                );
            } else {
                let err = result.unwrap_err();
                assert!(literal_requests.lock().unwrap().is_empty(), "{err}");
                assert_eq!(
                    err.matches("digest verification failed").count(),
                    2,
                    "{err}"
                );
            }
        }
    }

    #[test]
    fn checksum_failure_max_tries_parses_like_portage() {
        assert_eq!(checksum_failure_max_tries(None), (5, vec![]));
        assert_eq!(checksum_failure_max_tries(Some("3")), (3, vec![]));
        assert_eq!(
            checksum_failure_max_tries(Some("many")),
            (
                5,
                vec![
                    "!!! Variable PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS contains non-integer value: 'many'"
                        .to_string(),
                    "!!! Using PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS default value: 5".to_string(),
                ]
            )
        );
        assert_eq!(checksum_failure_max_tries(Some("0")).0, 5);
        assert_eq!(
            checksum_failure_max_tries(Some("0")).1[0],
            "!!! Variable PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS contains value less than 1: '0'"
        );
    }

    #[test]
    fn checksum_failures_rename_bad_files_with_deterministic_suffix() {
        let bad = || serve_mirror(vec![("/distfiles/bad-1.0.tar.gz", b"BAD DATA".to_vec())], 2);
        let (m1, _, _h1) = bad();
        let (m2, _, _h2) = bad();
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "bad-1.0.tar.gz", 8);

        let result = fetch_src_uri(
            &pkg_dir,
            &format!("{m1}/distfiles/bad-1.0.tar.gz"),
            &FetchOptions {
                distdir: distdir.clone(),
                gentoo_mirrors: vec![m2],
                checksum_failure_max_tries: 2,
                ..FetchOptions::default()
            },
        );

        assert!(
            result.is_err(),
            "fetch should fail after 2 checksum failures"
        );

        let entries = portage_util::read_dir_entries(&distdir).unwrap();
        let mut bad_files: Vec<_> = entries
            .into_iter()
            .filter_map(|e| {
                let path = e.path();
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.contains("_checksum_failure_"))
                    .unwrap_or(false)
                {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            bad_files.len(),
            2,
            "should have 2 renamed bad files with _checksum_failure_ suffix"
        );
        bad_files.sort();
        for (i, path) in bad_files.iter().enumerate() {
            assert!(
                path.to_string_lossy()
                    .contains(&format!("_checksum_failure_{}", i + 1)),
                "bad file {} should have suffix _checksum_failure_{}",
                i,
                i + 1
            );
        }
    }

    /// Entries grouped by distfile in first-seen order, duplicates dropped.
    #[test]
    fn group_by_filename_keeps_first_seen_order_and_drops_repeated_uris() {
        let entries = vec![
            test_entry("http://a.example/f-1.tar.gz", "f-1.tar.gz"),
            test_entry("http://a.example/g-1.tar.gz", "g-1.tar.gz"),
            test_entry("http://b.example/f-1.tar.gz", "f-1.tar.gz"),
            test_entry("http://a.example/f-1.tar.gz", "f-1.tar.gz"),
        ];
        let groups = group_by_filename(&entries);
        assert_eq!(
            groups
                .iter()
                .map(|(name, g)| (*name, g.iter().map(|e| e.uri.as_str()).collect::<Vec<_>>()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "f-1.tar.gz",
                    vec!["http://a.example/f-1.tar.gz", "http://b.example/f-1.tar.gz"]
                ),
                ("g-1.tar.gz", vec!["http://a.example/g-1.tar.gz"]),
            ]
        );
    }

    /// A multiply-sourced file is still listed once.
    #[test]
    fn fetch_src_uri_returns_a_multiply_sourced_file_once() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);
        fs::write(distdir.join("hello-1.0.tar.gz"), "hello world").unwrap();
        let filenames = fetch_src_uri(
            &pkg_dir,
            "http://a.example/hello-1.0.tar.gz http://b.example/hello-1.0.tar.gz",
            &FetchOptions {
                distdir,
                gentoo_mirrors: vec![],
                ..FetchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(filenames, vec!["hello-1.0.tar.gz".to_string()]);
    }

    /// `RESTRICT=fetch` bars the literal (and `force-mirror` even a
    /// re-permitted one); the mirror lists are unaffected.
    #[test]
    fn assemble_candidates_bars_the_literal_under_fetch_restrictions() {
        use std::collections::HashMap;
        let entry = test_entry("https://primary.example.com/f-1.0.tar.gz", "f-1.0.tar.gz");
        let mut options = test_options();
        options.restrict_fetch = true;
        let got = assemble_candidates(&[&entry], &HashMap::new(), &HashMap::new(), &options);
        assert!(
            got.is_empty(),
            "restrict_fetch bars the literal and the public list alike: {got:?}"
        );
        let mut options = test_options();
        options.force_mirror = true;
        let mut fetch_entry = entry.clone();
        fetch_entry.override_fetch = true;
        let got = assemble_candidates(&[&fetch_entry], &HashMap::new(), &HashMap::new(), &options);
        assert!(
            !got.iter()
                .any(|c| matches!(c, Candidate::Uri(u) if u.contains("primary.example.com"))),
            "force-mirror skips even a fetch+-re-permitted literal: {got:?}"
        );
    }

    /// A definitely-closed localhost port, so the failure text pins the tried
    /// order without any server.
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    /// The failure report lists candidates in tried order: mirrors before
    /// literals by default, literal first under primaryuri.
    #[test]
    fn fetch_src_uri_tries_candidates_in_assembled_order() {
        let public = format!("http://127.0.0.1:{}", closed_port());
        let literal = format!("http://127.0.0.1:{}/f-1.0.tar.gz", closed_port());
        let run = |restrict_primaryuri: bool| {
            let pkg_dir = tempdir();
            let distdir = tempdir();
            write_manifest(&pkg_dir, "f-1.0.tar.gz", 11);
            fetch_src_uri(
                &pkg_dir,
                &literal,
                &FetchOptions {
                    distdir,
                    gentoo_mirrors: vec![public.clone()],
                    restrict_primaryuri,
                    ..FetchOptions::default()
                },
            )
            .unwrap_err()
        };
        let err = run(false);
        let public_at = err.find(&public).expect("public mirror tried");
        let literal_at = err.find(&literal).expect("literal URI tried");
        assert!(
            public_at < literal_at,
            "mirrors before literals by default: {err}"
        );
        let err = run(true);
        let public_at = err.find(&public).expect("public mirror tried");
        let literal_at = err.find(&literal).expect("literal URI tried");
        assert!(
            literal_at < public_at,
            "literal first under primaryuri: {err}"
        );
    }

    /// A twice-listed third-party expansion is still attempted exactly once,
    /// in both primaryuri modes.
    #[test]
    fn fetch_src_uri_attempts_a_repeated_candidate_only_once() {
        let mirror_root = format!("http://127.0.0.1:{}", closed_port());
        let expanded = format!("{mirror_root}/foo-1.0.tar.gz");
        for restrict_primaryuri in [false, true] {
            let repo_root = tempdir();
            fs::create_dir_all(repo_root.join("profiles")).unwrap();
            fs::write(repo_root.join("profiles/repo_name"), "mirrortest\n").unwrap();
            fs::write(
                repo_root.join("profiles/thirdpartymirrors"),
                format!("testmirror {mirror_root}\n"),
            )
            .unwrap();
            let pkg_dir = repo_root.join("dev-libs/mirrorpkg");
            fs::create_dir_all(&pkg_dir).unwrap();
            write_manifest(&pkg_dir, "foo-1.0.tar.gz", 11);

            let err = fetch_src_uri(
                &pkg_dir,
                "mirror://testmirror/foo-1.0.tar.gz",
                &FetchOptions {
                    distdir: tempdir(),
                    gentoo_mirrors: vec![],
                    restrict_primaryuri,
                    ..FetchOptions::default()
                },
            )
            .unwrap_err();
            assert_eq!(
                err.matches(&expanded).count(),
                1,
                "primaryuri={restrict_primaryuri}: one attempt, one error: {err}"
            );
        }
    }
}
