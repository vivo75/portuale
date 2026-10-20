//! `emerge --getbinpkg` / `--getbinpkgonly` execution: refresh each remote
//! binhost's live index (in `--pretend` exactly like in a non-pretend merge),
//! resolve the graph, then merge every resolved entry -- per entry on its
//! `source` (a `Binary` candidate is downloaded+merged, anything else is
//! built+merged from source).
//!
//! Scope cuts a caller must know: a compressed remote index is tried
//! (`Packages.gz`/`Packages.zst`, then plain `Packages`); no `layout.conf`
//! negotiation (the index `PATH` is trusted); digest checks cover `SIZE` and
//! the `MD5`/`SHA1` fields present, plus a downloaded gpkg's internal
//! manifest and signature layer at merge time.

use crate::ebuild_merge::{self, MergeOptions};
use mrg_director::BinpkgIndex;
use mrg_director::MergeEngine;
use portage_profile::{BinRepo, Config};
use portage_repo::{GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

/// Portage's `fetch._hide_url_passwd`, verbatim: mask `//user:secret@` as
/// `//user:*password*@` with a single global substitution, so edge behaviour
/// matches (a password with whitespace is left alone; a user with `/` is
/// still masked).
pub(crate) fn hide_binhost_passwd(url: &str) -> String {
    regex::Regex::new(r"//([^:\s]+):[^@\s]+@")
        .expect("static regex is valid")
        .replace_all(url, "//$1:*password*@")
        .into_owned()
}

/// Recover Portage's `HTTPError` string (`"HTTP Error {code}: {reason}"`,
/// verified against the stdlib) from a captured wget transcript's
/// `awaiting response... {code} {reason}` line -- byte-identical whenever the
/// server sent a reason phrase. `None` when the transcript holds no response
/// line: the caller falls back to the fetch summary (documented, not parity).
pub(crate) fn wget_http_error(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        let Some((_, rest)) = line.split_once("awaiting response... ") else {
            continue;
        };
        let rest = rest.trim();
        let (code, reason) = rest.split_once(' ')?;
        if code.len() == 3 && code.bytes().all(|b| b.is_ascii_digit()) && !reason.trim().is_empty()
        {
            return Some(format!("HTTP Error {code}: {}", reason.trim()));
        }
    }
    None
}

/// The leading-blank-line `!!! [<name>] Error fetching binhost package info`
/// line plus the `!!! [<name>] <detail>` line and trailing blank line, as one
/// stderr block. `detail` is the `HTTP Error` shape (see [`wget_http_error`]),
/// else the fetch summary.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote
pub(crate) fn binhost_fetch_warning(binrepo_name: &str, sync_uri: &str, detail: &str) -> String {
    format!(
        "\n\n!!! [{binrepo_name}] Error fetching binhost package info from '{}'\n!!! [{binrepo_name}] {detail}\n\n",
        hide_binhost_passwd(sync_uri)
    )
}

/// `\n\n!!! [<name>] Binhost package index  has no TIMESTAMP field.\n` on
/// stderr. The double space is Portage's own implicit concatenation, kept
/// byte-identical.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote_repo
pub(crate) fn binhost_no_timestamp_warning(binrepo_name: &str) -> String {
    format!("\n\n!!! [{binrepo_name}] Binhost package index  has no TIMESTAMP field.\n")
}

/// `\n\n!!! [<name>] Binhost package index version is not supported: '<ver>'\n`
/// on stderr, where `<ver>` is the raw `VERSION` header (`'None'` when absent).
// Portage: lib/portage/dbapi/bintree.py, _populate_remote_repo
pub(crate) fn binhost_unsupported_version_warning(
    binrepo_name: &str,
    version: Option<&str>,
) -> String {
    format!(
        "\n\n!!! [{binrepo_name}] Binhost package index version is not supported: '{}'\n",
        version.unwrap_or("None")
    )
}

/// Refresh every binrepo's live `Packages` index and hand it to the resolver
/// in memory -- in `--pretend` exactly like in a non-pretend merge. Per
/// binrepo: a `frozen` or still-valid (`TTL`) cache is used with a `Local
/// copy ... will be used.` note; otherwise the remote index is fetched
/// (`file://` read from its own directory, `Packages.gz` first). A fetched
/// index no newer than the cache keeps the cache silently; a newer one is
/// cached with a fresh `DOWNLOAD_TIMESTAMP` and used. A failed fetch warns
/// and -- under `--pretend` -- resolves from the stale cache; otherwise even
/// the stale cache is dropped. A failed refresh is non-fatal either way:
/// resolution proceeds against whatever pool the rules leave.
///
/// Remote entries matching the effective `--getbinpkg-exclude` (or missing
/// from a non-empty `--getbinpkg-include`) never reach the in-memory pool:
/// each binrepo's own keys fold with the CLI sets and conflicting atoms
/// warn per repo, like Portage. The on-disk/EDB cache keeps the unfiltered
/// index (Portage filters when reading, not when writing, so a later run
/// without the option is unpoisoned). Local `PKGDIR` instances are never
/// filtered.
///
/// Deliberate narrowings: no conditional-GET (an unchanged index re-downloads;
/// resolution is identical, only one stdout line differs across runs); a
/// fetched index with no/unparseable `TIMESTAMP` or an unsupported `VERSION`
/// is dropped with Portage's own warning, discarding even a stale cache; no
/// `--verbose` detail, `ssh://`, pool filtering, or trust-helper gating.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote
pub fn refresh_binhost_indexes(
    binrepos: &[BinRepo],
    root: &Path,
    pretend: bool,
    cli_exclude: &[String],
    cli_include: &[String],
) {
    for binrepo in binrepos {
        let filter = GetbinpkgFilter::for_binrepo(binrepo, cli_exclude, cli_include);
        let outcome = refresh_one_binrepo(binrepo, root, pretend, &filter);
        if let Some(warning) = outcome.stderr_warning {
            eprint!("{warning}");
        }
        if let Some(note) = outcome.stdout_note {
            print!("{note}");
        }
    }
}

/// One binrepo's effective `--getbinpkg-exclude/include` sets: its own
/// `binrepos.conf` keys folded with the CLI sets, minus the atoms each
/// side loses to a conflict warning (warned here, in refresh order, like
/// Portage's `_populate_remote`, which runs before the CLI-overlap pass).
// Portage: lib/portage/dbapi/bintree.py, _populate_remote
struct GetbinpkgFilter {
    exclude: Vec<String>,
    include: Vec<String>,
}

impl GetbinpkgFilter {
    fn for_binrepo(binrepo: &BinRepo, cli_exclude: &[String], cli_include: &[String]) -> Self {
        fn split_atoms(values: &[String]) -> std::collections::BTreeSet<String> {
            values
                .iter()
                .flat_map(|s| s.split_whitespace())
                .map(str::to_string)
                .collect()
        }
        let mut exclude: std::collections::BTreeSet<String> =
            binrepo.getbinpkg_exclude.iter().cloned().collect();
        exclude.extend(split_atoms(cli_exclude));
        let mut include: std::collections::BTreeSet<String> =
            binrepo.getbinpkg_include.iter().cloned().collect();
        include.extend(split_atoms(cli_include));
        // `--getbinpkg-include` overrides `getbinpkg-exclude` in binrepos.conf.
        let conflicted_exclude: Vec<String> = exclude
            .intersection(&split_atoms(cli_include))
            .cloned()
            .collect();
        if !conflicted_exclude.is_empty() {
            eprint!(
                "\n!!! The following getbinpkg-exclude atoms for [{}] have been \
                 overridden by the --getbinpkg-include option:\n\n    {}\n",
                binrepo.name,
                conflicted_exclude.join("\n    ")
            );
            for a in &conflicted_exclude {
                exclude.remove(a);
            }
        }
        // `--getbinpkg-exclude` overrides `getbinpkg-include` in binrepos.conf.
        let conflicted_include: Vec<String> = include
            .intersection(&split_atoms(cli_exclude))
            .cloned()
            .collect();
        if !conflicted_include.is_empty() {
            eprint!(
                "\n!!! The following getbinpkg-include atoms for [{}] have been \
                 overridden by the --getbinpkg-exclude option:\n\n    {}\n",
                binrepo.name,
                conflicted_include.join("\n    ")
            );
            for a in &conflicted_include {
                include.remove(a);
            }
        }
        GetbinpkgFilter {
            exclude: exclude.into_iter().collect(),
            include: include.into_iter().collect(),
        }
    }

    /// Drop the filtered records, keeping order. Local `PKGDIR` records
    /// never pass through here.
    fn apply(
        &self,
        entries: Vec<std::collections::HashMap<String, String>>,
    ) -> Vec<std::collections::HashMap<String, String>> {
        entries
            .into_iter()
            .filter(|e| !portage_repo::getbinpkg_record_excluded(e, &self.exclude, &self.include))
            .collect()
    }
}

/// Portage `_emerge/actions.py` `binpkg_selection_config`: atoms in both
/// CLI lists warn and cancel from both. Runs after the refresh, like
/// Portage (which populates before adjusting configs, so the per-repo
/// warnings print first — S0 cell `g03-both`).
pub(crate) fn warn_getbinpkg_cli_overlap(exclude: &[String], include: &[String]) {
    let exclude_set: std::collections::BTreeSet<&str> =
        exclude.iter().flat_map(|s| s.split_whitespace()).collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut conflicted: Vec<&str> = Vec::new();
    for a in include.iter().flat_map(|s| s.split_whitespace()) {
        if exclude_set.contains(a) && seen.insert(a) {
            conflicted.push(a);
        }
    }
    if !conflicted.is_empty() {
        eprint!(
            "\n!!! The following atoms appear in both the --getbinpkg-exclude and \
             --getbinpkg-include command line arguments:\n\n    {}\n",
            conflicted.join("\n    ")
        );
    }
}

/// One binrepo's run index (installed via the remote-index override, keyed by
/// the same `packages_dir` the resolver reads) plus Portage's messages for the
/// caller to print.
struct RefreshOutcome {
    stderr_warning: Option<String>,
    stdout_note: Option<String>,
}

/// The cache file `<EROOT>/var/cache/edb/binhost/<host>/<url-path>/Packages`.
/// For `http(s)`/`ssh` that is [`BinRepo::packages_dir`]; for `file://` the
/// EROOT-side copy (Portage caches even local indexes).
// Portage: lib/portage/dbapi/bintree.py, _populate_remote_repo
fn edb_cache_packages_file(root: &Path, sync_uri: &str) -> std::path::PathBuf {
    let uri = sync_uri.trim_end_matches('/');
    if let Some(rest) = uri.strip_prefix("file://") {
        return root
            .join("var/cache/edb/binhost")
            .join(rest.trim_start_matches('/'))
            .join("Packages");
    }
    // Single mapping, so the cache is always written where the resolver reads.
    BinRepo {
        name: String::new(),
        sync_uri: uri.to_string(),
        priority: 0,
        location: None,
        verify_signature: true,
        frozen: false,
        getbinpkg_exclude: Vec::new(),
        getbinpkg_include: Vec::new(),
    }
    .packages_dir(root)
    .join("Packages")
}
fn refresh_one_binrepo(
    binrepo: &BinRepo,
    root: &Path,
    pretend: bool,
    filter: &GetbinpkgFilter,
) -> RefreshOutcome {
    // Messages print the configured URI; fetch URLs use the stripped form.
    let raw = binrepo.sync_uri.as_str();
    let uri = raw.trim_end_matches('/');
    let ok = |overlay: portage_repo::BinaryIndex| {
        portage_repo::set_remote_binary_index_override(
            &binrepo.packages_dir(root),
            Some(std::sync::Arc::new(overlay)),
        );
    };
    // Filtered entries feed the in-memory pool; the cache write below
    // keeps the unfiltered index (see `refresh_binhost_indexes`).
    let ok_entries = |entries: Vec<std::collections::HashMap<String, String>>| {
        ok(portage_repo::BinaryIndex::from_entries(
            filter.apply(entries),
        ));
    };
    let cached_file = edb_cache_packages_file(root, raw);
    let cached_text = std::fs::read_to_string(&cached_file).ok();
    let (cached_header, cached_entries) = cached_text
        .as_deref()
        .map(portage_repo::parse_packages_index)
        .unwrap_or_default();

    // Refresh is always requested here, so a cached index on a frozen repo is
    // used as-is.
    if cached_text.is_some()
        && cached_header
            .get("TIMESTAMP")
            .is_some_and(|s| !s.is_empty())
        && binrepo.frozen
    {
        ok_entries(cached_entries.clone());
        return RefreshOutcome {
            stderr_warning: None,
            stdout_note: Some(use_cached_note(&binrepo.name, "frozen")),
        };
    }
    // A still-valid cache is used without any fetch.
    let ttl_secs: f64 = cached_header
        .get("TTL")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let downloaded_at: f64 = cached_header
        .get("DOWNLOAD_TIMESTAMP")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    if downloaded_at > 0.0 && ttl_secs > 0.0 && downloaded_at + ttl_secs > unix_now() {
        ok_entries(cached_entries.clone());
        return RefreshOutcome {
            stderr_warning: None,
            stdout_note: Some(use_cached_note(&binrepo.name, "within TTL")),
        };
    }

    // The fetch runs BEFORE any cache-dir write and lands in temp files, never
    // straight into the cache: a stale cache must survive a failed or older
    // remote index.
    match fetch_remote_index_text(uri) {
        Err(failures) => {
            let warning = Some(binhost_fetch_warning(
                &binrepo.name,
                raw,
                &http_or_summary(&failures),
            ));
            if pretend {
                // Keep the stale copy and say so.
                ok_entries(cached_entries.clone());
                RefreshOutcome {
                    stderr_warning: warning,
                    stdout_note: Some(format!(
                        "[{}] Local copy of unavailable remote index will be used due to --pretend\n",
                        binrepo.name
                    )),
                }
            } else {
                // Drop even the stale copy: the override suppresses the disk cache
                // for the rest of the run.
                portage_repo::set_remote_binary_index_override(&binrepo.packages_dir(root), None);
                RefreshOutcome {
                    stderr_warning: warning,
                    stdout_note: None,
                }
            }
        }
        Ok(remote_text) => {
            let (remote_header, remote_entries) = portage_repo::parse_packages_index(&remote_text);
            // Drop a fetched index with no (or unparseable) `TIMESTAMP` with the
            // warning.
            if remote_header
                .get("TIMESTAMP")
                .and_then(|s| s.parse::<i64>().ok())
                .is_none()
            {
                portage_repo::set_remote_binary_index_override(&binrepo.packages_dir(root), None);
                return RefreshOutcome {
                    stderr_warning: Some(binhost_no_timestamp_warning(&binrepo.name)),
                    stdout_note: None,
                };
            }
            // Drop an index whose `VERSION` is missing, unparseable, or newer than
            // Portage's `_pkgindex_version` (0) with the warning.
            if !remote_header
                .get("VERSION")
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|v| v <= 0)
            {
                portage_repo::set_remote_binary_index_override(&binrepo.packages_dir(root), None);
                return RefreshOutcome {
                    stderr_warning: Some(binhost_unsupported_version_warning(
                        &binrepo.name,
                        remote_header.get("VERSION").map(String::as_str),
                    )),
                    stdout_note: None,
                };
            }
            // The remote index is served only when strictly newer; an equally-old
            // or older remote keeps the cache silently, with no rewrite.
            let stale_remote = match (
                cached_header
                    .get("TIMESTAMP")
                    .and_then(|s| s.parse::<i64>().ok()),
                remote_header
                    .get("TIMESTAMP")
                    .and_then(|s| s.parse::<i64>().ok()),
            ) {
                (Some(local), Some(remote)) => local >= remote,
                _ => false,
            };
            if stale_remote {
                ok_entries(cached_entries.clone());
                return RefreshOutcome {
                    stderr_warning: None,
                    stdout_note: None,
                };
            }
            let now = unix_now() as u64;
            let staged = stamp_download_timestamp(&remote_text, now);
            ok_entries(remote_entries);
            // Best-effort cache write: an unwritable cache dir is ignored; only a
            // write error on a writable dir warns (refreshes stay non-fatal).
            // Either way the in-memory index above is what the run resolves from.
            let write_error = (|| {
                if let Some(parent) = cached_file.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&cached_file, staged).map_err(|e| e.to_string())
            })();
            let stderr_warning = match write_error {
                Ok(()) => None,
                Err(_) if !cache_dir_writable(&cached_file) => None,
                Err(e) => Some(binhost_fetch_warning(
                    &binrepo.name,
                    raw,
                    &format!("{}: {e}", cached_file.display()),
                )),
            };
            RefreshOutcome {
                stderr_warning,
                stdout_note: None,
            }
        }
    }
}

/// `[name] Local copy of remote index is <why> and will be used.` on stdout
/// (Portage appends `Last-Modified` detail only under `--verbose`, unmodelled).
fn use_cached_note(binrepo_name: &str, why: &str) -> String {
    format!("[{binrepo_name}] Local copy of remote index is {why} and will be used.\n")
}

/// Seconds since the epoch as `f64` (Portage's `time.time()`).
fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `os.access(dirname, W_OK)` without libc: the parent dir's mode bits decide
/// (no probe file is ever created, so nothing can linger in the EROOT cache
/// dir). A missing directory counts as unwritable. Refreshes stay non-fatal,
/// so a failed write still only warns.
fn cache_dir_writable(cache_file: &Path) -> bool {
    let dir = cache_file.parent().unwrap_or_else(|| Path::new("."));
    std::fs::metadata(dir).is_ok_and(|m| !m.permissions().readonly())
}

/// Stamp (or replace) the `DOWNLOAD_TIMESTAMP` header before caching a fetched
/// index, so a later run's `TTL` check sees it.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote
fn stamp_download_timestamp(text: &str, now_secs: u64) -> String {
    let marker = format!("DOWNLOAD_TIMESTAMP: {now_secs}");
    let (header, rest) = match text.find("\n\n") {
        Some(i) => text.split_at(i),
        None => (text, ""),
    };
    let mut out = String::new();
    let mut stamped = false;
    for line in header.lines() {
        if line.starts_with("DOWNLOAD_TIMESTAMP:") && !stamped {
            out.push_str(&marker);
            stamped = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !stamped {
        out.push_str(&marker);
        out.push('\n');
    }
    out.push_str(rest);
    out
}

/// Fetch one binrepo's remote `Packages` body as text. A `file://` repo is
/// read from its own directory (`Packages.gz` first; a missing `.gz` falls
/// through, a corrupt one fails outright); anything else goes over quiet wget
/// into temp files (never the cache), preferring a compressed index
/// (`Packages.gz`, then the `Packages.zst` superset, then plain `Packages`).
/// Every attempt runs quiet: the transcript is captured for message shaping.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote
fn fetch_remote_index_text(uri: &str) -> Result<String, Vec<portage_fetch::QuietFetchError>> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let dir = Path::new(rest);
        let gz = dir.join("Packages.gz");
        if gz.is_file() {
            let out = std::process::Command::new("gzip")
                .arg("-dc")
                .arg(&gz)
                .output();
            return match out {
                Ok(out) if out.status.success() => {
                    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
                }
                Ok(out) => Err(vec![portage_fetch::QuietFetchError {
                    summary: "gzip -dc Packages.gz failed".to_string(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                }]),
                Err(e) => Err(vec![portage_fetch::QuietFetchError {
                    summary: format!("gzip -dc {}: {e}", gz.display()),
                    stderr: String::new(),
                }]),
            };
        }
        let plain = dir.join("Packages");
        return std::fs::read_to_string(&plain).map_err(|e| {
            vec![portage_fetch::QuietFetchError {
                summary: format!("{}: {e}", plain.display()),
                stderr: String::new(),
            }]
        });
    }

    static FETCH_TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut tmps: Vec<std::path::PathBuf> = Vec::new();
    let tmp = |tmps: &mut Vec<std::path::PathBuf>, suffix: &str| {
        let path = std::env::temp_dir().join(format!(
            "portuale-binhost-{}-{}-{suffix}",
            std::process::id(),
            FETCH_TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        tmps.push(path.clone());
        path
    };
    let cleanup = |tmps: &[std::path::PathBuf]| {
        for t in tmps {
            let _ = std::fs::remove_file(t);
        }
    };
    // A downloaded-but-corrupt compressed index fails immediately, with NO
    // plain-`Packages` attempt: Portage's `gzip.BadGzipFile` subclasses
    // `OSError` (verified against the stdlib), so it escapes the fetch loop
    // straight to the outer handler.
    let mut failures: Vec<portage_fetch::QuietFetchError> = Vec::new();
    for (ext, tool) in [("gz", "gzip"), ("zst", "zstd")] {
        let compressed = tmp(&mut tmps, ext);
        match crate::fetch::wget_fetch_quiet(&format!("{uri}/Packages.{ext}"), &compressed) {
            Err(e) => failures.push(e),
            Ok(()) => {
                let plain_tmp = tmp(&mut tmps, "plain");
                let decompressed = match std::fs::File::create(&plain_tmp) {
                    Err(e) => {
                        failures.push(portage_fetch::QuietFetchError {
                            summary: format!("{}: {e}", plain_tmp.display()),
                            stderr: String::new(),
                        });
                        cleanup(&tmps);
                        return Err(failures);
                    }
                    Ok(out) => std::process::Command::new(tool)
                        .arg("-dc")
                        .arg(&compressed)
                        .stdout(std::process::Stdio::from(out))
                        .status()
                        .is_ok_and(|s| s.success()),
                };
                match std::fs::read_to_string(&plain_tmp) {
                    Ok(text) if decompressed => {
                        cleanup(&tmps);
                        return Ok(text);
                    }
                    _ => {
                        cleanup(&tmps);
                        failures.push(portage_fetch::QuietFetchError {
                            summary: format!("{tool} -dc Packages.{ext} failed"),
                            stderr: String::new(),
                        });
                        return Err(failures);
                    }
                }
            }
        }
    }
    let plain_tmp = tmp(&mut tmps, "plain");
    match crate::fetch::wget_fetch_quiet(&format!("{uri}/Packages"), &plain_tmp) {
        Ok(()) => {
            let result = std::fs::read_to_string(&plain_tmp).map_err(|e| {
                vec![portage_fetch::QuietFetchError {
                    summary: format!("{}: {e}", plain_tmp.display()),
                    stderr: String::new(),
                }]
            });
            cleanup(&tmps);
            result
        }
        Err(e) => {
            failures.push(e);
            cleanup(&tmps);
            Err(failures)
        }
    }
}

/// Prefer a server status line (`HTTP Error 500: ...`, from the first attempt
/// that has one) over the wget summary. Attempts hit the same server, so the
/// first parseable one is representative.
fn http_or_summary(failures: &[portage_fetch::QuietFetchError]) -> String {
    failures
        .iter()
        .find_map(|f| wget_http_error(&f.stderr))
        .unwrap_or_else(|| {
            failures
                .last()
                .map(|f| f.summary.clone())
                .unwrap_or_else(|| "index refresh failed".to_string())
        })
}

/// Arguments of [`run_merge_plan`].
pub struct RunMergePlanArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub config: &'a Config,
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub pkgdir: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub merge_options: &'a MergeOptions,
    pub keep_going: bool,
    pub buildpkg: Option<&'a crate::ebuild_package::PackageOptions>,
    pub buildpkg_exclude: &'a [String],
    /// The scheduler-display mode, computed once by the caller. The mixed plan
    /// runs serially, so the mode only carries the blank/`>>>` rule; `Jobs:`
    /// events for the mixed plan are a later slice.
    pub mode: crate::emerge_build::StatusMode,
}

/// Merge every resolved entry (already in dependency-first merge order),
/// dispatching per entry on `entry.source` (Portage's "prefer a binary, fall
/// back to source" is the resolver's job, stamped on each `GraphEntry`):
/// a `Binary` entry is downloaded (if remote) and merged; anything else is
/// built+merged from source; `AlreadyInstalled` is a silent no-op.
/// `--getbinpkgonly` (binary-only resolve) never yields a non-`Binary` entry.
pub fn run_merge_plan(args: RunMergePlanArgs<'_>) -> Result<(), String> {
    let RunMergePlanArgs {
        entries,
        config,
        repos,
        root,
        pkgdir,
        portage_tmpdir,
        merge_options,
        keep_going,
        buildpkg,
        buildpkg_exclude,
        mode,
    } = args;
    // A source build always runs ebuild phases, so a binary that cannot find
    // the vendored runtime fails before anything is fetched. Binary merges are
    // not gated here (a hookless gpkg never spawns a shell).
    if entries.iter().any(|e| {
        e.source != portage_repo::CandidateSource::Binary
            && !matches!(
                e.outcome,
                portage_repo::PretendOutcome::AlreadyInstalled { .. }
                    | portage_repo::PretendOutcome::NoVisibleCandidate
            )
    }) {
        crate::ebuild_phases::require_phase_runtime()?;
    }
    // One display per merge run.
    let total_builds = entries
        .iter()
        .filter(|e| {
            e.source == portage_repo::CandidateSource::Binary
                || crate::emerge_build::entry_counts_toward_progress(e)
        })
        .count();
    let display = crate::emerge_build::StatusDisplay::new(
        mode,
        total_builds,
        crate::emerge_build::progress_color(),
    );
    let source_paths = |entry: &GraphEntry| {
        crate::emerge_build::source_paths_for(merge_options, entry, portage_tmpdir)
    };
    let report = crate::emerge_build::FailReport {
        root,
        source_paths: &source_paths,
        features: crate::emerge_build::resolved_features(merge_options),
    };
    crate::emerge_build::run_merge_loop(entries, keep_going, &report, |_, entry, entry_progress| {
        // Derive the entry's `MergeUnit` and dispatch on its kind through the
        // source/binary engines. An entry with nothing to merge stays a silent
        // no-op, exactly the merge functions' own early return.
        let Some(unit) = crate::merge_engines::merge_unit_for_entry(entry, root, entry_progress)
        else {
            return Ok(());
        };
        let ctx = mrg_director::MergeContext {
            root: root.to_path_buf(),
            builddir: portage_tmpdir.to_path_buf(),
            jobs: 1,
            keep_going,
        };
        let outcome = match unit.kind {
            mrg_director::MergeKind::Source => {
                let bp = buildpkg.filter(|opts| {
                    crate::emerge_build::entry_buildpkg_wanted(
                        entry,
                        repos,
                        buildpkg_exclude,
                        opts.buildpkg_live,
                    )
                });
                crate::merge_engines::SourceEngine {
                    repos,
                    root,
                    portage_tmpdir,
                    options: merge_options,
                    buildpkg: bp,
                    buildpkg_exclude,
                    display: &display,
                }
                .execute(&unit, &ctx)
            }
            mrg_director::MergeKind::Binary => crate::merge_engines::BinaryEngine {
                config,
                root,
                pkgdir,
                portage_tmpdir,
                options: merge_options,
                display: &display,
            }
            .execute(&unit, &ctx),
        };
        match outcome {
            mrg_director::MergeOutcome::Merged | mrg_director::MergeOutcome::Skipped(_) => Ok(()),
            mrg_director::MergeOutcome::Failed(e) => Err(e),
        }
    })
}

/// Arguments of [`merge_one_binary_entry`].
pub(crate) struct MergeOneBinaryEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) pkgdir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) merge_options: &'a MergeOptions,
    pub(crate) progress: mrg_director::MergeProgress,
    pub(crate) display: &'a crate::emerge_build::StatusDisplay,
}

/// One `Binary` entry of the merge loop: `AlreadyInstalled` is a silent no-op;
/// `New`/`Upgrade`/`Downgrade`/`Reinstall` are fetched (remote) or located
/// (`$PKGDIR`) and merged (a replaced same-slot version is unmerged itself).
pub(crate) fn merge_one_binary_entry(args: MergeOneBinaryEntryArgs<'_>) -> Result<(), String> {
    let MergeOneBinaryEntryArgs {
        entry,
        config,
        root,
        pkgdir,
        portage_tmpdir,
        merge_options,
        progress,
        display,
    } = args;
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        // A removal installs no binary.
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::Uninstall { .. } => return Ok(()),
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::NoVisibleCandidate => {
            return Err(format!("no binary package available for {cp}"));
        }
    };

    // The per-package `Emerging binary (N of M) cpv::repo` line: the `>>> Merging
    // binary package ...` line's shape.
    let color = crate::emerge_build::progress_color();
    display.status(&crate::emerge_build::emerging_line(
        entry, &version, progress, root, &color,
    ));

    let local = resolve_local_binpkg(
        pkgdir,
        &entry.category,
        &entry.package,
        &version,
        entry.build_id.as_deref(),
    );
    // A `$PKGDIR` file on disk wins; otherwise only a *remote* entry may fetch
    // from a binhost. A resumed or fresh local entry with no file is an error,
    // not a refetch. A repo-fetched binary uses that repo's own signature
    // policy; a local `$PKGDIR` file keeps `merge_options.gpg_verify`.
    let mut fetched_verify = None;
    let binpkg_path = match local {
        Some(path) => path,
        None if entry.remote_binary => {
            // The remote `Packages` lookup goes through the director's `BinpkgIndex`
            // seam, which scans the configured binrepos in order; the record travels
            // with the owning binrepo's name, recovering `sync-uri` and policy below.
            let local_index = portage_repo::BinaryIndex::from_pkgdir(pkgdir);
            let remote = mrg_director::RemoteBinhostIndex {
                config,
                root,
                local: &local_index,
            };
            let (binrepo_name, record) = remote
                .metadata_with_source_instance(
                    &entry.category,
                    &entry.package,
                    &version,
                    entry.build_id.as_deref(),
                )
                .ok_or_else(|| {
                    format!(
                        "{cp}-{version}: no binpkg file under {} and not in any binhost `Packages` index",
                        pkgdir.display()
                    )
                })?;
            let binrepo = config
                .binrepos
                .iter()
                .find(|b| b.name == binrepo_name)
                .ok_or_else(|| {
                    format!("{cp}-{version}: binhost record names unknown repo {binrepo_name:?}")
                })?;
            let path = download_and_verify(DownloadAndVerifyArgs {
                sync_uri: &binrepo.sync_uri,
                record: &record,
                category: &entry.category,
                package: &entry.package,
                version: &version,
                pkgdir: &binrepo_download_dir(config, binrepo, root, pkgdir),
                config,
            })?;
            if let Some(names) = verified_digest_names(&record) {
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                let file = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                crate::fetch::print_verified_names(&file, &names);
            }
            let features = if merge_options.features.is_empty() {
                std::env::var("FEATURES").unwrap_or_default()
            } else {
                merge_options.features.clone()
            };
            fetched_verify = Some(crate::binpkg::GpgVerify::from_binrepo(
                binrepo.verify_signature,
                &features,
            ));
            path
        }
        None => {
            // The `>>> Emerging binary` line already printed, so what remains is
            // the verifier's own output -- to stdout AND the package `build.log` --
            // plus the `>>> Failed to emerge ...` tail. The log holds exactly this
            // block. The branch condition is whether the local index still lists
            // the cpv.
            // Portage: _emerge/BinpkgVerifier.py, _start
            let cpv = format!("{cp}-{version}");
            let block = missing_binpkg_block(
                &cpv,
                local_index_lists_cpv(
                    pkgdir,
                    &entry.category,
                    &entry.package,
                    &version,
                    entry.build_id.as_deref(),
                ),
            );
            let log_path = crate::emerge_build::build_log_path(
                portage_tmpdir,
                &entry.category,
                &entry.package,
                &version,
                &crate::emerge_build::resolved_features(merge_options),
            );
            let log_ready = log_path
                .parent()
                .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
                && std::fs::write(&log_path, block.as_bytes()).is_ok()
                && std::fs::metadata(&log_path).is_ok_and(|st| st.len() > 0);
            print!("{block}");
            print!(
                "{}",
                failed_pkg_msg(&cpv, root, log_ready.then_some(log_path.as_path()))
            );
            return Err(binpkg_missing_failure(&cpv));
        }
    };

    // A local binpkg the `<pkgdir>/Packages` index vouches for is verified at
    // merge -- size first, then every digest the record carries -- against the
    // index's own values. A mismatch prints the `!!! Digest verification
    // failed:` block to stdout AND the package `build.log`, renames the file
    // to `._checksum_failure_.<rand>`, and fails the package with the `>>>
    // Failed to emerge ...` tail. A binpkg no record vouches for skips this
    // (nothing to verify against); remote downloads were already checked
    // against the remote record, so only a purely local binpkg is re-checked
    // here. The returned error is a silent sentinel the CLI boundary drops
    // (exit 1, no resume notice, no `emerge:` line).
    if !entry.remote_binary
        && let Some(record) = local_index_record(LocalIndexRecordArgs {
            pkgdir,
            category: &entry.category,
            package: &entry.package,
            version: &version,
            build_id: entry.build_id.as_deref(),
            binpkg_path: &binpkg_path,
        })
        && let Err(mismatch) = crate::binpkg::verify_binpkg_against_index(&binpkg_path, &record)
    {
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let renamed = crate::binpkg::checksum_failure_rename(&binpkg_path)
            .unwrap_or_else(|| binpkg_path.clone());
        let block = crate::binpkg::digest_failure_block(&binpkg_path, &mismatch, &renamed);
        // The log holds exactly this block: the block is appended to the fresh
        // `PORTAGE_LOG_FILE`, the same `${PORTAGE_BUILDDIR}/temp/build.log`.
        let log_path = crate::emerge_build::build_log_path(
            portage_tmpdir,
            &entry.category,
            &entry.package,
            &version,
            &crate::emerge_build::resolved_features(merge_options),
        );
        // Only a log that exists and is non-empty is reported (`, Log file:`
        // suffix), exactly like Portage with no located log.
        let log_ready = log_path
            .parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::write(&log_path, block.as_bytes()).is_ok()
            && std::fs::metadata(&log_path).is_ok_and(|st| st.len() > 0);
        print!("{block}");
        print!(
            "{}",
            failed_pkg_msg(&cpv, root, log_ready.then_some(log_path.as_path()))
        );
        return Err(binpkg_digest_failure(&cpv));
    }

    // The per-package line between locating/fetching above and the vdb merge
    // below.
    display.status(&crate::emerge_build::installing_line(
        entry, &version, progress, root, &color,
    ));
    // A binary merge is a package task too: refresh the protect lists its
    // `dblink` reads.
    let reloaded_protect = crate::emerge_build::reloaded_config_protect(merge_options);
    let mut entry_options;
    let merge_options = if fetched_verify.is_some() || reloaded_protect.is_some() {
        entry_options = merge_options.clone();
        if let Some(gpg) = fetched_verify {
            entry_options.gpg_verify = gpg;
        }
        if let Some((protect, mask)) = reloaded_protect {
            entry_options.config_protect = protect;
            entry_options.config_protect_mask = mask;
        }
        &entry_options
    } else {
        merge_options
    };
    let status = ebuild_merge::merge_binpkg(&binpkg_path, root, portage_tmpdir, merge_options)?;
    if status != 0 {
        return Err(format!("{cp}-{version}: binpkg merge failed ({status})"));
    }
    // The per-package `Completed (N of M) cpv::repo` line.
    display.status(&crate::emerge_build::completed_line(
        entry, &version, progress, root, &color,
    ));
    Ok(())
}

/// `>>> Failed to emerge <cpv>[ for <root>][, Log file:]` plus, when the
/// non-empty build log is found, `>>>  '<log>'`. Each message writes its own
/// leading blank line first; the log line's extra space is Portage's own
/// `f" '{log}'"`. The ` for <root>` suffix appears when `ROOT != "/"`.
// Portage: lib/_emerge/Scheduler.py, _failed_pkg_msg
pub(crate) fn failed_pkg_msg(cpv: &str, root: &Path, log_path: Option<&Path>) -> String {
    let for_root = if root == Path::new("/") {
        String::new()
    } else {
        format!(" for {}", root.display())
    };
    match log_path {
        Some(log) => format!(
            "\n>>> Failed to emerge {cpv}{for_root}, Log file:\n\n>>>  '{}'\n",
            log.display()
        ),
        None => format!("\n>>> Failed to emerge {cpv}{for_root}\n"),
    }
}

/// The error for a merge-time binpkg digest failure whose output is already
/// printed: the CLI boundary exits 1 with no resume notice and no `emerge:` line.
pub(crate) fn binpkg_digest_failure(cpv: &str) -> String {
    format!("{cpv}: binpkg digest verification failed")
}

/// Whether `e` is exactly one `binpkg_digest_failure`. A `--keep-going`
/// combined message embeds the same text among newlines and must keep
/// the normal tail, so only a lone single-line failure matches.
pub(crate) fn is_binpkg_digest_failure(e: &str) -> bool {
    !e.contains('\n') && e.ends_with(": binpkg digest verification failed")
}

/// The missing-file block as one unit (each line `\n`-terminated, no
/// surrounding blanks): the stale-index arm when the cpv is still in the
/// `<pkgdir>/Packages` index, else the `!!! Fetching Binary failed` arm.
/// Pure shaper, for tests and the `build.log` write alike.
// Portage: _emerge/BinpkgVerifier.py, _start
pub(crate) fn missing_binpkg_block(cpv: &str, index_lists_cpv: bool) -> String {
    if index_lists_cpv {
        format!(
            "!!! Tried to use non-existent binary for '{cpv}'\n\
             !!! Likely caused by an outdated index. Run 'emaint binhost -f'.\n"
        )
    } else {
        format!("!!! Fetching Binary failed for '{cpv}'\n")
    }
}

/// Whether the local `<pkgdir>/Packages` index still lists this cpv. Like
/// [`local_index_record`] minus the basename tie-break (no file to name it
/// after): with several same-`CPV` stanzas the entry's `BUILD_ID` picks.
// Portage: _emerge/BinpkgVerifier.py, _start
fn local_index_lists_cpv(
    pkgdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
) -> bool {
    let cpv = format!("{category}/{package}-{version}");
    let mut candidates = portage_repo::read_packages_index(pkgdir)
        .into_iter()
        .filter(|e| e.get("CPV").is_some_and(|c| c == &cpv))
        .peekable();
    if candidates.peek().is_none() {
        return false;
    }
    if let Some(want) = build_id.filter(|s| !s.is_empty()) {
        let narrowed: Vec<_> = candidates
            .filter(|e| e.get("BUILD_ID").is_some_and(|b| b == want))
            .collect();
        // A `BUILD_ID` matching nothing still leaves the plain `CPV` hit: Portage
        // resolves a plain string to the latest instance, so any surviving
        // instance keeps this true.
        if !narrowed.is_empty() {
            return true;
        }
    }
    true
}

/// The error for a merge-time missing binpkg whose output is already printed:
/// exit 1 with no resume notice and no `emerge:` line (a merge failure exits
/// via `FAILURE`, exactly like the digest arm).
pub(crate) fn binpkg_missing_failure(cpv: &str) -> String {
    format!("{cpv}: non-existent binary package")
}

/// Whether `e` is exactly one `binpkg_missing_failure` (same lone-
/// failure shape as [`is_binpkg_digest_failure`]).
pub(crate) fn is_binpkg_missing_failure(e: &str) -> bool {
    !e.contains('\n') && e.ends_with(": non-existent binary package")
}

/// Arguments of [`local_index_record`].
struct LocalIndexRecordArgs<'a> {
    pkgdir: &'a Path,
    category: &'a str,
    package: &'a str,
    version: &'a str,
    build_id: Option<&'a str>,
    binpkg_path: &'a Path,
}

/// The `<pkgdir>/Packages` stanza vouching for a local binpkg merge: the one
/// whose `CPV` matches, narrowed by `BUILD_ID`, else by the located file's
/// basename (a moved file still verifies against its own record). `None` when
/// no stanza vouches. A record without `SIZE` still returns here: "verifies
/// nothing" lives in `verify_binpkg_against_index`, not the lookup.
// Portage: lib/portage/dbapi/bintree.py, _get_digests
fn local_index_record(
    args: LocalIndexRecordArgs<'_>,
) -> Option<std::collections::HashMap<String, String>> {
    let LocalIndexRecordArgs {
        pkgdir,
        category,
        package,
        version,
        build_id,
        binpkg_path,
    } = args;
    let cpv = format!("{category}/{package}-{version}");
    let basename = binpkg_path.file_name()?.to_str()?;
    let mut candidates: Vec<std::collections::HashMap<String, String>> =
        portage_repo::read_packages_index(pkgdir)
            .into_iter()
            .filter(|e| e.get("CPV").is_some_and(|c| c == &cpv))
            .collect();
    if candidates.is_empty() {
        return None;
    }
    if candidates.len() > 1
        && let Some(want) = build_id.filter(|s| !s.is_empty())
    {
        candidates.retain(|e| e.get("BUILD_ID").is_some_and(|b| b == want));
    }
    if candidates.len() > 1 {
        candidates.retain(|e| {
            e.get("PATH")
                .and_then(|p| Path::new(p).file_name()?.to_str())
                .is_some_and(|b| b == basename)
        });
    }
    candidates.into_iter().next()
}

/// The on-disk binpkg for `<cat>/<package>-<version>` in `$PKGDIR`: the
/// single-instance `<cat>/<pf>.{tbz2,gpkg.tar}`, or the multi-instance
/// `<cat>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}` (a `<pn>/` subdir plus a
/// `-<build_id>` suffix). A `build_id` picks the exact file; without one the
/// `<pn>/` subdir is scanned (highest `<id>` first).
// Portage: lib/portage/dbapi/bintree.py, getname
pub(crate) fn resolve_local_binpkg(
    pkgdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
) -> Option<std::path::PathBuf> {
    let pf = format!("{package}-{version}");

    // Single-instance layout.
    for ext in ["tbz2", "gpkg.tar"] {
        let p = pkgdir.join(category).join(format!("{pf}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }

    // Multi-instance layout: `<cat>/<pn>/<pf>-<build_id>.<ext>`.
    let instance_dir = pkgdir.join(category).join(package);
    if let Some(build_id) = build_id.filter(|s| !s.is_empty()) {
        for ext in ["gpkg.tar", "xpak"] {
            let p = instance_dir.join(format!("{pf}-{build_id}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // Fallback: any `<pf>-<id>.{gpkg.tar,xpak}` in the instance dir,
    // highest numeric `<id>` first.
    let mut candidates: Vec<(u64, std::path::PathBuf)> =
        portage_util::read_dir_entries(&instance_dir)
            .ok()?
            .into_iter()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let stem = name
                    .strip_suffix(".gpkg.tar")
                    .or_else(|| name.strip_suffix(".xpak"))?;
                let id = stem.strip_prefix(&format!("{pf}-"))?;
                Some((id.parse::<u64>().ok()?, e.path()))
            })
            .collect();
    candidates.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    candidates.into_iter().map(|(_, p)| p).next()
}

/// Where a binrepo's packages are downloaded: its `location`, else (only when
/// `PORTAGE_BINHOST` is unset) `<root>/var/cache/binhost/<name>`, else `$PKGDIR`.
/// Portage roots the default at `EPREFIX`; under `ROOT` here so hermetic roots
/// never write to the host.
// Portage: lib/portage/binrepo/config.py (BinRepoConfigLoader), bintree.py (get_local_repo_location)
pub(crate) fn binrepo_download_dir(
    config: &portage_profile::Config,
    binrepo: &portage_profile::BinRepo,
    root: &Path,
    pkgdir: &Path,
) -> std::path::PathBuf {
    if let Some(location) = binrepo.location.as_deref() {
        return std::path::PathBuf::from(location);
    }
    if config
        .other_vars
        .get("PORTAGE_BINHOST")
        .is_some_and(|v| !v.trim().is_empty())
    {
        return pkgdir.to_path_buf();
    }
    root.join("var/cache/binhost").join(&binrepo.name)
}

/// Arguments of [`download_and_verify`].
pub(crate) struct DownloadAndVerifyArgs<'a> {
    pub(crate) sync_uri: &'a str,
    pub(crate) record: &'a std::collections::HashMap<String, String>,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) pkgdir: &'a Path,
    /// The resolved `FETCHCOMMAND` family and `PORTAGE_SSH_OPTS`, from the config.
    pub(crate) config: &'a portage_profile::Config,
}

/// The digest names Portage prints on a binary's verify line: the uppercase
/// checksum keys present in the index record plus `size`, ASCII-sorted
/// (`MD5 SHA1 size`). `None` without a parseable `SIZE`: then nothing is
/// verified and no line prints.
// Portage: _emerge/BinpkgVerifier.py (_start, _display_success); bintree.py, _get_digests
pub(crate) fn verified_digest_names(
    record: &std::collections::HashMap<String, String>,
) -> Option<Vec<String>> {
    record.get("SIZE")?.trim().parse::<u64>().ok()?;
    let mut names: Vec<String> = [
        "BLAKE2B", "BLAKE2S", "MD5", "SHA1", "SHA256", "SHA3_256", "SHA3_512", "SHA512",
    ]
    .iter()
    .filter(|k| record.get(**k).is_some_and(|v| !v.is_empty()))
    .map(|k| (*k).to_string())
    .collect();
    names.push("size".to_string());
    names.sort();
    Some(names)
}

/// Fetch `<sync_uri>/<PATH>` (or the default `<cat>/<pf>.tbz2`) into `$PKGDIR`,
/// then verify it against the index `SIZE` and the `MD5`/`SHA1` fields present.
/// A mismatch removes the file and fails.
// Portage: lib/portage/dbapi/bintree.py, gettbz2
pub(crate) fn download_and_verify(
    args: DownloadAndVerifyArgs<'_>,
) -> Result<std::path::PathBuf, String> {
    let DownloadAndVerifyArgs {
        sync_uri,
        record,
        category,
        package,
        version,
        pkgdir,
        config,
    } = args;
    let rel = record
        .get("PATH")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{category}/{package}-{version}.tbz2"));
    let final_dest = pkgdir.join(&rel);
    if let Some(parent) = final_dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let uri = format!("{}/{rel}", sync_uri.trim_end_matches('/'));
    // Downloaded as `<file>.partial`, renamed once verified.
    // Portage: _emerge/BinpkgFetcher.py, Binpkg.py (`_verifier_exit`)
    let mut partial = final_dest.clone().into_os_string();
    partial.push(".partial");
    let dest = std::path::PathBuf::from(partial);

    if let Some(local) = uri.strip_prefix("file://") {
        std::fs::copy(local, &dest).map_err(|e| format!("{local}: {e}"))?;
    } else {
        let commands = crate::fetch::fetch_commands_from_config(config);
        let ssh_opts = crate::fetch::portage_ssh_opts_from_config(config);
        portage_fetch::download_with_commands_stdout(portage_fetch::DownloadWithCommandsArgs {
            uri: &uri,
            dest: &dest,
            resume: false,
            distdir: dest.parent().unwrap_or(pkgdir),
            vars: portage_fetch::FetchCommandVars {
                digests: None,
                portage_ssh_opts: ssh_opts.as_deref(),
            },
            commands: &commands,
        })?;
    }

    if let Some(expected) = record.get("SIZE").and_then(|s| s.parse::<u64>().ok()) {
        let actual = std::fs::metadata(&dest)
            .map_err(|e| format!("{}: {e}", dest.display()))?
            .len();
        if actual != expected {
            let _ = std::fs::remove_file(&dest);
            return Err(format!(
                "{}: downloaded size {actual} != index SIZE {expected}",
                dest.display()
            ));
        }
    }

    // The `MD5`/`SHA1` fields are the md5/sha1 of the whole file, and every
    // digest present is checked; a mismatch removes the file and fails.
    let want_md5 = record.get("MD5").filter(|s| !s.is_empty()).cloned();
    let want_sha1 = record.get("SHA1").filter(|s| !s.is_empty()).cloned();
    if want_md5.is_some() || want_sha1.is_some() {
        let bytes = std::fs::read(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        if let Some(expected_md5) = want_md5 {
            use md5::Digest as _;
            let actual: String = md5::Md5::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !actual.eq_ignore_ascii_case(&expected_md5) {
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "{}: MD5 mismatch (index {expected_md5}, got {actual})",
                    dest.display()
                ));
            }
        }
        if let Some(expected_sha1) = want_sha1 {
            use sha1::Digest as _;
            let actual: String = sha1::Sha1::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !actual.eq_ignore_ascii_case(&expected_sha1) {
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "{}: SHA1 mismatch (index {expected_sha1}, got {actual})",
                    dest.display()
                ));
            }
        }
    }
    std::fs::rename(&dest, &final_dest).map_err(|e| format!("{}: {e}", final_dest.display()))?;
    Ok(final_dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::CandidateSource;
    use portage_repo::PretendOutcome;
    use portage_repo::find_remote_binpkg;
    use portage_util::TempDir;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fixtures_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-getbinpkg").keep()
    }

    /// Serves each `routes` entry over loopback HTTP, one response per
    /// connection, for `requests` connections total (the `Packages` fetch plus
    /// the run's binpkg fetches).
    fn serve(
        routes: HashMap<String, Vec<u8>>,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        serve_with_status(
            routes
                .into_iter()
                .map(|(p, b)| (p, ("200 OK".to_string(), b)))
                .collect(),
            requests,
        )
    }

    /// [`serve`] with an explicit status line per route, for fault injection;
    /// unlisted paths still 404.
    fn serve_with_status(
        routes: HashMap<String, (String, Vec<u8>)>,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = match routes.get(&path) {
                    Some((s, b)) => (s.clone(), b.clone()),
                    None => ("404 Not Found".to_string(), Vec::new()),
                };
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    fn packages_index(entries: &[&str]) -> Vec<u8> {
        // A genuine binhost index always carries both gated headers, so the
        // helper stamps both.
        let mut s = format!("TIMESTAMP: 0\nVERSION: 0\nPACKAGES: {}\n\n", entries.len());
        for e in entries {
            s.push_str(e);
            s.push_str("\n\n");
        }
        s.into_bytes()
    }

    fn graph_entry(package: &str, source: CandidateSource, version: &str) -> GraphEntry {
        GraphEntry {
            discovery: 0,
            category: "dev-libs".into(),
            package: package.into(),
            outcome: PretendOutcome::New {
                version: version.into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: vec![],
            required_by: vec![],
            source,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }

    #[test]
    fn resolve_local_binpkg_finds_the_multi_instance_layout() {
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");

        // Single-instance: `<cat>/<pf>.gpkg.tar`.
        std::fs::create_dir_all(pkgdir.join("media-fonts")).unwrap();
        let single = pkgdir.join("media-fonts/plain-1.gpkg.tar");
        std::fs::write(&single, b"x").unwrap();
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "plain", "1", None).as_deref(),
            Some(single.as_path())
        );

        // Multi-instance: `<cat>/<pn>/<pf>-<build_id>.gpkg.tar`. This is the
        // layout the lookup used to miss entirely.
        std::fs::create_dir_all(pkgdir.join("media-fonts/noto")).unwrap();
        let mi = pkgdir.join("media-fonts/noto/noto-20260901-1.gpkg.tar");
        std::fs::write(&mi, b"x").unwrap();
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "noto", "20260901", Some("1")).as_deref(),
            Some(mi.as_path()),
            "exact build_id"
        );
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "noto", "20260901", None).as_deref(),
            Some(mi.as_path()),
            "no build_id -> scan the <pn>/ subdir"
        );

        // Highest build_id wins when several instances are present.
        std::fs::write(
            pkgdir.join("media-fonts/noto/noto-20260901-4.gpkg.tar"),
            b"x",
        )
        .unwrap();
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "noto", "20260901", None)
                .unwrap()
                .file_name()
                .unwrap(),
            "noto-20260901-4.gpkg.tar"
        );

        assert!(resolve_local_binpkg(&pkgdir, "media-fonts", "absent", "1", Some("1")).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_one_binary_entry_merges_a_multi_instance_local_gpkg() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        // The fixture gpkg, placed in the multi-instance layout.
        std::fs::create_dir_all(pkgdir.join("dev-libs/gpkgreadpkg")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
            pkgdir.join("dev-libs/gpkgreadpkg/gpkgreadpkg-1.0-1.gpkg.tar"),
        )
        .unwrap();

        let mut entry = graph_entry("gpkgreadpkg", CandidateSource::Binary, "1.0");
        entry.build_id = Some("1".into());

        merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &Config::default(),
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("pt"),
            merge_options: &MergeOptions::default(),
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .expect("multi-instance local gpkg merges");

        assert!(
            root.join("var/db/pkg/dev-libs/gpkgreadpkg-1.0/CONTENTS")
                .is_file()
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_one_binary_entry_fails_a_truncated_index_vouched_binpkg_at_merge() {
        // End to end at the merge boundary: a truncated index-vouched binary is
        // selected at scan and fails at merge with `!!! Digest verification
        // failed:`, renaming the file, logging the block, and reporting `>>>
        // Failed to emerge ...` -- rc 1 with a clean root, before unpacking
        // anything. The `Err` is the silent sentinel, so the CLI boundary adds
        // no resume notice or `emerge:` line.
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
        let whole = std::fs::read(fixtures_root().join("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"))
            .unwrap();
        let dest = pkgdir.join("dev-libs/gpkgreadpkg-1.0.gpkg.tar");
        std::fs::write(&dest, &whole[..whole.len() / 2]).unwrap();
        use md5::Digest as _;
        let md5: String = md5::Md5::digest(&whole)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        std::fs::write(
            pkgdir.join("Packages"),
            format!(
                "TIMESTAMP: 0\n\nCPV: dev-libs/gpkgreadpkg-1.0\nSLOT: 0\nSIZE: {}\nMD5: {md5}\n_mtime_: 1\nPATH: dev-libs/gpkgreadpkg-1.0.gpkg.tar\n",
                whole.len()
            ),
        )
        .unwrap();

        // The scan half of the same shape: the truncated file is accepted on the
        // index's word, so resolution can select it.
        let scanned = crate::binpkg::populate_local_pkgdir(&pkgdir, true).expect("scan succeeds");
        assert_eq!(scanned.len(), 1, "{scanned:?}");

        let pt = tmp.join("pt");
        let entry = graph_entry("gpkgreadpkg", CandidateSource::Binary, "1.0");
        let err = merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &Config::default(),
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &pt,
            merge_options: &MergeOptions::default(),
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .expect_err("a truncated vouched binpkg must fail at merge");
        assert_eq!(
            err, "dev-libs/gpkgreadpkg-1.0: binpkg digest verification failed",
            "{err}"
        );
        assert!(
            is_binpkg_digest_failure(&err),
            "the CLI boundary must recognize the silent sentinel"
        );
        assert!(!dest.exists(), "the corrupt file is renamed away");
        let renamed: Vec<_> = portage_util::read_dir_paths(&pkgdir.join("dev-libs"))
            .unwrap()
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("gpkgreadpkg-1.0.gpkg.tar._checksum_failure_."))
            })
            .collect();
        assert_eq!(renamed.len(), 1, "one checksum-failure sibling");
        let rand = renamed[0]
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("gpkgreadpkg-1.0.gpkg.tar._checksum_failure_."))
            .unwrap_or("");
        assert_eq!(rand.len(), 8, "{renamed:?}");
        assert!(
            rand.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{renamed:?}"
        );
        // The digest block lands in the package `build.log` too, non-empty so it
        // is reported.
        let log = pt.join("portage/dev-libs/gpkgreadpkg-1.0/temp/build.log");
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            logged.contains("!!! Digest verification failed:")
                && logged.contains("!!! Reason: Failed on size verification"),
            "build.log holds the digest block: {log:?}"
        );
        // Clean root: nothing unpacked, no vdb entry, no merge marker.
        assert!(!root.join("var/db/pkg/dev-libs/gpkgreadpkg-1.0").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_one_binary_entry_aborts_a_stale_index_binary_like_portage() {
        // End to end at the merge boundary: a binary removed from `$PKGDIR` with
        // its stanza kept is selected from the trusted index and fails at merge
        // with the stale-index pair to stdout AND `build.log`, plus the `>>>
        // Failed to emerge ...` tail -- rc 1 with a clean root. The `Err` is the
        // silent sentinel. Used to fail with portuale's own `no binpkg file under
        // ...` error.
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
        // No binpkg file on disk at all -- only the stale stanza,
        // exactly like the bed's `$nopkg` after `find -delete`.
        std::fs::write(
            pkgdir.join("Packages"),
            "TIMESTAMP: 0\n\nCPV: dev-libs/packagepkg-1.0\nSLOT: 0\nSIZE: 12345\n_mtime_: 1\nPATH: dev-libs/packagepkg-1.0.tbz2\n",
        )
        .unwrap();

        // The scan half: the removed file's stanza rejoins the pool, so the
        // binary can be selected.
        let scanned = crate::binpkg::populate_local_pkgdir(&pkgdir, true).expect("scan succeeds");
        assert_eq!(scanned.len(), 1, "{scanned:?}");
        assert_eq!(
            scanned[0].get("CPV").map(String::as_str),
            Some("dev-libs/packagepkg-1.0")
        );

        let pt = tmp.join("pt");
        let entry = graph_entry("packagepkg", CandidateSource::Binary, "1.0");
        let err = merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &Config::default(),
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &pt,
            merge_options: &MergeOptions::default(),
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .expect_err("a stale-index binary must fail at merge, not fall back");
        assert_eq!(
            err, "dev-libs/packagepkg-1.0: non-existent binary package",
            "{err}"
        );
        assert!(
            is_binpkg_missing_failure(&err),
            "the CLI boundary must recognize the silent sentinel"
        );
        // The two `!!!` lines land in `build.log` too -- byte for byte Portage's
        // pair -- non-empty so it is reported.
        let log = pt.join("portage/dev-libs/packagepkg-1.0/temp/build.log");
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert_eq!(
            logged,
            "!!! Tried to use non-existent binary for 'dev-libs/packagepkg-1.0'\n\
             !!! Likely caused by an outdated index. Run 'emaint binhost -f'.\n",
            "build.log holds real's stale-index pair: {log:?}"
        );
        // Clean root: nothing unpacked, no vdb entry, no merge marker.
        assert!(!root.join("var/db/pkg/dev-libs/packagepkg-1.0").exists());
        assert!(!root.join("usr/share/packagepkg/hello.txt").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_one_binary_entry_reports_fetching_failed_without_an_index_stanza() {
        // Portage's other ENOENT arm -- the cpv is NOT in the index, so the text
        // is `!!! Fetching Binary failed` instead of the stale-index pair. Same
        // stdout + `build.log` + tail and the same silent sentinel.
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();

        let pt = tmp.join("pt");
        let entry = graph_entry("packagepkg", CandidateSource::Binary, "1.0");
        let err = merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &Config::default(),
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &pt,
            merge_options: &MergeOptions::default(),
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .expect_err("a stanza-less missing binary must fail at merge");
        assert_eq!(
            err, "dev-libs/packagepkg-1.0: non-existent binary package",
            "{err}"
        );
        assert!(is_binpkg_missing_failure(&err));
        let log = pt.join("portage/dev-libs/packagepkg-1.0/temp/build.log");
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert_eq!(
            logged, "!!! Fetching Binary failed for 'dev-libs/packagepkg-1.0'\n",
            "build.log holds real's fetching-failed line: {log:?}"
        );
        assert!(!root.join("var/db/pkg/dev-libs/packagepkg-1.0").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn failed_pkg_msg_matches_portage_failed_pkg_msg_bytes() {
        // Each message emits its own leading blank line; the log line carries
        // Portage's own leading space; ` for <root>` only when `ROOT != "/"`.
        // The oracle's tail is the `ROOT == "/"` row below, byte for byte.
        assert_eq!(
            failed_pkg_msg(
                "l32/faultpkg-1.0",
                Path::new("/"),
                Some(Path::new(
                    "/var/tmp/portage/l32/faultpkg-1.0/temp/build.log"
                ))
            ),
            "\n>>> Failed to emerge l32/faultpkg-1.0, Log file:\n\
             \n>>>  '/var/tmp/portage/l32/faultpkg-1.0/temp/build.log'\n"
        );
        assert_eq!(
            failed_pkg_msg(
                "dev-libs/packagepkg-1.0",
                Path::new("/tmp/r/root"),
                Some(Path::new(
                    "/tmp/r/pt/portage/dev-libs/packagepkg-1.0/temp/build.log"
                ))
            ),
            "\n>>> Failed to emerge dev-libs/packagepkg-1.0 for /tmp/r/root, Log file:\n\
             \n>>>  '/tmp/r/pt/portage/dev-libs/packagepkg-1.0/temp/build.log'\n"
        );
        assert_eq!(
            failed_pkg_msg("dev-libs/packagepkg-1.0", Path::new("/tmp/r/root"), None),
            "\n>>> Failed to emerge dev-libs/packagepkg-1.0 for /tmp/r/root\n"
        );
        // Only a lone failure is silent: a `--keep-going` combined
        // message keeps the normal tail.
        assert!(!is_binpkg_digest_failure(
            "2 package(s) failed to merge (--keep-going):\n  dev-libs/a-1.0: binpkg digest verification failed"
        ));
    }

    /// A writable copy of the committed GnuPG test keyring (see `binpkg.rs`).
    /// `gpg` refuses a homedir it doesn't own, so the tree itself is never used.
    fn test_gpg_home() -> std::path::PathBuf {
        fn copy_dir(src: &std::path::Path, dest: &std::path::Path) {
            std::fs::create_dir_all(dest).unwrap();
            for entry in portage_util::read_dir_entries(src).unwrap() {
                let to = dest.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_dir(&entry.path(), &to);
                } else {
                    std::fs::copy(entry.path(), &to).unwrap();
                }
            }
        }
        let dest = tempdir().join("gpg-home");
        copy_dir(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/helpers/gpg-keyring"),
            &dest,
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700)).unwrap();
        dest
    }

    fn test_gpg_verify(home: &std::path::Path) -> crate::binpkg::GpgVerify {
        crate::binpkg::GpgVerify {
            verify_signature: true,
            request_signature: false,
            base_command: crate::binpkg::DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string(),
            gpg_home: home.display().to_string(),
        }
    }

    #[test]
    fn merge_binpkg_verifies_a_signed_gpkg_against_the_test_keyring() {
        // The committed signed fixture merges end to end with the test keyring:
        // policy enforced, image + metadata land in the vdb like any other merge.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let home = test_gpg_home();
        let options = MergeOptions {
            gpg_verify: test_gpg_verify(&home),
            ..MergeOptions::default()
        };

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar"),
            &root,
            &tmp.join("portage_tmpdir"),
            &options,
        )
        .expect("signed gpkg merge succeeds");
        assert_eq!(status, 0);

        let vdb = root.join("var/db/pkg/dev-libs/gpgsignedpkg-1.0");
        assert!(vdb.join("CONTENTS").is_file());
        assert!(
            std::fs::read_to_string(vdb.join("CONTENTS"))
                .unwrap()
                .contains(" /hello.txt "),
            "the image file merged"
        );
        assert!(root.join("hello.txt").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_rejects_a_tampered_signed_gpkg() {
        // Flip a byte of the signed container's `image.tar.gz` (repacked so the
        // tar framing stays parseable): the merge must fail on the `.sig` before
        // anything is unpacked.
        let tmp = tempdir();
        let outer = tmp.join("outer");
        std::fs::create_dir_all(&outer).unwrap();
        let src = fixtures_root().join("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar");
        let status = std::process::Command::new("tar")
            .args(["-xf"])
            .arg(&src)
            .args(["-C"])
            .arg(&outer)
            .status()
            .unwrap();
        assert!(status.success());
        let member = outer.join("gpgsignedpkg-1.0/image.tar.gz");
        let mut bytes = std::fs::read(&member).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xff;
        std::fs::write(&member, bytes).unwrap();
        let tampered = tmp.join("tampered.gpkg.tar");
        // List the prefix's files, not the prefix dir: a `<prefix>/` directory
        // member would be refused by the one-level structure check.
        let mut pack = std::process::Command::new("tar");
        pack.args(["-cf"]).arg(&tampered).args(["-C"]).arg(&outer);
        for file in std::fs::read_dir(outer.join("gpgsignedpkg-1.0")).unwrap() {
            pack.arg(format!(
                "gpgsignedpkg-1.0/{}",
                file.unwrap().file_name().to_string_lossy()
            ));
        }
        let status = pack.status().unwrap();
        assert!(status.success());

        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let home = test_gpg_home();
        let options = MergeOptions {
            gpg_verify: test_gpg_verify(&home),
            ..MergeOptions::default()
        };
        let err =
            ebuild_merge::merge_binpkg(&tampered, &root, &tmp.join("portage_tmpdir"), &options)
                .unwrap_err();
        assert!(err.contains("GnuPG verification failed"), "{err}");
        assert!(
            !root.join("var/db/pkg/dev-libs/gpgsignedpkg-1.0").exists(),
            "nothing is merged on a signature failure"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_installs_a_real_tbz2_into_the_vdb() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let binpkg = fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2");

        let status = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        // The image file landed under ROOT.
        let hello = root.join("usr/share/packagepkg/hello.txt");
        assert!(hello.is_file(), "{}", hello.display());
        assert!(std::fs::read_to_string(&hello).unwrap().contains("hello"));

        // A complete vdb entry, with CONTENTS naming the file and the
        // binpkg's own RDEPEND copied through.
        let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
        assert!(vdb.join("CONTENTS").is_file());
        assert!(
            std::fs::read_to_string(vdb.join("CONTENTS"))
                .unwrap()
                .contains("/usr/share/packagepkg/hello.txt")
        );
        assert_eq!(
            std::fs::read_to_string(vdb.join("RDEPEND")).unwrap().trim(),
            "dev-libs/samepkg"
        );
        assert_eq!(
            std::fs::read_to_string(vdb.join("SLOT")).unwrap().trim(),
            "0"
        );
        // The saved env + ebuild are kept in the vdb now (needed for
        // pkg_preinst/pkg_postinst). This fixture's DEFINED_PHASES is `install`
        // only, so no hook ran.
        assert!(vdb.join("environment.bz2").is_file());
        assert!(vdb.join("packagepkg-1.0.ebuild").is_file());

        // The vdb entry carries a consolidated `metadata` file: `#format=1`
        // header, sorted `KEY=value` lines, `#dir_mtime=` last and matching the
        // entry dir's own `st_mtime_ns` (a stale value is rejected).
        let metadata = std::fs::read_to_string(vdb.join("metadata")).expect("metadata file");
        assert!(metadata.starts_with("#format=1\n"), "{metadata}");
        assert!(metadata.contains("\nSLOT=0\n"), "{metadata}");
        assert!(
            metadata.contains("\nRDEPEND=dev-libs/samepkg\n"),
            "{metadata}"
        );
        let dir_mtime_line = metadata
            .lines()
            .find_map(|l| l.strip_prefix("#dir_mtime="))
            .expect("#dir_mtime= line");
        assert_eq!(
            metadata.lines().last(),
            Some(format!("#dir_mtime={dir_mtime_line}").as_str()),
            "#dir_mtime= must be the final line"
        );
        {
            use std::os::unix::fs::MetadataExt as _;
            let st = std::fs::metadata(&vdb).unwrap();
            let ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;
            assert_eq!(
                dir_mtime_line,
                ns.to_string(),
                "recorded #dir_mtime= must equal the entry dir's st_mtime_ns"
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_gpkg_strips_the_image_prefix_and_records_binpkgmd5_and_metadata() {
        // Regression: a gpkg merge used to write `/image`-prefixed CONTENTS, no
        // `BINPKGMD5`, no consolidated `metadata` -- all blocking `emerge -C`.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let binpkg = fixtures_root().join("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar");

        let status = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("gpkg merge succeeds");
        assert_eq!(status, 0);

        let vdb = root.join("var/db/pkg/dev-libs/gpkgreadpkg-1.0");
        let contents = std::fs::read_to_string(vdb.join("CONTENTS")).unwrap();
        assert!(
            !contents.contains("/image"),
            "CONTENTS must not carry the gpkg `image/` prefix: {contents}"
        );
        assert!(contents.contains(" /hello.txt "), "{contents}");
        assert!(
            root.join("hello.txt").is_file(),
            "the image file merged at the real path, not under /image"
        );

        // BINPKGMD5 is the md5 of the whole binpkg file.
        let want_md5 = {
            use md5::Digest as _;
            let mut h = md5::Md5::new();
            h.update(std::fs::read(&binpkg).unwrap());
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(
            std::fs::read_to_string(vdb.join("BINPKGMD5"))
                .unwrap()
                .trim(),
            want_md5
        );

        // Consolidated metadata file present and stamped.
        let metadata = std::fs::read_to_string(vdb.join("metadata")).expect("metadata file");
        assert!(metadata.starts_with("#format=1\n"), "{metadata}");
        assert!(
            metadata.lines().last().unwrap().starts_with("#dir_mtime="),
            "{metadata}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_collision_protect_aborts_when_another_package_owns_the_file() {
        // `_collision_protect` (shared with the source merge): a pre-existing vdb
        // entry owning the same path aborts the binpkg merge rather than
        // overwriting it.
        let tmp = tempdir();
        let root = tmp.join("root");
        let shared = root.join("usr/share/packagepkg/hello.txt");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, "owned by collideowner\n").unwrap();

        let owner = root.join("var/db/pkg/dev-libs/collideowner-1.0");
        std::fs::create_dir_all(&owner).unwrap();
        std::fs::write(owner.join("SLOT"), "0\n").unwrap();
        std::fs::write(owner.join("PF"), "collideowner-1.0\n").unwrap();
        std::fs::write(owner.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(
            owner.join("CONTENTS"),
            "obj /usr/share/packagepkg/hello.txt 0000 0\n",
        )
        .unwrap();

        let options = MergeOptions {
            collision_protect: true,
            ..MergeOptions::default()
        };
        let err = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &root,
            &tmp.join("pt"),
            &options,
        )
        .unwrap_err();
        assert!(err.contains("dev-libs/collideowner-1.0"), "{err}");
        assert!(err.contains("/usr/share/packagepkg/hello.txt"), "{err}");
        assert!(err.contains("NOT merged"), "{err}");
        // Nothing was written: the file is untouched, no vdb entry.
        assert_eq!(
            std::fs::read_to_string(&shared).unwrap(),
            "owned by collideowner\n"
        );
        assert!(!root.join("var/db/pkg/dev-libs/packagepkg-1.0").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_runs_pkg_preinst_and_pkg_postinst_from_the_saved_env() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let binpkg = fixtures_root().join("pkgdir/dev-libs/binpkgphasepkg-1.0.tbz2");

        let status = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0, "both hooks exited 0");

        // The image landed, vdb entry written.
        assert!(root.join("usr/share/binpkgphasepkg/payload.txt").is_file());
        assert!(
            root.join("var/db/pkg/dev-libs/binpkgphasepkg-1.0/CONTENTS")
                .is_file()
        );

        // The fixture's preinst `die`s if the payload is merged and postinst
        // `die`s if it is not -- so both lines prove the treewalk() ordering
        // (preinst before the copy, postinst after) held.
        let phases = root.join("var/lib/binpkgphasepkg.phases");
        assert_eq!(
            std::fs::read_to_string(&phases).unwrap(),
            "preinst\npostinst\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_runs_pkg_setup_then_preinst_then_postinst() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        assert!(root.join("usr/share/binpkgrmpkg/payload-1.0.txt").is_file());
        // Each hook appends its own `<phase>-<PVR>` line, in call order.
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
            "setup-1.0\npreinst-1.0\npostinst-1.0\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// `pkg_pretend` runs for a binary package too, folded into `merge_binpkg`'s
    /// hook chain before `pkg_setup`; the fixture proves it ran at the right point.
    #[test]
    fn merge_binpkg_runs_pkg_pretend_before_pkg_setup() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        assert!(
            root.join("usr/share/binpkgpretendpkg/payload.txt")
                .is_file()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgpretendpkg.log")).unwrap(),
            "pretend\nsetup\npreinst\npostinst\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The vdb `environment.bz2` is regenerated from the live merge-time
    /// environment, so it carries merge-time `FEATURES`, not build-time ones.
    #[test]
    fn merge_binpkg_regenerates_a_curated_merge_time_vdb_environment() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let opts = MergeOptions {
            features: "mergetimefeat splitdebug".to_string(),
            ..MergeOptions::default()
        };
        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &opts,
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        let env_bz2 = root.join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0/environment.bz2");
        let out = std::process::Command::new("bzip2")
            .args(["-dc", "--"])
            .arg(&env_bz2)
            .output()
            .expect("bunzip2 the vdb environment");
        let env = String::from_utf8_lossy(&out.stdout);

        assert!(
            env.contains(r#"FEATURES="mergetimefeat splitdebug""#),
            "vdb environment must carry the merge-time FEATURES, got:\n{}",
            env.lines()
                .filter(|l| l.contains("FEATURES="))
                .collect::<Vec<_>>()
                .join("\n")
        );
        // The brush-wrapper's own `___`-prefixed locals must not leak.
        assert!(
            !env.contains("___sfe_") && !env.contains("___save_and_filter_ebuild_env"),
            "the env-regeneration wrapper's internals leaked into the vdb env"
        );
        // The phase env is filtered to the whitelist, so a non-whitelisted
        // process-env var never reaches the regenerated vdb env.
        assert!(
            std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
            "test precondition: CARGO_MANIFEST_DIR is set under `cargo test`"
        );
        assert!(
            !env.contains("CARGO_MANIFEST_DIR"),
            "a non-whitelisted process-env var leaked into the vdb environment"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A binary merge's calling env is narrowed to the whitelist, so a
    /// harness/portuale-only key in `build_env` never reaches the vdb env.
    #[test]
    fn merge_binpkg_filters_non_whitelisted_build_env_from_the_vdb_environment() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let opts = MergeOptions {
            features: "mergetimefeat".to_string(),
            build_env: vec![
                (
                    "CARGO_MANIFEST_DIR".to_string(),
                    "/tmp/not-whitelisted".to_string(),
                ),
                ("L1_SKIP_PORTAGE_UPGRADE".to_string(), "0".to_string()),
                ("FEATURES".to_string(), "mergetimefeat".to_string()),
            ],
            ..MergeOptions::default()
        };
        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &opts,
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        let env_bz2 = root.join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0/environment.bz2");
        let out = std::process::Command::new("bzip2")
            .args(["-dc", "--"])
            .arg(&env_bz2)
            .output()
            .expect("bunzip2 the vdb environment");
        let env = String::from_utf8_lossy(&out.stdout);
        assert!(
            !env.contains("CARGO_MANIFEST_DIR") && !env.contains("L1_SKIP_PORTAGE_UPGRADE"),
            "non-whitelisted build_env keys must be filtered from the vdb env"
        );
        assert!(
            env.contains(r#"FEATURES="mergetimefeat""#),
            "whitelisted FEATURES must survive the filter"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_replace_runs_the_replaced_versions_pkg_prerm_and_pkg_postrm() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let portage_tmpdir = tmp.join("portage_tmpdir");

        // Install 1.0, then merge 2.0 over it (same slot).
        ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("1.0 merges");
        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-2.0.tbz2"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("2.0 merges");
        assert_eq!(status, 0);

        // 2.0 replaced 1.0 in the vdb; 1.0's own file is unmerged.
        assert!(
            root.join("var/db/pkg/dev-libs/binpkgrmpkg-2.0/CONTENTS")
                .is_file()
        );
        assert!(!root.join("var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists());
        assert!(root.join("usr/share/binpkgrmpkg/payload-2.0.txt").is_file());
        assert!(!root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists());

        // The full interleaving: 2.0 setup+preinst, then 1.0's prerm+postrm (from
        // 1.0's own vdb-stored environment, inside the replace loop), then 2.0
        // postinst.
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
            "setup-1.0\npreinst-1.0\npostinst-1.0\n\
             setup-2.0\npreinst-2.0\nprerm-1.0\npostrm-1.0\npostinst-2.0\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_replaces_a_same_slot_installed_version() {
        let tmp = tempdir();
        let root = tmp.join("root");

        // An older same-slot version already installed: it owns one file the new
        // binpkg also ships (a shared path) and one it does not (a genuine orphan).
        let pkgshare = root.join("usr/share/packagepkg");
        std::fs::create_dir_all(&pkgshare).unwrap();
        std::fs::write(pkgshare.join("hello.txt"), "old hello\n").unwrap();
        std::fs::write(pkgshare.join("old-only.txt"), "gone after upgrade\n").unwrap();

        let installed = root.join("var/db/pkg/dev-libs/packagepkg-0.9");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("SLOT"), "0\n").unwrap();
        std::fs::write(installed.join("COUNTER"), "1").unwrap();
        std::fs::write(installed.join("PF"), "packagepkg-0.9\n").unwrap();
        std::fs::write(installed.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(
            installed.join("CONTENTS"),
            "dir /usr\ndir /usr/share\ndir /usr/share/packagepkg\n\
             obj /usr/share/packagepkg/hello.txt 0000 0\n\
             obj /usr/share/packagepkg/old-only.txt 0000 0\n",
        )
        .unwrap();

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &root,
            &tmp.join("pt"),
            &MergeOptions::default(),
        )
        .expect("replace merge succeeds");
        assert_eq!(status, 0);

        // The new version is in the vdb; the old one is gone.
        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file()
        );
        assert!(
            !root.join("var/db/pkg/dev-libs/packagepkg-0.9").exists(),
            "the replaced version's vdb entry is removed"
        );

        // A file only the old version owned is unmerged; a file the new
        // version now owns survives with the new version's content.
        assert!(
            !pkgshare.join("old-only.txt").exists(),
            "the orphaned file is unmerged"
        );
        let hello = pkgshare.join("hello.txt");
        assert!(
            hello.is_file(),
            "the shared file the new version owns stays"
        );
        assert!(std::fs::read_to_string(&hello).unwrap().contains("hello"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_and_verify_runs_the_configured_fetchcommand_into_partial() {
        // Portage 3.0.82.2 (LLM/evidence/2026-10-20-b428, `f-fetchcommand`): the
        // FETCHCOMMAND runs with DISTDIR=<dir>, FILE=<name>.partial, then the
        // file is renamed once verified.
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");
        let body =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let mut routes = HashMap::new();
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), body.clone());
        let (base, _h) = serve(routes, 4);
        let marker = tmp.join("ran");
        let script = tmp.join("fc.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"$1 $2\" > {}\nexec wget -q -O \"$1/$2\" \"$3\"\n",
                marker.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut config = Config::default();
        config.other_vars.insert(
            "FETCHCOMMAND".to_string(),
            format!("{} ${{DISTDIR}} ${{FILE}} ${{URI}}", script.display()),
        );
        let record: HashMap<String, String> =
            [("SIZE", "4618"), ("PATH", "dev-libs/packagepkg-1.0.tbz2")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        let got = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &record,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &config,
        })
        .unwrap();
        assert_eq!(got, pkgdir.join("dev-libs/packagepkg-1.0.tbz2"));
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            format!(
                "{} packagepkg-1.0.tbz2.partial",
                pkgdir.join("dev-libs").display()
            )
        );
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2.partial").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn verified_digest_names_sort_uppercase_hashes_before_size() {
        let rec = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(
            verified_digest_names(&rec(&[("SIZE", "1"), ("SHA1", "x"), ("MD5", "y")])),
            Some(vec!["MD5".into(), "SHA1".into(), "size".into()])
        );
        assert_eq!(
            verified_digest_names(&rec(&[("SIZE", "1")])),
            Some(vec!["size".into()])
        );
        // No parseable SIZE: Portage verifies nothing and prints no line.
        assert_eq!(verified_digest_names(&rec(&[("MD5", "y")])), None);
    }

    #[test]
    fn binrepo_download_dir_follows_location_then_binhost_then_default() {
        let root = Path::new("/r");
        let pkgdir = Path::new("/p");
        let mut repo = portage_profile::BinRepo {
            name: "bh".into(),
            sync_uri: "http://x".into(),
            priority: 0,
            location: Some("/loc".into()),
            verify_signature: true,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let mut config = Config::default();
        assert_eq!(
            binrepo_download_dir(&config, &repo, root, pkgdir),
            Path::new("/loc")
        );
        repo.location = None;
        assert_eq!(
            binrepo_download_dir(&config, &repo, root, pkgdir),
            Path::new("/r/var/cache/binhost/bh")
        );
        config
            .other_vars
            .insert("PORTAGE_BINHOST".into(), "http://x".into());
        assert_eq!(binrepo_download_dir(&config, &repo, root, pkgdir), pkgdir);
    }

    #[test]
    fn download_and_verify_fetches_then_size_and_md5_checks() {
        // Committed fixture .tbz2 digests (`md5sum`/`sha1sum`, not invented).
        const FIXTURE_MD5: &str = "54cab52a68eda02d7d41561b8f7d318a";
        const FIXTURE_SHA1: &str = "750847f2903fcc4f84bc657c6bb172501c4db490";
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");
        let body =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let mut routes = HashMap::new();
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), body.clone());
        let (base, _h) = serve(routes, 4);

        let record = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };

        // SIZE + matching MD5 + matching SHA1 -> ok.
        let ok = record(&[
            ("SIZE", "4618"),
            ("MD5", FIXTURE_MD5),
            ("SHA1", FIXTURE_SHA1),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let got = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &ok,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &Config::default(),
        })
        .unwrap();
        assert_eq!(got, pkgdir.join("dev-libs/packagepkg-1.0.tbz2"));
        assert_eq!(std::fs::metadata(&got).unwrap().len(), 4618);

        std::fs::remove_file(&got).unwrap();

        // Wrong SIZE -> rejected, file removed.
        let bad_size = record(&[("SIZE", "9999"), ("PATH", "dev-libs/packagepkg-1.0.tbz2")]);
        let err = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &bad_size,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &Config::default(),
        })
        .unwrap_err();
        assert!(err.contains("!= index SIZE 9999"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2.partial").exists());

        // Right SIZE, wrong MD5 -> rejected, file removed.
        let bad_md5 = record(&[
            ("SIZE", "4618"),
            ("MD5", "00000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &bad_md5,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &Config::default(),
        })
        .unwrap_err();
        assert!(err.contains("MD5 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());

        // Right SIZE + MD5, wrong SHA1 -> rejected, file removed (every digest
        // present is verified, not just MD5).
        let bad_sha1 = record(&[
            ("SIZE", "4618"),
            ("MD5", FIXTURE_MD5),
            ("SHA1", "0000000000000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &bad_sha1,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &Config::default(),
        })
        .unwrap_err();
        assert!(err.contains("SHA1 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_binhost_indexes_decompresses_a_packages_gz() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let plain = b"TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/foo-1.0\n\n".to_vec();
        // Genuine `gzip` output of `plain`.
        let mut gz_child = std::process::Command::new("gzip")
            .arg("-c")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::Write;
            gz_child.stdin.take().unwrap().write_all(&plain).unwrap();
        }
        let gz = gz_child.wait_with_output().unwrap().stdout;

        let mut routes = HashMap::new();
        routes.insert("/Packages.gz".to_string(), gz);
        let (base, _h) = serve(routes, 1);

        let binrepo = BinRepo {
            name: "test".to_string(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        refresh_binhost_indexes(std::slice::from_ref(&binrepo), &root, false, &[], &[]);
        let cached = binrepo.packages_dir(&root).join("Packages");
        // The promoted index is cached with a fresh DOWNLOAD_TIMESTAMP header, so
        // it differs from the served bytes by exactly that line.
        let (header, entries) =
            portage_repo::parse_packages_index(&std::fs::read_to_string(&cached).unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("CPV").map(String::as_str),
            Some("dev-libs/foo-1.0")
        );
        assert!(
            header.contains_key("DOWNLOAD_TIMESTAMP"),
            "promotion stamps DOWNLOAD_TIMESTAMP"
        );
        assert!(!binrepo.packages_dir(&root).join("Packages.gz").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_binhost_indexes_warns_immediately_on_a_corrupt_packages_gz() {
        // A downloaded-but-corrupt `Packages.gz` must NOT fall back to plain
        // `Packages`: Portage's `gzip.BadGzipFile` subclasses `OSError`
        // (verified against the stdlib), escaping to the outer handler. The stub
        // serves garbage for `.gz` and a valid plain index; no cache file may be
        // left at all.
        let tmp = tempdir();
        let root = tmp.join("root");
        let plain = b"TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 0\n\n".to_vec();
        let mut routes = HashMap::new();
        routes.insert("/Packages.gz".to_string(), b"this is not gzip\n".to_vec());
        routes.insert("/Packages".to_string(), plain);
        // The `.gz` attempt plus the never-made plain attempt's slot
        // (the helper thread just blocks on accept until teardown).
        let (base, _h) = serve(routes, 2);
        let binrepo = BinRepo {
            name: "test".to_string(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        refresh_binhost_indexes(std::slice::from_ref(&binrepo), &root, false, &[], &[]);
        assert!(
            !binrepo.packages_dir(&root).join("Packages").exists(),
            "a corrupt Packages.gz warns with no plain-Packages fallback"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_warns_with_the_untrimmed_sync_uri_but_fetches_stripped_urls() {
        // The warning keeps the configured trailing slash while fetches use the
        // stripped form: the stub 500s only stripped `/sub/...` paths, so the
        // `HTTP Error 500` detail proves the fetches hit stripped URLs and the
        // `from '.../sub/'` line proves the warning kept the slash.
        let tmp = tempdir();
        let root = tmp.join("root");
        let mut routes = HashMap::new();
        for path in ["/sub/Packages.gz", "/sub/Packages.zst", "/sub/Packages"] {
            routes.insert(
                path.to_string(),
                (
                    "500 Internal Server Error".to_string(),
                    b"stub 500\n".to_vec(),
                ),
            );
        }
        // One connection per attempt: Packages.gz + Packages.zst +
        // Packages (wget does not retry a 500).
        let (base, _h) = serve_with_status(routes, 3);
        let binrepo = BinRepo {
            name: "trail".to_string(),
            sync_uri: format!("{base}/sub/"),
            priority: 50,
            location: None,
            verify_signature: false,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let warning = refresh_one_binrepo(
            &binrepo,
            &root,
            false,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        )
        .stderr_warning
        .expect("a 500ing binhost warns");
        assert!(
            warning.contains(&format!("from '{base}/sub/'")),
            "warning keeps the configured trailing slash: {warning}"
        );
        assert!(
            warning.contains("HTTP Error 500: Internal Server Error"),
            "fetches hit the stripped URLs and recovered the status: {warning}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn wget_http_error_recovers_urllibs_http_error_shape() {
        // The shaped detail is byte-identical whenever the server sent a reason
        // phrase (verified against the stdlib); this transcript is a live `wget`
        // capture, not hand-written.
        let transcript = "--2026-09-27 06:19:14--  http://127.0.0.1:18765/Packages\n\
             Connecting to 127.0.0.1:18765... connected.\n\
             HTTP request sent, awaiting response... 500 Internal Server Error\n\
             2026-09-27 06:19:14 ERROR 500: Internal Server Error.\n";
        assert_eq!(
            wget_http_error(transcript).as_deref(),
            Some("HTTP Error 500: Internal Server Error")
        );
        // No response line (DNS failure, refused connection, ...): Portage
        // would print its own `URLError` string, which wget cannot reproduce --
        // the caller falls back to the fetch summary.
        assert_eq!(
            wget_http_error("wget: unable to resolve host address 'example.invalid'\n"),
            None
        );
        assert_eq!(wget_http_error(""), None);
    }

    #[test]
    fn hide_binhost_passwd_masks_only_a_userinfo_password() {
        // Portage's `_hide_url_passwd` (`//user:secret@` -> `//user:*password*@`),
        // as the same single regex substitution.
        assert_eq!(
            hide_binhost_passwd("http://user:secret@host:1234/path"),
            "http://user:*password*@host:1234/path"
        );
        assert_eq!(
            hide_binhost_passwd("http://127.0.0.1:18765"),
            "http://127.0.0.1:18765"
        );
        assert_eq!(
            hide_binhost_passwd("http://user@host/"),
            "http://user@host/"
        );
        // A space inside the password is left alone: `[^@\s]+` cannot span it,
        // so the regex never matches.
        assert_eq!(
            hide_binhost_passwd("http://user:pass word@host/"),
            "http://user:pass word@host/"
        );
        // A `/` inside the user is still masked: `[^:\s]+` allows it.
        assert_eq!(
            hide_binhost_passwd("http://a/b:c@host/"),
            "http://a/b:*password*@host/"
        );
    }

    #[test]
    fn binhost_fetch_warning_matches_portages_two_line_shape() {
        // The leading blank line, two `!!!` lines, trailing blank line -- all on
        // stderr -- for the bed's all-500ing stub.
        assert_eq!(
            binhost_fetch_warning(
                "l32-500",
                "http://127.0.0.1:18765",
                "HTTP Error 500: Internal Server Error"
            ),
            "\n\n!!! [l32-500] Error fetching binhost package info from \
             'http://127.0.0.1:18765'\n\
             !!! [l32-500] HTTP Error 500: Internal Server Error\n\n"
        );
    }

    #[test]
    fn refresh_binhost_indexes_warns_and_continues_on_a_500ing_binhost() {
        // Every index URL 500s: the refresh must not fail and must leave no cache
        // file behind, so resolution proceeds against the local pool. (Stderr
        // itself is not capturable from a unit test; the run log shows the pair
        // and no wget transcript.)
        let tmp = tempdir();
        let root = tmp.join("root");
        let mut routes = HashMap::new();
        for path in ["/Packages.gz", "/Packages.zst", "/Packages"] {
            routes.insert(
                path.to_string(),
                (
                    "500 Internal Server Error".to_string(),
                    b"stub 500\n".to_vec(),
                ),
            );
        }
        // One connection per attempt: Packages.gz + Packages.zst +
        // Packages (wget does not retry a 500).
        let (base, _h) = serve_with_status(routes, 3);
        let binrepo = BinRepo {
            name: "l32-500".to_string(),
            sync_uri: base.clone(),
            priority: 50,
            location: None,
            verify_signature: false,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        refresh_binhost_indexes(std::slice::from_ref(&binrepo), &root, false, &[], &[]);
        assert!(
            !binrepo.packages_dir(&root).join("Packages").exists(),
            "a failed refresh leaves no cache file for the resolver to trust"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Remote versions of `package` the resolver would see for
    /// `binrepos` under `root` -- i.e. the live-refresh override when a
    /// refresh ran, else the on-disk index.
    fn remote_versions(binrepos: &[BinRepo], root: &Path, package: &str) -> Vec<String> {
        let local = portage_repo::BinaryIndex::from_entries(Vec::new());
        portage_repo::list_remote_binary_candidates(binrepos, root, &local, "dev-libs", package)
            .into_iter()
            .map(|c| c.version)
            .collect()
    }

    #[test]
    fn refresh_pretend_reads_a_file_binhost_with_an_unwritable_edb_cache() {
        file_binhost_unwritable_cache_case(true);
    }

    #[test]
    fn refresh_real_merge_reads_a_file_binhost_with_an_unwritable_edb_cache() {
        file_binhost_unwritable_cache_case(false);
    }

    /// The remote index is fetched first and a cache-write failure ignored -- no
    /// `create_dir_all` before fetching, in `--pretend` exactly like in a
    /// non-pretend merge. With `<EROOT>/var/cache/edb` unwritable the refresh
    /// must still resolve from the in-memory index, warn about nothing, and
    /// write no cache file.
    fn file_binhost_unwritable_cache_case(pretend: bool) {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir();
        let root = tmp.join("root");
        let binhost = tmp.join("binhost");
        std::fs::create_dir_all(&binhost).unwrap();
        std::fs::write(
            binhost.join("Packages"),
            packages_index(&[
                "CPV: dev-libs/remotebinpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo",
            ]),
        )
        .unwrap();
        // Unwritable *ancestor*: the fetch must run before any mkdir, and the
        // write failure must be ignored like Portage.
        let edb = root.join("var/cache/edb");
        std::fs::create_dir_all(&edb).unwrap();
        std::fs::set_permissions(&edb, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(edb.join(".portuale-probe"), b"").is_ok() {
            let _ = std::fs::remove_file(edb.join(".portuale-probe"));
            std::fs::set_permissions(&edb, std::fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!("skipping unwritable-cache case: test user writes through 0o555 (root?)");
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        }
        let binrepo = BinRepo {
            name: "filecache".to_string(),
            sync_uri: format!("file://{}", binhost.display()),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            pretend,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert!(
            outcome.stderr_warning.is_none(),
            "pretend={pretend}: an unwritable cache dir is ignored silently like real: {:?}",
            outcome.stderr_warning
        );
        assert!(
            outcome.stdout_note.is_none(),
            "pretend={pretend}: a fresh fetch carries no skip note: {:?}",
            outcome.stdout_note
        );
        assert_eq!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "remotebinpkg"),
            vec!["1.0"],
            "pretend={pretend}: the run resolves from the in-memory index",
        );
        assert!(
            !edb_cache_packages_file(&root, &binrepo.sync_uri).is_file(),
            "pretend={pretend}: no cache file is fabricated on failure",
        );
        std::fs::set_permissions(&edb, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_pretend_failure_falls_back_to_the_stale_cache() {
        // On a fetch failure `--pretend` keeps the stale copy with the fallback
        // note.
        let tmp = tempdir();
        let root = tmp.join("root");
        let stale = "TIMESTAMP: 5\nPACKAGES: 1\n\nCPV: dev-libs/stalepkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let mut routes = HashMap::new();
        for path in ["/Packages.gz", "/Packages.zst", "/Packages"] {
            routes.insert(
                path.to_string(),
                (
                    "500 Internal Server Error".to_string(),
                    b"stub 500\n".to_vec(),
                ),
            );
        }
        let (base, _h) = serve_with_status(routes, 3);
        let binrepo = BinRepo {
            name: "stale500".to_string(),
            sync_uri: base.clone(),
            priority: 50,
            location: None,
            verify_signature: false,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        // A stale cache with no TTL headers (the TTL check must not fire).
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            true,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert!(
            outcome
                .stderr_warning
                .is_some_and(|w| w.contains("Error fetching binhost package info")),
            "the fetch failure still warns",
        );
        assert_eq!(
            outcome.stdout_note.as_deref(),
            Some(
                "[stale500] Local copy of unavailable remote index will be used due to --pretend\n"
            ),
        );
        assert_eq!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "stalepkg"),
            vec!["1.0"],
            "pretend resolves from the stale cache",
        );
        assert_eq!(
            std::fs::read_to_string(&cached).unwrap(),
            stale,
            "a failed refresh never rewrites the cache",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_real_merge_failure_drops_the_stale_cache() {
        // Outside `--pretend` even the stale copy is dropped -- the same warning
        // pair, no fallback note, no candidates.
        let tmp = tempdir();
        let root = tmp.join("root");
        let stale = "TIMESTAMP: 5\nPACKAGES: 1\n\nCPV: dev-libs/stalepkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let mut routes = HashMap::new();
        for path in ["/Packages.gz", "/Packages.zst", "/Packages"] {
            routes.insert(
                path.to_string(),
                (
                    "500 Internal Server Error".to_string(),
                    b"stub 500\n".to_vec(),
                ),
            );
        }
        let (base, _h) = serve_with_status(routes, 3);
        let binrepo = BinRepo {
            name: "stale500".to_string(),
            sync_uri: base.clone(),
            priority: 50,
            location: None,
            verify_signature: false,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            false,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert!(
            outcome
                .stderr_warning
                .is_some_and(|w| w.contains("Error fetching binhost package info")),
            "the fetch failure still warns",
        );
        assert!(
            outcome.stdout_note.is_none(),
            "a real merge carries no pretend-fallback note: {:?}",
            outcome.stdout_note
        );
        assert!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "stalepkg").is_empty(),
            "a real merge drops even the stale cache",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_within_ttl_uses_the_cache_without_fetching() {
        // `DOWNLOAD_TIMESTAMP + TTL` freshness: no fetch at all, Portage's own
        // note.
        let tmp = tempdir();
        let root = tmp.join("root");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let cached_body = format!(
            "TIMESTAMP: 100\nTTL: 3600\nDOWNLOAD_TIMESTAMP: {now}\nPACKAGES: 1\n\nCPV: dev-libs/ttlpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n"
        );
        // Zero expected connections: any fetch would hang this stub.
        let (base, _h) = serve_with_status(HashMap::new(), 0);
        let binrepo = BinRepo {
            name: "ttl".to_string(),
            sync_uri: base.clone(),
            priority: 50,
            location: None,
            verify_signature: false,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, &cached_body).unwrap();
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            true,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert!(outcome.stderr_warning.is_none());
        assert_eq!(
            outcome.stdout_note.as_deref(),
            Some("[ttl] Local copy of remote index is within TTL and will be used.\n"),
        );
        assert_eq!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "ttlpkg"),
            vec!["1.0"],
        );
        assert_eq!(
            std::fs::read_to_string(&cached).unwrap(),
            cached_body,
            "a TTL hit rewrites nothing",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_promotes_a_newer_remote_index_over_a_stale_cache() {
        // The remote index is served when strictly newer.
        let tmp = tempdir();
        let root = tmp.join("root");
        let stale = "TIMESTAMP: 5\nPACKAGES: 1\n\nCPV: dev-libs/tstpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let fresh = "TIMESTAMP: 9\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/tstpkg-2.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), fresh.as_bytes().to_vec());
        // gz + zst 404, then the plain index: 3 connections.
        let (base, _h) = serve(routes, 3);
        let binrepo = BinRepo {
            name: "newer".to_string(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: false,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            false,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert!(outcome.stderr_warning.is_none());
        assert!(outcome.stdout_note.is_none());
        let (header, entries) =
            portage_repo::parse_packages_index(&std::fs::read_to_string(&cached).unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("CPV").map(String::as_str),
            Some("dev-libs/tstpkg-2.0")
        );
        assert!(
            header.contains_key("DOWNLOAD_TIMESTAMP"),
            "promotion stamps DOWNLOAD_TIMESTAMP like real"
        );
        assert_eq!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "tstpkg"),
            vec!["2.0"],
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_keeps_a_newer_cache_over_a_stale_remote_index() {
        // An equally-old or older remote keeps the cache silently, with no
        // rewrite.
        let tmp = tempdir();
        let root = tmp.join("root");
        let cached_body = "TIMESTAMP: 9\nPACKAGES: 1\n\nCPV: dev-libs/tstpkg-2.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let stale_remote = "TIMESTAMP: 5\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/tstpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), stale_remote.as_bytes().to_vec());
        let (base, _h) = serve(routes, 3);
        let binrepo = BinRepo {
            name: "older".to_string(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: false,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, cached_body).unwrap();
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            false,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert!(outcome.stderr_warning.is_none());
        assert!(outcome.stdout_note.is_none());
        assert_eq!(
            std::fs::read_to_string(&cached).unwrap(),
            cached_body,
            "a stale remote rewrites nothing",
        );
        assert_eq!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "tstpkg"),
            vec!["2.0"],
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_frozen_repo_uses_the_cache_without_fetching() {
        // A frozen repo is never refreshed, used as-is.
        let tmp = tempdir();
        let root = tmp.join("root");
        let cached_body = "TIMESTAMP: 5\nPACKAGES: 1\n\nCPV: dev-libs/frzpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let (base, _h) = serve_with_status(HashMap::new(), 0);
        let binrepo = BinRepo {
            name: "frz".to_string(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: false,
            frozen: true,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, cached_body).unwrap();
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            true,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert!(outcome.stderr_warning.is_none());
        assert_eq!(
            outcome.stdout_note.as_deref(),
            Some("[frz] Local copy of remote index is frozen and will be used.\n"),
        );
        assert_eq!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "frzpkg"),
            vec!["1.0"],
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_pretend_drops_a_remote_index_without_a_timestamp() {
        timestamp_or_version_drop_case(
            true,
            "VERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index  has no TIMESTAMP field.\n",
        );
    }

    #[test]
    fn refresh_real_merge_drops_a_remote_index_without_a_timestamp() {
        timestamp_or_version_drop_case(
            false,
            "VERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index  has no TIMESTAMP field.\n",
        );
    }

    #[test]
    fn refresh_pretend_drops_a_remote_index_with_an_unparseable_timestamp() {
        timestamp_or_version_drop_case(
            true,
            "TIMESTAMP: yesterday\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index  has no TIMESTAMP field.\n",
        );
    }

    #[test]
    fn refresh_real_merge_drops_a_remote_index_with_an_unparseable_timestamp() {
        timestamp_or_version_drop_case(
            false,
            "TIMESTAMP: yesterday\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index  has no TIMESTAMP field.\n",
        );
    }

    #[test]
    fn refresh_pretend_drops_a_remote_index_with_an_unsupported_version() {
        timestamp_or_version_drop_case(
            true,
            "TIMESTAMP: 9\nVERSION: 1\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index version is not supported: '1'\n",
        );
    }

    #[test]
    fn refresh_real_merge_drops_a_remote_index_with_an_unsupported_version() {
        timestamp_or_version_drop_case(
            false,
            "TIMESTAMP: 9\nVERSION: 1\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index version is not supported: '1'\n",
        );
    }

    #[test]
    fn refresh_pretend_drops_a_remote_index_without_a_version() {
        timestamp_or_version_drop_case(
            true,
            "TIMESTAMP: 9\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index version is not supported: 'None'\n",
        );
    }

    #[test]
    fn refresh_real_merge_drops_a_remote_index_without_a_version() {
        timestamp_or_version_drop_case(
            false,
            "TIMESTAMP: 9\nPACKAGES: 1\n\nCPV: dev-libs/newpkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n",
            "\n\n!!! [drop] Binhost package index version is not supported: 'None'\n",
        );
    }

    /// A fetched index with no (or unparseable) `TIMESTAMP` or an unsupported
    /// `VERSION` is dropped -- `pkgindex = None`, so even a stale cache
    /// contributes nothing for the run, in `--pretend` exactly like in a
    /// non-pretend merge. A valid stale cache is seeded to prove suppression.
    fn timestamp_or_version_drop_case(pretend: bool, remote_body: &str, expected_warning: &str) {
        let tmp = tempdir();
        let root = tmp.join("root");
        let binhost = tmp.join("binhost");
        std::fs::create_dir_all(&binhost).unwrap();
        std::fs::write(binhost.join("Packages"), remote_body).unwrap();
        let binrepo = BinRepo {
            name: "drop".to_string(),
            sync_uri: format!("file://{}", binhost.display()),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let stale = "TIMESTAMP: 5\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/stalepkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let cached = edb_cache_packages_file(&root, &binrepo.sync_uri);
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(
            &binrepo,
            &root,
            pretend,
            &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
        );
        assert_eq!(
            outcome.stderr_warning.as_deref(),
            Some(expected_warning),
            "pretend={pretend}: the drop warns exactly like real",
        );
        assert!(
            outcome.stdout_note.is_none(),
            "pretend={pretend}: a drop carries no skip note: {:?}",
            outcome.stdout_note
        );
        assert!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "newpkg").is_empty(),
            "pretend={pretend}: the dropped index contributes nothing",
        );
        assert!(
            remote_versions(std::slice::from_ref(&binrepo), &root, "stalepkg").is_empty(),
            "pretend={pretend}: the drop discards even the stale cache like real's pkgindex = None",
        );
        assert_eq!(
            std::fs::read_to_string(&cached).unwrap(),
            stale,
            "pretend={pretend}: a dropped index never rewrites the cache",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_merge_plan_downloads_a_remote_binpkg_and_merges_it() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();

        let tbz2 =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let index = packages_index(&[
            "BUILD_ID: 1\nCPV: dev-libs/packagepkg-1.0\nDEFINED_PHASES: install\n\
             EAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/packagepkg-1.0.tbz2\n\
             RDEPEND: dev-libs/samepkg\nREPO: gentoo\nSIZE: 4618\nSLOT: 0\nUSE:",
        ]);
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), index);
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), tbz2);
        // Refresh tries Packages.gz + Packages.zst (both 404 here) before the
        // plain Packages, then the binpkg download = 4 connections.
        let (base, _h) = serve(routes, 4);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        }];
        // A failed refresh warns on stderr and resolves against the local pool --
        // here the plain Packages succeeds, so the live index lands in the cache.
        refresh_binhost_indexes(&binrepos, &root, false, &[], &[]);
        assert!(
            root.join("var/cache/edb/binhost/127.0.0.1/Packages")
                .is_file(),
            "the live Packages landed in the edb cache"
        );

        let config = Config {
            binrepos: binrepos.clone(),
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };
        let entry = GraphEntry {
            discovery: 0,
            category: "dev-libs".into(),
            package: "packagepkg".into(),
            outcome: PretendOutcome::New {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("gentoo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: vec![],
            required_by: vec![],
            source: CandidateSource::Binary,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: true,
            build_id: None,
            deps: Vec::new(),
        };

        run_merge_plan(RunMergePlanArgs {
            entries: &[entry],
            config: &config,
            repos: &[],
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            merge_options: &MergeOptions::default(),
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            mode: crate::emerge_build::StatusMode::for_tests(),
        })
        .expect("getbinpkg merge succeeds");

        assert!(
            root.join("var/cache/binhost")
                .join(&config.binrepos[0].name)
                .join("dev-libs/packagepkg-1.0.tbz2")
                .is_file(),
            "a binrepo without `location` downloads under <root>/var/cache/binhost/<name>"
        );
        assert!(
            root.join("usr/share/packagepkg/hello.txt").is_file(),
            "the binpkg image was merged into ROOT"
        );
        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file()
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_merge_plan_upgrades_over_an_installed_binpkg() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");

        // packagepkg-0.9 already installed, owning a soon-orphaned file.
        let pkgshare = root.join("usr/share/packagepkg");
        std::fs::create_dir_all(&pkgshare).unwrap();
        std::fs::write(pkgshare.join("old-only.txt"), "orphan\n").unwrap();
        let installed = root.join("var/db/pkg/dev-libs/packagepkg-0.9");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("SLOT"), "0\n").unwrap();
        std::fs::write(installed.join("COUNTER"), "1").unwrap();
        std::fs::write(installed.join("PF"), "packagepkg-0.9\n").unwrap();
        std::fs::write(installed.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(
            installed.join("CONTENTS"),
            "dir /usr\ndir /usr/share\ndir /usr/share/packagepkg\n\
             obj /usr/share/packagepkg/old-only.txt 0000 0\n",
        )
        .unwrap();

        let tbz2 =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let index = packages_index(&[
            "BUILD_ID: 1\nCPV: dev-libs/packagepkg-1.0\nDEFINED_PHASES: install\n\
             EAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/packagepkg-1.0.tbz2\n\
             RDEPEND: dev-libs/samepkg\nREPO: gentoo\nSIZE: 4618\nSLOT: 0\nUSE:",
        ]);
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), index);
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), tbz2);
        // Refresh tries Packages.gz + Packages.zst (both 404 here) before the
        // plain Packages, then the binpkg download = 4 connections.
        let (base, _h) = serve(routes, 4);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        }];
        refresh_binhost_indexes(&binrepos, &root, false, &[], &[]);

        let config = Config {
            binrepos: binrepos.clone(),
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };
        let entry = GraphEntry {
            discovery: 0,
            category: "dev-libs".into(),
            package: "packagepkg".into(),
            outcome: PretendOutcome::Upgrade {
                from: "0.9".into(),
                to: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("gentoo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: vec![],
            required_by: vec![],
            source: CandidateSource::Binary,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: true,
            build_id: None,
            deps: Vec::new(),
        };

        run_merge_plan(RunMergePlanArgs {
            entries: &[entry],
            config: &config,
            repos: &[],
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            merge_options: &MergeOptions::default(),
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            mode: crate::emerge_build::StatusMode::for_tests(),
        })
        .expect("getbinpkg merge succeeds");

        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file(),
            "the new version is installed"
        );
        assert!(
            !root.join("var/db/pkg/dev-libs/packagepkg-0.9").exists(),
            "the old version's vdb entry is gone"
        );
        assert!(
            !pkgshare.join("old-only.txt").exists(),
            "the old version's orphaned file is unmerged"
        );
        assert!(
            root.join("usr/share/packagepkg/hello.txt").is_file(),
            "the new version's own file is present"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_merge_plan_merges_a_binary_and_a_source_entry_in_one_run() {
        // A mixed plan: one `Binary` entry (a local `$PKGDIR` `.tbz2`) and one
        // `Source` entry (built from its fixture ebuild), merged in one pass.
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            pkgdir.join("dev-libs/packagepkg-1.0.tbz2"),
        )
        .unwrap();

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };

        run_merge_plan(RunMergePlanArgs {
            entries: &[
                graph_entry("packagepkg", CandidateSource::Binary, "1.0"),
                graph_entry("binpkgphasepkg", CandidateSource::Ebuild, "1.0"),
            ],
            config: &Config::default(),
            repos: &repos,
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            merge_options: &options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            mode: crate::emerge_build::StatusMode::for_tests(),
        })
        .expect("mixed merge plan succeeds");

        // The Binary entry: merged from the local .tbz2.
        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file()
        );
        assert!(root.join("usr/share/packagepkg/hello.txt").is_file());
        // The Source entry: built + merged from its ebuild, hooks ran.
        assert!(
            root.join("var/db/pkg/dev-libs/binpkgphasepkg-1.0/CONTENTS")
                .is_file()
        );
        assert!(root.join("usr/share/binpkgphasepkg/payload.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgphasepkg.phases")).unwrap(),
            "preinst\npostinst\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A resumed binary entry (`remote_binary: false` -- all `resume_entry`
    /// records) with no local `$PKGDIR` file must FAIL with the local-only
    /// error, never re-hit the binhost: Portage replays a resumed binary
    /// against its local bintree only (a never-downloaded binary isn't
    /// resumable at all).
    #[test]
    fn merge_one_binary_entry_never_refetches_for_a_non_remote_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&pkgdir).unwrap();
        let binhost = tmp.join("binhost");
        std::fs::create_dir_all(binhost.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"),
        )
        .unwrap();
        let size = std::fs::metadata(binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"))
            .unwrap()
            .len();
        std::fs::write(
            binhost.join("Packages"),
            format!(
                "TIMESTAMP: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/binpkgrmpkg-1.0\n\
                 DEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgrmpkg-1.0.tbz2\n\
                 REPO: testrepo\nSIZE: {size}\nSLOT: 0\nUSE:\n"
            ),
        )
        .unwrap();
        let config = Config {
            binrepos: vec![BinRepo {
                name: "test".into(),
                sync_uri: format!("file://{}", binhost.display()),
                priority: 1,
                location: None,
                verify_signature: false,
                frozen: false,
                getbinpkg_exclude: Vec::new(),
                getbinpkg_include: Vec::new(),
            }],
            ..Default::default()
        };
        // Sanity: the binhost serves this cpv (a fetch WOULD succeed) -- so the
        // failure below proves no fetch was tried.
        assert!(
            find_remote_binpkg(&config.binrepos, &root, "dev-libs", "binpkgrmpkg", "1.0").is_some()
        );

        // Resumed shape: `graph_entry` defaults match `resume_entry`
        // (`remote_binary: false`, `build_id: None`).
        let entry = graph_entry("binpkgrmpkg", CandidateSource::Binary, "1.0");
        assert!(!entry.remote_binary);
        // No local stanza vouches for it (empty `$PKGDIR`), so the failure is
        // Portage's `!!! Fetching Binary failed` arm -- never a refetch, and
        // never portuale's old `no binpkg file under ...` text.
        let err = merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &config,
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("pt"),
            merge_options: &MergeOptions::default(),
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .expect_err("a resumed binary with no local file must fail, not refetch");
        assert!(
            is_binpkg_missing_failure(&err),
            "silent missing-file sentinel, got: {err}"
        );
        assert!(
            !err.contains("no binpkg file under"),
            "portuale's own wording is gone, got: {err}"
        );
        assert!(
            !err.contains("binhost"),
            "must not mention the index, got: {err}"
        );
        assert!(
            portage_util::read_dir_entries(&pkgdir).unwrap().is_empty(),
            "nothing may be downloaded into $PKGDIR"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
