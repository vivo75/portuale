// Real network fetch -- the second half of "actually fetch a package's
// real sources" (see `portage_fetch`'s own module doc comment for the
// pure-logic half: SRC_URI flattening, Manifest parsing, digest
// verification, all offline and 100% unit-testable). Shells out to real
// `wget`, using real `make.globals`'s own default `FETCHCOMMAND`
// template verbatim (`cnf/make.globals`), rather than an in-process HTTP
// client -- matching portuale's own "run the same real external
// process portage would" precedent (`bin/*.sh`, `xpak-helper.py`, ...).
//
// Real `FEATURES=distlocks` is real too (`lib/portage/locks.py:175-`'s
// own `lockfile(mypath, wantnewlockfile=1)`, called at real
// `fetch.py:1315-1330`/unlocked at `:2032-2033`, wrapping the *entire*
// per-file fetch-and-verify sequence, not just the actual download):
// a real, blocking `flock(2)` exclusive lock (real `fcntl.flock`) on a
// real, separate sibling lockfile (real `'.' + basename +
// '.portage_lockfile'`) -- guards against two concurrent portage
// processes racing the same distfile. Confirmed via `cnf/make.globals`
// (line 77-84) that `distlocks` is one of real portage's own *default*
// `FEATURES` tokens (`PortageLockfile::acquire`'s own callers default to
// locking accordingly). Released by simply closing the lock file's own
// fd (`PortageLockfile`'s own `Drop`), the same real effect real
// `unlockfile()`'s own explicit `flock(fd, LOCK_UN)` has -- POSIX
// guarantees all of a process's own `flock` locks on an fd are released
// when that fd is closed. Real `unlinkfile=0` (portuale's own default
// too, matching real `fetch.py`'s own call): the lockfile itself
// persists on disk after release, just unlocked, ready for reuse.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching portuale's own
// "narrow v1, document the cut" pattern):
//   - Resume IS modelled now (the candidate loop passes real's
//     fresh-vs-resume bit through the `mrg_director::FetchRequest` seam,
//     `resume: true` running `make.globals`'s own default
//     `RESUMECOMMAND`, `FETCHCOMMAND` + `-c`): once a non-empty partial
//     file is on disk (a dropped connection, a mirror that closed
//     mid-transfer), the candidate loop switches to `wget -c` to continue
//     it rather than restarting, matching real `fetch.py`. A complete-
//     but-corrupt file (digest mismatch after a full download) is still
//     dropped -- it can't be resumed. Not modelled: real portage's
//     `PORTAGE_FETCH_RESUME_MIN_SIZE` threshold (it only resumes a
//     partial past 350000 bytes) -- portuale resumes any non-empty one.
//   - `mirror://` resolution is real now (`portage_fetch::
//     resolve_mirror_candidates` + `mirror_url`'s real `layout.conf`
//     negotiation and `.mirror-cache.json`, see that
//     crate's own module doc comment for the exact real mechanics
//     covered -- including real `custommirrors`, an admin-configured
//     `${PORTAGE_CONFIGROOT}/etc/portage/mirrors` file, real
//     `RESTRICT=mirror` (`FetchOptions::restrict_mirror` -- the public
//     `GENTOO_MIRRORS` fallback is skipped), real
//     `RESTRICT=primaryuri` (`FetchOptions::restrict_primaryuri` -- the
//     file's own literal URIs move to the front of its candidate list),
//     real `FEATURES=force-mirror` (`FetchOptions::force_mirror` --
//     plain URIs never enter the list), and the real candidate order
//     itself (`assemble_candidates`: local mirrors, then
//     public `GENTOO_MIRRORS`, then `mirror://` expansions, then
//     literals -- rather than portuale's old most-specific-first).
//     On-filesystem mirrors (real `fsmirrors`) are copied from before
//     any download (`copy_from_fsmirrors`). The one real behaviour
//     deliberately not attempted: third-party shuffle (portuale stays
//     deterministic). The
//     `mirror+`/`fetch+`
//     SRC_URI prefixes ARE parsed (`portage_fetch::SrcUriEntry::
//     override_mirror`/`override_fetch`): `mirror+` re-permits the
//     public `GENTOO_MIRRORS` fallback even under `RESTRICT=mirror`, and
//     `override_fetch` (from either prefix) re-permits a plain URI under
//     `RESTRICT=fetch` -- which IS modelled now
//     (`FetchOptions::restrict_fetch`): a plain (non-`mirror://`) URI is
//     barred from the candidate list, and the public mirrors too, so a
//     `RESTRICT=fetch` package fetches OK only from an already-verified
//     `DISTDIR` copy (or `custommirrors`/a `mirror://`-named mirror).
//     Running the ebuild's own `pkg_nofetch` phase for a missing file IS
//     modelled now -- `fetch_src_uri` fails, and its caller
//     `ebuild_phases::fetch_sources` then runs `run_one_phase(env,
//     "nofetch")` so the ebuild's custom "download it from … and place it
//     in DISTDIR" instructions print, before the fetch error propagates.
//   - No `FEATURES=verify-sig` GPG check -- this backlog item was
//     mis-scoped when first written: real `verify-sig`/signature
//     verification is a `gpkg` (the newer GPG-signed binary package
//     format, `lib/portage/gpkg.py`) and repo-sync concept (real
//     `lib/portage/sync/modules/webrsync`'s own gemato-based Manifest
//     signing), not a `SRC_URI`/distfile-fetch one at all -- confirmed
//     by grepping `fetch.py` directly and finding zero hits for either
//     term. Neither `gpkg` nor repo syncing are in portuale's own
//     scope at all yet, so there's nothing to port here.

use mrg_director::{FetchRequest, Fetcher, WgetFetcher};
use portage_fetch::{
    SrcUriEntry, flatten_src_uri, parse_manifest, parse_thirdpartymirrors,
    resolve_mirror_candidates, verify_digests,
};
use std::path::{Path, PathBuf};

use crate::portage_lock::PortageLockfile;

/// Real `make.globals`'s own `GENTOO_MIRRORS="http://distfiles.gentoo.
/// org"` default. Used only by `ebuild_phases::fetch_sources`'s own
/// `FetchOptions` construction, NOT read inside `fetch_src_uri` itself
/// -- unlike `DISTDIR`/`ROOT`/`PORTAGE_TMPDIR`, `GENTOO_MIRRORS` has no
/// dedicated CLI flag of its own yet, but the same "explicit parameter,
/// not an ambient env read inside library code" reasoning still applies
/// here: `FetchOptions.gentoo_mirrors` lets each test control it
/// directly (most tests set it to `vec![]`, so a deliberately-failing
/// fetch doesn't silently reach out to the real `distfiles.gentoo.org`
/// as an unintended fallback) without needing `std::env::set_var`'s own
/// unsoundness under parallel test execution.
pub fn gentoo_mirrors_from_env() -> Vec<String> {
    match std::env::var("GENTOO_MIRRORS") {
        Ok(value) if !value.trim().is_empty() => {
            value.split_whitespace().map(String::from).collect()
        }
        _ => vec!["http://distfiles.gentoo.org".to_string()],
    }
}

/// `distdir` is env-var-sourced at the `ebuild.rs`/`pretend.rs` CLI
/// boundary (`DISTDIR`, same "env var/hardcoded default" shortcut
/// `PKGDIR`/`CONFIG_PROTECT` already use); `Default` matches real
/// `make.globals`'s own `DISTDIR="/var/cache/distfiles"` exactly.
/// `gentoo_mirrors` real make.globals default, see `gentoo_mirrors_
/// from_env`'s own doc comment for why it's a field here rather than
/// read directly inside `fetch_src_uri`. `config_root` (real
/// `PORTAGE_CONFIGROOT`) is consulted only for real `custommirrors`
/// (`${config_root}/etc/portage/mirrors`) -- deliberately a field, not
/// an ambient env read inside this module, mirroring `ebuild_merge::
/// MergeOptions::config_root`'s own doc comment exactly (portuale's
/// own dev/test machine is a real Gentoo system with a real, populated
/// `/etc/portage/mirrors`-shaped tree, so a silent real-`/`-style
/// default here would make every test that doesn't override this field
/// read real host config); `Default` below uses the same deliberately
/// impossible path `MergeOptions::config_root` does, so `fetch_src_uri`
/// always degrades to an empty `custommirrors` map unless a caller
/// opts in explicitly. `distlocks` (real `"distlocks" in self.settings.
/// features`) defaults to `true`: real `distlocks` *is* one of real
/// `make.globals`'s own default `FEATURES` tokens (`cnf/make.globals:
/// 77-84`, confirmed by reading it directly) -- unlike, say,
/// `collision-protect`, which genuinely isn't.
pub struct FetchOptions {
    pub distdir: PathBuf,
    pub gentoo_mirrors: Vec<String>,
    pub config_root: PathBuf,
    pub distlocks: bool,
    /// Real `RESTRICT=mirror` (real `fetch.py:880` --
    /// `restrict_mirror = "mirror" in restrict or "nomirror" in
    /// restrict`): when set, the public `GENTOO_MIRRORS`
    /// fallback (`Candidate::Mirror` roots) is NOT tried for this package
    /// -- real `file_restrict_mirror` gates `location_lists.append(
    /// public_mirrors)` at `fetch.py:1126`. A `mirror://` URI's own
    /// `thirdpartymirrors`/`custommirrors` expansion and any explicit
    /// `SRC_URI` URI are still tried (real portage only drops the
    /// *public* mirror list). Sourced from the ebuild's own
    /// `RESTRICT` md5-cache field by `ebuild_phases::fetch_sources`.
    ///
    /// Real portage's own `mirror+` `SRC_URI` prefix
    /// (`portage_fetch::SrcUriEntry::override_mirror`) re-permits the
    /// public `GENTOO_MIRRORS` fallback for that one file even when this
    /// is set -- `fetch_src_uri` checks `entry.override_mirror`
    /// per-entry, matching real `file_restrict_mirror = ... and not
    /// override_mirror` (`fetch.py:1117-1119`).
    pub restrict_mirror: bool,
    /// Real `RESTRICT=fetch` (real `fetch.py:1061` -- `restrict_fetch =
    /// "fetch" in restrict`): a *plain* (non-`mirror://`) `SRC_URI` URI
    /// is barred from the fetchable-candidate list (real
    /// `fetch.py:1167`, `if (restrict_fetch and not override_fetch) …:
    /// continue`), and the public `GENTOO_MIRRORS` fallback is barred
    /// too (real `(restrict_fetch or restrict_mirror)`). A
    /// `fetch+`/`mirror+` `SRC_URI` prefix
    /// (`portage_fetch::SrcUriEntry::override_fetch`) re-permits the URI.
    /// So a `RESTRICT=fetch` package only fetches OK when its distfile
    /// is already verified in `DISTDIR` (or comes from `custommirrors` /
    /// a `mirror://`-named mirror). Sourced from the ebuild's own
    /// `RESTRICT` md5-cache field by `ebuild_phases::fetch_sources`.
    /// Portuale does NOT run the ebuild's own `pkg_nofetch` phase for
    /// a missing file (a documented cut) -- `fetch_src_uri` fails with a
    /// generic "place it in DISTDIR by hand" pointer instead.
    pub restrict_fetch: bool,
    /// Real `RESTRICT=primaryuri` (real `fetch.py:1187-1189`): the
    /// file's own literal `SRC_URI` URIs (plus the `mirror://`
    /// third-party expansions, real `primaryuri_dict`) move to the
    /// FRONT of that file's candidate list, ahead of the local/public
    /// mirror layouts and the inline `mirror://` expansions -- instead
    /// of the back, where they sit otherwise. Sourced from the ebuild's
    /// own `RESTRICT` md5-cache field by `ebuild_phases::fetch_sources`.
    pub restrict_primaryuri: bool,
    /// Real `FEATURES=force-mirror` (real `fetch.py:1058` +
    /// `fetch.py:1167`): a *plain* (non-`mirror://`) `SRC_URI` URI is
    /// never a fetchable candidate -- the file fetches from mirrors
    /// only. Unlike `restrict_fetch` this is a `FEATURES` token, read
    /// from the process env by `ebuild_phases::fetch_sources` (the same
    /// env-var shortcut `distlocks` above uses).
    pub force_mirror: bool,
    /// The package's resolved `USE` (real `PORTAGE_USE`, the phase env's
    /// `USE`) that `SRC_URI`'s `flag?` groups reduce against -- real
    /// `use_reduce(SRC_URI, uselist=mysettings["PORTAGE_USE"].split())`.
    /// Empty (every `flag?` off, every `!flag?` on) when no resolved
    /// `USE` was threaded (standalone `ebuild <file>` with no graph).
    pub use_flags: std::collections::HashSet<String>,
    /// Real `time.time()` for the `.mirror-cache.json` day-long freshness
    /// check (`mirror_url`); `None` reads the system clock. Tests pin it.
    pub mirror_cache_now: Option<f64>,
    /// Real `PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` (default 5): after this
    /// many downloads of a file fail digest verification, no further
    /// location is tried (`checksum_failure_max_tries`).
    pub checksum_failure_max_tries: usize,
    /// The resolved `FETCHCOMMAND`/`RESUMECOMMAND` family (real
    /// `fetch.py:1652-1700`); `None` keeps the `make.globals` defaults.
    /// Built by the caller from the resolved config scalars via
    /// [`fetch_commands_from_config`].
    pub fetch_commands: Option<portage_fetch::FetchCommands>,
    /// Real `PORTAGE_RO_DISTDIRS` (`fetch.py:1055-1059`): read-only
    /// distdir sources, existing directories only (`shlex.split` +
    /// `isdir` at option-build time), each layout-resolved and
    /// digest-checked, then **symlinked** into a writable DISTDIR.
    pub ro_distdirs: Vec<std::path::PathBuf>,
    /// Real `mysettings.get("PORTAGE_SSH_OPTS")` (`fetch.py:1806-1810`):
    /// the `FETCHCOMMAND_SSH`/`_SFTP` templates' `"${PORTAGE_SSH_OPTS}"`
    /// argument; `None` (the unset setting) expands to empty, same as
    /// real. Built from the resolved config via
    /// [`portage_ssh_opts_from_config`].
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

/// Real `make.globals`'s own default `FETCHCOMMAND`:
/// `wget -t 3 -T 60 --passive-ftp -U "Portage (Gentoo,
/// https://www.gentoo.org) distfile-fetch" -O "${DISTDIR}/${FILE}"
/// "${URI}"` -- invoked as a real subprocess with the exact same
/// arguments, not reimplemented as an in-process HTTP client. A failed
/// fetch removes whatever partial file `wget` may have left behind,
/// same "don't leave broken state around" reasoning `emerge_build.rs`'s
/// own build-failure handling already applies elsewhere.
///
/// The transport itself is `portage_fetch::download_via_wget` (shared
/// with the `mrg-director` `Fetcher` seam); this wrapper names the
/// non-resume `FETCHCOMMAND` for its own callers (the standalone mirror
/// `layout.conf` read below and the remote-`Packages` fetches in
/// `emerge_getbinpkg`), while `fetch_src_uri`'s candidate loop dispatches
/// real's fresh-vs-resume split through the seam.
pub(crate) fn wget_fetch(uri: &str, dest: &Path) -> Result<(), String> {
    portage_fetch::download_via_wget(uri, dest, false)
}

/// Real `mysettings.get("PORTAGE_SSH_OPTS")` (`fetch.py:1806-1810`):
/// the resolved scalar when the config carries it, else `None` (absent
/// expands to empty in the fetch command, exactly like real's missing
/// settings key).
pub fn portage_ssh_opts_from_config(config: &portage_profile::Config) -> Option<String> {
    config.other_vars.get("PORTAGE_SSH_OPTS").cloned()
}

/// Real's `FETCHCOMMAND` family out of a resolved config: the plain
/// scalars (`other_vars`, where `make.globals`/`make.conf` scalars land)
/// plus every `FETCHCOMMAND_<PROTO>`/`RESUMECOMMAND_<PROTO>` override.
///
/// An absent `FETCHCOMMAND`/`RESUMECOMMAND` means the `make.globals`
/// default (T2): real always reads `GLOBAL_CONFIG_PATH`'s `make.globals`
/// (`config.py:446-480`), which a fixture config root does not carry, so
/// the resolved value here is the shipped default template -- never
/// `None`. A `None` field (the "unset" error real `fetch.py:1652-1660`
/// prints when `make.globals` itself is missing) is only reachable from
/// a hand-built [`portage_fetch::FetchCommands`].
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

/// Real `doebuild()`'s own `SRC_URI`-vs-`DISTDIR` fetch check, run once
/// before a real `unpack` phase (see `ebuild_phases.rs`'s own call
/// Real `fetch.py` per-file candidate order (`fetch()`'s own
/// `filedict[myfile]` build, `:1099-1192`) for one distfile's `group` of
/// `SRC_URI` entries (`group_by_filename`, real `_parse_uri_map`).
///
/// Order (real positions in parentheses):
/// 1. `custommirrors["local"]` non-`/` entries as mirror roots
///    (real `local_mirrors`, always tried -- even under
///    `RESTRICT=fetch`/`mirror`, real `location_lists =
///    [local_mirrors] + ...`). `/`-rooted entries are on-filesystem
///    mirrors (real `fsmirrors`), copied from before this list is
///    tried (`copy_from_fsmirrors`), never download candidates.
/// 2. public `GENTOO_MIRRORS` roots (real `public_mirrors`), once per
///    file, unless mirror-restricted (real `file_restrict_mirror`, which
///    only the first entry's `mirror+` prefix re-permits).
/// 3. every entry's `mirror://` expansions inline, in `SRC_URI` order
///    (`custommirrors` then `thirdpartymirrors`, unshuffled -- real
///    shuffles the third-party half for load-balancing; portuale stays
///    deterministic).
/// 4. the primary-uri group: the file's literal URIs in REVERSE
///    `SRC_URI` order (real `uris.reverse()`), then every third-party
///    expansion again -- APPENDED last normally, PREPENDED first under
///    `RESTRICT=primaryuri` (real `fetch.py:1186-1192`). The repeated
///    third-party URLs are attempted once (real `tried_locations`).
///    A plain literal barred by `RESTRICT=fetch` (without a
///    `fetch+`/`mirror+` re-permit) or by `FEATURES=force-mirror` never
///    enters the list (real `fetch.py:1167` `continue` -- `force-mirror`
///    skips even a re-permitted literal).
///
/// Steps 1-2 are mirror *roots* (`Candidate::Mirror`), resolved to a URL
/// only when the loop reaches them (`mirror_url`, real's
/// `functools.partial(async_mirror_url, ...)`), so a file fetched from an
/// earlier candidate never downloads a mirror's `layout.conf`.
fn assemble_candidates(
    group: &[&SrcUriEntry],
    custommirrors: &std::collections::HashMap<String, Vec<String>>,
    thirdpartymirrors: &std::collections::HashMap<String, Vec<String>>,
    options: &FetchOptions,
) -> Vec<Candidate> {
    let Some(first) = group.first() else {
        return Vec::new();
    };
    // Real `local_mirrors` keep their spelling (no `rstrip`), real
    // `public_mirrors` are `x.rstrip("/")` -- both are also the
    // `.mirror-cache.json` keys, so the spelling matters.
    let mut filedict: Vec<Candidate> = custommirrors
        .get("local")
        .into_iter()
        .flatten()
        .filter(|r| !r.starts_with('/'))
        .map(|r| Candidate::Mirror(r.clone()))
        .collect();
    // Real `file_restrict_mirror` is decided once, when the filename is
    // first seen -- by the FIRST URI's `mirror+` override.
    let public_barred =
        (options.restrict_mirror || options.restrict_fetch) && !first.override_mirror;
    if !public_barred {
        filedict.extend(
            options
                .gentoo_mirrors
                .iter()
                // `/`-rooted entries are real `fsmirrors`
                // (`copy_from_fsmirrors`), never download candidates.
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
        // Real `filedict[myfile] = primaryuri_dict.get(myfile, []) + uris`.
        [primary, filedict].concat()
    } else {
        [filedict, primary].concat()
    }
}

/// Real `primaryuri_dict[myfile]` (`fetch.py:1165-1183`): the file's
/// literal URIs that may be fetched, in REVERSE `SRC_URI` order (real
/// `uris.reverse()` -- as real `fetch(..., listonly=1)` shows, tried
/// last-listed first), then every third-party `mirror://` expansion.
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

/// Real `checksum_failure_max_tries` (`fetch.py:896-934`): real
/// `PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` as an integer, default 5; a
/// non-integer or a value below 1 warns (the returned lines, real's
/// `writemsg` text) and uses the default.
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

/// Real `_parse_uri_map` (`porttree.py:1828`): `SRC_URI` entries grouped
/// by distfile name in first-seen order, identical URIs listed once.
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

/// Real `async_mirror_url(mirror_url, filename, mysettings, cache_path)`
/// (`fetch.py:731-785`): the URL (or, for a `/`-rooted mirror, the local
/// path) of `filename` under `mirror`, laid out per the mirror's own
/// `layout.conf`.
///
/// - `cache_path` (real: `${DISTDIR}/.mirror-cache.json` when `DISTDIR`
///   is writable, else `None`) holds each mirror's `[structure]` with the
///   time it was read; an entry younger than a day is used as is.
/// - Otherwise the `layout.conf` is read: from `<mirror>/layout.conf` for
///   a `/`-rooted mirror (a missing file reads as empty, real
///   `read_configs` swallows `OSError`), else downloaded from
///   `<mirror>/distfiles/layout.conf` into `${DISTDIR}/.layout.conf.<host>`
///   (real `async_fetch(..., force=1, try_mirrors=0)`; left in place like
///   real). Only a successful read is cached; a failed download or an
///   unparseable file silently means "flat, nothing cached".
/// - `now` is real `time.time()`, passed in so the day-long freshness
///   window is testable.
fn mirror_url(
    mirror: &str,
    filename: &str,
    digests: &std::collections::HashMap<String, String>,
    distdir: &Path,
    cache_path: Option<&Path>,
    now: f64,
) -> String {
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
            // Real `atomic_ofstream`: write a sibling, then rename over.
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

/// Real `fsmirrors` (`fetch.py:1019-1030`): the `/`-rooted
/// `custommirrors["local"]` entries (as written), then the `/`-rooted
/// `GENTOO_MIRRORS` entries (`rstrip("/")`).
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

/// Real `fetch.py:1503-1513`: try each on-filesystem mirror in order,
/// resolving the file's path through that directory's own `layout.conf`
/// (real `async_mirror_url(mydir, myfile, mysettings)` -- no cache path,
/// so it is re-read every time), copy the first one that exists to
/// `dest`, and print real's `Local mirror has file: <file>`. A missing
/// file (`ENOENT`/`ESTALE`) moves on to the next mirror; any other copy
/// error is fatal, as real re-raises it. Returns whether a copy was made.
fn copy_from_fsmirrors(
    fsmirrors: &[String],
    filename: &str,
    digests: &std::collections::HashMap<String, String>,
    options: &FetchOptions,
    dest: &Path,
) -> Result<bool, String> {
    for dir in fsmirrors {
        let source = mirror_url(dir, filename, digests, &options.distdir, None, 0.0);
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

/// Real `os.access(DISTDIR, os.W_OK)` (`fetch.py:991`), which decides
/// whether `.mirror-cache.json` is used at all.
fn distdir_writable(distdir: &Path) -> bool {
    let Ok(c) = std::ffi::CString::new(distdir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path that outlives the call; the
    // second argument is a plain access-mode flag.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Real `fetch.py:1503`: check if there's enough free space on the
/// filesystem holding `distdir` to accommodate a file of size `bytes`.
/// Uses `os.statvfs` (real `_emerge/main.py:1063`) to get filesystem stats.
fn has_enough_space(distdir: &Path, bytes: u64) -> bool {
    let Ok(c) = std::ffi::CString::new(distdir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path; `stat` is a valid mutable buffer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return false;
    }
    // Available space = block size * available blocks.
    let available_bytes = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
    available_bytes >= bytes
}

/// site): for every file `src_uri` (this ebuild's own real, md5-cache-
/// sourced `SRC_URI` string) names for the current USE set, fetches it
/// into `options.distdir` unless a real, Manifest-verified copy is
/// already there. `flag?` groups reduce against `options.use_flags`
/// (real `use_reduce(..., uselist=PORTAGE_USE)`, #37 S4): before the
/// resolved `USE` was threaded here every `flag?` group was treated as
/// off, so an enabled `eselect? ( bashcomp.tar.gz )` was never fetched
/// nor unpacked while `use eselect` was true in `src_prepare`.
///
/// Returns the real filename list real `A` should be set to (the
/// caller is responsible for actually exporting it -- this module has
/// no opinion on shell environment setup). A file with no `Manifest`
/// entry at all is refused outright rather than fetched-but-unverified:
/// unverifiable content is worse than a loud failure, the same
/// reasoning `emerge_build.rs`'s own (now-superseded) blanket SRC_URI
/// refusal established for the "no fetch machinery at all" case this
/// slice replaces.
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

    // Real `mirror://` resolution (`profiles/thirdpartymirrors`, the
    // ebuild's own repo's copy -- `repo_root_for` tolerates a
    // standalone ebuild outside any repo checkout the same way it
    // already does for eclass resolution, yielding an empty map, i.e.
    // no thirdpartymirror candidates at all) plus real `custommirrors`
    // (`${config_root}/etc/portage/mirrors`, real `grabdict()`'s own
    // format -- reuses `parse_thirdpartymirrors` directly, since it's
    // the exact same real format, just a different real source file)
    // plus the real `GENTOO_MIRRORS` fallback -- see this
    // module's own doc comment for the exact real mechanics
    // covered/not covered.
    let thirdpartymirrors = crate::ebuild_phases::repo_root_for(pkg_dir)
        .map(|repo_root| parse_thirdpartymirrors(&repo_root.join("profiles/thirdpartymirrors")))
        .transpose()?
        .unwrap_or_default();
    // `.unwrap_or_default()`, not `?`: `options.config_root` is
    // deliberately an impossible sentinel path by default (see
    // `FetchOptions::config_root`'s own doc comment) -- joining
    // `etc/portage/mirrors` onto it can fail with `ENOTDIR` (an
    // *ancestor* component isn't a directory), not just the `NotFound`
    // `parse_thirdpartymirrors` itself already tolerates, so this
    // degrades to an empty `custommirrors` map on *any* resolution
    // failure, the same graceful-degrade precedent `ebuild_merge::
    // blocked_installed_packages`'s own `find_repos(config_root).ok()?`
    // already established for this exact sentinel-path pattern.
    let custommirrors = parse_thirdpartymirrors(&options.config_root.join("etc/portage/mirrors"))
        .unwrap_or_default();

    let mut filenames = Vec::new();
    for (_, group) in group_by_filename(&entries) {
        // The first entry names the file (and any error message).
        let entry = group[0];
        let dest = options.distdir.join(&entry.filename);
        // Real `FEATURES=distlocks`: acquired before even checking
        // whether the file is already fetched (real `fetch.py:1315`,
        // ahead of its own `_check_distfile` call at `:1336`), held for
        // the entire per-file sequence below, released when `_lock`
        // drops at the end of this loop iteration -- see this module's
        // own doc comment. Deliberately acquired *after* the "no
        // Manifest entry" refusal just below rather than strictly
        // mirroring real ordering: portuale's own single unified
        // refusal for unverifiable content has no real single-point
        // equivalent (real portage's own structure is different here),
        // and there's nothing to actually fetch or protect with a lock
        // when refusing outright -- no reason to require `DISTDIR`
        // write access just to reach that refusal.
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

        // Real `fetch.py:1456-1475`: `PORTAGE_RO_DISTDIRS` -- a
        // read-only distdir source is layout-resolved and
        // digest-checked, and on a match **symlinked** into DISTDIR
        // (never copied), only when DISTDIR itself is writable. Runs
        // before the local fsmirrors copy and every download candidate.
        if !already_verified
            && distdir_writable(&options.distdir)
            && !options.ro_distdirs.is_empty()
        {
            for dir in &options.ro_distdirs {
                if !dir.is_dir() {
                    continue;
                }
                let source = mirror_url(
                    &dir.to_string_lossy(),
                    &entry.filename,
                    &digests.hashes,
                    &options.distdir,
                    None,
                    0.0,
                );
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

        // Real `fetch.py:1503-1513`: before any download (and regardless
        // of `RESTRICT=fetch`/`mirror`, which only shape the download
        // list), a missing file is copied from the first on-filesystem
        // mirror that has it -- only if there is enough free space.
        if !already_verified && !dest.exists() && has_enough_space(&options.distdir, digests.size) {
            let fsmirrors = fsmirrors(&custommirrors, &options.gentoo_mirrors);
            if copy_from_fsmirrors(&fsmirrors, &entry.filename, &digests.hashes, options, &dest)? {
                match verify_digests(&dest, digests) {
                    Ok(()) => already_verified = true,
                    // Real keeps a short copy to resume from; a full-size
                    // but corrupt one is replaced by the download.
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
            // Real `fetch.py:1166-1174`: `if (restrict_fetch and not
            // override_fetch) or force_mirror: continue` -- a *plain*
            // (non-`mirror://`) `SRC_URI` URI is NOT a fetchable
            // candidate under `RESTRICT=fetch` (only `mirror://`-named
            // mirrors + `custommirrors` are). A `fetch+`/`mirror+` prefix
            // (`entry.override_fetch`) re-permits it under
            // `RESTRICT=fetch` (but not under `force-mirror`). A
            // `mirror://` URI's own candidates already come only from
            // the expansions, never the raw token, so nothing to strip
            // there.
            let plain_uri_barred_by_restrict_fetch =
                (options.restrict_fetch && !entry.override_fetch || options.force_mirror)
                    && !entry.uri.starts_with("mirror://");
            // Real `file_restrict_mirror = (restrict_fetch or
            // restrict_mirror) and not override_mirror`
            // (`fetch.py:1117-1119`): the public `GENTOO_MIRRORS`
            // mirror list is appended unless mirroring is
            // restricted -- but a `mirror+` SRC_URI prefix on this URI
            // (`entry.override_mirror`) re-permits it for this file even
            // then. `RESTRICT=fetch` implies mirror restriction too
            // (real: `(restrict_fetch or restrict_mirror)`).
            let public_mirrors_barred =
                (options.restrict_mirror || options.restrict_fetch) && !entry.override_mirror;
            if candidates.is_empty() {
                // Real `fetch.py`: a `RESTRICT=fetch` file that isn't
                // already in `DISTDIR` fails here; the caller
                // (`ebuild_phases::fetch_sources`) then runs the ebuild's
                // own `pkg_nofetch` phase, which prints custom "download
                // it from … and place it in DISTDIR" instructions.
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
            // Real `fetch.py`'s `tried_locations`: the assembled list
            // legitimately repeats a location (a `mirror://` URI's
            // third-party expansions sit both inline and in the
            // primary-uri group), but each is attempted once per file.
            let mut tried = std::collections::HashSet::new();
            let cache_path = distdir_writable(&options.distdir)
                .then(|| options.distdir.join(".mirror-cache.json"));
            let now = options.mirror_cache_now.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_secs_f64())
            });
            // Real `uri_list`: a stack popped from the end, so the
            // primary-uri switch below jumps ahead of what is left.
            let mut uri_list: Vec<Candidate> = candidates.iter().rev().cloned().collect();
            let mut checksum_failures = 0;
            // The download half is the director's `Fetcher` seam: one
            // resolved candidate URI -> `dest`, fresh or resumed, with no
            // Manifest context -- digest verification stays right below
            // at this call site, which holds the `Manifest` entry. The
            // one Manifest-derived value the transport needs is real's
            // `DIGESTS` substitution (`fetch.py:1797-1803`), formatted
            // here (this is where the entry lives) and carried
            // pre-formatted.
            let digests_var = portage_fetch::digests_variable(digests);
            let fetcher: &dyn Fetcher = &WgetFetcher;
            while let Some(candidate) = uri_list.pop() {
                let candidate = match &candidate {
                    Candidate::Uri(uri) => uri.clone(),
                    Candidate::Mirror(root) => mirror_url(
                        root,
                        &entry.filename,
                        &digests.hashes,
                        &options.distdir,
                        cache_path.as_deref(),
                        now,
                    ),
                };
                if !tried.insert(candidate.clone()) {
                    continue;
                }
                let candidate = candidate.as_str();
                // Real `fetch.py`: once a non-empty partial is on disk
                // (from an earlier candidate that dropped mid-transfer, or
                // a previous interrupted run), switch from `FETCHCOMMAND`
                // to `RESUMECOMMAND` -- `wget -c` continues it rather than
                // restarting.
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
                            // Real `_checksum_failure_temp_file`: rename the
                            // corrupt file to preserve it as evidence, with a
                            // deterministic suffix (counter) instead of random.
                            let bad_file = format!(
                                "{}._checksum_failure_{}",
                                dest.display(),
                                checksum_failures
                            );
                            let _ = std::fs::rename(&dest, &bad_file);
                            eprintln!("Refetching... File renamed to '{bad_file}'");
                            // Real `checksum_failure_count`: the second
                            // failure switches to "primaryuri" mode (the
                            // file's primary URIs are tried next); the
                            // cap stops trying further locations at all.
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
                    // A transport failure may have left a resumable
                    // partial -- keep it for the next candidate.
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
    use std::fs;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale_fetch_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Real, independently-known BLAKE2b-512 and SHA-512 digests of the
    // literal bytes "hello world" (confirmed via the real `b2sum`/
    // `sha512sum` system tools, not invented) -- reused across these
    // tests as a fixed, known-good distfile payload.
    const HELLO_BLAKE2B: &str = "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbcc05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0";
    const HELLO_SHA512: &str = "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f";

    fn write_manifest(pkg_dir: &Path, filename: &str, size: u64) {
        fs::write(
            pkg_dir.join("Manifest"),
            format!("DIST {filename} {size} BLAKE2B {HELLO_BLAKE2B} SHA512 {HELLO_SHA512}\n"),
        )
        .unwrap();
    }

    /// Serves `body` over real, plain HTTP on `127.0.0.1` for exactly
    /// one connection, on an OS-assigned ephemeral port -- lets a test
    /// exercise the real, unmodified `wget` subprocess end-to-end
    /// (spawn, `-O`, real HTTP response parsing) without needing
    /// genuine internet access. `file://` URIs would be simpler, but
    /// this system's own `wget` build has no `file://` support at all
    /// (confirmed empirically: `wget file:///etc/hostname` ->
    /// `"Unsupported scheme."`) -- real loopback HTTP has no such gap.
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

    /// A mirror-shaped HTTP server: answers `connections` requests,
    /// serving `routes`' bodies by request path and 404 for anything
    /// else, and records every requested path in order. Returns the
    /// mirror root (`http://127.0.0.1:<port>`), the recorded paths, and
    /// the thread handle. A caller expecting fewer requests than
    /// `connections` stops the thread with `unblock_server` (an empty
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

    /// Connects (and sends nothing) so a `serve_mirror` thread still
    /// waiting in `accept` can finish.
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

        // Real `unlinkfile=0`: the lockfile persists on disk, just
        // unlocked -- a second acquire (in the same process, a
        // re-entrant flock on a fresh fd for the same file) must
        // succeed immediately, not block on itself.
        PortageLockfile::acquire(&dest).expect("second acquire succeeds once the first is dropped");
    }

    /// Real, end-to-end proof of the actual blocking behavior
    /// `flock(2)` provides: a second acquire on the same distfile, from
    /// a different thread, genuinely blocks until the first lock is
    /// dropped -- not merely that the API happens to return `Ok`.
    #[test]
    fn distfile_lock_blocks_a_second_acquire_until_released() {
        let dir = tempdir();
        let dest = dir.join("foo-1.0.tar.gz");

        let lock1 = PortageLockfile::acquire(&dest).expect("first acquire succeeds");

        let dest2 = dest.clone();
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

        // The URI is deliberately unreachable (a reserved, non-routable
        // TEST-NET address, RFC 5737) -- if the already-verified skip
        // logic didn't work, this would hang/fail on the real network
        // instead of returning immediately.
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

    /// #37 S4 (L2 real set, `app-shells/bash-completion[eselect]`): a
    /// `flag?` group reduces against the resolved `USE`, so an enabled
    /// conditional distfile is part of `A`, and a `!flag?` one is not.
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
        // #70 S1: FETCHCOMMAND from the config replaces the built-in
        // `wget` template; `${DISTDIR}`/`${FILE}`/`${URI}` are
        // substituted and the command is spawned directly.
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
        // #70 R2 / T2: a config without FETCHCOMMAND/RESUMECOMMAND (every
        // fixture config root, which has no make.globals) resolves to the
        // shipped default templates -- never `None`. `None` is only
        // reachable from a hand-built `FetchCommands`.
        let commands = fetch_commands_from_config(&portage_profile::Config::default());
        assert_eq!(commands, portage_fetch::FetchCommands::default());
    }

    #[test]
    fn portage_ssh_opts_from_config_reads_the_scalar() {
        // #70 R4: the FETCHCOMMAND_SSH/SFTP templates' `"${PORTAGE_SSH_OPTS}"`
        // argument comes from the resolved settings; absent stays None
        // (which expands to empty, like real).
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
        // #70 R2 end-to-end through the production parser: the config
        // comes from `resolve_config` on a root with no
        // `usr/share/portage/config/make.globals` (T2's fixture shape);
        // the default `wget` template must still download `serve_once`'s
        // file.
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let uri = format!("{uri_base} -> hello-1.0.tar.gz");
        let pkg_dir = tempdir();
        let distdir = tempdir();
        write_manifest(&pkg_dir, "hello-1.0.tar.gz", 11);

        let config_root = tempdir();
        let eroot = tempdir();
        let config = portage_profile::resolve_config(
            &config_root,
            &config_root,
            &[],
            &[],
            "gentoo",
            &std::collections::HashMap::new(),
            &eroot,
        )
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
        // #70 S2: PORTAGE_RO_DISTDIRS -- a verified read-only copy is
        // symlinked into a writable DISTDIR, never downloaded.
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

    /// Serves `body` in two halves: connection 1 gets the real
    /// `Content-Length` header but only the first `split` bytes before the
    /// socket closes (a dropped transfer); connection 2, which `wget -c`
    /// makes with a `Range: bytes=<split>-` header, gets a real `206
    /// Partial Content` with the rest.
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
        // The server returns different content entirely from what the
        // Manifest (still claiming the "hello world" digests/size)
        // expects.
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

    /// Real, end-to-end `mirror://` resolution: `pkg_dir` sits under a
    /// real (if fixture-only) repo checkout (`profiles/repo_name` +
    /// `profiles/thirdpartymirrors`, exactly the layout `repo_root_for`
    /// already looks for), whose own `thirdpartymirrors` names a mirror
    /// pointing at a real local HTTP server -- a genuine `mirror://
    /// testmirror/foo-1.0.tar.gz` SRC_URI is resolved through that file,
    /// fetched via a real `wget` subprocess, and digest-verified,
    /// proving the whole chain (`repo_root_for` -> `parse_
    /// thirdpartymirrors` -> `resolve_mirror_candidates` ->
    /// `Fetcher::fetch` -> `verify_digests`) works together, not just
    /// each piece in isolation.
    #[test]
    fn fetch_src_uri_resolves_a_real_mirror_uri_via_thirdpartymirrors() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        // `serve_once`'s own handler doesn't look at the request path at
        // all, so appending an extra path segment (the way a real
        // `mirror://` expansion would: `<mirror_root>/<path>`) is safe.
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

    /// Real, end-to-end `custommirrors` proof: a real
    /// `${config_root}/etc/portage/mirrors` file (real `grabdict()`
    /// format, same as `profiles/thirdpartymirrors`) resolves a
    /// `mirror://<name>` token via a real local HTTP server, with no
    /// `profiles/thirdpartymirrors` entry for that name at all --
    /// proving `custommirrors` is consulted independently, not merely
    /// as a fallback when `thirdpartymirrors` already has the name.
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

    /// Regression test for a real bug this slice's own implementation
    /// hit and fixed: `FetchOptions::default()`'s own deliberately
    /// impossible `config_root` sentinel (`/dev/null/...`) makes
    /// `${config_root}/etc/portage/mirrors` fail with `ENOTDIR` (an
    /// *ancestor* path component isn't a directory), not the `NotFound`
    /// `parse_thirdpartymirrors` itself already tolerates -- an earlier
    /// version of this code propagated that as a raw I/O error instead
    /// of degrading gracefully to "no custommirrors", producing a
    /// confusing low-level error instead of the real, clean "no working
    /// candidate mirror" message for an unknown `mirror://` name.
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

    /// Real, end-to-end `GENTOO_MIRRORS` fallback: the literal `SRC_URI`
    /// itself is deliberately unreachable (port 1, which real,
    /// unprivileged `wget` gets an immediate real "Connection refused"
    /// for -- fast and deterministic, unlike a black-holed address that
    /// would make this test hang for real `wget -t 3 -T 60`'s own full
    /// multi-minute retry budget), so the fetch only succeeds because
    /// `FetchOptions.gentoo_mirrors` names a real local HTTP server. That
    /// mirror has no `layout.conf` (404), so real `async_mirror_url` uses
    /// the flat `<root>/distfiles/<filename>` path and caches nothing.
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

    /// The real Gentoo mirrors' layout (`0=filename-hash BLAKE2B 8`,
    /// where the flat path 404s): the file is fetched from the hashed
    /// path real portage computes (`ce/hello-1.0.tar.gz`, real
    /// `FilenameHashLayout('BLAKE2B', '8').get_path`), the downloaded
    /// `layout.conf` is left as `.layout.conf.<host>`, and the structure
    /// is cached in real's `.mirror-cache.json` shape.
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

    /// Real `ts >= time.time() - 86400`: a cache entry younger than a day
    /// is used without asking the mirror again; an older one is
    /// refreshed (and rewritten with the new time).
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

    /// Real resolves a mirror candidate lazily (`functools.partial`): a
    /// file fetched from an earlier candidate never asks the later
    /// mirror for its `layout.conf`.
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

    /// Real `RESTRICT=mirror` (`file_restrict_mirror`,
    /// `fetch.py:1117-1127`): the public `GENTOO_MIRRORS`
    /// fallback is NOT tried. Identical setup to
    /// `fetch_src_uri_falls_back_to_gentoo_mirrors_when_the_primary_uri_is_unreachable`
    /// (its "without restrict" counterpart -- there the mirror server
    /// rescues the fetch), but with `restrict_mirror: true`: the primary
    /// URI is unreachable (`127.0.0.1:1` -> immediate "Connection
    /// refused") and the mirror is barred, so the whole fetch fails and
    /// the mirror server is never contacted.
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

    /// Real `mirror+` SRC_URI prefix (`fetch.py:1103` -> real
    /// `override_mirror`): re-permits the public `GENTOO_MIRRORS`
    /// fallback for this file even under `RESTRICT=mirror`. Identical
    /// setup to `fetch_src_uri_restrict_mirror_skips_the_gentoo_mirrors_
    /// fallback` (unreachable primary URI, `restrict_mirror: true`) --
    /// but the SRC_URI token has a `mirror+` prefix, so the mirror
    /// server IS tried and rescues the fetch.
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

    /// `RESTRICT=mirror` bars only the *public* `GENTOO_MIRRORS`
    /// mirror list -- a `mirror://` URI's own `custommirrors`
    /// expansion is still tried (real portage keeps `local_mirrors` in
    /// `location_lists` regardless, `fetch.py:1125`). Same fixture as
    /// `fetch_src_uri_resolves_a_real_mirror_uri_via_custommirrors`,
    /// plus `restrict_mirror: true`.
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

    /// Real `fsmirrors` (`fetch.py:1503-1513`): a `/`-rooted
    /// `GENTOO_MIRRORS` entry is a local directory the missing file is
    /// copied from -- flat (`<dir>/<file>`, real `os.path.join`, no
    /// `distfiles/`) without a `layout.conf`, per the directory's own
    /// `layout.conf` otherwise -- before any download, and even when
    /// `RESTRICT=fetch` leaves nothing to download at all.
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

    /// Real order: `custommirrors["local"]` `/` entries before `/`-rooted
    /// `GENTOO_MIRRORS`, first mirror that has the file wins, a mirror
    /// without it is skipped. The public directory holds a corrupt
    /// same-size copy, so only the local one can satisfy the digest.
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

    /// A `/`-rooted `GENTOO_MIRRORS` entry is never handed to `wget` as a
    /// URL: when the directory lacks the file, only the real download
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

    /// Real `RESTRICT=fetch` (`fetch.py:1061`/`:1167`): a plain `SRC_URI`
    /// URI is not a fetchable candidate, and the public mirrors are
    /// barred -- so a fetch-restricted package whose distfile isn't
    /// already in `DISTDIR` fails, without ever contacting the URI or
    /// the mirror server.
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

    /// `RESTRICT=fetch` still accepts an already-verified `DISTDIR` copy
    /// (the normal way a fetch-restricted package is satisfied -- the
    /// user placed the file by hand).
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

    /// A `fetch+` `SRC_URI` prefix (`override_fetch`) re-permits the
    /// plain URI even under `RESTRICT=fetch` (real `fetch.py:1167`, `if
    /// (restrict_fetch and not override_fetch)`).
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

    /// Real `fetch.py:1112-1192` order for a plain URI: local
    /// mirrors, public `GENTOO_MIRRORS`, then the literal
    /// URI itself last (no `mirror://` expansions, no third-party
    /// tail).
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

    /// Real `fetch.py:1187-1189` (`RESTRICT=primaryuri`): the literal
    /// URI (plus the third-party expansions) moves ahead of the mirror
    /// lists -- including real's own double listing of the third-party
    /// expansions (inline below, and again in the primary-uri group).
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

    /// Several `SRC_URI` entries for one distfile share one list (real
    /// `filedict[myfile]`). Expected orders are real portage's own
    /// `fetch(OrderedDict({file: uris}), settings, listonly=1)` output
    /// (portage 3.0.82.2, `GENTOO_MIRRORS` = one mirror, `gnome` = its
    /// single-root `thirdpartymirrors` entry): the public mirror once,
    /// inline `mirror://` expansions, then the literals LAST-LISTED FIRST
    /// and the third-party expansions again (tried once) -- or that
    /// primary-uri group first under `RESTRICT=primaryuri`.
    #[test]
    fn assemble_candidates_groups_a_files_uris_like_real_listonly() {
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

    /// Three public mirrors serving a corrupt same-size copy, then the
    /// file's own (good) literal URI. Real `fetch.py:1975-2000`: the
    /// second digest failure jumps to the primary URIs ahead of the
    /// remaining mirror, so mirror 3 is never contacted; with a cap of 2
    /// (`PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS=2`) nothing after the second
    /// failure is tried at all.
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
    fn checksum_failure_max_tries_parses_like_real() {
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

    /// Real `_parse_uri_map`: entries grouped by distfile in first-seen
    /// order, an identical URI listed once.
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

    /// Real `A` lists a distfile once however many `SRC_URI` entries
    /// name it.
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

    /// `RESTRICT=fetch` bars the literal (real `fetch.py:1167`) while
    /// `FEATURES=force-mirror` bars even a re-permitted one; the
    /// mirror lists are unaffected.
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

    /// A definitely-closed localhost port (bound, then released): `wget`
    /// fails fast with "connection refused", so the "every candidate
    /// failed" error text pins the tried order without any real server.
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    /// The assembled order is the tried order end to end: with every
    /// candidate refusing connections, the failure report lists the
    /// public `GENTOO_MIRRORS` fallback *before* the literal URI --
    /// and with `restrict_primaryuri` the literal first.
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

    /// Real `fetch.py`'s `tried_locations`: a `mirror://` URI's
    /// third-party expansion is listed twice by `assemble_candidates`
    /// (inline, and again in the primary-uri group -- in both
    /// `RESTRICT=primaryuri` modes), but an unreachable mirror is
    /// attempted exactly once.
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
