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
}
