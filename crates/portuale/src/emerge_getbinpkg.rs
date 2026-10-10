// Real `emerge --getbinpkg <atom>` / `--getbinpkgonly <atom>` execution:
// refresh each remote binhost's live index (in `--pretend` exactly like
// in a real merge, backlog #192), resolve the graph, then merge every
// resolved entry -- dispatching per entry on its `source` (a `Binary`
// candidate is downloaded+merged, anything else is built+merged from
// source).
//
// The `--pretend` half of `--getbinpkg`/`--getbinpkgonly` already shipped
// (real `bintree`'s `binrepos.conf`/`PORTAGE_BINHOST` parsing, remote
// binhost candidates from each binhost's `Packages` index, the `g`
// bracket column). This module is the other half: the live index
// refresh + the file download + the merge.
//
//   - `refresh_binhost_indexes`: real `bintree._populate_remote` --
//     fetch `<sync_uri>/Packages` (a `file://` binhost is read from its
//     own directory) and best-effort-cache it at the same
//     `<EROOT>/var/cache/edb/binhost/<host>/<path>/Packages` location
//     `list_remote_binary_candidates` reads, with the fetched index
//     handed to the resolver in memory. Runs BEFORE resolution, in
//     `--pretend` exactly like in a real merge, so the resolver sees
//     the fresh pool.
//   - `run_merge_plan`: iterate the resolved entries (already in real
//     topological merge order), and per entry -- `Binary` ->
//     `merge_one_binary_entry` (find its `Packages` record via
//     `find_remote_binpkg`, `wget`/copy into `$PKGDIR`, `SIZE`-check,
//     `ebuild_merge::merge_binpkg`); else ->
//     `emerge_build::merge_one_source_entry`. Real `--getbinpkg`'s own
//     "prefer a binary, fall back to source" is the resolver's job, not
//     this loop's; `--getbinpkgonly` (binary-only resolve) simply never
//     produces a non-`Binary` entry.
//
// v1 cuts specific to this module:
//   - `Packages.gz` / `Packages.zst` (a compressed remote index) ARE
//     tried now (`refresh_binhost_indexes` fetches `Packages.<ext>` and
//     pipes it through `gzip -dc` / `zstd -dc` into the plain `Packages`
//     cache file), with the plain `Packages` as the fallback. Not
//     modelled: `Packages.bz2`/`.lz4` and real portage's exact
//     preference ordering.
//   - live `layout.conf` negotiation (`binpkg-multi-instance`, path
//     layout) is not done -- the index `PATH` field (or the default
//     `<cat>/<pf>.tbz2`) is trusted outright, same "trust the index"
//     stance the `--pretend` half already takes.
//   - digest verification checks `SIZE` and the `Packages` record's
//     `MD5` and `SHA1` (the md5/sha1 of the whole `.tbz2`, real
//     `bintree`'s own fields, `_pkgindex_hashes = ["MD5", "SHA1"]`).
//     Each field present is verified (real `_get_digests` collects every
//     valid checksum key present, `digestCheck` verifies them all). A downloaded gpkg
//     additionally has its internal `Manifest` (`DATA` BLAKE2B/SHA512
//     lines) *and* its GPG signature layer (detached `.sig` sidecars +
//     clear-signed `Manifest`, real `_verify_binpkg` -- see
//     `binpkg::GpgVerify`) verified at merge time
//     (`binpkg::extract_binpkg` -> `verify_gpkg_manifest`).

use crate::ebuild_merge::{self, MergeOptions};
use mrg_director::BinpkgIndex;
use mrg_director::MergeEngine;
use portage_profile::{BinRepo, Config};
use portage_repo::{GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

/// Real `fetch.py::_hide_url_passwd`, verbatim (`fetch.py:73-74`):
/// `re.sub(r"//([^:\s]+):[^@\s]+@", r"//\1:*password*@", url)`.
/// Real applies it to the binhost URL in its own
/// `Error fetching binhost package info` line (`bintree.py:1794-1796`).
/// The single global substitution is implemented exactly (same pattern
/// and replacement text), so edge behaviour matches: a password
/// containing whitespace is left alone (`[^@\s]+` cannot span it),
/// while a user containing `/` is still masked (`[^:\s]+` allows it).
pub(crate) fn hide_binhost_passwd(url: &str) -> String {
    regex::Regex::new(r"//([^:\s]+):[^@\s]+@")
        .expect("static regex is valid")
        .replace_all(url, "//$1:*password*@")
        .into_owned()
}

/// Real `urllib.error.HTTPError`'s own `str` (`"HTTP Error {code}:
/// {reason}"`, verified against the stdlib: `HTTPError(url, 500,
/// "Internal Server Error", ...)` stringifies to exactly
/// `"HTTP Error 500: Internal Server Error"`): recover that shape from
/// a captured wget transcript. wget's own
/// `HTTP request sent, awaiting response... {code} {reason}` line
/// carries the server's status line verbatim, so the reconstruction is
/// byte-identical whenever the server sent a reason phrase. `None` when
/// the transcript holds no response line (DNS failure, connection
/// refused, ...): real would print its own urllib `URLError` string
/// there, which wget cannot reproduce -- the caller falls back to the
/// fetch summary instead (documented, not invented parity).
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

/// Real `bintree._populate_remote`'s `except OSError` pair
/// (`bintree.py:1793-1798`): the leading-blank-line
/// `!!! [<name>] Error fetching binhost package info from '<url>'`
/// line plus the `!!! [<name>] <detail>` line and its trailing blank
/// line, as one stderr block. `detail` is real's `str(err)` -- the
/// `HTTP Error 500: Internal Server Error` shape for a 500 (see
/// [`wget_http_error`]), the fetch summary otherwise.
pub(crate) fn binhost_fetch_warning(binrepo_name: &str, sync_uri: &str, detail: &str) -> String {
    format!(
        "\n\n!!! [{binrepo_name}] Error fetching binhost package info from '{}'\n!!! [{binrepo_name}] {detail}\n\n",
        hide_binhost_passwd(sync_uri)
    )
}

/// Real `bintree._populate_remote_repo`'s no-`TIMESTAMP` drop
/// (`bintree.py:1722-1732`): `\n\n!!! [<name>] Binhost package index
///  has no TIMESTAMP field.\n` on stderr. The double space is real's
/// own implicit concatenation (`"index " " has no ..."`), kept
/// byte-identical.
pub(crate) fn binhost_no_timestamp_warning(binrepo_name: &str) -> String {
    format!("\n\n!!! [{binrepo_name}] Binhost package index  has no TIMESTAMP field.\n")
}

/// Real `bintree._populate_remote_repo`'s version-gate drop
/// (`bintree.py:1735-1744`): `\n\n!!! [<name>] Binhost package index
/// version is not supported: '<ver>'\n` on stderr, where `<ver>` is
/// the raw `VERSION` header (`'None'` when absent -- real formats
/// `header.get("VERSION")`, i.e. `None`).
pub(crate) fn binhost_unsupported_version_warning(
    binrepo_name: &str,
    version: Option<&str>,
) -> String {
    format!(
        "\n\n!!! [{binrepo_name}] Binhost package index version is not supported: '{}'\n",
        version.unwrap_or("None")
    )
}

/// Real `bintree._populate_remote`: refresh every binrepo's live
/// `Packages` index and hand it to the resolver in memory -- in
/// `--pretend` exactly like in a real merge (real
/// `actions.py:3752` passes `getbinpkg_refresh=True` unconditionally;
/// `pretend` only selects the stale-fallback message, never skips the
/// fetch, `bintree.py:917-920`, backlog #192).
///
/// Per binrepo (real `_populate_remote_repo`):
///   - a `frozen` repo, or one whose cached index is still within its
///     `TTL`, is used from cache with real's own
///     `[name] Local copy of remote index is ... and will be used.`
///     note (stdout);
///   - otherwise the remote index is fetched (a `file://` repo is read
///     from its own directory, `Packages.gz` first like real;
///     `http(s)` tries `Packages.gz` / `Packages.zst` / `Packages`);
///   - a fetched index with a `TIMESTAMP` no newer than the cache keeps
///     the cache silently; a newer one (or any fetch with no cache to
///     compare against) is cached with a fresh `DOWNLOAD_TIMESTAMP` and
///     used -- the cache write is best-effort (an unwritable cache dir
///     is ignored, `bintree.py:1819-1823`; only a write error on a
///     *writable* dir warns, where real would re-raise);
///   - a failed fetch warns real's `!!! [repo] Error fetching ...` pair
///     (stderr) and then -- `--pretend`: real's
///     `[name] Local copy of unavailable remote index will be used due
///     to --pretend` note (stdout), resolving from the stale cache; --
///     real merge: nothing more, and even a stale cache is dropped (real
///     `pkgindex = None`, `bintree.py:1809`).
///
/// A failed refresh is NON-FATAL either way (backlog #175): resolution
/// proceeds against whatever pool the rules above leave.
///
/// Deliberate narrowings (all documented, none invented parity):
///   - conditional-GET (`If-Modified-Since` / `304 Not Modified`) is not
///     sent -- wget always downloads the body, so a repeat run against
///     an unchanged index re-downloads where real would print
///     `... is up-to-date and will be used.` Resolution is identical
///     (the `TIMESTAMP` compare keeps the cache silently either way);
///     only that one stdout line differs, and only across runs.
///   - a fetched index with no `TIMESTAMP` header, or an
///     unparseable one, is dropped with real's own
///     `!!! [name] Binhost package index  has no TIMESTAMP field.`
///     (stderr, `bintree.py:1722-1732` -- the double space is real's
///     own `"index " " has"` concatenation; real only tests falsiness
///     there, so an unparseable stamp takes the same arm rather than
///     real's `int()` `ValueError` unwind); a fetched index whose
///     `VERSION` is missing, unparseable, or newer than real's
///     `_pkgindex_version` (`0`, `bintree.py:2429-2437`) is dropped
///     with `!!! [name] Binhost package index version is not
///     supported: '<ver>' (`bintree.py:1735-1744`, `'None'` when the
///     header is absent). Either drop discards even a stale cache for
///     the run (real `pkgindex = None`), in `--pretend` exactly like
///     in a real merge -- real draws no pretend distinction here.
///   - `--verbose`'s `Last-Modified` mismatch warning, `ssh://`
///     transports, `getbinpkg-exclude`/`-include` pool filtering and the
///     trust-helper/`gpkg_only` gating are not modelled.
pub fn refresh_binhost_indexes(binrepos: &[BinRepo], root: &Path, pretend: bool) {
    for binrepo in binrepos {
        let outcome = refresh_one_binrepo(binrepo, root, pretend);
        if let Some(warning) = outcome.stderr_warning {
            eprint!("{warning}");
        }
        if let Some(note) = outcome.stdout_note {
            print!("{note}");
        }
    }
}

/// One binrepo of [`refresh_binhost_indexes`]: installs that repo's
/// run index via [`portage_repo::set_remote_binary_index_override`]
/// (keyed by the same `packages_dir` the resolver reads) and returns
/// real's messages for the caller to print -- the `!!! [repo] ...`
/// warning block for stderr, the `Local copy ...` note for stdout.
struct RefreshOutcome {
    stderr_warning: Option<String>,
    stdout_note: Option<String>,
}

/// Real `bintree._populate_remote_repo`'s own cache file:
/// `<EROOT>/var/cache/edb/binhost/<host>/<url-path>/Packages`
/// (`bintree.py:1497-1504`). For `http(s)`/`ssh` that is exactly
/// [`BinRepo::packages_dir`]; for `file://` it is the EROOT-side copy
/// (real caches even local indexes: `host` is empty, so
/// `file:///srv/pkgs` caches under
/// `<EROOT>/var/cache/edb/binhost/srv/pkgs/Packages`).
fn edb_cache_packages_file(root: &Path, sync_uri: &str) -> std::path::PathBuf {
    let uri = sync_uri.trim_end_matches('/');
    if let Some(rest) = uri.strip_prefix("file://") {
        return root
            .join("var/cache/edb/binhost")
            .join(rest.trim_start_matches('/'))
            .join("Packages");
    }
    // Real `BinRepo::packages_dir` (kept as the single mapping so the
    // cache is always written where the resolver reads).
    BinRepo {
        name: String::new(),
        sync_uri: uri.to_string(),
        priority: 0,
        location: None,
        verify_signature: true,
        frozen: false,
    }
    .packages_dir(root)
    .join("Packages")
}
fn refresh_one_binrepo(binrepo: &BinRepo, root: &Path, pretend: bool) -> RefreshOutcome {
    // Real warns with `base_url` as configured (`bintree.py:1488,1795`):
    // only `uri` (the trailing-`/`-stripped form) builds fetch URLs,
    // every message below prints `raw`.
    let raw = binrepo.sync_uri.as_str();
    let uri = raw.trim_end_matches('/');
    let ok = |overlay: portage_repo::BinaryIndex| {
        portage_repo::set_remote_binary_index_override(
            &binrepo.packages_dir(root),
            Some(std::sync::Arc::new(overlay)),
        );
    };
    let cached_file = edb_cache_packages_file(root, raw);
    let cached_text = std::fs::read_to_string(&cached_file).ok();
    let (cached_header, cached_entries) = cached_text
        .as_deref()
        .map(portage_repo::parse_packages_index)
        .unwrap_or_default();
    let cached_index = || portage_repo::BinaryIndex::from_entries(cached_entries.clone());

    // Real `if local_timestamp and (repo.frozen or not getbinpkg_refresh)`
    // (`bintree.py:1528-1533`) -- refresh is always requested here, so a
    // cached index on a frozen repo is used as-is.
    if cached_text.is_some()
        && cached_header
            .get("TIMESTAMP")
            .is_some_and(|s| !s.is_empty())
        && binrepo.frozen
    {
        ok(cached_index());
        return RefreshOutcome {
            stderr_warning: None,
            stdout_note: Some(use_cached_note(&binrepo.name, "frozen")),
        };
    }
    // Real `DOWNLOAD_TIMESTAMP + TTL` freshness (`bintree.py:1535-1545`):
    // a still-valid cache is used without any fetch.
    let ttl_secs: f64 = cached_header
        .get("TTL")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let downloaded_at: f64 = cached_header
        .get("DOWNLOAD_TIMESTAMP")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    if downloaded_at > 0.0 && ttl_secs > 0.0 && downloaded_at + ttl_secs > unix_now() {
        ok(cached_index());
        return RefreshOutcome {
            stderr_warning: None,
            stdout_note: Some(use_cached_note(&binrepo.name, "within TTL")),
        };
    }

    // The fetch itself runs BEFORE any cache-dir write (real fetches
    // first and only then tries `ensure_dirs` + the atomic write,
    // `bintree.py:1463-1823` -- never `create_dir_all`-then-abort like
    // portuale used to, backlog #192). It lands in temp files, never
    // straight into the cache: a stale cache must survive a failed or
    // older remote index.
    match fetch_remote_index_text(uri) {
        Err(failures) => {
            let warning = Some(binhost_fetch_warning(
                &binrepo.name,
                raw,
                &http_or_summary(&failures),
            ));
            if pretend {
                // Real keeps the stale copy and says so
                // (`bintree.py:1801-1808`).
                ok(cached_index());
                RefreshOutcome {
                    stderr_warning: warning,
                    stdout_note: Some(format!(
                        "[{}] Local copy of unavailable remote index will be used due to --pretend\n",
                        binrepo.name
                    )),
                }
            } else {
                // Real drops even the stale copy (`pkgindex = None`,
                // `bintree.py:1809`): the override suppresses the disk
                // cache for the rest of the run.
                portage_repo::set_remote_binary_index_override(&binrepo.packages_dir(root), None);
                RefreshOutcome {
                    stderr_warning: warning,
                    stdout_note: None,
                }
            }
        }
        Ok(remote_text) => {
            let (remote_header, remote_entries) = portage_repo::parse_packages_index(&remote_text);
            // Real drops a fetched index with no `TIMESTAMP` header
            // (`pkgindex = None` + the `!!! ... has no TIMESTAMP
            // field.` warning, `bintree.py:1722-1732`).
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
            // Real `_pkgindex_version_supported`
            // (`bintree.py:2429-2437`, `_pkgindex_version = 0`,
            // `bintree.py:547`): the `VERSION` header must parse to an
            // int `<= 0`, else the index is dropped with the
            // `!!! ... version is not supported: ...` warning
            // (`bintree.py:1735-1744`).
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
            // Real serves the remote index only when strictly newer
            // (`not local_timestamp or int(local) < int(remote)`,
            // `bintree.py:1739-1744`); an equally-old or older remote
            // keeps the cache silently, with no rewrite.
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
                ok(cached_index());
                return RefreshOutcome {
                    stderr_warning: None,
                    stdout_note: None,
                };
            }
            let now = unix_now() as u64;
            let staged = stamp_download_timestamp(&remote_text, now);
            ok(portage_repo::BinaryIndex::from_entries(remote_entries));
            // Best-effort cache write (real `ensure_dirs` +
            // `atomic_ofstream`, `bintree.py:1811-1818`): an unwritable
            // cache dir is ignored ("that's alright"); only a write
            // error on a writable dir warns (where real would re-raise
            // -- portuale refreshes stay non-fatal per backlog #175).
            // Either way the in-memory index above is what the run
            // resolves from.
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

/// Real `bintree._populate_remote_repo`'s skip note
/// (`bintree.py:1785-1789`):
/// `[name] Local copy of remote index is <why> and will be used.`
/// (stdout; real appends `Last-Modified` detail only under `--verbose`,
/// which portuale does not model).
fn use_cached_note(binrepo_name: &str, why: &str) -> String {
    format!("[{binrepo_name}] Local copy of remote index is {why} and will be used.\n")
}

/// Seconds since the epoch as `f64` (real `time.time()`).
fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Real `os.access(dirname, W_OK)` (`bintree.py:1821`) without libc:
/// the parent dir's mode bits decide (review M3 -- no probe file is
/// ever created, so nothing can linger in the EROOT cache dir). A
/// missing directory counts as unwritable (its own creation is the
/// caller's `create_dir_all`, whose failure lands here the same way).
/// Like `access(2)` this is a permission-bit check rather than a write
/// attempt, so on a read-only filesystem with writable bits a failed
/// write still warns (where real would re-raise -- portuale refreshes
/// stay non-fatal per backlog #175).
fn cache_dir_writable(cache_file: &Path) -> bool {
    let dir = cache_file.parent().unwrap_or_else(|| Path::new("."));
    std::fs::metadata(dir).is_ok_and(|m| !m.permissions().readonly())
}

/// Insert (or replace) the `DOWNLOAD_TIMESTAMP` header real stamps
/// before caching a fetched index (`pkgindex.header[
/// "DOWNLOAD_TIMESTAMP"] = "%d" % time.time()`, `bintree.py:1806`),
/// so a later run's `TTL` check sees it.
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

/// Fetch one binrepo's remote `Packages` index body as text. A `file://`
/// repo is read from its own directory (`Packages.gz` first, like
/// real's `("Packages.gz", "Packages")` loop -- a missing `.gz` falls
/// through to the plain file, a corrupt `.gz` fails outright, real
/// `bintree.py:1595-1604`); anything else goes over wget into temp
/// files (never the cache itself), preferring a compressed index real
/// serves (`Packages.gz`, then portuale's `Packages.zst` superset cut,
/// then plain `Packages`). Every attempt runs quiet
/// (`download_via_wget_quiet`): the transcript is captured for message
/// shaping, never inherited onto stdout/stderr.
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
    // Real `bintree._populate_remote` prefers a compressed index when
    // the binhost serves one, decompressing it into the plain cache
    // file. A downloaded-but-corrupt compressed index fails
    // immediately, with NO plain-`Packages` attempt: real's
    // `gzip.BadGzipFile` subclasses `OSError` (verified against the
    // stdlib), so it escapes the `("Packages.gz", "Packages")` loop
    // straight to the outer `except OSError` (`bintree.py:1790-1809`).
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

/// Real warns with the fetch's own error string; prefer a
/// server status line (`HTTP Error 500: ...`, from the first
/// attempt that has one) over the wget summary, exactly like
/// real prefers `str(HTTPError)` -- the attempts hit the same
/// server, so the first parseable one is representative.
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

/// Real `emerge --getbinpkg <atom>` / `--getbinpkgonly <atom>` (no
/// `--pretend`): merge every resolved entry (already in real dependency-
/// first merge order), dispatching **per entry** on `entry.source` --
/// real `--getbinpkg`'s own "prefer a binary package, fall back to a
/// source build" is entirely the resolver's job (it already stamped the
/// right `source` on each `GraphEntry`); this just executes the plan.
///
///   - a `Binary` entry -> download it (if remote) and merge it
///     (`merge_one_binary_entry` -> `ebuild_merge::merge_binpkg`, all
///     four `pkg_*` hooks + same-slot replace).
///   - anything else -> build + merge from source
///     (`emerge_build::merge_one_source_entry` -> `ebuild_merge::
///     run_merge`).
///   - `AlreadyInstalled` is a silent no-op either way.
///
/// `--getbinpkgonly` (binary-only resolve, `usepkgonly`) simply never
/// yields a non-`Binary` entry, so the same function serves both.
#[allow(clippy::too_many_arguments)]
pub fn run_merge_plan(
    entries: &[GraphEntry],
    config: &Config,
    repos: &[RepoConfig],
    root: &Path,
    pkgdir: &Path,
    portage_tmpdir: &Path,
    merge_options: &MergeOptions,
    keep_going: bool,
    buildpkg: Option<&crate::ebuild_package::PackageOptions>,
    buildpkg_exclude: &[String],
    // Backlog #197: the scheduler-display mode (background + Jobs
    // visibility), computed once by the caller. The mixed plan runs
    // serially, so no `>>> Jobs:` events fire -- the mode only carries
    // the blank/`>>>` rule (a `--quiet` binary merge prints no leading
    // blanks, exactly like real's background scheduler). Stated cut:
    // real drives binary tasks through the same display (real
    // `Scheduler._merge_exit` / `_do_merge_exit`,
    // `Scheduler.py:1532-1560`), so a real `--quiet --verbose` (or
    // `-jN`) binary merge shows `>>> Jobs:` lines while portuale shows
    // none here -- unprobed against real, Jobs events for the mixed
    // plan are a later slice.
    mode: crate::emerge_build::StatusMode,
) -> Result<(), String> {
    // Backlog #322: a source build always runs ebuild phases, so a binary
    // that cannot find the vendored runtime says so before anything is
    // fetched. Binary merges are not gated here: a hookless gpkg never
    // spawns a shell, and one with hooks reports the same message at its
    // first phase.
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
    // Real `Scheduler._pkg_count` for this run (backlog #177): every
    // entry below prints its own snapshot of these counters.
    let progress = crate::emerge_build::merge_progress_map(entries);
    // Backlog #197: real `Scheduler` owns one `JobStatusDisplay` per
    // merge run.
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
    crate::emerge_build::run_merge_loop(entries, keep_going, root, |idx, entry| {
        // The director seam executes every unit: derive the entry's
        // `MergeUnit` and dispatch on its kind through the real source /
        // binary engines (real `MergeListItem._start`'s own `type_name`
        // routing). An entry with nothing to merge (`AlreadyInstalled` /
        // `NoVisibleCandidate`) stays a silent no-op, exactly the merge
        // functions' own early return.
        let entry_progress = progress[idx];
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

/// One `Binary` entry of `run_merge_plan`'s own loop: `AlreadyInstalled`
/// is a silent no-op; `New`/`Upgrade`/`Downgrade`/`Reinstall` are
/// fetched (remote) or located (`$PKGDIR`) and merged (`merge_binpkg`
/// unmerges a replaced same-slot version itself).
#[allow(clippy::too_many_arguments)]
pub(crate) fn merge_one_binary_entry(
    entry: &GraphEntry,
    config: &Config,
    root: &Path,
    pkgdir: &Path,
    portage_tmpdir: &Path,
    merge_options: &MergeOptions,
    progress: mrg_director::MergeProgress,
    display: &crate::emerge_build::StatusDisplay,
) -> Result<(), String> {
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        // #72 B3: a removal installs no binary (execution is a non-goal).
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::Uninstall { .. } => return Ok(()),
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::NoVisibleCandidate => {
            return Err(format!("no binary package available for {cp}"));
        }
    };

    // Real `MergeListItem._start`'s per-package line for a binary entry
    // (backlog #177): `Emerging binary (N of M) cpv::repo`. This is the
    // `>>> Merging binary package ...` line's real shape.
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
    // A `$PKGDIR` file already on disk wins; otherwise only a *remote*
    // entry (`entry.remote_binary`, set by the resolver for a
    // binhost-sourced candidate) may fetch from a binhost. A resumed
    // entry always lands here with `remote_binary: false`
    // (`resume_entry` records only `cat/pkg-ver`, so "was this fetched
    // remotely" is not re-derived) -- and real replays a resumed
    // binary against its *local* bintree, matched by the recorded
    // `mtimedb["resume"]["binpkgs"]` build metadata, never by
    // re-hitting the binhost. A resumed binary that was never
    // downloaded therefore fails here (real: `PackageNotFound`, "An
    // expected package is not available") instead of silently fetching
    // a possibly-different file. The same holds for a fresh
    // local-`$PKGDIR` entry: its fetch already happened, so a missing
    // file is an error, not a refetch.
    // Real `gpkg.__init__(verify_signature=...)`: a repo fetched from a
    // configured binrepo uses **that repo's** own policy, not the bare
    // `FEATURES` default (backlog #43). `None` for a local `$PKGDIR`
    // file, which has no binrepo and keeps `merge_options.gpg_verify`.
    let mut fetched_verify = None;
    let binpkg_path = match local {
        Some(path) => path,
        None if entry.remote_binary => {
            // H3: the remote `Packages` lookup goes through the
            // director's `BinpkgIndex` seam (`RemoteBinhostIndex`),
            // which scans the configured binrepos in order exactly like
            // `portage_repo::find_remote_binpkg` did -- the record
            // travels with the owning binrepo's section name, which is
            // how the download recovers its `sync-uri` and
            // `verify-signature` policy below.
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
            let path = download_and_verify(
                &binrepo.sync_uri,
                &record,
                &entry.category,
                &entry.package,
                &version,
                pkgdir,
            )?;
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
            // Real `_emerge/BinpkgVerifier._start`'s `os.stat` failure
            // arm (backlog #187): the `>>> Emerging binary` line above
            // already printed (real's scheduler prints it before the
            // verifier runs), so what remains is the verifier's own
            // output -- to stdout AND the package `build.log` (real
            // `SchedulerInterface.output(msg, log_path)` appends to
            // both), plus real's `>>> Failed to emerge ...` tail (the
            // same `failed_pkg_msg` + `build_log_path` machinery the
            // digest arm reuses; the log holds exactly this block).
            // `local_index_lists_cpv` is real's own branch condition
            // (`bintree.dbapi.cpv_exists`, `BinpkgVerifier.py:48`).
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

    // Real `_emerge/Binpkg.py` + `BinpkgVerifier` (backlog #174): a
    // local binpkg the `<pkgdir>/Packages` index vouches for is
    // verified at merge -- size first, then every digest the record
    // carries -- against the index's own values, exactly like real's
    // merge-time check (real `bintree._get_digests` reads the index,
    // never the re-scanned file; the honored digest list is fixed --
    // see `binpkg::verify_binpkg_against_index`'s own disclosure). A
    // mismatch prints real's `!!! Digest verification failed:` block to
    // stdout AND to the package `build.log` (real
    // `SchedulerInterface.output(msg, log_path)` appends to both),
    // renames the file to `._checksum_failure_.<rand>` (real
    // `_checksum_failure_temp_file`), and fails the package with real's
    // `>>> Failed to emerge <cpv>[ for <root>][, Log file:]` shape
    // (real `Scheduler._failed_pkg_msg(..., "emerge", "for")`; the `,
    // Log file:` suffix and `>>>  '<log>'` line appear because the
    // just-written `build.log` is non-empty, real
    // `_locate_failure_log`). A binpkg no record vouches for was fully
    // parsed at scan and skips this -- there is nothing to verify
    // against (real's own `if "size" not in digests: return OK` halves
    // this: a record without `SIZE` verifies nothing either). Remote
    // (`file://` or binhost) downloads were already checked by
    // `download_and_verify` against the *remote* record (real
    // `_get_digests` prefers the remote metadata too), so only a purely
    // local binpkg is re-checked here against the local index.
    //
    // Real prints nothing else for this failure: no resume-list notice
    // (real's notices -- `actions.py:348,363`, `Scheduler.py:2493` --
    // fire only for resolution-time `UnsatisfiedResumeDep` /
    // `PackageNotFound`, never for a merge-time package failure, and
    // the end-of-run failed-packages summary only for `> 1` failure or
    // `--keep-going`) and no `emerge: ...` line (a merge failure exits
    // via `FAILURE`, not the action error path). So the returned error
    // is a `binpkg_digest_failure` sentinel the CLI boundary drops
    // silently (exit 1).
    if !entry.remote_binary
        && let Some(record) = local_index_record(
            pkgdir,
            &entry.category,
            &entry.package,
            &version,
            entry.build_id.as_deref(),
            &binpkg_path,
        )
        && let Err(mismatch) = crate::binpkg::verify_binpkg_against_index(&binpkg_path, &record)
    {
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let renamed = crate::binpkg::checksum_failure_rename(&binpkg_path)
            .unwrap_or_else(|| binpkg_path.clone());
        let block = crate::binpkg::digest_failure_block(&binpkg_path, &mismatch, &renamed);
        // Real `Binpkg._start` runs `prepare_build_dirs` + `clean_log`
        // before `BinpkgVerifier`, and `_digest_exception` appends the
        // block to the fresh `PORTAGE_LOG_FILE`: the log holds exactly
        // this block. `build_log_path` is that same
        // `${PORTAGE_BUILDDIR}/temp/build.log` (plus real's
        // `PORTAGE_LOGDIR`/compress naming).
        let log_path = crate::emerge_build::build_log_path(
            portage_tmpdir,
            &entry.category,
            &entry.package,
            &version,
            &crate::emerge_build::resolved_features(merge_options),
        );
        // Real `_locate_failure_log` only reports a log that exists and
        // is non-empty: without a writable log there is no `, Log file:`
        // suffix, exactly like real with no located log.
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

    // Real `PackageMerge._start`'s per-package line (backlog #177):
    // the binpkg is located/fetched above (real's `Binpkg` chain),
    // the vdb merge runs below (real's `EbuildMerge` chain) -- this
    // line lands exactly between them, like real's.
    display.status(&crate::emerge_build::installing_line(
        entry, &version, progress, root, &color,
    ));
    // #333: a binary merge is a package task too; real's per-task
    // `config.reload()` refreshes the protect lists its `dblink` reads
    // (`emerge_build::reloaded_config_protect`).
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
    // Real `PackageMerge._install_exit`'s per-package line (backlog
    // #177): `Completed (N of M) cpv::repo`.
    display.status(&crate::emerge_build::completed_line(
        entry, &version, progress, root, &color,
    ));
    Ok(())
}

/// Real `Scheduler._failed_pkg_msg(pkg, "emerge", "for")`'s own tail
/// (`Scheduler.py:2366-2381`, backlog #174): `>>> Failed to emerge
/// <cpv>[ for <root>][, Log file:]` plus, when `_locate_failure_log`
/// finds the non-empty build log, `>>>  '<log>'`. Each `_status_msg`
/// writes its own leading blank line first (`writemsg_level("\n")`
/// before `displayMessage`), and the `>>> ` prefix is
/// `JobStatusDisplay._format_msg`; the log line's extra space is
/// real's own `f" '{log}'"`. The ` for <root>` suffix appears when
/// `ROOT != "/"` (real `pkg.root_config.settings["ROOT"]`).
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

/// The error for a merge-time binpkg digest failure whose real output
/// (the digest block + `failed_pkg_msg`) is already printed: the CLI
/// boundary must exit 1 WITHOUT a resume-list notice or an `emerge:`
/// line (real prints neither -- see the call site).
pub(crate) fn binpkg_digest_failure(cpv: &str) -> String {
    format!("{cpv}: binpkg digest verification failed")
}

/// Whether `e` is exactly one `binpkg_digest_failure`. A `--keep-going`
/// combined message embeds the same text among newlines and must keep
/// the normal tail, so only a lone single-line failure matches.
pub(crate) fn is_binpkg_digest_failure(e: &str) -> bool {
    !e.contains('\n') && e.ends_with(": binpkg digest verification failed")
}

/// Real `_emerge/BinpkgVerifier._start`'s `os.stat` ENOENT text
/// (`BinpkgVerifier.py:40-60`, backlog #187), as one block (each line
/// `\n`-terminated, no surrounding blanks -- the bed oracle's bytes):
/// the stale-index arm when the cpv is still in the `<pkgdir>/Packages`
/// index (the file was removed without refreshing it), else real's
/// `!!! Fetching Binary failed` arm. Pure shaper, for tests and the
/// `build.log` write alike.
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

/// Real `bintree.dbapi.cpv_exists` as `BinpkgVerifier._start` consults
/// it (`BinpkgVerifier.py:48`): does the local `<pkgdir>/Packages`
/// index still list this cpv. The narrowing mirrors
/// [`local_index_record`] minus the basename tie-break (there is no
/// file to name it after) -- with several same-`CPV` stanzas (multi-
/// instance `BUILD_ID`s) the entry's `BUILD_ID` picks.
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
        // A `BUILD_ID` that matches nothing still leaves the plain
        // `CPV` hit: real's `cpv_exists` passes a plain string, which
        // `_instance_key_multi_instance` resolves to the latest
        // instance (`virtual.py:53-68`) -- any surviving instance
        // keeps it true.
        if !narrowed.is_empty() {
            return true;
        }
    }
    true
}

/// The error for a merge-time missing binpkg file whose real output
/// (the `missing_binpkg_block` text + `failed_pkg_msg`) is already
/// printed: the CLI boundary must exit 1 WITHOUT a resume-list notice
/// or an `emerge:` line (real exits a merge failure via `FAILURE`,
/// exactly like the digest arm).
pub(crate) fn binpkg_missing_failure(cpv: &str) -> String {
    format!("{cpv}: non-existent binary package")
}

/// Whether `e` is exactly one `binpkg_missing_failure` (same lone-
/// failure shape as [`is_binpkg_digest_failure`]).
pub(crate) fn is_binpkg_missing_failure(e: &str) -> bool {
    !e.contains('\n') && e.ends_with(": non-existent binary package")
}

/// The `<pkgdir>/Packages` index record vouching for a local binpkg
/// merge (real `bintree._get_digests`' own index read, backlog #174):
/// the stanza whose `CPV` is `<category>/<package>-<version>`. With
/// several (multi-instance `BUILD_ID`s), the `BUILD_ID` picks -- else
/// the stanza whose `PATH` basename is the located file's own name, so
/// a moved file still verifies against its own record. `None` when no
/// stanza vouches (the file was synthesized at scan from its own
/// bytes). A record without `SIZE` still returns here -- real's
/// "verifies nothing without one" (`BinpkgVerifier._start`'s early OK)
/// lives in `verify_binpkg_against_index`, not in the lookup.
fn local_index_record(
    pkgdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
    binpkg_path: &Path,
) -> Option<std::collections::HashMap<String, String>> {
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

/// The on-disk binpkg for `<cat>/<package>-<version>` in `$PKGDIR`.
///
/// Two real naming contracts (real `bintree.getname` /
/// `_allocate_filename` vs `_allocate_filename_multi`):
///  - single instance: `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}`
///  - `FEATURES=binpkg-multi-instance`: `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}`
///    -- a `<pn>/` subdir and a `-<build_id>` suffix.
///
/// A `build_id` (from the resolved candidate / `Packages` index) picks
/// the exact multi-instance file; without one, the `<pn>/` subdir is
/// still scanned for a `<pf>-<id>` file (an older index with no
/// `BUILD_ID` field), preferring the highest `<id>`.
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

/// Fetch `<sync_uri>/<PATH>` (or the default `<cat>/<pf>.tbz2`) into
/// `$PKGDIR`, then verify it against the index `SIZE` and, if present,
/// the `MD5` / `SHA1` fields. A mismatch removes the file and fails.
pub(crate) fn download_and_verify(
    sync_uri: &str,
    record: &std::collections::HashMap<String, String>,
    category: &str,
    package: &str,
    version: &str,
    pkgdir: &Path,
) -> Result<std::path::PathBuf, String> {
    let rel = record
        .get("PATH")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{category}/{package}-{version}.tbz2"));
    let dest = pkgdir.join(&rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let uri = format!("{}/{rel}", sync_uri.trim_end_matches('/'));

    if let Some(local) = uri.strip_prefix("file://") {
        std::fs::copy(local, &dest).map_err(|e| format!("{local}: {e}"))?;
    } else {
        crate::fetch::wget_fetch(&uri, &dest)?;
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

    // Real `bintree.gettbz2` / `_get_digests` + `digestCheck`: the
    // `Packages` record's `MD5`/`SHA1` fields are the md5/sha1 of the
    // whole `.tbz2`, and real verifies every digest present. Same here --
    // each present field is checked; a mismatch removes the file and
    // fails.
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
    Ok(dest)
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

    /// Serves each `routes` entry (`"/path" -> body`) over real plain
    /// HTTP on `127.0.0.1`, one response per connection, for `requests`
    /// connections total -- enough for the `Packages` fetch + N binpkg
    /// fetches a `--getbinpkg` run makes (each `wget` uses its own
    /// `Connection: close`).
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

    /// [`serve`] with an explicit status line per route
    /// (`"/path" -> ("500 Internal Server Error", body)`), for the
    /// fault-injection refresh tests below -- unlisted paths still 404.
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
        // A genuine binhost index always carries both headers real
        // gates on (`TIMESTAMP`, `bintree.py:1722-1732`; `VERSION`,
        // `bintree.py:1735-1744`), so the helper stamps both.
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

        // Multi-instance: `<cat>/<pn>/<pf>-<build_id>.gpkg.tar` (real
        // `_allocate_filename_multi`). This is the `noto-20260901-1.gpkg.tar`
        // layout `resolve_local_binpkg` used to miss entirely.
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
        // The real gpkg fixture, placed in the multi-instance layout.
        std::fs::create_dir_all(pkgdir.join("dev-libs/gpkgreadpkg")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
            pkgdir.join("dev-libs/gpkgreadpkg/gpkgreadpkg-1.0-1.gpkg.tar"),
        )
        .unwrap();

        let mut entry = graph_entry("gpkgreadpkg", CandidateSource::Binary, "1.0");
        entry.build_id = Some("1".into());

        merge_one_binary_entry(
            &entry,
            &Config::default(),
            &root,
            &pkgdir,
            &tmp.join("pt"),
            &MergeOptions::default(),
            mrg_director::MergeProgress::single(),
            &crate::emerge_build::StatusDisplay::for_tests(),
        )
        .expect("multi-instance local gpkg merges");

        assert!(
            root.join("var/db/pkg/dev-libs/gpkgreadpkg-1.0/CONTENTS")
                .is_file()
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_one_binary_entry_fails_a_truncated_index_vouched_binpkg_at_merge() {
        // Backlog #174, end to end at the merge boundary: real `emerge
        // --oneshot --usepkgonly l32/faultpkg` with the gpkg truncated
        // to half *selects* the binary (the `Packages` stanza vouches
        // for it at scan) and fails at merge with `!!! Digest
        // verification failed:` (`Failed on size verification`,
        // `Got`/`Expected`), renames the file to
        // `._checksum_failure_.<rand>`, logs the block to the package
        // `build.log`, and reports `>>> Failed to emerge <cpv> for
        // <root>, Log file:` + `>>>  '<log>'` (real
        // `Scheduler._failed_pkg_msg`) -- rc 1 with a clean root. The
        // merge must fail *before* unpacking anything: no vdb entry, no
        // installed file. The `>>>` lines go to stdout (pinned by the
        // pmtest contract test); the `Err` is the silent
        // `binpkg_digest_failure` sentinel, so the CLI boundary adds no
        // resume notice or `emerge:` line.
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

        // The scan half of the same shape: the truncated file is
        // accepted on the index's word (no `!!! Invalid binary
        // package`), so resolution can select it.
        let scanned = crate::binpkg::populate_local_pkgdir(&pkgdir, true).expect("scan succeeds");
        assert_eq!(scanned.len(), 1, "{scanned:?}");

        let pt = tmp.join("pt");
        let entry = graph_entry("gpkgreadpkg", CandidateSource::Binary, "1.0");
        let err = merge_one_binary_entry(
            &entry,
            &Config::default(),
            &root,
            &pkgdir,
            &pt,
            &MergeOptions::default(),
            mrg_director::MergeProgress::single(),
            &crate::emerge_build::StatusDisplay::for_tests(),
        )
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
        // Real `SchedulerInterface.output(msg, log_path)`: the digest
        // block lands in the package `build.log` too, non-empty so real
        // `_locate_failure_log` reports it.
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
    fn merge_one_binary_entry_aborts_a_stale_index_binary_like_real() {
        // Backlog #187, end to end at the merge boundary: the bed's
        // F3a shape (`l32/faultpkg` removed from `$PKGDIR`, its
        // `Packages` stanza kept). Real selects the indexed binary
        // (trusted index, `bintree._populate_local(reindex=False)`)
        // and fails at merge: `BinpkgVerifier._start` stats the
        // missing file, finds the cpv still in the dbapi
        // (`cpv_exists`), and prints `!!! Tried to use non-existent
        // binary for '<cpv>'` + `!!! Likely caused by an outdated
        // index. Run 'emaint binhost -f'.` to stdout AND the package
        // `build.log`, followed by real's `>>> Failed to emerge
        // <cpv>[ for <root>][, Log file:]` tail (`Scheduler.py`,
        // byte-pinned by `failed_pkg_msg_matches_real_failed_pkg_msg_bytes`).
        // The `>>>` lines go to stdout (pinned by the pmtest contract
        // test); the `Err` is the silent `binpkg_missing_failure`
        // sentinel, so the CLI boundary adds no resume notice or
        // `emerge:` line -- rc 1 with a clean root. Portuale used to
        // fail here with its own `no binpkg file under ...` error.
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

        // The scan half of the same shape: the removed file's stanza
        // rejoins the pool, so resolution can select the binary.
        let scanned = crate::binpkg::populate_local_pkgdir(&pkgdir, true).expect("scan succeeds");
        assert_eq!(scanned.len(), 1, "{scanned:?}");
        assert_eq!(
            scanned[0].get("CPV").map(String::as_str),
            Some("dev-libs/packagepkg-1.0")
        );

        let pt = tmp.join("pt");
        let entry = graph_entry("packagepkg", CandidateSource::Binary, "1.0");
        let err = merge_one_binary_entry(
            &entry,
            &Config::default(),
            &root,
            &pkgdir,
            &pt,
            &MergeOptions::default(),
            mrg_director::MergeProgress::single(),
            &crate::emerge_build::StatusDisplay::for_tests(),
        )
        .expect_err("a stale-index binary must fail at merge, not fall back");
        assert_eq!(
            err, "dev-libs/packagepkg-1.0: non-existent binary package",
            "{err}"
        );
        assert!(
            is_binpkg_missing_failure(&err),
            "the CLI boundary must recognize the silent sentinel"
        );
        // Real `SchedulerInterface.output(msg, log_path)`: the two
        // `!!!` lines land in the package `build.log` too, non-empty
        // so real `_locate_failure_log` reports it -- byte for byte
        // real's `BinpkgVerifier.py:50-51` text.
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
        // Backlog #187: real's other `BinpkgVerifier._start` ENOENT arm
        // (`BinpkgVerifier.py:57`) -- the cpv is NOT in the index
        // (`cpv_exists` false), so the text is `!!! Fetching Binary
        // failed for '<cpv>' instead of the stale-index pair. Same
        // stdout + `build.log` + `>>> Failed to emerge ...` tail and
        // the same silent sentinel as the stale arm.
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();

        let pt = tmp.join("pt");
        let entry = graph_entry("packagepkg", CandidateSource::Binary, "1.0");
        let err = merge_one_binary_entry(
            &entry,
            &Config::default(),
            &root,
            &pkgdir,
            &pt,
            &MergeOptions::default(),
            mrg_director::MergeProgress::single(),
            &crate::emerge_build::StatusDisplay::for_tests(),
        )
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
    fn failed_pkg_msg_matches_real_failed_pkg_msg_bytes() {
        // Real `Scheduler._failed_pkg_msg(..., "emerge", "for")`
        // (`Scheduler.py:2366`): each `_status_msg` emits its own
        // leading blank line, then `>>> <msg>`; the log line carries
        // real's own leading space (`f" '{log}'"`); ` for <root>` only
        // when `ROOT != "/"`. The bed oracle's tail is the `ROOT == "/"`
        // row below, byte for byte.
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

    /// A writable copy of portage's own committed GnuPG test keyring
    /// (see `binpkg.rs`'s own `test_gpg_home` doc comment), for the
    /// merge-time signature tests below. `gpg` refuses a homedir it
    /// doesn't own outright, so the committed tree itself is never used
    /// directly.
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
        // The committed signed fixture (`binpkg.rs`'s own verify tests
        // cover the container layer) merges end to end with the test
        // keyring: signature policy enforced, image + metadata land in
        // the vdb like any other binpkg merge.
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
        // Flip a byte of the signed container's `image.tar.gz` (via an
        // unpack-mutate-repack, so the tar framing stays parseable):
        // the merge must fail on the detached `.sig` before anything
        // is unpacked -- no vdb entry, no image file under ROOT.
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
        // List the prefix's files, not the prefix dir: a `<prefix>/`
        // directory member would be refused by real's one-level
        // structure check (`#58` S5).
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

        // A real vdb entry, with CONTENTS naming the file and the
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
        // The saved env + ebuild are kept in the vdb now (real portage
        // does; portuale needs them for pkg_preinst/pkg_postinst). This
        // fixture's DEFINED_PHASES is `install` only, so no hook ran.
        assert!(vdb.join("environment.bz2").is_file());
        assert!(vdb.join("packagepkg-1.0.ebuild").is_file());

        // Real `_consolidate_to_metadata_file`: the vdb entry carries a
        // consolidated `metadata` file -- `#format=1` header, sorted
        // `KEY=value` lines for the per-field files, `#dir_mtime=` last
        // and matching the entry dir's own `st_mtime_ns` (real's reader
        // rejects a stale value, which is what broke `emerge -C`).
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
        // Regression for the broken `/var/db/pkg/<cat>/<pf>` a gpkg merge
        // used to write: `CONTENTS` prefixed with `/image`, no
        // `BINPKGMD5`, no consolidated `metadata` file -- all of which
        // blocked `emerge -C` (real portage's and portuale's).
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

        // Real `_emerge/Binpkg._start_task`: BINPKGMD5 = md5 of the whole
        // binpkg file.
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
        // Real `dblink.merge()`'s `_collision_protect` -- now shared with
        // the source `merge_after_install`. A pre-existing vdb entry owns
        // `/usr/share/packagepkg/hello.txt`; `FEATURES=collision-protect`
        // must abort the binpkg merge rather than overwrite it.
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

        // The fixture's own pkg_preinst `die`s if the payload is already
        // merged and pkg_postinst `die`s if it is not -- so this file
        // existing with both lines proves the real treewalk() ordering
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

    /// Real `Scheduler._run_pkg_pretend` runs `pkg_pretend` for a binary
    /// package too -- portuale folds it into `merge_binpkg`'s hook chain,
    /// before `pkg_setup`. The fixture's `pkg_pretend` `die`s if its own
    /// payload is already merged, so a `pretend` line landing first (and
    /// the merge still succeeding) proves it ran at the right point.
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

    /// L1-c: real `dblink.treewalk` regenerates the vdb `environment.bz2`
    /// from the *live* merge-time environment (`PORTAGE_UPDATE_ENV`
    /// around the `postinst` phase), so it carries the merge-time
    /// `FEATURES`, not the binpkg's build-time one. `merge_binpkg` runs
    /// that regeneration unconditionally now.
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
        // L1-f: the phase env is filtered to real's `environ_whitelist`,
        // so a non-whitelisted process-env var (`cargo test` always sets
        // `CARGO_MANIFEST_DIR`) never reaches the regenerated vdb env.
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

    /// Real `config.environ()`'s `filter_calling_env`: a binary merge's
    /// calling env is narrowed to real's `environ_whitelist`, so a
    /// harness/portuale-only key in `build_env` never reaches the
    /// regenerated vdb environment (L3 finding: the L2 real-set control
    /// leg leaked `PORTAGE_RUNNING_ROOT` / `L1_SKIP_PORTAGE_UPGRADE` /
    /// `GNUMAKEFLAGS`).
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

        // The full real interleaving: 2.0 setup+preinst, then 1.0's
        // prerm+postrm (from 1.0's own vdb-stored environment.bz2, inside
        // treewalk()'s replace loop), then 2.0 postinst.
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

        // An older same-slot version already installed: it owns one
        // file the new binpkg also ships (`hello.txt`, a shared path)
        // and one it does not (`old-only.txt`, a genuine orphan).
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
    fn download_and_verify_fetches_then_size_and_md5_checks() {
        // Real digests of the committed fixture .tbz2 (`md5sum`/`sha1sum`,
        // not invented).
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
        let got =
            download_and_verify(&base, &ok, "dev-libs", "packagepkg", "1.0", &pkgdir).unwrap();
        assert_eq!(got, pkgdir.join("dev-libs/packagepkg-1.0.tbz2"));
        assert_eq!(std::fs::metadata(&got).unwrap().len(), 4618);

        // Wrong SIZE -> rejected, file removed.
        let bad_size = record(&[("SIZE", "9999"), ("PATH", "dev-libs/packagepkg-1.0.tbz2")]);
        let err = download_and_verify(&base, &bad_size, "dev-libs", "packagepkg", "1.0", &pkgdir)
            .unwrap_err();
        assert!(err.contains("!= index SIZE 9999"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());

        // Right SIZE, wrong MD5 -> rejected, file removed.
        let bad_md5 = record(&[
            ("SIZE", "4618"),
            ("MD5", "00000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(&base, &bad_md5, "dev-libs", "packagepkg", "1.0", &pkgdir)
            .unwrap_err();
        assert!(err.contains("MD5 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());

        // Right SIZE + MD5, wrong SHA1 -> rejected, file removed (real
        // `digestCheck` verifies every digest present, not just MD5).
        let bad_sha1 = record(&[
            ("SIZE", "4618"),
            ("MD5", FIXTURE_MD5),
            ("SHA1", "0000000000000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(&base, &bad_sha1, "dev-libs", "packagepkg", "1.0", &pkgdir)
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
        // Real `gzip` output of `plain`.
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
        };
        refresh_binhost_indexes(std::slice::from_ref(&binrepo), &root, false);
        let cached = binrepo.packages_dir(&root).join("Packages");
        // The promoted index is cached with a fresh DOWNLOAD_TIMESTAMP
        // header (real `bintree.py:1806`), so it differs from the served
        // bytes by exactly that line.
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
        // Review fix (#175): a downloaded-but-corrupt `Packages.gz`
        // must NOT fall back to plain `Packages`. Real's
        // `gzip.BadGzipFile` subclasses `OSError` (verified against
        // the stdlib), so it escapes the `("Packages.gz", "Packages")`
        // loop straight to the outer `except OSError`
        // (`bintree.py:1790-1809`). The stub serves garbage for
        // `Packages.gz` and a valid index for `Packages`: pre-fix the
        // cache would hold the plain body (the fallback was tried);
        // post-fix no cache file is left at all.
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
        };
        refresh_binhost_indexes(std::slice::from_ref(&binrepo), &root, false);
        assert!(
            !binrepo.packages_dir(&root).join("Packages").exists(),
            "a corrupt Packages.gz warns with no plain-Packages fallback"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_warns_with_the_untrimmed_sync_uri_but_fetches_stripped_urls() {
        // Review fix (#175): real warns with `base_url` as configured
        // (`bintree.py:1488,1795`), so a `sync-uri` with a trailing
        // slash keeps it in the warning -- while fetch URLs are still
        // built from the stripped form. The stub 500s only the
        // stripped `/sub/...` paths: the `HTTP Error 500` detail in
        // the returned warning proves the fetches hit the stripped
        // URLs (unstripped doubleslash paths would 404 with a wget
        // summary instead), and the `from '.../sub/'` line proves the
        // warning kept the configured slash.
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
        };
        let warning = refresh_one_binrepo(&binrepo, &root, false)
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
        // Real `str(urllib.error.HTTPError(url, 500, "Internal Server
        // Error", ...))` is `"HTTP Error 500: Internal Server Error"`
        // (verified against the stdlib, not invented). wget's own
        // `awaiting response... 500 Internal Server Error` line carries
        // the server's status line verbatim, so the shaped detail is
        // byte-identical -- this transcript is a live `wget` capture
        // against the bed's own 500 stub shape, not hand-written.
        let transcript = "--2026-09-27 06:19:14--  http://127.0.0.1:18765/Packages\n\
             Connecting to 127.0.0.1:18765... connected.\n\
             HTTP request sent, awaiting response... 500 Internal Server Error\n\
             2026-09-27 06:19:14 ERROR 500: Internal Server Error.\n";
        assert_eq!(
            wget_http_error(transcript).as_deref(),
            Some("HTTP Error 500: Internal Server Error")
        );
        // No response line (DNS failure, refused connection, ...): real
        // would print its own urllib `URLError` string there, which wget
        // cannot reproduce -- the caller falls back to the fetch summary.
        assert_eq!(
            wget_http_error("wget: unable to resolve host address 'example.invalid'\n"),
            None
        );
        assert_eq!(wget_http_error(""), None);
    }

    #[test]
    fn hide_binhost_passwd_masks_only_a_userinfo_password() {
        // Real `fetch.py::_hide_url_passwd` (`//user:secret@` ->
        // `//user:*password*@`), implemented as the same single regex
        // substitution (`fetch.py:73-74`).
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
        // A space inside the password is left alone: real's
        // `[^@\s]+` cannot span it, so the regex never matches.
        assert_eq!(
            hide_binhost_passwd("http://user:pass word@host/"),
            "http://user:pass word@host/"
        );
        // A `/` inside the user is still masked: real's `[^:\s]+`
        // allows it.
        assert_eq!(
            hide_binhost_passwd("http://a/b:c@host/"),
            "http://a/b:*password*@host/"
        );
    }

    #[test]
    fn binhost_fetch_warning_matches_reals_two_line_shape() {
        // Real `bintree.py:1793-1798` for the bed's F3 stub (repo
        // `l32-500`, every GET/HEAD 500): the leading blank line, the
        // two `!!!` lines, the trailing blank line -- all on stderr.
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
        // Backlog #175: every index URL 500s. The refresh must not fail
        // (it returns nothing now) and must leave no cache file behind,
        // so resolution proceeds against the local pool; the real
        // `!!! [repo] ...` pair goes to stderr (shaped by
        // `binhost_fetch_warning`, pinned above -- stderr itself is not
        // capturable from a unit test, but the run log shows the pair
        // and no wget transcript).
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
        };
        refresh_binhost_indexes(std::slice::from_ref(&binrepo), &root, false);
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

    /// Backlog #192: real fetches the remote index first and ignores a
    /// cache-write failure (`bintree.py:1819-1823`) -- no
    /// `create_dir_all` before fetching, in `--pretend` exactly like in
    /// a real merge. A `file://` binhost goes through the same cache
    /// machinery (real caches even local indexes), so with
    /// `<EROOT>/var/cache/edb` unwritable the refresh must still
    /// resolve from the in-memory index, warn about nothing, and write
    /// no cache file.
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
        // Unwritable *ancestor*: the fetch must run before any mkdir,
        // and the write failure must be ignored like real.
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
        };
        let outcome = refresh_one_binrepo(&binrepo, &root, pretend);
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
        // Backlog #192: real `--pretend` on a fetch failure keeps the
        // stale copy with `[name] Local copy of unavailable remote
        // index will be used due to --pretend` (`bintree.py:1801-1808`).
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
        };
        // A stale cache with no TTL headers (the TTL check must not fire).
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(&binrepo, &root, true);
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
        // Backlog #192: outside `--pretend` real drops even the stale
        // copy (`pkgindex = None`, `bintree.py:1809`) -- the same
        // warning pair, no fallback note, no candidates.
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
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(&binrepo, &root, false);
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
        // Real `DOWNLOAD_TIMESTAMP + TTL` freshness
        // (`bintree.py:1535-1545`): no fetch at all, real's own note.
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
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, &cached_body).unwrap();
        let outcome = refresh_one_binrepo(&binrepo, &root, true);
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
        // Real serves the remote index when strictly newer
        // (`int(local) < int(remote)`, `bintree.py:1739-1744`).
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
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(&binrepo, &root, false);
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
        // The mirror arm: an equally-old or older remote keeps the cache
        // silently, with no rewrite (`bintree.py:1739-1744`).
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
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, cached_body).unwrap();
        let outcome = refresh_one_binrepo(&binrepo, &root, false);
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
        // Real `UseCachedCopyOfRemoteIndex("frozen")`
        // (`bintree.py:1528-1533`): never refreshed, used as-is.
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
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, cached_body).unwrap();
        let outcome = refresh_one_binrepo(&binrepo, &root, true);
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

    /// Backlog #192 fix round: real drops a fetched index with no (or
    /// unparseable) `TIMESTAMP` (`bintree.py:1722-1732`) or with a
    /// `VERSION` its `_pkgindex_version_supported` rejects
    /// (`bintree.py:1735-1744`, `_pkgindex_version = 0`) -- `pkgindex =
    /// None`, so even a stale cache contributes nothing for the run, in
    /// `--pretend` exactly like in a real merge. A `file://` binhost
    /// serves `remote_body`; a valid stale cache is seeded to prove it
    /// is suppressed, not resurrected.
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
        };
        let stale = "TIMESTAMP: 5\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/stalepkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let cached = edb_cache_packages_file(&root, &binrepo.sync_uri);
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(&binrepo, &root, pretend);
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
        // refresh tries Packages.gz + Packages.zst (both 404 here) before
        // the plain Packages, then the binpkg download = 4 connections.
        let (base, _h) = serve(routes, 4);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
        }];
        // Non-fatal by design (backlog #175): a failed refresh warns on
        // stderr and resolves against the local pool -- here the plain
        // Packages succeeds, so the live index lands in the edb cache.
        refresh_binhost_indexes(&binrepos, &root, false);
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

        run_merge_plan(
            &[entry],
            &config,
            &[],
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
            false,
            None,
            &[],
            crate::emerge_build::StatusMode::for_tests(),
        )
        .expect("getbinpkg merge succeeds");

        assert!(
            pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file(),
            "the binpkg was downloaded into $PKGDIR"
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
        // refresh tries Packages.gz + Packages.zst (both 404 here) before
        // the plain Packages, then the binpkg download = 4 connections.
        let (base, _h) = serve(routes, 4);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
            frozen: false,
        }];
        refresh_binhost_indexes(&binrepos, &root, false);

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

        run_merge_plan(
            &[entry],
            &config,
            &[],
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
            false,
            None,
            &[],
            crate::emerge_build::StatusMode::for_tests(),
        )
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
        // `emerge --getbinpkg`'s own mixed plan: one `Binary` entry
        // (a local `$PKGDIR` `.tbz2`) and one `Source` entry (built from
        // its fixture ebuild), both merged in the same pass.
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

        run_merge_plan(
            &[
                graph_entry("packagepkg", CandidateSource::Binary, "1.0"),
                graph_entry("binpkgphasepkg", CandidateSource::Ebuild, "1.0"),
            ],
            &Config::default(),
            &repos,
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &options,
            false,
            None,
            &[],
            crate::emerge_build::StatusMode::for_tests(),
        )
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

    /// A resumed binary entry (`remote_binary: false`, `build_id: None`
    /// -- all `resume_entry` records) with no local `$PKGDIR` file must
    /// FAIL with the local-only error, never re-hit the binhost: real
    /// replays a resumed binary against its local bintree only (a
    /// never-downloaded binary isn't resumable at all). The `file://`
    /// binhost below serves a REAL fixture binary, so pre-fix code
    /// downloads it into `$PKGDIR` and merges successfully -- post-fix
    /// the pkgdir stays empty and the error names the local dir.
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
            }],
            ..Default::default()
        };
        // Sanity: the binhost really serves this cpv (a fetch WOULD
        // succeed) -- so the failure below proves no fetch was tried.
        assert!(
            find_remote_binpkg(&config.binrepos, &root, "dev-libs", "binpkgrmpkg", "1.0").is_some()
        );

        // Resumed shape: `graph_entry` defaults to `remote_binary:
        // false`, `build_id: None`, exactly like `resume_entry`.
        let entry = graph_entry("binpkgrmpkg", CandidateSource::Binary, "1.0");
        assert!(!entry.remote_binary);
        // No local stanza vouches for it (empty `$PKGDIR`), so the
        // failure is real's `!!! Fetching Binary failed` arm
        // (`BinpkgVerifier.py:57`, backlog #187) -- never a refetch,
        // and never portuale's old `no binpkg file under ...` text.
        let err = merge_one_binary_entry(
            &entry,
            &config,
            &root,
            &pkgdir,
            &tmp.join("pt"),
            &MergeOptions::default(),
            mrg_director::MergeProgress::single(),
            &crate::emerge_build::StatusDisplay::for_tests(),
        )
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
