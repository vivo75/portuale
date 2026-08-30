// Real network fetch -- the second half of "actually fetch a package's
// real sources" (see `portage_fetch`'s own module doc comment for the
// pure-logic half: SRC_URI flattening, Manifest parsing, digest
// verification, all offline and 100% unit-testable). Shells out to real
// `wget`, using real `make.globals`'s own default `FETCHCOMMAND`
// template verbatim (`cnf/make.globals`), rather than an in-process HTTP
// client -- matching this pilot's own "run the same real external
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
// `FEATURES` tokens (`DistfileLock::acquire`'s own callers default to
// locking accordingly). Released by simply closing the lock file's own
// fd (`DistfileLock`'s own `Drop`), the same real effect real
// `unlockfile()`'s own explicit `flock(fd, LOCK_UN)` has -- POSIX
// guarantees all of a process's own `flock` locks on an fd are released
// when that fd is closed. Real `unlinkfile=0` (this pilot's own default
// too, matching real `fetch.py`'s own call): the lockfile itself
// persists on disk after release, just unlocked, ready for reuse.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching this whole pilot's own
// "narrow v1, document the cut" pattern):
//   - No resume support (real `RESUMECOMMAND`'s own retry-with-`-c`
//     behavior) -- a failed download is simply removed and retried from
//     scratch next time, never resumed.
//   - `mirror://` resolution is real now (`portage_fetch::
//     resolve_mirror_candidates`/`gentoo_mirror_fallback`, see that
//     crate's own module doc comment for the exact real mechanics
//     covered -- including real `custommirrors`, an admin-configured
//     `${PORTAGE_CONFIGROOT}/etc/portage/mirrors` file, and real
//     `RESTRICT=mirror` (`FetchOptions::restrict_mirror` -- the public
//     `GENTOO_MIRRORS` flat-layout fallback is skipped) -- and the real
//     ones deliberately not attempted: live per-mirror `layout.conf`
//     negotiation, real candidate-ordering/shuffling, and `RESTRICT=
//     primaryuri` (doesn't port cleanly -- this pilot's candidate
//     ordering already deviates from real). The `mirror+`/`fetch+`
//     SRC_URI prefixes ARE parsed (`portage_fetch::SrcUriEntry::
//     override_mirror`/`override_fetch`): `mirror+` re-permits the
//     public `GENTOO_MIRRORS` fallback even under `RESTRICT=mirror`, and
//     `override_fetch` (from either prefix) re-permits a plain URI under
//     `RESTRICT=fetch` -- which IS modelled now
//     (`FetchOptions::restrict_fetch`): a plain (non-`mirror://`) URI is
//     barred from the candidate list, and the public mirrors too, so a
//     `RESTRICT=fetch` package fetches OK only from an already-verified
//     `DISTDIR` copy (or `custommirrors`/a `mirror://`-named mirror).
//     The one real thing still cut here: running the ebuild's own
//     `pkg_nofetch` phase for a missing file -- `fetch_src_uri` fails
//     with a generic "place it in DISTDIR by hand" pointer instead.
//   - No `FEATURES=verify-sig` GPG check -- this backlog item was
//     mis-scoped when first written: real `verify-sig`/signature
//     verification is a `gpkg` (the newer GPG-signed binary package
//     format, `lib/portage/gpkg.py`) and repo-sync concept (real
//     `lib/portage/sync/modules/webrsync`'s own gemato-based Manifest
//     signing), not a `SRC_URI`/distfile-fetch one at all -- confirmed
//     by grepping `fetch.py` directly and finding zero hits for either
//     term. Neither `gpkg` nor repo syncing are in this pilot's own
//     scope at all yet, so there's nothing to port here.

use portage_fetch::{
    flatten_src_uri, gentoo_mirror_fallback, parse_manifest, parse_thirdpartymirrors,
    resolve_mirror_candidates, verify_digests,
};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Real `lockfile(mypath, wantnewlockfile=1)` (`lib/portage/locks.py`):
/// a real, separate `.{basename}.portage_lockfile` sibling of `dest`,
/// locked via a real, blocking `flock(2)` exclusive lock for the
/// lifetime of this guard -- see this module's own doc comment for the
/// full real grounding. Held open for as long as the returned guard
/// lives; dropping it closes the fd, which releases the `flock` (POSIX
/// guarantees this), the same real effect real `unlockfile()` has.
struct DistfileLock {
    _file: std::fs::File,
}

impl DistfileLock {
    fn acquire(dest: &Path) -> Result<Self, String> {
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        let basename = dest.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lock_path = parent.join(format!(".{basename}.portage_lockfile"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            // Never truncate: only the lock itself matters, real
            // `os.open(lockfile, O_CREAT)` doesn't `O_TRUNC` either, and
            // truncating would race a concurrent holder's own in-flight
            // read of the file (moot in practice since nothing is ever
            // written to it, but explicit is cheap and correct).
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| format!("{}: {e}", lock_path.display()))?;
        // Real default (no `os.O_NONBLOCK`, real `fetchonly`-only
        // override this pilot's own CLI has no equivalent mode for):
        // block until the lock is available.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(format!(
                "{}: {}",
                lock_path.display(),
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { _file: file })
    }
}

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
/// MergeOptions::config_root`'s own doc comment exactly (this pilot's
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
    /// restrict`): when set, the public `GENTOO_MIRRORS` flat-layout
    /// fallback (`gentoo_mirror_fallback`) is NOT tried for this package
    /// -- real `file_restrict_mirror` gates `location_lists.append(
    /// public_mirrors)` at `fetch.py:1126`. A `mirror://` URI's own
    /// `thirdpartymirrors`/`custommirrors` expansion and any explicit
    /// `SRC_URI` URI are still tried (real portage only drops the
    /// *public* flat-layout mirror list). Sourced from the ebuild's own
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
    /// This pilot does NOT run the ebuild's own `pkg_nofetch` phase for
    /// a missing file (a documented cut) -- `fetch_src_uri` fails with a
    /// generic "place it in DISTDIR by hand" pointer instead.
    pub restrict_fetch: bool,
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
pub(crate) fn wget_fetch(uri: &str, dest: &Path) -> Result<(), String> {
    let status = Command::new("wget")
        .args(["-t", "3", "-T", "60", "--passive-ftp"])
        .args([
            "-U",
            "Portage (Gentoo, https://www.gentoo.org) distfile-fetch",
        ])
        .arg("-O")
        .arg(dest)
        .arg(uri)
        .status()
        .map_err(|e| format!("failed to spawn wget: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(dest);
        return Err(format!("wget failed to fetch {uri:?} ({status})"));
    }
    Ok(())
}

/// Real `doebuild()`'s own `SRC_URI`-vs-`DISTDIR` fetch check, run once
/// before a real `unpack` phase (see `ebuild_phases.rs`'s own call
/// site): for every file `src_uri` (this ebuild's own real, md5-cache-
/// sourced `SRC_URI` string) names for the current USE set, fetches it
/// into `options.distdir` unless a real, Manifest-verified copy is
/// already there. This pilot's own USE is always empty (see
/// `ebuild_phases.rs`'s own `phase_setup_script`, which always exports
/// `USE=""`) -- so a `flag?` group never fires and a `!flag?` one
/// always does, matching real `use_reduce(pkgsettings["USE"].split())`
/// against an empty set exactly.
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
    let entries = flatten_src_uri(src_uri, |negated, _flag| negated)
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
    // plus the real `GENTOO_MIRRORS` flat-layout fallback -- see this
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
    for entry in &entries {
        let dest = options.distdir.join(&entry.filename);
        // Real `FEATURES=distlocks`: acquired before even checking
        // whether the file is already fetched (real `fetch.py:1315`,
        // ahead of its own `_check_distfile` call at `:1336`), held for
        // the entire per-file sequence below, released when `_lock`
        // drops at the end of this loop iteration -- see this module's
        // own doc comment. Deliberately acquired *after* the "no
        // Manifest entry" refusal just below rather than strictly
        // mirroring real ordering: this pilot's own single unified
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
            Some(DistfileLock::acquire(&dest)?)
        } else {
            None
        };

        let already_verified = dest.is_file() && verify_digests(&dest, digests).is_ok();

        if !already_verified {
            // Real portage's own dedicated `mirror://` candidates
            // (or, for a plain URI, the URI itself) tried first, the
            // real `GENTOO_MIRRORS` flat-layout fallback tried last --
            // a real, deliberate deviation from real portage's own
            // precise interleaving, not a bug (see `portage_fetch`'s
            // own doc comment). The first candidate that both fetches
            // *and* real-digest-verifies wins; every candidate's own
            // fetch error is collected so the final failure message
            // (if all of them fail) mentions every URL actually tried,
            // not just the last one.
            let mut candidates =
                resolve_mirror_candidates(&entry.uri, &custommirrors, &thirdpartymirrors);
            // Real `fetch.py:1166-1174`: `if (restrict_fetch and not
            // override_fetch) or force_mirror: continue` -- a *plain*
            // (non-`mirror://`) `SRC_URI` URI is NOT a fetchable
            // candidate under `RESTRICT=fetch` (only `mirror://`-named
            // mirrors + `custommirrors` are). A `fetch+`/`mirror+` prefix
            // (`entry.override_fetch`) re-permits it. A `mirror://` URI's
            // own candidates already come only from
            // `resolve_mirror_candidates`'s expansions, never the raw
            // token, so nothing to strip there.
            let plain_uri_barred_by_restrict_fetch = options.restrict_fetch
                && !entry.override_fetch
                && !entry.uri.starts_with("mirror://");
            if plain_uri_barred_by_restrict_fetch {
                candidates.retain(|c| c != &entry.uri);
            }
            // Real `file_restrict_mirror = (restrict_fetch or
            // restrict_mirror) and not override_mirror`
            // (`fetch.py:1117-1119`): the public `GENTOO_MIRRORS`
            // flat-layout list is appended unless mirroring is
            // restricted -- but a `mirror+` SRC_URI prefix on this URI
            // (`entry.override_mirror`) re-permits it for this file even
            // then. `RESTRICT=fetch` implies mirror restriction too
            // (real: `(restrict_fetch or restrict_mirror)`).
            let public_mirrors_barred =
                (options.restrict_mirror || options.restrict_fetch) && !entry.override_mirror;
            if !public_mirrors_barred {
                candidates.extend(gentoo_mirror_fallback(
                    &entry.filename,
                    &options.gentoo_mirrors,
                ));
            }
            if candidates.is_empty() {
                // Real `fetch.py`: a `RESTRICT=fetch` file that isn't
                // already in `DISTDIR` runs the ebuild's own
                // `pkg_nofetch` phase (custom "download it from … and
                // place it in DISTDIR" instructions) and fails. This
                // pilot doesn't run `pkg_nofetch` (a documented cut) --
                // it fails with a generic pointer instead.
                let why = if plain_uri_barred_by_restrict_fetch {
                    format!(
                        "RESTRICT=fetch bars downloading it -- place a verified copy in {} \
                         by hand (the ebuild's own pkg_nofetch phase would print specific \
                         instructions)",
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
            for candidate in &candidates {
                match wget_fetch(candidate, &dest) {
                    Ok(()) => match verify_digests(&dest, digests) {
                        Ok(()) => {
                            fetched = true;
                            break;
                        }
                        Err(e) => {
                            let _ = std::fs::remove_file(&dest);
                            errors.push(format!("{candidate}: digest verification failed: {e}"));
                        }
                    },
                    Err(e) => errors.push(e),
                }
            }
            if !fetched {
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

    #[test]
    fn distfile_lock_creates_a_real_sibling_lockfile() {
        let dir = tempdir();
        let dest = dir.join("foo-1.0.tar.gz");

        let _lock = DistfileLock::acquire(&dest).expect("acquire succeeds");

        assert!(
            dir.join(".foo-1.0.tar.gz.portage_lockfile").is_file(),
            "real lockfile naming: '.' + basename + '.portage_lockfile'"
        );
    }

    #[test]
    fn distfile_lock_release_on_drop_lets_a_second_acquire_succeed_immediately() {
        let dir = tempdir();
        let dest = dir.join("foo-1.0.tar.gz");

        let lock1 = DistfileLock::acquire(&dest).expect("first acquire succeeds");
        drop(lock1);

        // Real `unlinkfile=0`: the lockfile persists on disk, just
        // unlocked -- a second acquire (in the same process, a
        // re-entrant flock on a fresh fd for the same file) must
        // succeed immediately, not block on itself.
        DistfileLock::acquire(&dest).expect("second acquire succeeds once the first is dropped");
    }

    /// Real, end-to-end proof of the actual blocking behavior
    /// `flock(2)` provides: a second acquire on the same distfile, from
    /// a different thread, genuinely blocks until the first lock is
    /// dropped -- not merely that the API happens to return `Ok`.
    #[test]
    fn distfile_lock_blocks_a_second_acquire_until_released() {
        let dir = tempdir();
        let dest = dir.join("foo-1.0.tar.gz");

        let lock1 = DistfileLock::acquire(&dest).expect("first acquire succeeds");

        let dest2 = dest.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _lock2 =
                DistfileLock::acquire(&dest2).expect("second acquire succeeds once unblocked");
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
    /// thirdpartymirrors` -> `resolve_mirror_candidates` -> `wget_fetch`
    /// -> `verify_digests`) works together, not just each piece in
    /// isolation.
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

    /// Real, end-to-end `GENTOO_MIRRORS` flat-layout fallback: the
    /// literal `SRC_URI` itself is deliberately unreachable (port 1,
    /// which real, unprivileged `wget` gets an immediate real
    /// "Connection refused" for -- fast and deterministic, unlike a
    /// black-holed address that would make this test hang for real
    /// `wget -t 3 -T 60`'s own full multi-minute retry budget), so the
    /// fetch only succeeds because `FetchOptions.gentoo_mirrors` names
    /// a real local HTTP server that `gentoo_mirror_fallback` expands
    /// into `<root>/distfiles/<filename>` and that candidate is tried
    /// next.
    #[test]
    fn fetch_src_uri_falls_back_to_gentoo_mirrors_when_the_primary_uri_is_unreachable() {
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let mirror_root = uri_base.trim_end_matches("/file").to_string();

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
    }

    /// Real `RESTRICT=mirror` (`file_restrict_mirror`,
    /// `fetch.py:1117-1127`): the public `GENTOO_MIRRORS` flat-layout
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
        let (uri_base, handle) = serve_once(b"hello world".to_vec());
        let mirror_root = uri_base.trim_end_matches("/file").to_string();

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
    /// flat-layout list -- a `mirror://` URI's own `custommirrors`
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
}
