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
use std::sync::OnceLock;

/// Portage's `fetch._hide_url_passwd`, verbatim: mask `//user:secret@` as
/// `//user:*password*@` with a single global substitution, so edge behaviour
/// matches (a password with whitespace is left alone; a user with `/` is
/// still masked).
pub(crate) fn hide_binhost_passwd(url: &str) -> String {
    static PASSWD: OnceLock<regex::Regex> = OnceLock::new();
    PASSWD
        .get_or_init(|| regex::Regex::new(r"//([^:\s]+):[^@\s]+@").expect("static regex is valid"))
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
/// warn per repo, like Portage. Under this binrepo's `gpkg_only` rule
/// (Portage 3.0.83: `FEATURES=binpkg-request-signature` or its own
/// `verify-signature`) xpak `PATH` entries are dropped with the ignored
/// warning, and unknown `PATH`s with an `Unsupported …` line. The on-disk/EDB
/// cache keeps the unfiltered index (Portage filters when reading, not when
/// writing, so a later run without the option is unpoisoned). Local
/// `PKGDIR` instances are never filtered.
///
/// A cached `TIMESTAMP` turns the fetch into a conditional GET
/// (`If-Modified-Since: TIMESTAMP + 5 s`; 304 keeps the cache). Deliberate
/// narrowings: Portage's "did not respect If-Modified-Since" warning is not
/// modelled; a
/// fetched index with no/unparseable `TIMESTAMP` or an unsupported `VERSION`
/// is dropped with Portage's own warning, discarding even a stale cache; no
/// `--verbose` detail, `ssh://`, pool filtering, or trust-helper gating.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote
/// Arguments of [`refresh_binhost_indexes`].
pub struct RefreshBinhostIndexesArgs<'a> {
    pub binrepos: &'a [BinRepo],
    pub root: &'a Path,
    pub pretend: bool,
    pub cli_exclude: &'a [String],
    pub cli_include: &'a [String],
    pub binpkg_request_signature: bool,
}

pub fn refresh_binhost_indexes(args: RefreshBinhostIndexesArgs<'_>) {
    let RefreshBinhostIndexesArgs {
        binrepos,
        root,
        pretend,
        cli_exclude,
        cli_include,
        binpkg_request_signature,
    } = args;
    // Descending priority: Portage's `_populate_remote` runs
    // `reversed(...)` over the ascending config, so every per-repo message
    // (fetch warnings, the exclude/include conflict warnings, the `Local
    // copy …` notes, the `gpkg_only` warnings) prints highest-priority
    // first. `seen` is the cross-repo `cpv_exists` gate: an instance a
    // higher-priority repo already injected shadows the same instance
    // below, before its `gpkg_only` check.
    let mut seen: std::collections::HashSet<RemoteInstanceKey> = std::collections::HashSet::new();
    for binrepo in binrepos.iter().rev() {
        let filter = GetbinpkgFilter::for_binrepo(binrepo, cli_exclude, cli_include);
        let gpkg_only = binrepo_gpkg_only(binrepo, binpkg_request_signature);
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo,
            root,
            pretend,
            filter: &filter,
            gpkg_only,
            seen: &mut seen,
        });
        if let Some(warning) = outcome.stderr_warning {
            eprint!("{warning}");
        }
        if let Some(note) = outcome.stdout_note {
            eprint!("{note}");
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

/// Per-binrepo `gpkg_only`: a repo whose index must be signature-clean
/// serves no xpak entry (`bintree._populate_remote`). True when the run
/// requests signed binaries
/// (`FEATURES=binpkg-request-signature`, which also covers local binpkgs)
/// or when this binrepo verifies signatures. An absent
/// `verify-signature` key counts as true (stock Portage ships
/// `[DEFAULT] verify-signature = true` in the global `binrepos.conf`).
// Portage: lib/portage/dbapi/bintree.py, _populate_remote
pub(crate) fn binrepo_gpkg_only(binrepo: &BinRepo, binpkg_request_signature: bool) -> bool {
    binpkg_request_signature || binrepo.verify_signature
}

/// The ignored-remote-xpak warning: printed once per binrepo, naming
/// the index's `URI` header (else the sync URI, which Portage does not
/// mask here). Plain text: the colour Portage's
/// `colorize("WARN", …)` adds under `--color=y` is not modelled, like the
/// other binhost warnings here.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote_repo
pub(crate) fn remote_xpak_ignored_warning(binrepo_name: &str, remote_base_uri: &str) -> String {
    format!(
        "[{binrepo_name}] Remote XPAK binpkgs in '{remote_base_uri}' ignored: signatures are missing (FEATURES=binpkg-request-signature or verify-signature in binrepos.conf).\n"
    )
}

/// Portage's `InvalidBinaryPackageFormat` text for a remote `PATH` with an
/// unknown extension (an absent `PATH` reads as `''`): one line per entry,
/// with no once-per-repo gate.
// Portage: lib/portage/binpkg.py, get_binpkg_format (remote=True)
pub(crate) fn unsupported_binpkg_format_warning(path: &str) -> String {
    format!("Unsupported binary package format from '{path}'\n")
}

/// The inter-repo overlap gate (`bintree._populate_remote_repo`): an
/// entry whose instance a higher-priority repo already injected is skipped before its `gpkg_only` check, so neither the
/// `Unsupported …` line nor the once-per-repo xpak warning fires for it.
/// The key mirrors `dbapi._instance_key` under binpkg-multi-instance
/// (`(cpv, build_id, file_size, build_time, mtime)`); the index carries no
/// `MTIME` key, so same-`(CPV, BUILD_ID, SIZE, BUILD_TIME)` entries share
/// one key. Like Portage, only injected (kept) entries populate the set.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RemoteInstanceKey {
    cpv: String,
    build_id: String,
    size: String,
    build_time: String,
}

fn remote_instance_key(entry: &std::collections::HashMap<String, String>) -> RemoteInstanceKey {
    let get = |k: &str| entry.get(k).cloned().unwrap_or_default();
    RemoteInstanceKey {
        cpv: get("CPV"),
        build_id: get("BUILD_ID"),
        size: get("SIZE"),
        build_time: get("BUILD_TIME"),
    }
}

/// Split a binrepo's fetched `Packages` entries into the pool
/// (`kept`) plus the `gpkg_only` stderr text. A `.gpkg.tar` `PATH` is kept;
/// an xpak one (`.tbz2`/`.xpak`, Portage's `SUPPORTED_XPAK_EXTENSIONS`) is
/// dropped with the once-per-repo ignored warning; anything else (unknown
/// or absent `PATH`) is dropped with its own `Unsupported …` line. Runs
/// after the exclude/include filter, like Portage's populate order.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote_repo
pub(crate) fn apply_gpkg_only(
    entries: Vec<std::collections::HashMap<String, String>>,
    binrepo_name: &str,
    remote_base_uri: &str,
) -> (Vec<std::collections::HashMap<String, String>>, String) {
    let mut kept = Vec::with_capacity(entries.len());
    let mut stderr = String::new();
    let mut warned = false;
    for entry in entries {
        let path = entry.get("PATH").map(String::as_str).unwrap_or("");
        if path.ends_with(".gpkg.tar") {
            kept.push(entry);
        } else if path.ends_with(".tbz2") || path.ends_with(".xpak") {
            if !warned {
                warned = true;
                stderr.push_str(&remote_xpak_ignored_warning(binrepo_name, remote_base_uri));
            }
        } else {
            stderr.push_str(&unsupported_binpkg_format_warning(path));
        }
    }
    (kept, stderr)
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
        fetchcommand: None,
        resumecommand: None,
        getbinpkg_exclude: Vec::new(),
        getbinpkg_include: Vec::new(),
    }
    .packages_dir(root)
    .join("Packages")
}
/// Arguments of [`refresh_one_binrepo`].
struct RefreshOneBinrepoArgs<'a> {
    binrepo: &'a BinRepo,
    root: &'a Path,
    pretend: bool,
    filter: &'a GetbinpkgFilter,
    gpkg_only: bool,
    seen: &'a mut std::collections::HashSet<RemoteInstanceKey>,
}

fn refresh_one_binrepo(args: RefreshOneBinrepoArgs<'_>) -> RefreshOutcome {
    let RefreshOneBinrepoArgs {
        binrepo,
        root,
        pretend,
        filter,
        gpkg_only,
        seen,
    } = args;
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
    // keeps the unfiltered index (see `refresh_binhost_indexes`). Under
    // `gpkg_only` the exclude/include survivors are split once more (xpak
    // dropped with the warning, unknown `PATH`s with their own line).
    let mut ok_entries =
        |header: &std::collections::HashMap<String, String>,
         entries: Vec<std::collections::HashMap<String, String>>| {
            let entries = filter.apply(entries);
            // Inter-repo overlap, before the `gpkg_only` check: an instance
            // a higher-priority repo already injected never reaches this
            // repo's format check or warnings.
            let entries: Vec<_> = entries
                .into_iter()
                .filter(|e| !seen.contains(&remote_instance_key(e)))
                .collect();
            let (entries, gpkg_warning) = if gpkg_only {
                // Portage reads the index's own `URI` header, else the base URL.
                let base = header.get("URI").map(String::as_str).unwrap_or(uri);
                apply_gpkg_only(entries, &binrepo.name, base)
            } else {
                (entries, String::new())
            };
            // Only injected entries populate the gate: dropped ones must not
            // shadow the same instance below.
            for e in &entries {
                seen.insert(remote_instance_key(e));
            }
            if !gpkg_warning.is_empty() {
                eprint!("{gpkg_warning}");
            }
            ok(portage_repo::BinaryIndex::from_entries(entries));
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
        ok_entries(&cached_header, cached_entries.clone());
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
        ok_entries(&cached_header, cached_entries.clone());
        return RefreshOutcome {
            stderr_warning: None,
            stdout_note: Some(use_cached_note(&binrepo.name, "within TTL")),
        };
    }

    // A conditional GET against the cached `TIMESTAMP`, like Portage's `_urlopen`.
    let if_modified_since = cached_header
        .get("TIMESTAMP")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|_| cached_text.is_some());
    // The fetch runs BEFORE any cache-dir write and lands in temp files, never
    // straight into the cache: a stale cache must survive a failed or older
    // remote index.
    match fetch_remote_index_text(uri, if_modified_since) {
        Err(failures) => {
            let warning = Some(binhost_fetch_warning(
                &binrepo.name,
                raw,
                &http_or_summary(&failures),
            ));
            if pretend {
                // Keep the stale copy and say so.
                ok_entries(&cached_header, cached_entries.clone());
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
        Ok(None) => {
            // 304: Portage's `UseCachedCopyOfRemoteIndex("up-to-date")`, no rewrite.
            ok_entries(&cached_header, cached_entries.clone());
            RefreshOutcome {
                stderr_warning: None,
                stdout_note: Some(use_cached_note(&binrepo.name, "up-to-date")),
            }
        }
        Ok(Some(remote_text)) => {
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
            // or older remote keeps the cache, with no rewrite.
            let local_ts = cached_header
                .get("TIMESTAMP")
                .and_then(|s| s.parse::<i64>().ok());
            let remote_ts = remote_header
                .get("TIMESTAMP")
                .and_then(|s| s.parse::<i64>().ok());
            let stale_remote = match (local_ts, remote_ts) {
                (Some(local), Some(remote)) => local >= remote,
                _ => false,
            };
            if stale_remote {
                ok_entries(&cached_header, cached_entries.clone());
                let up_to_date =
                    !uri.starts_with("file://") && local_ts.is_some() && local_ts == remote_ts;
                return RefreshOutcome {
                    stderr_warning: None,
                    stdout_note: up_to_date.then(|| use_cached_note(&binrepo.name, "up-to-date")),
                };
            }
            let now = unix_now() as u64;
            let staged = stamp_download_timestamp(&remote_text, now);
            ok_entries(&remote_header, remote_entries);
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
    // `rest` keeps the blank separator line; the header lines below each end
    // with their own newline, so cut at the header's last newline (Portage
    // reads a second blank line as the end of the index: zero packages).
    let (header, rest) = match text.find("\n\n") {
        Some(i) => text.split_at(i + 1),
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
fn fetch_remote_index_text(
    uri: &str,
    if_modified_since: Option<i64>,
) -> Result<Option<String>, Vec<portage_fetch::QuietFetchError>> {
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
                    Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
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
        return std::fs::read_to_string(&plain).map(Some).map_err(|e| {
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
    let extra_args: Vec<String> = if_modified_since
        .map(|ts| vec![format!("--header=If-Modified-Since: {}", http_date(ts + 5))])
        .unwrap_or_default();
    let mut failures: Vec<portage_fetch::QuietFetchError> = Vec::new();
    for (ext, tool) in [("gz", "gzip"), ("zst", "zstd")] {
        let compressed = tmp(&mut tmps, ext);
        match crate::fetch::wget_fetch_quiet(
            &format!("{uri}/Packages.{ext}"),
            &compressed,
            &extra_args,
        ) {
            Err(e) if is_not_modified(&e.stderr) => {
                cleanup(&tmps);
                return Ok(None);
            }
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
                        return Ok(Some(text));
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
    match crate::fetch::wget_fetch_quiet(&format!("{uri}/Packages"), &plain_tmp, &extra_args) {
        Ok(()) => {
            let result = std::fs::read_to_string(&plain_tmp).map(Some).map_err(|e| {
                vec![portage_fetch::QuietFetchError {
                    summary: format!("{}: {e}", plain_tmp.display()),
                    stderr: String::new(),
                }]
            });
            cleanup(&tmps);
            result
        }
        Err(e) if is_not_modified(&e.stderr) => {
            cleanup(&tmps);
            Ok(None)
        }
        Err(e) => {
            failures.push(e);
            cleanup(&tmps);
            Err(failures)
        }
    }
}

/// The wget transcript of a conditional GET the server answered with 304.
// Portage: lib/portage/dbapi/bintree.py, _populate_remote (`err.code == 304`)
fn is_not_modified(stderr: &str) -> bool {
    stderr
        .lines()
        .any(|l| l.contains("awaiting response... 304"))
}

/// RFC 1123 GMT date of a Unix timestamp, as Python's `formatdate(usegmt=True)`.
// Portage: lib/portage/util/_urlopen.py, timestamp_to_http
fn http_date(ts: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{}, {day:02} {} {year} {:02}:{:02}:{:02} GMT",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(month - 1) as usize],
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
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

/// Whether an index record carries a `pkg_pretend` the pass must run:
/// EAPI 4+ with `pretend` in `DEFINED_PHASES`.
fn index_record_needs_pretend(record: &std::collections::HashMap<String, String>) -> bool {
    let get = |key: &str| record.get(key).map(|s| s.trim().to_string());
    let eapi_ok = !matches!(get("EAPI").as_deref(), Some("0" | "1" | "2" | "3"));
    let pretend_defined =
        get("DEFINED_PHASES").is_some_and(|d| d.split_whitespace().any(|p| p == "pretend"));
    eapi_ok && pretend_defined
}

/// Arguments of [`binary_entry_needs_pretend`].
pub(crate) struct BinaryEntryNeedsPretendArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) pkgdir: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
}

/// Portage's `_pkg_needs_pretend` EAPI/`DEFINED_PHASES` half for a binary
/// entry: a local `$PKGDIR` file is gated on its `<pkgdir>/Packages`
/// stanza; a remote entry with no local file is gated on the remote index
/// record (Portage reads `x.metadata`, the bintree record), like the file
/// the pass is about to fetch. Unreadable local metadata fails open so the
/// pretend itself reports; a remote entry with no record stays silent (the
/// merge reports the missing file).
// Portage: _emerge/Scheduler.py, `_pkg_needs_pretend`
fn binary_entry_needs_pretend(args: BinaryEntryNeedsPretendArgs<'_>) -> bool {
    let BinaryEntryNeedsPretendArgs {
        entry,
        pkgdir,
        config,
        root,
    } = args;
    let Some(version) = crate::emerge_build::entry_version(entry) else {
        return false;
    };
    let Some(binpkg_path) = resolve_local_binpkg(
        pkgdir,
        &entry.category,
        &entry.package,
        version,
        entry.build_id.as_deref(),
    ) else {
        // No local file: only a remote entry may still pretend, from its
        // index record. A resumed or fresh local entry with no file fails
        // at merge, never in the pass.
        if !entry.remote_binary {
            return false;
        }
        let local = portage_repo::BinaryIndex::from_pkgdir(pkgdir);
        let remote = mrg_director::RemoteBinhostIndex {
            config,
            root,
            local: &local,
        };
        let Some((_, record)) = remote.metadata_with_source_instance(
            &entry.category,
            &entry.package,
            version,
            entry.build_id.as_deref(),
        ) else {
            return false;
        };
        return index_record_needs_pretend(&record);
    };
    // The gate reads the `<pkgdir>/Packages` stanza, never the file: a
    // truncated archive still filters exactly like Portage (whose
    // `_pkg_needs_pretend` reads `x.metadata`, the index record), so a
    // pretend-less truncated binpkg reaches the merge-time verifier with
    // its `>>> Emerging binary` + digest block instead of dying in the
    // pass on the unreadable file (pinned by the pmtest contract suite).
    // No vouching stanza fails open: the merge reports it.
    let record = local_index_record(LocalIndexRecordArgs {
        pkgdir,
        category: &entry.category,
        package: &entry.package,
        version,
        build_id: entry.build_id.as_deref(),
        binpkg_path: &binpkg_path,
    });
    let Some(record) = record else { return true };
    index_record_needs_pretend(&record)
}

/// Arguments of [`pretend_one_mixed_entry`].
pub(crate) struct PretendOneMixedEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) config: &'a Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) pkgdir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) merge_options: &'a MergeOptions,
    pub(crate) log_path: &'a Path,
}

/// Arguments of [`pretend_binary_entry_from_path`].
struct PretendBinaryEntryFromPathArgs<'a> {
    entry: &'a GraphEntry,
    binpkg_path: &'a Path,
    version: &'a str,
    root: &'a Path,
    portage_tmpdir: &'a Path,
    merge_options: &'a MergeOptions,
    log_path: &'a Path,
}

/// Run one binary entry's up-front `pkg_pretend` from an already-located
/// (local or just-fetched remote) binpkg file.
fn pretend_binary_entry_from_path(args: PretendBinaryEntryFromPathArgs<'_>) -> Result<(), String> {
    let PretendBinaryEntryFromPathArgs {
        entry,
        binpkg_path,
        version,
        root,
        portage_tmpdir,
        merge_options,
        log_path,
    } = args;
    let entry_tmpdir = crate::emerge_build::binary_entry_portage_tmpdir(
        merge_options,
        entry,
        version,
        portage_tmpdir,
    );
    let mut pretend_options = merge_options.clone();
    pretend_options.log_file = Some(log_path.to_path_buf());
    crate::ebuild_merge::pretend_binary_entry(crate::ebuild_merge::PretendBinaryEntryArgs {
        binpkg_path,
        root,
        portage_tmpdir: &entry_tmpdir,
        options: &pretend_options,
    })
}

/// One mixed-plan entry's up-front `pkg_pretend`, captured to
/// `log_path`. Source entries pretend from their ebuild (the scheduler's
/// own helper); binary entries with a local `$PKGDIR` file pretend from
/// the extracted binpkg metadata; a remote entry is fetched and verified
/// first (the merge reuses the fetched file). A resumed or fresh local
/// entry with no file stays silent here and fails at merge instead.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend_one
fn pretend_one_mixed_entry(args: PretendOneMixedEntryArgs<'_>) -> Result<(), String> {
    let PretendOneMixedEntryArgs {
        entry,
        config,
        repos,
        root,
        pkgdir,
        portage_tmpdir,
        merge_options,
        log_path,
    } = args;
    if entry.source != portage_repo::CandidateSource::Binary {
        return crate::emerge_build::pretend_one_entry(crate::emerge_build::PretendOneEntryArgs {
            entry,
            repos,
            root,
            portage_tmpdir,
            options: merge_options,
            log_path,
        });
    }
    let Some(version) = crate::emerge_build::entry_version(entry) else {
        return Ok(());
    };
    let Some(binpkg_path) = resolve_local_binpkg(
        pkgdir,
        &entry.category,
        &entry.package,
        version,
        entry.build_id.as_deref(),
    ) else {
        // No local file: only a remote entry may still pretend, fetched
        // and verified up front like Portage; the merge reuses the file.
        // A resumed or fresh local entry with no file fails at merge.
        if !entry.remote_binary {
            return Ok(());
        }
        let fetched = fetch_remote_binpkg_for_pretend(FetchRemoteBinpkgArgs {
            entry,
            version,
            config,
            root,
            pkgdir,
            log_path,
            merge_options,
        })?;
        return pretend_binary_entry_from_path(PretendBinaryEntryFromPathArgs {
            entry,
            binpkg_path: &fetched,
            version,
            root,
            portage_tmpdir,
            merge_options,
            log_path,
        });
    };
    pretend_binary_entry_from_path(PretendBinaryEntryFromPathArgs {
        entry,
        binpkg_path: &binpkg_path,
        version,
        root,
        portage_tmpdir,
        merge_options,
        log_path,
    })
}

/// One mixed-plan entry plus everything the pretend pass needs for it.
/// A binary with no local `$PKGDIR` file stays silent up front and fails
/// at merge instead, unless it is a remote entry (fetched in the pass).
struct MixedPretendEntry<'a> {
    entry: &'a GraphEntry,
    config: &'a Config,
    repos: &'a [RepoConfig],
    root: &'a Path,
    pkgdir: &'a Path,
    portage_tmpdir: &'a Path,
    options: &'a MergeOptions,
}

impl crate::emerge_build::PretendEntry for MixedPretendEntry<'_> {
    fn entry(&self) -> &GraphEntry {
        self.entry
    }
    fn needs_pretend(&self) -> bool {
        if !crate::emerge_build::pretend_needs_run(self.entry) {
            return false;
        }
        if self.entry.source == portage_repo::CandidateSource::Binary {
            binary_entry_needs_pretend(BinaryEntryNeedsPretendArgs {
                entry: self.entry,
                pkgdir: self.pkgdir,
                config: self.config,
                root: self.root,
            })
        } else {
            crate::emerge_build::source_entry_needs_pretend(self.entry, self.repos)
        }
    }
    fn entry_tmpdir(&self) -> Result<std::path::PathBuf, String> {
        let Some(version) = self.version() else {
            return Err(format!(
                "{}/{}: no version to pretend",
                self.entry.category, self.entry.package
            ));
        };
        if self.entry.source == portage_repo::CandidateSource::Binary {
            Ok(crate::emerge_build::binary_entry_portage_tmpdir(
                self.options,
                self.entry,
                version,
                self.portage_tmpdir,
            ))
        } else {
            crate::emerge_build::entry_portage_tmpdir(
                self.options,
                self.entry,
                version,
                self.portage_tmpdir,
            )
        }
    }
    fn run_pretend(&self) -> crate::emerge_build::PretendRun {
        if self.entry.source == portage_repo::CandidateSource::Binary {
            // The capture stages outside the builddir: the binary hook
            // resets the builddir (and with it any pre-created log).
            let (cp, version) = match crate::emerge_build::scheduler_cp_version(self.entry) {
                Ok(found) => found,
                Err(_) => {
                    return crate::emerge_build::PretendRun {
                        log: None,
                        result: Ok(()),
                    };
                }
            };
            let t_base = match self.entry_tmpdir() {
                Ok(t_base) => t_base,
                Err(e) => {
                    return crate::emerge_build::PretendRun {
                        log: None,
                        result: Err(e),
                    };
                }
            };
            return crate::emerge_build::run_staged_pretend(
                crate::emerge_build::RunStagedPretendArgs {
                    entry: self.entry,
                    cp: &cp,
                    version: &version,
                    portage_tmpdir: self.portage_tmpdir,
                    t_base: &t_base,
                    run: &|staging| {
                        pretend_one_mixed_entry(PretendOneMixedEntryArgs {
                            entry: self.entry,
                            config: self.config,
                            repos: self.repos,
                            root: self.root,
                            pkgdir: self.pkgdir,
                            portage_tmpdir: self.portage_tmpdir,
                            merge_options: self.options,
                            log_path: staging,
                        })
                    },
                },
            );
        }
        crate::emerge_build::SourcePretendEntry {
            entry: self.entry,
            repos: self.repos,
            root: self.root,
            portage_tmpdir: self.portage_tmpdir,
            options: self.options,
        }
        .run_pretend()
    }
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
    /// `--jobs` after the interactive-package fallback; the up-front
    /// `pkg_pretend` pass runs up to this many pretends at once.
    pub jobs: usize,
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
pub fn run_merge_plan(
    args: RunMergePlanArgs<'_>,
) -> Result<crate::emerge_build::MergePassReport, crate::emerge_build::MergePassError> {
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
        jobs,
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
        quiet: merge_options.quiet,
        background: mode.background,
        quiet_fail: merge_options.quiet_fail,
    };
    // The up-front `pkg_pretend` pass runs before any merge, its outputs
    // replayed in merge-list order; a failure aborts before anything merges.
    // Source entries and binaries pretend here (a remote binary is fetched
    // and verified first); a resumed or fresh local entry with no file
    // fails at merge instead.
    // Portage: _emerge/Scheduler.py, _run_pkg_pretend
    //
    // The pass borrows `merge_options` read-only; the merges below run on a
    // clone with `skip_pretend` set, so neither the source engine nor the
    // binary hook replays `pretend` a second time.
    let failures = {
        let stdout = std::io::stdout();
        let mut replay = stdout.lock();
        let wrapped: Vec<MixedPretendEntry<'_>> = entries
            .iter()
            .map(|entry| MixedPretendEntry {
                entry,
                config,
                repos,
                root,
                pkgdir,
                portage_tmpdir,
                options: merge_options,
            })
            .collect();
        crate::emerge_build::run_pretend_pass(crate::emerge_build::RunPretendPassArgs {
            entries: &wrapped,
            jobs,
            replay: &mut replay,
        })
    };
    if crate::emerge_build::finish_failed_pretend(crate::emerge_build::FinishFailedPretendArgs {
        report: &report,
        entries,
        failures,
        keep_going,
    })? {
        return Ok(crate::emerge_build::MergePassReport::default());
    }
    let mut merge_options = merge_options.clone();
    merge_options.skip_pretend = true;
    let merge_options = &merge_options;
    crate::emerge_build::run_merge_loop(entries, keep_going, &report, |_, entry, entry_progress| {
        // An `IDEPEND` dep merges into the running root, like Portage.
        let entry_root = crate::emerge_build::entry_merge_root(entry, root);
        let root = entry_root.as_path();
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
                    // The up-front pass above already replayed `pretend`.
                    skip_pretend: true,
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

    // The entry's own `PORTAGE_TMPDIR` (`package.env`) holds its build
    // directory, log and elog, like the source path.
    // Portage: _emerge/Binpkg.py, _start
    let entry_tmpdir = crate::emerge_build::binary_entry_portage_tmpdir(
        merge_options,
        entry,
        &version,
        portage_tmpdir,
    );
    let portage_tmpdir = entry_tmpdir.as_path();

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
            let target = remote_binpkg_target(RemoteBinpkgLocationArgs {
                entry,
                version: &version,
                config,
                root,
                pkgdir,
            })?;
            // The pass fetched and verified this file already (or an
            // earlier run left it): reuse it instead of downloading again.
            let filter = checksum_filter_from_config(config);
            let verified = target.dest.is_file()
                && crate::binpkg::index_has_size_and_checksum(&target.record)
                && crate::binpkg::verify_binpkg_against_index(
                    &target.dest,
                    &target.record,
                    filter.as_deref(),
                )
                .is_ok();
            let downloaded = if verified {
                Ok(target.dest.clone())
            } else {
                download_and_verify(DownloadAndVerifyArgs {
                    sync_uri: &target.binrepo.sync_uri,
                    record: &target.record,
                    category: &entry.category,
                    package: &entry.package,
                    version: &version,
                    pkgdir: &target.dest_dir,
                    config,
                })
            };
            let path = match downloaded {
                Ok(path) => path,
                Err(DownloadError::MissingDigests { partial }) => {
                    let block = crate::binpkg::missing_digests_block(&partial);
                    return Err(report_digest_failure(
                        &block,
                        &DigestFailureCtx {
                            entry,
                            version: &version,
                            root,
                            portage_tmpdir,
                            merge_options,
                            has_log: false,
                        },
                    ));
                }
                Err(DownloadError::Other(e)) => return Err(e),
            };
            let portage_quiet = portage_quiet_enabled(merge_options);
            if !portage_quiet
                && let Some(names) =
                    verified_digest_names_filtered(&target.record, filter.as_deref())
            {
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
                target.binrepo.verify_signature,
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
    let local_record = (!entry.remote_binary)
        .then(|| {
            local_index_record(LocalIndexRecordArgs {
                pkgdir,
                category: &entry.category,
                package: &entry.package,
                version: &version,
                build_id: entry.build_id.as_deref(),
                binpkg_path: &binpkg_path,
            })
        })
        .flatten();
    if let Some(record) = &local_record
        && let Err(mismatch) = crate::binpkg::verify_binpkg_against_index(
            &binpkg_path,
            record,
            checksum_filter_from_config(config).as_deref(),
        )
    {
        let renamed = crate::binpkg::checksum_failure_rename(&binpkg_path)
            .unwrap_or_else(|| binpkg_path.clone());
        let block = crate::binpkg::digest_failure_block(&binpkg_path, &mismatch, &renamed);
        return Err(report_digest_failure(
            &block,
            &DigestFailureCtx {
                entry,
                version: &version,
                root,
                portage_tmpdir,
                merge_options,
                has_log: true,
            },
        ));
    }

    // A vouched local binpkg's verifier line, then the extract steps: all
    // of them terminal output only in the foreground (the build log gets
    // the extract steps in `merge_binpkg`).
    // Portage: _emerge/BinpkgVerifier.py, _display_success; _emerge/Binpkg.py
    if !display.mode.background {
        if let Some(names) = local_record.as_ref().and_then(|record| {
            let filter = checksum_filter_from_config(config);
            verified_digest_names_filtered(record, filter.as_deref())
        }) {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let file = binpkg_path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            crate::fetch::print_verified_names(&file, &names);
        }
        println!(">>> Extracting info");
        print_user_patch_table(PrintUserPatchTableArgs {
            binpkg_path: &binpkg_path,
            config,
            category: &entry.category,
            package: &entry.package,
            version: &version,
            slot: entry.slot.as_deref(),
            color: &color,
        });
        println!(">>> Extracting {cp}-{version}");
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
    // The hooks' output is captured to this entry's build log in
    // background mode (like the scheduler's merge half): the `Failed to
    // install` tail and the end-of-run `Messages` flush read it back. In
    // the foreground the hooks inherit the terminal, like the source
    // serial path (the failure tail's log path is derived at record
    // time instead).
    // Portage: _emerge/Binpkg (PORTAGE_LOG_FILE)
    let mut per_entry = merge_options.clone();
    if display.mode.background {
        per_entry.log_file = Some(crate::emerge_build::build_log_path(
            portage_tmpdir,
            &entry.category,
            &entry.package,
            &version,
            &crate::emerge_build::resolved_features(merge_options),
        ));
    }
    let outcome = ebuild_merge::merge_binpkg(&binpkg_path, root, portage_tmpdir, &per_entry)?;
    if outcome.postinst_failed {
        // The merge landed (files and vdb entry are live): `>>> Completed`
        // prints like `PackageMerge._install_exit`, and the world record
        // below is skipped for this entry. The clean never runs after a
        // postinst failure (the `_postinst_failure` builddir rule); the
        // error carries the endgame wording.
        // Portage: _emerge/PackageMerge.py, _install_exit; vartree.py
        display.status(&crate::emerge_build::completed_line(
            entry, &version, progress, root, &color,
        ));
        return Err(crate::emerge_build::postinst_failed_error(
            &cp,
            &version,
            outcome.status,
        ));
    }
    if outcome.status != 0 {
        return Err(format!(
            "{cp}-{version}: binpkg merge failed ({})",
            outcome.status
        ));
    }
    if let Some(record) = &merge_options.world_recorder {
        for line in record(entry) {
            display.status(&line);
        }
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
        format!(" for {}", portage_repo::root_with_trailing_slash(root))
    };
    match log_path {
        Some(log) => format!(
            "\n>>> Failed to emerge {cpv}{for_root}, Log file:\n\n>>>  '{}'\n",
            log.display()
        ),
        None => format!("\n>>> Failed to emerge {cpv}{for_root}\n"),
    }
}

/// `>>> Failed to install <cpv>[ to <root>][, Log file:]` plus, when
/// `log_path` is `Some`, the `>>>  '<log>'` tail: the merge-phase half of
/// [`failed_pkg_msg`] (a postinst failure still merges, so the run reports
/// `install`/`to`, never `emerge`/`for`).
// Portage: lib/_emerge/Scheduler.py, _failed_pkg_msg
pub(crate) fn failed_install_pkg_msg(cpv: &str, root: &Path, log_path: Option<&Path>) -> String {
    let for_root = if root == Path::new("/") {
        String::new()
    } else {
        format!(" to {}", portage_repo::root_with_trailing_slash(root))
    };
    match log_path {
        Some(log) => format!(
            "\n>>> Failed to install {cpv}{for_root}, Log file:\n\n>>>  '{}'\n",
            log.display()
        ),
        None => format!("\n>>> Failed to install {cpv}{for_root}\n"),
    }
}

/// What [`report_digest_failure`] needs to name the package and find its log.
struct DigestFailureCtx<'a> {
    entry: &'a GraphEntry,
    version: &'a str,
    root: &'a Path,
    portage_tmpdir: &'a Path,
    merge_options: &'a MergeOptions,
    /// Whether the package has a `build.log` to hold the block. A local
    /// binpkg does; a remote one is refused before Portage initialises
    /// `PORTAGE_LOG_FILE`, so it has none and the tail names no log.
    // Portage: _emerge/Binpkg.py, _start_fetcher / _fetcher_exit
    has_log: bool,
}

/// Print a verifier failure `block` to stdout and (with `has_log`) the
/// package `build.log` (the log holds exactly this block), then the `>>> Failed to emerge ...`
/// tail, and return the silent sentinel error the CLI boundary drops (exit
/// 1, no resume notice, no `emerge:` line).
// Portage: _emerge/BinpkgVerifier.py, _digest_exception / _missing_digests_exception
fn report_digest_failure(block: &str, ctx: &DigestFailureCtx<'_>) -> String {
    let DigestFailureCtx {
        entry,
        version,
        root,
        portage_tmpdir,
        merge_options,
        has_log,
    } = ctx;
    let cpv = format!("{}/{}-{version}", entry.category, entry.package);
    let log_path = crate::emerge_build::build_log_path(
        portage_tmpdir,
        &entry.category,
        &entry.package,
        version,
        &crate::emerge_build::resolved_features(merge_options),
    );
    // Only a log that exists and is non-empty is reported (`, Log file:`
    // suffix), exactly like Portage with no located log.
    let log_ready = *has_log
        && log_path
            .parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
        && std::fs::write(&log_path, block.as_bytes()).is_ok()
        && std::fs::metadata(&log_path).is_ok_and(|st| st.len() > 0);
    print!("{block}");
    print!(
        "{}",
        failed_pkg_msg(&cpv, root, log_ready.then_some(log_path.as_path()))
    );
    binpkg_digest_failure(&cpv)
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
/// `PORTAGE_BINHOST` is unset) `<eprefix>/var/cache/binhost/<name>`, else `$PKGDIR`.
/// `eprefix` is Portage's `EPREFIX` ([`eprefix_from_env`]); `ROOT` plays no part.
// Portage: lib/portage/binrepo/config.py (BinRepoConfigLoader), bintree.py (get_local_repo_location)
pub(crate) fn binrepo_download_dir(
    config: &portage_profile::Config,
    binrepo: &portage_profile::BinRepo,
    eprefix: &str,
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
    Path::new(&format!("{eprefix}/var/cache/binhost")).join(&binrepo.name)
}

/// Portage's `EPREFIX`: `PORTAGE_OVERRIDE_EPREFIX` without trailing slashes, else empty.
// Portage: lib/portage/const.py (EPREFIX)
pub(crate) fn eprefix_from_env() -> String {
    std::env::var("PORTAGE_OVERRIDE_EPREFIX")
        .map(|v| v.trim_end_matches('/').to_string())
        .unwrap_or_default()
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

/// `PORTAGE_CHECKSUM_FILTER`: process env first, then `config.other_vars`.
pub(crate) fn checksum_filter_from_config(config: &portage_profile::Config) -> Option<String> {
    std::env::var("PORTAGE_CHECKSUM_FILTER")
        .ok()
        .or_else(|| config.other_vars.get("PORTAGE_CHECKSUM_FILTER").cloned())
        .filter(|s| !s.trim().is_empty())
}

/// Arguments of [`print_user_patch_table`].
pub(crate) struct PrintUserPatchTableArgs<'a> {
    pub(crate) binpkg_path: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: Option<&'a str>,
    pub(crate) color: &'a crate::color::Colorizer,
}

/// The merge-time user-patch table on the foreground terminal, between
/// `>>> Extracting info` and `>>> Extracting <cpv>`: the table from the
/// binpkg's `user_patch.digests` member (stdout) plus the mismatch /
/// malformed warnings (stderr). A binpkg without the member prints nothing;
/// an unreadable archive prints nothing either (the merge itself fails just
/// below with its own error). The local half mirrors the resolve filter's
/// (`binary_user_patches_ok`): whether any patch set matches, and the
/// combined digest -- with no `--binpkg-respect-user-patches` exemption,
/// like Portage (the oracle `16-merge-mismatch` warns under `=n`).
// Portage: _emerge/Binpkg.py, `_unpack_metadata`
pub(crate) fn print_user_patch_table(args: PrintUserPatchTableArgs<'_>) {
    let PrintUserPatchTableArgs {
        binpkg_path,
        config,
        category,
        package,
        version,
        slot,
        color,
    } = args;
    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let meta = if name.ends_with(".gpkg.tar") {
        crate::binpkg::read_gpkg_metadata(binpkg_path)
    } else {
        crate::binpkg::read_xpak_metadata(binpkg_path)
    };
    let Ok(meta) = meta else { return };
    let Some(raw) = meta.get("user_patch.digests") else {
        return;
    };
    let slot = slot.unwrap_or("0");
    let local_has = config
        .user_patches
        .as_ref()
        .is_some_and(|set| set.has(category, package, version, slot));
    let local_digest = config
        .user_patches
        .as_ref()
        .map(|set| set.digest(category, package, version, slot))
        .unwrap_or_default();
    let index_digest = meta
        .get("USER_PATCHES")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let Some((stdout, stderr)) = crate::ebuild_merge::user_patch_extract_report(
        Some(raw.as_bytes()),
        local_has,
        &local_digest,
        &index_digest,
        crate::ebuild_merge::merge_term_columns(),
    ) else {
        return;
    };
    for line in stdout {
        println!("{}{line}", color.c("INFORM", " * "));
    }
    for line in stderr {
        eprintln!("{}{line}", color.c("WARN", " * "));
    }
}

/// The digest names Portage prints on a binary's verify line: the same
/// selection the verifier checks (the uppercase checksum keys present in
/// the index record, filtered by `PORTAGE_CHECKSUM_FILTER`) plus `size`,
/// ASCII-sorted (`RMD160 SHA512 WHIRLPOOL size`). `None` without a
/// parseable `SIZE`: then nothing is verified and no line prints.
// Portage: _emerge/BinpkgVerifier.py (_start, _display_success); bintree.py, _get_digests
pub(crate) fn verified_digest_names_filtered(
    record: &std::collections::HashMap<String, String>,
    checksum_filter: Option<&str>,
) -> Option<Vec<String>> {
    record.get("SIZE")?.trim().parse::<u64>().ok()?;
    let mut names = crate::binpkg::selected_digest_names(record, checksum_filter);
    names.push("size".to_string());
    names.sort();
    Some(names)
}

/// Why [`download_and_verify`] failed.
#[derive(Debug)]
pub(crate) enum DownloadError {
    /// The index record lacks a `SIZE` or every checksum; the downloaded
    /// `.partial` stays on disk.
    MissingDigests { partial: std::path::PathBuf },
    /// Any other failure, as the one-line message.
    Other(String),
}

impl From<String> for DownloadError {
    fn from(e: String) -> Self {
        Self::Other(e)
    }
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingDigests { partial } => write!(
                f,
                "{}: binhost's Packages index lacks size and checksum",
                partial.display()
            ),
            Self::Other(e) => f.write_str(e),
        }
    }
}

/// The default binpkg file name when the index carries no `PATH`.
fn default_binpkg_rel(category: &str, package: &str, version: &str) -> String {
    format!("{category}/{package}-{version}.tbz2")
}

/// Arguments of [`fetch_binpkg_bytes`].
pub(crate) struct FetchBinpkgBytesArgs<'a> {
    /// The owning binrepo's `sync-uri`.
    pub(crate) sync_uri: &'a str,
    /// The index `PATH` (or the default `<cat>/<pf>.tbz2` name).
    pub(crate) rel: &'a str,
    /// The directory the file lands in.
    pub(crate) dest_dir: &'a Path,
    /// The resolved `FETCHCOMMAND` family and `PORTAGE_SSH_OPTS`, from the config.
    pub(crate) config: &'a portage_profile::Config,
}

/// Copy/wget `<sync_uri>/<rel>` to `<dest_dir>/<rel>.partial` (resuming a
/// leftover `.partial`), creating the parent dirs. Returns the `.partial`
/// path; verification and the rename stay with the caller, which differs
/// between the pass fetch and the merge download.
pub(crate) fn fetch_binpkg_bytes(
    args: FetchBinpkgBytesArgs<'_>,
) -> Result<std::path::PathBuf, String> {
    let FetchBinpkgBytesArgs {
        sync_uri,
        rel,
        dest_dir,
        config,
    } = args;
    let final_dest = dest_dir.join(rel);
    if let Some(parent) = final_dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let uri = format!("{}/{rel}", sync_uri.trim_end_matches('/'));
    // Downloaded as `<file>.partial`, renamed once verified.
    // Portage: _emerge/BinpkgFetcher.py, Binpkg.py (`_verifier_exit`)
    let mut partial = final_dest.clone().into_os_string();
    partial.push(".partial");
    let dest = std::path::PathBuf::from(partial);
    let resume = dest.is_file();

    if let Some(local) = uri.strip_prefix("file://") {
        std::fs::copy(local, &dest).map_err(|e| format!("{local}: {e}"))?;
    } else {
        let mut commands = crate::fetch::fetch_commands_from_config(config);
        if let Some(binrepo) = config
            .binrepos
            .iter()
            .find(|b| b.sync_uri.trim_end_matches('/') == sync_uri.trim_end_matches('/'))
        {
            if let Some(fc) = &binrepo.fetchcommand {
                commands.fetchcommand_proto.clear();
                commands.fetchcommand = Some(fc.clone());
            }
            if let Some(rc) = &binrepo.resumecommand {
                commands.resumecommand_proto.clear();
                commands.resumecommand = Some(rc.clone());
            }
        }
        let ssh_opts = crate::fetch::portage_ssh_opts_from_config(config);
        portage_fetch::download_with_commands_stdout(portage_fetch::DownloadWithCommandsArgs {
            uri: &uri,
            dest: &dest,
            resume,
            distdir: dest.parent().unwrap_or(dest_dir),
            vars: portage_fetch::FetchCommandVars {
                digests: None,
                portage_ssh_opts: ssh_opts.as_deref(),
            },
            commands: &commands,
        })?;
    }
    Ok(dest)
}

/// Fetch `<sync_uri>/<PATH>` (or the default `<cat>/<pf>.tbz2`) into `$PKGDIR`,
/// then verify it against the index `SIZE` and the `MD5`/`SHA1` fields present.
/// A mismatch removes the file and fails.
// Portage: lib/portage/dbapi/bintree.py, gettbz2
pub(crate) fn download_and_verify(
    args: DownloadAndVerifyArgs<'_>,
) -> Result<std::path::PathBuf, DownloadError> {
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
        .unwrap_or_else(|| default_binpkg_rel(category, package, version));
    let final_dest = pkgdir.join(&rel);
    let dest = fetch_binpkg_bytes(FetchBinpkgBytesArgs {
        sync_uri,
        rel: &rel,
        dest_dir: pkgdir,
        config,
    })?;

    // A remote record needs both a SIZE and a checksum; one without leaves
    // nothing to compare against. Refused after the download, with the
    // `.partial` left in place.
    // Portage: _emerge/BinpkgVerifier.py, _start
    if !crate::binpkg::index_has_size_and_checksum(record) {
        return Err(DownloadError::MissingDigests { partial: dest });
    }

    // The downloaded bytes are verified against the remote record exactly
    // like a local binpkg: `SIZE` first, then every digest the record
    // carries that survives `PORTAGE_CHECKSUM_FILTER`. A mismatch removes
    // the `.partial` and fails.
    // Portage: _emerge/BinpkgVerifier.py, _start
    let filter = checksum_filter_from_config(config);
    if let Err(mismatch) =
        crate::binpkg::verify_binpkg_against_index(&dest, record, filter.as_deref())
    {
        let _removed = std::fs::remove_file(&dest);
        if mismatch.name == "size" {
            return Err(DownloadError::Other(format!(
                "{}: downloaded size {} != index SIZE {}",
                dest.display(),
                mismatch.got,
                mismatch.expected
            )));
        }
        return Err(DownloadError::Other(format!(
            "{}: {} mismatch (index {}, got {})",
            dest.display(),
            mismatch.name,
            mismatch.expected,
            mismatch.got
        )));
    }
    std::fs::rename(&dest, &final_dest).map_err(|e| format!("{}: {e}", final_dest.display()))?;
    Ok(final_dest)
}

/// Arguments of [`remote_binpkg_target`].
pub(crate) struct RemoteBinpkgLocationArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) version: &'a str,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) pkgdir: &'a Path,
}

/// A remote entry's download target: the owning binrepo (cloned), its
/// index record, and the destination file (the binrepo's download
/// directory plus the index `PATH`, else the default name). The `Packages`
/// lookup goes through the director's `BinpkgIndex` seam, which scans the
/// configured binrepos in order; the record travels with the owning
/// binrepo's name. Shared by the pass fetch and the merge download, so the
/// merge reuses the pass's file instead of downloading a second time. The
/// errors are the merge's own missing-file texts.
// Portage: _emerge/Binpkg.py, _start_fetcher; bintree, getname
pub(crate) struct RemoteBinpkgTarget {
    pub(crate) binrepo: BinRepo,
    pub(crate) record: std::collections::HashMap<String, String>,
    /// The binrepo's download directory (`location`, `$PKGDIR` for a
    /// `PORTAGE_BINHOST` repo, else the `var/cache/binhost` dir).
    pub(crate) dest_dir: std::path::PathBuf,
    pub(crate) dest: std::path::PathBuf,
}

/// The download target of a remote entry (see [`RemoteBinpkgTarget`]).
// Portage: _emerge/Binpkg.py, _start_fetcher
pub(crate) fn remote_binpkg_target(
    args: RemoteBinpkgLocationArgs<'_>,
) -> Result<RemoteBinpkgTarget, String> {
    let RemoteBinpkgLocationArgs {
        entry,
        version,
        config,
        root,
        pkgdir,
    } = args;
    let cp = format!("{}/{}", entry.category, entry.package);
    let local = portage_repo::BinaryIndex::from_pkgdir(pkgdir);
    let remote = mrg_director::RemoteBinhostIndex {
        config,
        root,
        local: &local,
    };
    let (binrepo_name, record) = remote
        .metadata_with_source_instance(
            &entry.category,
            &entry.package,
            version,
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
        .cloned()
        .ok_or_else(|| {
            format!("{cp}-{version}: binhost record names unknown repo {binrepo_name:?}")
        })?;
    let rel = record
        .get("PATH")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| default_binpkg_rel(&entry.category, &entry.package, version));
    let dest_dir = binrepo_download_dir(config, &binrepo, &eprefix_from_env(), pkgdir);
    let dest = dest_dir.join(&rel);
    Ok(RemoteBinpkgTarget {
        binrepo,
        record,
        dest_dir,
        dest,
    })
}

/// Whether `PORTAGE_QUIET=1` silences the verifier's success line: the
/// merge env first, then the process env.
// Portage: _emerge/BinpkgVerifier.py, _digester_exit
pub(crate) fn portage_quiet_enabled(merge_options: &MergeOptions) -> bool {
    merge_options
        .build_env
        .iter()
        .rev()
        .find(|(k, _)| k == "PORTAGE_QUIET")
        .is_some_and(|(_, v)| v == "1")
        || std::env::var("PORTAGE_QUIET").as_deref() == Ok("1")
}

/// Append `text` to a pretend pass staging capture (created by the runner
/// before the entry runs).
fn append_pretend_log(log_path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| format!("{}: {e}", log_path.display()))
}

/// Arguments of [`fetch_remote_binpkg_for_pretend`].
pub(crate) struct FetchRemoteBinpkgArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) version: &'a str,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) pkgdir: &'a Path,
    /// This entry's staging capture: the verifier's success line (or the
    /// failure block with its tail) lands here, ahead of the hook output.
    pub(crate) log_path: &'a Path,
    /// Merge options, for the `PORTAGE_QUIET` check on the success line.
    pub(crate) merge_options: &'a MergeOptions,
}

/// Fetch and verify a remote binary for the up-front pretend pass, reusing
/// an already-verified file the pass (or an earlier run) left in the
/// binrepo download directory, so the hook pretends from the fetched
/// bytes. The verifier's success line -- or, on failure, Portage's
/// failure block with the `>>> Failed to emerge` tail -- is appended to
/// the staging capture; a failure returns the silent sentinel the pass
/// reports without a second tail.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend_one (the `x.built` arm:
// BinpkgFetcher, then BinpkgVerifier, then the metadata unpack)
pub(crate) fn fetch_remote_binpkg_for_pretend(
    args: FetchRemoteBinpkgArgs<'_>,
) -> Result<std::path::PathBuf, String> {
    let FetchRemoteBinpkgArgs {
        entry,
        version,
        config,
        root,
        pkgdir,
        log_path,
        merge_options,
    } = args;
    let cpv = format!("{}/{}-{version}", entry.category, entry.package);
    let failed = |output: String| -> Result<std::path::PathBuf, String> {
        append_pretend_log(log_path, &output)?;
        Err(binpkg_digest_failure(&cpv))
    };
    let target = match remote_binpkg_target(RemoteBinpkgLocationArgs {
        entry,
        version,
        config,
        root,
        pkgdir,
    }) {
        Ok(target) => target,
        Err(_) => {
            // The gate saw a record, so the index lost it mid-run; report
            // it like the merge reports a missing file (no build log in
            // the pass).
            let block = missing_binpkg_block(
                &cpv,
                local_index_lists_cpv(
                    pkgdir,
                    &entry.category,
                    &entry.package,
                    version,
                    entry.build_id.as_deref(),
                ),
            );
            let mut output = block;
            output.push_str(&failed_pkg_msg(&cpv, root, None));
            return failed(output);
        }
    };
    let filter = checksum_filter_from_config(config);
    let success_line = |dest: &Path| -> Result<(), String> {
        if portage_quiet_enabled(merge_options) {
            return Ok(());
        }
        let Some(names) = verified_digest_names_filtered(&target.record, filter.as_deref()) else {
            return Ok(());
        };
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let file = dest
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        append_pretend_log(log_path, &crate::fetch::verified_names_line(&file, &names))
    };
    // A verified file the pass (or an earlier run) left behind is reused:
    // the merge finds the same file, so nothing is fetched twice.
    if crate::binpkg::index_has_size_and_checksum(&target.record)
        && target.dest.is_file()
        && crate::binpkg::verify_binpkg_against_index(
            &target.dest,
            &target.record,
            filter.as_deref(),
        )
        .is_ok()
    {
        success_line(&target.dest)?;
        return Ok(target.dest);
    }
    let rel = target
        .record
        .get("PATH")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| default_binpkg_rel(&entry.category, &entry.package, version));
    // A fetch failure carries the merge's one-line text, with no block:
    // the merge reports it the same way.
    let partial = fetch_binpkg_bytes(FetchBinpkgBytesArgs {
        sync_uri: &target.binrepo.sync_uri,
        rel: &rel,
        dest_dir: &target.dest_dir,
        config,
    })?;
    // A remote record needs both a SIZE and a checksum; one without leaves
    // nothing to compare against. Refused after the download, with the
    // `.partial` left in place.
    // Portage: _emerge/BinpkgVerifier.py, _start
    if !crate::binpkg::index_has_size_and_checksum(&target.record) {
        let mut output = crate::binpkg::missing_digests_block(&partial);
        output.push_str(&failed_pkg_msg(&cpv, root, None));
        return failed(output);
    }
    if let Err(mismatch) =
        crate::binpkg::verify_binpkg_against_index(&partial, &target.record, filter.as_deref())
    {
        let renamed =
            crate::binpkg::checksum_failure_rename(&partial).unwrap_or_else(|| partial.clone());
        let mut output = crate::binpkg::digest_failure_block(&partial, &mismatch, &renamed);
        output.push_str(&failed_pkg_msg(&cpv, root, None));
        return failed(output);
    }
    std::fs::rename(&partial, &target.dest)
        .map_err(|e| format!("{}: {e}", target.dest.display()))?;
    success_line(&target.dest)?;
    Ok(target.dest)
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

    fn tempdir() -> TempDir {
        TempDir::new("portuale-getbinpkg")
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
        graph_entry_cat("dev-libs", package, source, version)
    }

    fn graph_entry_cat(
        category: &str,
        package: &str,
        source: CandidateSource,
        version: &str,
    ) -> GraphEntry {
        GraphEntry {
            discovery: 0,
            category: category.into(),
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
            build_time: None,
            deps: Vec::new(),
        }
    }

    /// Portage reads a second blank line after the header as the end of the
    /// index (zero packages), so stamping must keep exactly one blank line.
    /// Expected shape: Portage's own cache writer (`bintree.py`,
    /// `_populate_remote_repo`: header, one blank line, entries).
    #[test]
    fn stamping_the_download_timestamp_keeps_one_blank_line() {
        let fresh = "TIMESTAMP: 1\nVERSION: 0\n\nCPV: a/b-1\n\n";
        assert_eq!(
            stamp_download_timestamp(fresh, 5),
            "TIMESTAMP: 1\nVERSION: 0\nDOWNLOAD_TIMESTAMP: 5\n\nCPV: a/b-1\n\n"
        );
        let stamped = "TIMESTAMP: 1\nDOWNLOAD_TIMESTAMP: 2\n\nCPV: a/b-1\n\n";
        assert_eq!(
            stamp_download_timestamp(stamped, 9),
            "TIMESTAMP: 1\nDOWNLOAD_TIMESTAMP: 9\n\nCPV: a/b-1\n\n"
        );
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
        let scanned =
            crate::binpkg::populate_local_pkgdir(&pkgdir, true, false).expect("scan succeeds");
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
        let scanned =
            crate::binpkg::populate_local_pkgdir(&pkgdir, true, false).expect("scan succeeds");
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
            "\n>>> Failed to emerge dev-libs/packagepkg-1.0 for /tmp/r/root/, Log file:\n\
             \n>>>  '/tmp/r/pt/portage/dev-libs/packagepkg-1.0/temp/build.log'\n"
        );
        assert_eq!(
            failed_pkg_msg("dev-libs/packagepkg-1.0", Path::new("/tmp/r/root"), None),
            "\n>>> Failed to emerge dev-libs/packagepkg-1.0 for /tmp/r/root/\n"
        );
        // `root_with_trailing_slash` is idempotent: a root that already
        // carries the slash keeps exactly one.
        assert_eq!(
            failed_pkg_msg("dev-libs/packagepkg-1.0", Path::new("/tmp/r/root/"), None),
            "\n>>> Failed to emerge dev-libs/packagepkg-1.0 for /tmp/r/root/\n"
        );
        // Only a lone failure is silent: a `--keep-going` combined
        // message keeps the normal tail.
        assert!(!is_binpkg_digest_failure(
            "2 package(s) failed to merge (--keep-going):\n  dev-libs/a-1.0: binpkg digest verification failed"
        ));
    }

    #[test]
    fn failed_install_pkg_msg_matches_portage_failed_pkg_msg_bytes() {
        // The merge-phase half of `failed_pkg_msg`: `install`/`to`, never
        // `emerge`/`for` (backlog #444: a postinst failure still merges).
        // Ground truth: `LLM/evidence/2026-10-21-b444/portage.txt`
        // (`>>> Failed to install ... to ..., Log file:` rows).
        assert_eq!(
            failed_install_pkg_msg(
                "dev-util/postinstfail-1",
                Path::new("/var/tmp/b444/root"),
                Some(Path::new(
                    "/var/tmp/b444/tmp/portage/dev-util/postinstfail-1/temp/build.log"
                ))
            ),
            "\n>>> Failed to install dev-util/postinstfail-1 to /var/tmp/b444/root/, Log file:\n\
             \n>>>  '/var/tmp/b444/tmp/portage/dev-util/postinstfail-1/temp/build.log'\n"
        );
        assert_eq!(
            failed_install_pkg_msg("dev-util/postinstfail-1", Path::new("/"), None),
            "\n>>> Failed to install dev-util/postinstfail-1\n"
        );
        assert_eq!(
            failed_install_pkg_msg("dev-util/postinstfail-1", Path::new("/tmp/r/root"), None),
            "\n>>> Failed to install dev-util/postinstfail-1 to /tmp/r/root/\n"
        );
    }

    /// A writable copy of the committed GnuPG test keyring (see `binpkg.rs`).
    /// `gpg` refuses a homedir it doesn't own, so the tree itself is never used.
    fn test_gpg_home() -> (TempDir, std::path::PathBuf) {
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
        let tmp = tempdir();
        let dest = tmp.join("gpg-home");
        copy_dir(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/helpers/gpg-keyring"),
            &dest,
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700)).unwrap();
        (tmp, dest)
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
        let (_guard, home) = test_gpg_home();
        let options = MergeOptions {
            gpg_verify: test_gpg_verify(&home),
            ..MergeOptions::default()
        };

        let outcome = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar"),
            &root,
            &tmp.join("portage_tmpdir"),
            &options,
        )
        .expect("signed gpkg merge succeeds");
        assert_eq!(outcome.status, 0);

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
        let (_guard, home) = test_gpg_home();
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

        let outcome = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(outcome.status, 0);

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

        let outcome = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("gpkg merge succeeds");
        assert_eq!(outcome.status, 0);

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

        let outcome = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(outcome.status, 0, "both hooks exited 0");

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
    fn merge_one_binary_entry_postinst_failure_matches_the_portage_endgame() {
        // Backlog #459 (Portage `vartree.py::dblink.merge`
        // `_postinst_failure`, `_emerge/EbuildMerge.py::_merge_exit`,
        // `_emerge/PackageMerge.py::_install_exit`,
        // `Scheduler.py::_do_merge_exit`; oracle
        // `LLM/evidence/2026-10-21-f459/`): a binary merge whose
        // `pkg_postinst` dies still lands (files and vdb entry) but the
        // entry reports the postinst marker, so the loop prints
        // `>>> Completed`, skips the world record and keeps the builddir.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let entry = graph_entry_cat(
            "dev-util",
            "f459-postinstfail",
            CandidateSource::Binary,
            "1",
        );
        let err = merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &Config::default(),
            root: &root,
            pkgdir: &fixtures_root().join("pkgdir"),
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            merge_options: &MergeOptions::default(),
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .unwrap_err();
        assert!(
            crate::emerge_build::is_postinst_failure(&err),
            "the postinst marker, not a plain merge failure: {err}"
        );
        // The merge landed: the vdb entry is live ...
        assert!(
            root.join("var/db/pkg/dev-util/f459-postinstfail-1/CONTENTS")
                .is_file()
        );
        // ... but the builddir is kept (bug #704866), and the elog the
        // end-of-run `Messages` flush reads carries `FAILED postinst`.
        assert!(
            tmp.join("portage_tmpdir/portage/dev-util/f459-postinstfail-1")
                .is_dir(),
            "the builddir must be kept"
        );
        let logging = std::fs::read_to_string(
            tmp.join("portage_tmpdir/portage/dev-util/f459-postinstfail-1/temp/logging/postinst"),
        )
        .unwrap();
        assert!(
            logging.starts_with("ERROR FAILED postinst:"),
            "the elog heads the phase file: {logging:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_one_binary_entry_builds_under_the_entrys_own_portage_tmpdir() {
        // Backlog #467 (Portage `_emerge/Binpkg.py::_start` reads
        // `settings["PORTAGE_TMPDIR"]` after `setcpv`, so `package.env`
        // moves the binary entry's build dir; oracle
        // `LLM/evidence/2026-10-21-r463/`). The matched directory need not
        // exist: `Binpkg` runs no `_check_temp_dir`, the merge creates it.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let entry_tmp = tmp.join("entry-tmp");
        let entry = graph_entry_cat(
            "dev-util",
            "f459-postinstfail",
            CandidateSource::Binary,
            "1",
        );
        let options = MergeOptions {
            package_env_vars: vec![(
                "dev-util/f459-postinstfail".to_string(),
                vec![(
                    "PORTAGE_TMPDIR".to_string(),
                    entry_tmp.to_str().unwrap().to_string(),
                )],
            )],
            ..MergeOptions::default()
        };
        let err = merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &Config::default(),
            root: &root,
            pkgdir: &fixtures_root().join("pkgdir"),
            portage_tmpdir: &tmp.join("run-tmp"),
            merge_options: &options,
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .unwrap_err();
        assert!(crate::emerge_build::is_postinst_failure(&err), "{err}");
        assert!(
            entry_tmp
                .join("portage/dev-util/f459-postinstfail-1")
                .is_dir(),
            "the kept builddir is under the entry's tmpdir"
        );
        assert!(!tmp.join("run-tmp/portage").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_runs_pkg_setup_then_preinst_then_postinst() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let outcome = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(outcome.status, 0);

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

        let outcome = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(outcome.status, 0);

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
        let outcome = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &opts,
        )
        .expect("merge succeeds");
        assert_eq!(outcome.status, 0);

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
        let outcome = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &opts,
        )
        .expect("merge succeeds");
        assert_eq!(outcome.status, 0);

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
        let outcome = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-2.0.tbz2"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("2.0 merges");
        assert_eq!(outcome.status, 0);

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

        let outcome = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &root,
            &tmp.join("pt"),
            &MergeOptions::default(),
        )
        .expect("replace merge succeeds");
        assert_eq!(outcome.status, 0);

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
            format!("sh {} ${{DISTDIR}} ${{FILE}} ${{URI}}", script.display()),
        );
        let record: HashMap<String, String> = [
            ("SIZE", "4618"),
            ("MD5", "54cab52a68eda02d7d41561b8f7d318a"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]
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
            verified_digest_names_filtered(
                &rec(&[("SIZE", "1"), ("SHA1", "x"), ("MD5", "y")]),
                None
            ),
            Some(vec!["MD5".into(), "SHA1".into(), "size".into()])
        );
        assert_eq!(
            verified_digest_names_filtered(&rec(&[("SIZE", "1")]), None),
            Some(vec!["size".into()])
        );
        // No parseable SIZE: Portage verifies nothing and prints no line.
        assert_eq!(
            verified_digest_names_filtered(&rec(&[("MD5", "y")]), None),
            None
        );
        // Backlog #436 (d): PORTAGE_CHECKSUM_FILTER filters hashes while preserving at least one.
        assert_eq!(
            verified_digest_names_filtered(
                &rec(&[("SIZE", "1"), ("SHA1", "x"), ("MD5", "y")]),
                Some("-md5")
            ),
            Some(vec!["SHA1".into(), "size".into()])
        );
        assert_eq!(
            verified_digest_names_filtered(
                &rec(&[("SIZE", "1"), ("SHA1", "x"), ("MD5", "y")]),
                Some("-*")
            )
            .map(|v| v.len()),
            Some(2),
            "at least one hash plus size must survive -*"
        );
    }

    /// A record with `SIZE`, `RMD160`, `WHIRLPOOL` and `SHA512` prints all
    /// three hashes before `size`, ASCII-sorted the way Portage joins
    /// `sorted(self._digests)`; `PORTAGE_CHECKSUM_FILTER="RMD160"` narrows
    /// the line to `RMD160 size`.
    // Portage: `lib/_emerge/BinpkgVerifier.py`, `_display_success`; `lib/portage/dbapi/bintree.py`, `_get_digests`
    #[test]
    fn success_line_lists_rmd160_and_whirlpool() {
        let rec = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let record = rec(&[
            ("SIZE", "16"),
            ("RMD160", "r"),
            ("WHIRLPOOL", "w"),
            ("SHA512", "s"),
        ]);
        assert_eq!(
            verified_digest_names_filtered(&record, None),
            Some(vec![
                "RMD160".into(),
                "SHA512".into(),
                "WHIRLPOOL".into(),
                "size".into()
            ])
        );
        assert_eq!(
            verified_digest_names_filtered(&record, Some("RMD160")),
            Some(vec!["RMD160".into(), "size".into()])
        );
    }

    #[test]
    fn binrepo_fetchcommand_and_resumecommand_override_global_config_and_equal_timestamp_notes_up_to_date()
     {
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");
        let root = tmp.join("root");
        let body =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let pkg_index = "TIMESTAMP: 42\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/packagepkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), pkg_index.as_bytes().to_vec());
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), body.clone());
        let (base, _h) = serve(routes, 6);
        let marker = tmp.join("ran");
        let fc_script = tmp.join("fc.sh");
        std::fs::write(
            &fc_script,
            format!(
                "#!/bin/sh\necho \"FC $1 $2\" >> {}\nexec wget -q -O \"$1/$2\" \"$3\"\n",
                marker.display()
            ),
        )
        .unwrap();
        let rc_script = tmp.join("rc.sh");
        std::fs::write(
            &rc_script,
            format!(
                "#!/bin/sh\necho \"RC $1 $2\" >> {}\nexec wget -q -O \"$1/$2\" \"$3\"\n",
                marker.display()
            ),
        )
        .unwrap();
        let binrepo = BinRepo {
            name: "perrepo".to_string(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: false,
            frozen: false,
            fetchcommand: Some(format!(
                "sh {} ${{DISTDIR}} ${{FILE}} ${{URI}}",
                fc_script.display()
            )),
            resumecommand: Some(format!(
                "sh {} ${{DISTDIR}} ${{FILE}} ${{URI}}",
                rc_script.display()
            )),
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, pkg_index).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: false,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
        assert_eq!(
            outcome.stdout_note.as_deref(),
            Some("[perrepo] Local copy of remote index is up-to-date and will be used.\n")
        );

        let mut config = Config {
            binrepos: vec![binrepo],
            ..Default::default()
        };
        config
            .other_vars
            .insert("FETCHCOMMAND".into(), "/bin/false ${FILE}".into());
        config
            .other_vars
            .insert("RESUMECOMMAND".into(), "/bin/false ${FILE}".into());
        let record: HashMap<String, String> = [
            ("SIZE", "4618"),
            ("MD5", "54cab52a68eda02d7d41561b8f7d318a"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        // Pre-seed `.partial` so the first call exercises `resumecommand`, then
        // remove the downloaded file so the second exercises `fetchcommand`.
        let partial = pkgdir.join("dev-libs/packagepkg-1.0.tbz2.partial");
        std::fs::create_dir_all(partial.parent().unwrap()).unwrap();
        std::fs::write(&partial, b"part").unwrap();
        let got1 = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &record,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &config,
        })
        .unwrap();
        std::fs::remove_file(&got1).unwrap();
        let _got2 = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &record,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &config,
        })
        .unwrap();
        let log = std::fs::read_to_string(&marker).unwrap();
        assert!(log.contains("RC ") && log.contains("FC "), "log: {log}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn binrepo_download_dir_follows_location_then_binhost_then_default() {
        let root = "/r";
        let pkgdir = Path::new("/p");
        let mut repo = portage_profile::BinRepo {
            name: "bh".into(),
            sync_uri: "http://x".into(),
            priority: 0,
            location: Some("/loc".into()),
            verify_signature: true,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
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
        assert_eq!(
            binrepo_download_dir(&config, &repo, "", pkgdir),
            Path::new("/var/cache/binhost/bh")
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
        let bad_size = record(&[
            ("SIZE", "9999"),
            ("MD5", FIXTURE_MD5),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &bad_size,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
            config: &Config::default(),
        })
        .unwrap_err()
        .to_string();
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
        .unwrap_err()
        .to_string();
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
        .unwrap_err()
        .to_string();
        assert!(err.contains("SHA1 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_and_verify_refuses_a_record_without_size_and_checksum_after_the_fetch() {
        // The file IS fetched, then refused with rc 1, and the `.partial`
        // stays. The same for no digest at all, SIZE only and checksum only.
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");
        let body =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        for pairs in [
            vec![],
            vec![("SIZE", "4618")],
            vec![("MD5", "54cab52a68eda02d7d41561b8f7d318a")],
        ] {
            let mut routes = HashMap::new();
            routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), body.clone());
            let (base, h) = serve(routes, 1);
            let mut record: HashMap<String, String> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            record.insert("PATH".into(), "dev-libs/packagepkg-1.0.tbz2".into());
            let err = download_and_verify(DownloadAndVerifyArgs {
                sync_uri: &base,
                record: &record,
                category: "dev-libs",
                package: "packagepkg",
                version: "1.0",
                pkgdir: &pkgdir,
                config: &Config::default(),
            })
            .unwrap_err();
            h.join().unwrap();
            let partial = pkgdir.join("dev-libs/packagepkg-1.0.tbz2.partial");
            match err {
                DownloadError::MissingDigests { partial: p } => assert_eq!(p, partial),
                DownloadError::Other(e) => panic!("{pairs:?}: {e}"),
            }
            assert!(partial.is_file(), "{pairs:?}: the .partial is left behind");
            assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").exists());
            std::fs::remove_file(&partial).unwrap();
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_and_verify_refuses_a_tampered_payload_on_any_supported_digest() {
        // Portage verifies every digest the remote record carries, not
        // just MD5/SHA1: SIZE plus only SHA512 (or only BLAKE2B) with a
        // tampered payload is refused. `file://` serves the bytes without
        // an HTTP server.
        // Portage: _emerge/BinpkgVerifier.py, _start;
        // lib/portage/checksum.py, verify_all
        let tmp = tempdir();
        let binhost = tmp.join("binhost");
        let body = b"tampered-payload";
        std::fs::create_dir_all(binhost.join("dev-libs")).unwrap();
        std::fs::write(binhost.join("dev-libs/tamp-1.0.tbz2"), body).unwrap();
        let pkgdir = tmp.join("pkgdir");
        let record = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            let mut record: HashMap<String, String> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            record.insert("PATH".into(), "dev-libs/tamp-1.0.tbz2".into());
            record
        };
        let fetch = |record: &HashMap<String, String>| {
            download_and_verify(DownloadAndVerifyArgs {
                sync_uri: &format!("file://{}", binhost.display()),
                record,
                category: "dev-libs",
                package: "tamp",
                version: "1.0",
                pkgdir: &pkgdir,
                config: &Config::default(),
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
        };
        // One import covers both hashers: sha2/blake2 re-export the same
        // `digest::Digest` trait.
        use sha2::Digest as _;
        let sha512: String = sha2::Sha512::digest(body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let blake2b: String = blake2::Blake2b512::digest(body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        // Right SIZE, wrong SHA512 alone: refused, nothing kept.
        let err = fetch(&record(&[("SIZE", "16"), ("SHA512", &"0".repeat(128))]))
            .expect_err("tampered payload with wrong SHA512 must fail");
        assert!(err.contains("SHA512 mismatch"), "{err}");
        // Right SIZE, wrong BLAKE2B alone: refused, nothing kept.
        let err = fetch(&record(&[("SIZE", "16"), ("BLAKE2B", &"0".repeat(128))]))
            .expect_err("tampered payload with wrong BLAKE2B must fail");
        assert!(err.contains("BLAKE2B mismatch"), "{err}");
        // Right SIZE plus the matching SHA512 alone: verified and renamed.
        fetch(&record(&[("SIZE", "16"), ("SHA512", &sha512)]))
            .expect("matching SHA512 must verify");
        assert!(pkgdir.join("dev-libs/tamp-1.0.tbz2").is_file());
        std::fs::remove_file(pkgdir.join("dev-libs/tamp-1.0.tbz2")).unwrap();
        // Right SIZE plus the matching BLAKE2B alone: verified and renamed.
        fetch(&record(&[("SIZE", "16"), ("BLAKE2B", &blake2b)]))
            .expect("matching BLAKE2B must verify");
        assert!(pkgdir.join("dev-libs/tamp-1.0.tbz2").is_file());
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: std::slice::from_ref(&binrepo),
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: std::slice::from_ref(&binrepo),
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let warning = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: false,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        })
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
    fn http_date_matches_pythons_formatdate() {
        // Python: formatdate(timeval=ts, localtime=False, usegmt=True).
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(1_792_496_587), "Tue, 20 Oct 2026 11:43:07 GMT");
        assert_eq!(http_date(951_782_400), "Tue, 29 Feb 2000 00:00:00 GMT");
    }

    #[test]
    fn refresh_binhost_indexes_sends_if_modified_since_and_keeps_cache_on_304() {
        // Portage `_urlopen(if_modified_since=TIMESTAMP)` adds TIMESTAMP + 5 s;
        // the 304 keeps the cache (no rewrite). Without the header the server
        // would hand out a newer index, so an unchanged cache proves the 304.
        let tmp = tempdir();
        let root = tmp.join("root");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let _h = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let ims = req
                .lines()
                .find_map(|l| l.strip_prefix("If-Modified-Since: "))
                .map(str::to_string);
            let _ = tx.send(req.lines().next().unwrap_or("").to_string());
            let _ = tx.send(ims.clone().unwrap_or_default());
            let resp = if ims.is_some() {
                "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".to_string()
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            };
            let _ = stream.write_all(resp.as_bytes());
        });
        let binrepo = BinRepo {
            name: "ims".to_string(),
            sync_uri: base,
            priority: 1,
            location: None,
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cache = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        let cached = "TIMESTAMP: 100\nVERSION: 0\nPACKAGES: 1\nDOWNLOAD_TIMESTAMP: 1\n\nCPV: dev-libs/foo-1.0\n\n";
        std::fs::write(&cache, cached).unwrap();
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: std::slice::from_ref(&binrepo),
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
        assert_eq!(rx.recv().unwrap(), "GET /Packages.gz HTTP/1.1");
        assert_eq!(rx.recv().unwrap(), "Thu, 01 Jan 1970 00:01:45 GMT");
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), cached);
        let _ = std::fs::remove_dir_all(&tmp);
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: std::slice::from_ref(&binrepo),
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
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

    /// Per-binrepo `gpkg_only`: the run-wide
    /// `FEATURES=binpkg-request-signature` forces every repo; otherwise each
    /// repo's own `verify-signature` decides (an absent key inherits true
    /// from the global `[DEFAULT]`, so only explicit `false` opts out).
    #[test]
    fn binrepo_gpkg_only_follows_request_signature_or_verify_signature() {
        let verifying = BinRepo {
            verify_signature: true,
            ..BinRepo::default()
        };
        let plain = BinRepo {
            verify_signature: false,
            ..BinRepo::default()
        };
        assert!(binrepo_gpkg_only(&verifying, false));
        assert!(binrepo_gpkg_only(&verifying, true));
        assert!(!binrepo_gpkg_only(&plain, false));
        assert!(binrepo_gpkg_only(&plain, true));
    }

    /// The inter-repo overlap gate keys shadowed entries by
    /// instance (`CPV`, `BUILD_ID`, `SIZE`, `BUILD_TIME`), mirroring
    /// `dbapi._instance_key` under binpkg-multi-instance — not by bare CPV.
    /// Same instance in two repos shares one key however the `PATH`s differ;
    /// a different `BUILD_ID` is a different key (both stay visible, newest
    /// build wins at pick time, like Portage).
    #[test]
    fn remote_instance_key_groups_by_instance_not_path() {
        use std::collections::HashMap;
        let entry = |cpv: &str, path: &str, bid: &str| {
            let mut m = HashMap::new();
            m.insert("CPV".to_string(), cpv.to_string());
            m.insert("PATH".to_string(), path.to_string());
            m.insert("BUILD_ID".to_string(), bid.to_string());
            m.insert("SIZE".to_string(), "100".to_string());
            m.insert("BUILD_TIME".to_string(), "1000".to_string());
            m
        };
        // Same CPV+BUILD_ID, gpkg in one repo and unknown extension in the
        // other: one key, so the lower-priority copy is skipped silently.
        assert_eq!(
            remote_instance_key(&entry(
                "app-misc/s511ov-1",
                "app-misc/s511ov-1.gpkg.tar",
                "1"
            )),
            remote_instance_key(&entry("app-misc/s511ov-1", "app-misc/s511ov-1.weird", "1")),
        );
        // Same CPV, different BUILD_ID: different keys, both stay visible.
        assert_ne!(
            remote_instance_key(&entry(
                "app-misc/s511ov-1",
                "app-misc/s511ov-1.gpkg.tar",
                "1"
            )),
            remote_instance_key(&entry(
                "app-misc/s511ov-1",
                "app-misc/s511ov-1.gpkg.tar",
                "2"
            )),
        );
    }

    /// `apply_gpkg_only`: a `.gpkg.tar` `PATH` is kept; xpak ones
    /// (`.tbz2`/`.xpak`) are dropped with a single ignored warning no matter
    /// how many; unknown or absent `PATH`s are dropped with one
    /// `Unsupported …` line each, in index order. Exact texts are Portage's
    /// (`get_binpkg_format` remote error).
    #[test]
    fn apply_gpkg_only_keeps_gpkg_warns_once_for_xpak_and_per_unknown_path() {
        let entry = |path: Option<&str>| {
            let mut e = std::collections::HashMap::new();
            e.insert("CPV".to_string(), "dev-libs/mix-1.0".to_string());
            if let Some(p) = path {
                e.insert("PATH".to_string(), p.to_string());
            }
            e
        };
        let (kept, stderr) = apply_gpkg_only(
            vec![
                entry(Some("dev-libs/mix-1-1.gpkg.tar")),
                entry(Some("dev-libs/mix-1-1.tbz2")),
                entry(Some("dev-libs/mix-1-1.xpak")),
                entry(Some("dev-libs/mix-1-1.weird")),
                entry(None),
            ],
            "mixsig",
            "file:///binhost-sig",
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].get("PATH").map(String::as_str),
            Some("dev-libs/mix-1-1.gpkg.tar")
        );
        assert_eq!(
            stderr,
            "[mixsig] Remote XPAK binpkgs in 'file:///binhost-sig' ignored: signatures are missing (FEATURES=binpkg-request-signature or verify-signature in binrepos.conf).\n\
             Unsupported binary package format from 'dev-libs/mix-1-1.weird'\n\
             Unsupported binary package format from ''\n"
        );
    }

    /// End to end through `refresh_binhost_indexes`: a verifying repo serves
    /// only the gpkg entry while a non-verifying repo serves both, and
    /// `binpkg-request-signature` filters the non-verifying repo too.
    #[test]
    fn refresh_binhost_indexes_applies_gpkg_only_per_binrepo() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let sig = tmp.join("binhost-sig");
        let plain = tmp.join("binhost-plain");
        std::fs::create_dir_all(&sig).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(
            sig.join("Packages"),
            packages_index(&[
                "CPV: dev-libs/sig-g-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: s\nPATH: dev-libs/sig-g-1.0.gpkg.tar",
                "CPV: dev-libs/sig-x-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: s\nPATH: dev-libs/sig-x-1.0.tbz2",
            ]),
        )
        .unwrap();
        std::fs::write(
            plain.join("Packages"),
            packages_index(&[
                "CPV: dev-libs/plain-x-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: s\nPATH: dev-libs/plain-x-1.0.tbz2",
            ]),
        )
        .unwrap();
        let repo = |name: &str, dir: &std::path::Path, verify_signature: bool| BinRepo {
            name: name.to_string(),
            sync_uri: format!("file://{}", dir.display()),
            priority: 1,
            location: None,
            verify_signature,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let sig_repo = repo("mixsig", &sig, true);
        let plain_repo = repo("mixplain", &plain, false);
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: std::slice::from_ref(&sig_repo),
            root: &root,
            pretend: true,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
        assert_eq!(
            remote_versions(std::slice::from_ref(&sig_repo), &root, "sig-g"),
            vec!["1.0"],
            "the verifying repo serves the gpkg entry",
        );
        assert!(
            remote_versions(std::slice::from_ref(&sig_repo), &root, "sig-x").is_empty(),
            "the verifying repo drops the xpak entry",
        );
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: std::slice::from_ref(&plain_repo),
            root: &root,
            pretend: true,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
        assert_eq!(
            remote_versions(std::slice::from_ref(&plain_repo), &root, "plain-x"),
            vec!["1.0"],
            "the plain repo serves the xpak entry",
        );
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: std::slice::from_ref(&plain_repo),
            root: &root,
            pretend: true,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: true,
        });
        assert!(
            remote_versions(std::slice::from_ref(&plain_repo), &root, "plain-x").is_empty(),
            "binpkg-request-signature filters every repo",
        );
        let _ = std::fs::remove_dir_all(&tmp);
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
            // `false`: the served entry has no `PATH`, which Portage 3.0.83
            // drops from a verifying repo (`gpkg_only`); this case covers the
            // unwritable cache, not that policy.
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        // A stale cache with no TTL headers (the TTL check must not fire).
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: true,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: false,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, &cached_body).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: true,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: false,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, cached_body).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: false,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let cached = binrepo.packages_dir(&root).join("Packages");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, cached_body).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend: true,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
            // `false`: the entries here have no `PATH`, which Portage 3.0.83
            // drops from a verifying repo (`gpkg_only`); these cases cover
            // the TIMESTAMP/VERSION drop, not that policy.
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        };
        let stale = "TIMESTAMP: 5\nVERSION: 0\nPACKAGES: 1\n\nCPV: dev-libs/stalepkg-1.0\nSLOT: 0\nKEYWORDS: amd64\nREPO: gentoo\n\n";
        let cached = edb_cache_packages_file(&root, &binrepo.sync_uri);
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, stale).unwrap();
        let outcome = refresh_one_binrepo(RefreshOneBinrepoArgs {
            binrepo: &binrepo,
            root: &root,
            pretend,
            filter: &GetbinpkgFilter::for_binrepo(&binrepo, &[], &[]),
            gpkg_only: false,
            seen: &mut std::collections::HashSet::new(),
        });
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
             RDEPEND: dev-libs/samepkg\nREPO: gentoo\nSIZE: 4618\nMD5: 54cab52a68eda02d7d41561b8f7d318a\nSLOT: 0\nUSE:",
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
            location: Some(
                root.join("var/cache/binhost/test")
                    .to_string_lossy()
                    .to_string(),
            ),
            // `false`: the tests below serve a `.tbz2`, which Portage 3.0.83
            // ignores from a verifying repo (`gpkg_only`); policy is covered
            // by the `gpkg_only` unit tests, not these download/merge paths.
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        }];
        // A failed refresh warns on stderr and resolves against the local pool --
        // here the plain Packages succeeds, so the live index lands in the cache.
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: &binrepos,
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
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
            build_time: None,
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
            jobs: 1,
            mode: crate::emerge_build::StatusMode::for_tests(),
        })
        .expect("getbinpkg merge succeeds");

        assert!(
            root.join("var/cache/binhost")
                .join(&config.binrepos[0].name)
                .join("dev-libs/packagepkg-1.0.tbz2")
                .is_file(),
            "a binrepo `location` is the download directory"
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
    fn run_merge_plan_refuses_a_remote_binpkg_whose_index_lacks_a_checksum() {
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
            location: Some(
                root.join("var/cache/binhost/test")
                    .to_string_lossy()
                    .to_string(),
            ),
            // `false`: the tests below serve a `.tbz2`, which Portage 3.0.83
            // ignores from a verifying repo (`gpkg_only`); policy is covered
            // by the `gpkg_only` unit tests, not these download/merge paths.
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        }];
        // A failed refresh warns on stderr and resolves against the local pool --
        // here the plain Packages succeeds, so the live index lands in the cache.
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: &binrepos,
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
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
            build_time: None,
            deps: Vec::new(),
        };

        // Fetched, then refused before install; the `.partial` stays.
        let err = run_merge_plan(RunMergePlanArgs {
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
            jobs: 1,
            mode: crate::emerge_build::StatusMode::for_tests(),
        })
        .expect_err("a record without a checksum is refused");
        // The block and tail already went to stdout; the pass reports it silently.
        assert_eq!(err.to_string(), "failed packages already reported");
        let dir = root.join("var/cache/binhost/test/dev-libs");
        assert!(dir.join("packagepkg-1.0.tbz2.partial").is_file());
        assert!(!dir.join("packagepkg-1.0.tbz2").exists());
        assert!(!root.join("usr/share/packagepkg/hello.txt").exists());
        assert!(!root.join("var/db/pkg/dev-libs/packagepkg-1.0").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A digest mismatch on the file the pass fetched ends the pass with
    /// Portage's verify-failure block: no `build-info` is unpacked, no
    /// `pretend` phase runs, nothing merges, and nothing is downloaded a
    /// second time. Ground: `t6/portage-bad-digest` (the `!!!
    /// Digest verification failed:` block naming the `.partial` path,
    /// `Got`/`Expected`, the `File renamed to ...` line, then
    /// `>>> Failed to emerge ... for <root>/` with no `, Log file:` tail,
    /// rc 1, nothing merged) and `lib/_emerge/Scheduler.py:1206-1212`
    /// (`finish(verifier.returncode)` returns before the metadata step).
    #[test]
    fn remote_binary_verify_failure_aborts_before_anything_merges() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&pkgdir).unwrap();
        std::fs::create_dir_all(&root).unwrap();

        let tbz2 = std::fs::read(fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"))
            .unwrap();
        // The index lies about the size (S0(c) used 12345 for these bytes),
        // so the pass's verify fails like Portage's `BinpkgVerifier`.
        let index = "TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 1\n\n\
             CPV: dev-libs/binpkgpretendpkg-1.0\nDEFINED_PHASES: install postinst preinst pretend setup\n\
             EAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgpretendpkg-1.0.tbz2\nREPO: testrepo\n\
             SIZE: 12345\nMD5: 2891327f6efd7961432a0a2434431ade\nSHA1: e4d92ef1d47b305068a4efcc32a21abf27ace619\nSLOT: 0\nUSE:\n\n"
            .to_string();
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), index.into_bytes());
        routes.insert("/dev-libs/binpkgpretendpkg-1.0.tbz2".to_string(), tbz2);
        // Refresh tries Packages.gz + Packages.zst (both 404 here) before
        // the plain Packages, then one binpkg download per pass run = 5
        // connections: the explicit pass below plus the plan's own pass.
        // Any merge download would hit a closed stub.
        let (base, _h) = serve(routes, 5);

        let cache = tmp.join("cache");
        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
            location: Some(cache.to_string_lossy().to_string()),
            // `false`: the tests serve a `.tbz2`, which Portage 3.0.83
            // ignores from a verifying repo (`gpkg_only`).
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        }];
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: &binrepos,
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });

        let config = Config {
            binrepos: binrepos.clone(),
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };
        let mut entry = graph_entry("binpkgpretendpkg", CandidateSource::Binary, "1.0");
        entry.remote_binary = true;

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };
        let wrapped = [MixedPretendEntry {
            entry: &entry,
            config: &config,
            repos: &repos,
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            options: &options,
        }];
        let mut replay: Vec<u8> = Vec::new();
        let failures =
            crate::emerge_build::run_pretend_pass(crate::emerge_build::RunPretendPassArgs {
                entries: &wrapped,
                jobs: 1,
                replay: &mut replay,
            });
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].0, 0);
        assert!(
            is_binpkg_digest_failure(&failures[0].1),
            "the silent sentinel, got: {}",
            failures[0].1
        );
        let text = String::from_utf8_lossy(&replay);
        assert!(
            text.contains(">>> Running pre-merge checks for dev-libs/binpkgpretendpkg-1.0"),
            "the pass got that far:\n{text}"
        );
        assert!(
            text.contains("!!! Digest verification failed:"),
            "Portage's block:\n{text}"
        );
        assert!(text.contains("!!! Got: 3394"), "actual size:\n{text}");
        assert!(text.contains("!!! Expected: 12345"), "index size:\n{text}");
        assert!(
            text.contains(">>> Failed to emerge dev-libs/binpkgpretendpkg-1.0 for "),
            "the tail:\n{text}"
        );
        assert!(
            !text.contains("Log file:"),
            "no build log, like Portage:\n{text}"
        );
        // The whole plan aborts: the merge never runs, so nothing is
        // downloaded a second time (the stub served exactly 4 connections).
        let err = run_merge_plan(RunMergePlanArgs {
            entries: &[entry],
            config: &config,
            repos: &repos,
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            merge_options: &options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            jobs: 1,
            mode: crate::emerge_build::StatusMode::for_tests(),
        })
        .expect_err("a verify failure in the pass aborts the plan");
        assert_eq!(err.to_string(), "failed packages already reported");
        assert!(
            !root
                .join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0")
                .exists(),
            "nothing merged"
        );
        assert!(
            !root.join("usr/share/binpkgpretendpkg/payload.txt").exists(),
            "the image never landed"
        );
        let leftovers: Vec<_> = portage_util::read_dir_paths(&cache.join("dev-libs")).unwrap();
        assert!(
            leftovers.len() == 1
                && leftovers[0]
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(
                        |n| n.starts_with("binpkgpretendpkg-1.0.tbz2.partial._checksum_failure_.")
                    ),
            "renamed, not re-downloaded: {leftovers:?}"
        );
        assert!(
            portage_util::read_dir_entries(&pkgdir).unwrap().is_empty(),
            "nothing was fetched into $PKGDIR"
        );
        portage_repo::set_remote_binary_index_override(
            &config.binrepos[0].packages_dir(&root),
            None,
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
             RDEPEND: dev-libs/samepkg\nREPO: gentoo\nSIZE: 4618\nMD5: 54cab52a68eda02d7d41561b8f7d318a\nSLOT: 0\nUSE:",
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
            location: Some(
                root.join("var/cache/binhost/test")
                    .to_string_lossy()
                    .to_string(),
            ),
            // `false`: the tests below serve a `.tbz2`, which Portage 3.0.83
            // ignores from a verifying repo (`gpkg_only`); policy is covered
            // by the `gpkg_only` unit tests, not these download/merge paths.
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        }];
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: &binrepos,
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });

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
            build_time: None,
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
            jobs: 1,
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
            jobs: 1,
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
        let md5 = {
            use md5::Digest as _;
            md5::Md5::digest(std::fs::read(binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2")).unwrap())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        std::fs::write(
            binhost.join("Packages"),
            format!(
                "TIMESTAMP: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/binpkgrmpkg-1.0\n\
                 DEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgrmpkg-1.0.tbz2\n\
                 REPO: testrepo\nSIZE: {size}\nMD5: {md5}\nSLOT: 0\nUSE:\n"
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
                fetchcommand: None,
                resumecommand: None,
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

#[cfg(test)]
mod mixed_pretend_tests {
    use super::*;
    use portage_repo::CandidateSource;

    fn fixtures_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn tempdir() -> portage_util::TempDir {
        portage_util::TempDir::new("portuale-mixed-pretend")
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
            download_files: Vec::new(),
            required_by: vec![],
            source,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            build_time: None,
            deps: Vec::new(),
        }
    }

    /// The mixed plan replays binary pretends up front, in merge-list
    /// order, before any merge. The `binpkgpretendpkg` local `.tbz2`
    /// pretends from its extracted metadata; the source `schedppa` entry
    /// pretends from its ebuild. Both blocks replay, binary first
    /// (mergelist order), with no vdb entry written by the pass itself.
    #[test]
    fn mixed_pretend_pass_replays_binary_and_source_in_merge_list_order() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            pkgdir.join("dev-libs/binpkgpretendpkg-1.0.tbz2"),
        )
        .unwrap();

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };
        let config = Config {
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };
        let entries = [
            graph_entry("binpkgpretendpkg", CandidateSource::Binary, "1.0"),
            graph_entry("schedppa", CandidateSource::Ebuild, "1.0"),
        ];
        let mut replay: Vec<u8> = Vec::new();
        let portage_tmpdir = tmp.join("portage_tmpdir");
        let wrapped: Vec<MixedPretendEntry<'_>> = entries
            .iter()
            .map(|entry| MixedPretendEntry {
                entry,
                config: &config,
                repos: &repos,
                root: &root,
                pkgdir: &pkgdir,
                portage_tmpdir: &portage_tmpdir,
                options: &options,
            })
            .collect();
        let failures =
            crate::emerge_build::run_pretend_pass(crate::emerge_build::RunPretendPassArgs {
                entries: &wrapped,
                jobs: 2,
                replay: &mut replay,
            });
        assert!(failures.is_empty(), "{failures:?}");
        // The source pretend replays to stdout (its `einfo` marker); the
        // binary pretend has no hook stdout of its own (the fixture's hook
        // only appends to `${EROOT}/var/lib/<pn>.log`), so its run is proven
        // by the log file -- while both entries' `Running pre-merge checks`
        // status lines replay in merge-list order (binary first).
        // Portage: _emerge/Scheduler.py, `_status_msg`.
        let text = String::from_utf8_lossy(&replay);
        assert!(
            text.contains("SCHED-PRETEND schedppa"),
            "source pretend replays:\n{text}"
        );
        let b = text
            .find(">>> Running pre-merge checks for dev-libs/binpkgpretendpkg-1.0")
            .expect("binary status line replays");
        let s = text
            .find(">>> Running pre-merge checks for dev-libs/schedppa-1.0")
            .expect("source status line replays");
        assert!(b < s, "merge-list order, not finish order:\n{text}");
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgpretendpkg.log")).unwrap(),
            "pretend\n",
            "binary pretend ran up front, from the extracted metadata"
        );
        assert!(
            !root
                .join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0")
                .exists(),
            "the pass pretends without merging"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A dying binary pretend aborts the mixed pass before anything
    /// merges. The `binpkgpretendpkg` hook dies when the payload is
    /// already visible, so pre-creating it makes its pretend fail.
    #[test]
    fn mixed_pretend_pass_binary_failure_aborts_before_anything_merges() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            pkgdir.join("dev-libs/binpkgpretendpkg-1.0.tbz2"),
        )
        .unwrap();
        // The fixture's `pkg_pretend` dies when the payload is already
        // merged: pre-create it to force the failure.
        let payload = root.join("usr/share/binpkgpretendpkg/payload.txt");
        std::fs::create_dir_all(payload.parent().unwrap()).unwrap();
        std::fs::write(&payload, "already here\n").unwrap();

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };
        let config = Config {
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };
        let entries = [
            graph_entry("binpkgpretendpkg", CandidateSource::Binary, "1.0"),
            graph_entry("schedppa", CandidateSource::Ebuild, "1.0"),
        ];
        let mut replay: Vec<u8> = Vec::new();
        let portage_tmpdir = tmp.join("portage_tmpdir");
        let wrapped: Vec<MixedPretendEntry<'_>> = entries
            .iter()
            .map(|entry| MixedPretendEntry {
                entry,
                config: &config,
                repos: &repos,
                root: &root,
                pkgdir: &pkgdir,
                portage_tmpdir: &portage_tmpdir,
                options: &options,
            })
            .collect();
        let failures =
            crate::emerge_build::run_pretend_pass(crate::emerge_build::RunPretendPassArgs {
                entries: &wrapped,
                jobs: 2,
                replay: &mut replay,
            });
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].0, 0);
        assert!(
            failures[0].1.contains("pkg_pretend failed"),
            "{}",
            failures[0].1
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A `file://` binhost serving the `binpkgpretendpkg` fixture: the
    /// committed `.tbz2` bytes with the stanza a live index would carry
    /// (`pretend` in `DEFINED_PHASES`, like the binpkg's own metadata).
    /// The `SIZE`/`MD5`/`SHA1` are the fixture's own (`md5sum`/`sha1sum`,
    /// not invented); `size_override` stands in for a corrupt index row.
    fn pretend_binhost(binhost: &Path, size_override: Option<u64>) {
        std::fs::create_dir_all(binhost.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            binhost.join("dev-libs/binpkgpretendpkg-1.0.tbz2"),
        )
        .unwrap();
        let size = size_override.unwrap_or(3394);
        let stanza = format!(
            "CPV: dev-libs/binpkgpretendpkg-1.0\nDEFINED_PHASES: install postinst preinst pretend setup\n\
             EAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgpretendpkg-1.0.tbz2\nREPO: testrepo\n\
             SIZE: {size}\nMD5: 2891327f6efd7961432a0a2434431ade\nSHA1: e4d92ef1d47b305068a4efcc32a21abf27ace619\nSLOT: 0\nUSE:"
        );
        std::fs::write(
            binhost.join("Packages"),
            format!("TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 1\n\n{stanza}\n\n"),
        )
        .unwrap();
    }

    fn remote_graph_entry(package: &str, version: &str) -> GraphEntry {
        let mut entry = graph_entry(package, CandidateSource::Binary, version);
        entry.remote_binary = true;
        entry
    }

    fn pretend_binrepo(binhost: &Path, cache: &Path) -> BinRepo {
        BinRepo {
            name: "test".into(),
            sync_uri: format!("file://{}", binhost.display()),
            priority: 1,
            location: Some(cache.to_string_lossy().to_string()),
            // `false`: a verifying repo would drop the `.tbz2` under
            // `gpkg_only`; policy is covered by the `gpkg_only` unit tests.
            verify_signature: false,
            frozen: false,
            fetchcommand: None,
            resumecommand: None,
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
        }
    }

    /// A remote binary whose index record carries `pretend` pretends up
    /// front in the pass, in merge-list order, before anything merges: the
    /// pass fetches and verifies it (the `MD5 SHA1 size` line) and runs its
    /// hook from the fetched metadata. Ground: `t6/portage-ok` (Portage's
    /// `>>> Running pre-merge checks`, then the verifier's `;-) ...
    /// [ ok ]`, then `>>> Emerging binary`) and
    /// `lib/_emerge/Scheduler.py:1147-1180` (fetch, then verify, then
    /// unpack to `build-info` before the `pretend` phase).
    #[test]
    fn remote_binary_pretends_in_the_pass_before_any_merge() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        let binhost = tmp.join("binhost");
        let cache = tmp.join("cache");
        pretend_binhost(&binhost, None);

        let binrepos = vec![pretend_binrepo(&binhost, &cache)];
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: &binrepos,
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
        let config = Config {
            binrepos,
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };
        let entries = [
            remote_graph_entry("binpkgpretendpkg", "1.0"),
            graph_entry("schedppa", CandidateSource::Ebuild, "1.0"),
        ];
        let mut replay: Vec<u8> = Vec::new();
        let portage_tmpdir = tmp.join("portage_tmpdir");
        let wrapped: Vec<MixedPretendEntry<'_>> = entries
            .iter()
            .map(|entry| MixedPretendEntry {
                entry,
                config: &config,
                repos: &repos,
                root: &root,
                pkgdir: &pkgdir,
                portage_tmpdir: &portage_tmpdir,
                options: &options,
            })
            .collect();
        let failures =
            crate::emerge_build::run_pretend_pass(crate::emerge_build::RunPretendPassArgs {
                entries: &wrapped,
                jobs: 2,
                replay: &mut replay,
            });
        assert!(failures.is_empty(), "{failures:?}");
        let text = String::from_utf8_lossy(&replay);
        // Portage's pass order: the status line, then the fetch's verify
        // line, then the pretend output -- binary before source.
        let status = text
            .find(">>> Running pre-merge checks for dev-libs/binpkgpretendpkg-1.0")
            .expect("binary status line replays");
        let verified = text
            .find("binpkgpretendpkg-1.0.tbz2 MD5 SHA1 size ;-)")
            .expect("verify line replays");
        assert!(status < verified, "status before verify:\n{text}");
        assert!(text.contains("[ ok ]"), "verify ok:\n{text}");
        let source = text
            .find(">>> Running pre-merge checks for dev-libs/schedppa-1.0")
            .expect("source status line replays");
        assert!(verified < source, "merge-list order, binary first:\n{text}");
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgpretendpkg.log")).unwrap(),
            "pretend\n",
            "the hook ran up front, from the fetched metadata"
        );
        assert!(
            cache.join("dev-libs/binpkgpretendpkg-1.0.tbz2").is_file(),
            "the pass fetched the remote binary"
        );
        assert!(
            !root
                .join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0")
                .exists(),
            "the pass pretends without merging"
        );
        portage_repo::set_remote_binary_index_override(
            &config.binrepos[0].packages_dir(&root),
            None,
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A dying remote pretend aborts the plan before anything merges.
    /// Ground: `t6/portage-fail-pretend` (Portage's `* ERROR ... failed
    /// (pretend phase)`, then `>>> Failed to emerge ... Log file:`, rc 1,
    /// no `>>> Emerging binary`) and `lib/_emerge/Scheduler.py:1310-1315`
    /// (the pretend phase runs before any merge).
    #[test]
    fn remote_binary_pretend_failure_aborts_before_anything_merges() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        let binhost = tmp.join("binhost");
        let cache = tmp.join("cache");
        pretend_binhost(&binhost, None);
        // The fixture's `pkg_pretend` dies when the payload is already
        // visible: pre-create it to force the failure.
        let payload = root.join("usr/share/binpkgpretendpkg/payload.txt");
        std::fs::create_dir_all(payload.parent().unwrap()).unwrap();
        std::fs::write(&payload, "already here\n").unwrap();

        let binrepos = vec![pretend_binrepo(&binhost, &cache)];
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: &binrepos,
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
        let config = Config {
            binrepos,
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };
        let entries = [remote_graph_entry("binpkgpretendpkg", "1.0")];
        let mut replay: Vec<u8> = Vec::new();
        let portage_tmpdir = tmp.join("portage_tmpdir");
        let wrapped: Vec<MixedPretendEntry<'_>> = entries
            .iter()
            .map(|entry| MixedPretendEntry {
                entry,
                config: &config,
                repos: &repos,
                root: &root,
                pkgdir: &pkgdir,
                portage_tmpdir: &portage_tmpdir,
                options: &options,
            })
            .collect();
        let failures =
            crate::emerge_build::run_pretend_pass(crate::emerge_build::RunPretendPassArgs {
                entries: &wrapped,
                jobs: 1,
                replay: &mut replay,
            });
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].0, 0);
        assert!(
            failures[0].1.contains("pkg_pretend failed"),
            "{}",
            failures[0].1
        );
        let text = String::from_utf8_lossy(&replay);
        assert!(
            text.contains(">>> Running pre-merge checks for dev-libs/binpkgpretendpkg-1.0"),
            "status line replays:\n{text}"
        );
        assert!(
            text.contains("pkg_pretend ran after the image was merged"),
            "the hook's death replays:\n{text}"
        );
        // The whole plan aborts: the merge never runs.
        let err = run_merge_plan(RunMergePlanArgs {
            entries: &entries,
            config: &config,
            repos: &repos,
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            merge_options: &options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            jobs: 1,
            mode: crate::emerge_build::StatusMode::for_tests(),
        })
        .expect_err("a failing remote pretend aborts the plan");
        assert!(
            err.to_string().contains("pkg_pretend failed"),
            "the failure, not a silent sentinel, got: {err}"
        );
        assert!(
            !root
                .join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0")
                .exists(),
            "nothing merged"
        );
        assert!(
            !root.join("var/lib/binpkgpretendpkg.log").exists(),
            "the hook died before appending"
        );
        portage_repo::set_remote_binary_index_override(
            &config.binrepos[0].packages_dir(&root),
            None,
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The merge reuses the file the pass fetched: no second download.
    /// Ground: `t6/portage-ok` (one fetch in the pass, then the merge goes
    /// straight to `>>> Emerging binary`) and
    /// `lib/_emerge/Scheduler.py:1154-1180` (the pass owns the fetch; the
    /// merge finds the file local).
    #[test]
    fn remote_binary_is_fetched_once() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&pkgdir).unwrap();
        let binhost = tmp.join("binhost");
        let cache = tmp.join("cache");
        pretend_binhost(&binhost, None);

        let binrepos = vec![pretend_binrepo(&binhost, &cache)];
        refresh_binhost_indexes(RefreshBinhostIndexesArgs {
            binrepos: &binrepos,
            root: &root,
            pretend: false,
            cli_exclude: &[],
            cli_include: &[],
            binpkg_request_signature: false,
        });
        let config = Config {
            binrepos,
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };
        let entry = remote_graph_entry("binpkgpretendpkg", "1.0");
        let wrapped = [MixedPretendEntry {
            entry: &entry,
            config: &config,
            repos: &repos,
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("portage_tmpdir"),
            options: &options,
        }];
        let mut replay: Vec<u8> = Vec::new();
        let failures =
            crate::emerge_build::run_pretend_pass(crate::emerge_build::RunPretendPassArgs {
                entries: &wrapped,
                jobs: 1,
                replay: &mut replay,
            });
        assert!(failures.is_empty(), "{failures:?}");
        assert!(
            cache.join("dev-libs/binpkgpretendpkg-1.0.tbz2").is_file(),
            "the pass fetched the remote binary"
        );
        // The binhost file vanishes: any second fetch would fail. The merge
        // must reuse the pass's file.
        std::fs::remove_file(binhost.join("dev-libs/binpkgpretendpkg-1.0.tbz2")).unwrap();
        merge_one_binary_entry(MergeOneBinaryEntryArgs {
            entry: &entry,
            config: &config,
            root: &root,
            pkgdir: &pkgdir,
            portage_tmpdir: &tmp.join("pt"),
            merge_options: &MergeOptions::default(),
            progress: mrg_director::MergeProgress::single(),
            display: &crate::emerge_build::StatusDisplay::for_tests(),
        })
        .expect("the merge reuses the pass's fetched file");
        assert!(
            root.join("usr/share/binpkgpretendpkg/payload.txt")
                .is_file(),
            "the binpkg image was merged"
        );
        assert!(
            root.join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0/CONTENTS")
                .is_file(),
            "the vdb entry is live"
        );
        assert!(
            portage_util::read_dir_entries(&pkgdir).unwrap().is_empty(),
            "nothing was fetched into $PKGDIR"
        );
        portage_repo::set_remote_binary_index_override(
            &config.binrepos[0].packages_dir(&root),
            None,
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
