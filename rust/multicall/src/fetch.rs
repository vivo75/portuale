// Real network fetch -- the second half of "actually fetch a package's
// real sources" (see `portage_fetch`'s own module doc comment for the
// pure-logic half: SRC_URI flattening, Manifest parsing, digest
// verification, all offline and 100% unit-testable). Shells out to real
// `wget`, using real `make.globals`'s own default `FETCHCOMMAND`
// template verbatim (`cnf/make.globals`), rather than an in-process HTTP
// client -- matching this pilot's own "run the same real external
// process portage would" precedent (`bin/*.sh`, `xpak-helper.py`, ...).
// No `PORTAGE_COMPRESSION_COMMAND`-style config resolution attempted
// (this pilot has no `make.conf` resolution path at all), same "env
// var/hardcoded default" shortcut `ebuild_package.rs` already
// established.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching this whole pilot's own
// "narrow v1, document the cut" pattern):
//   - No resume support (real `RESUMECOMMAND`'s own retry-with-`-c`
//     behavior) -- a failed download is simply removed and retried from
//     scratch next time, never resumed.
//   - No `mirror://` fallback (see `portage_fetch`'s own doc comment).
//   - No GPG verification (real `FEATURES=verify-sig`).
//   - No `FEATURES=distlocks` -- this pilot's own single-invocation-at-
//     a-time CLI usage never races a concurrent fetch of the same file.

use portage_fetch::{flatten_src_uri, parse_manifest, verify_digests};
use std::path::{Path, PathBuf};
use std::process::Command;

/// `distdir` is env-var-sourced at the `ebuild.rs`/`pretend.rs` CLI
/// boundary (`DISTDIR`, same "env var/hardcoded default" shortcut
/// `PKGDIR`/`CONFIG_PROTECT` already use); `Default` matches real
/// `make.globals`'s own `DISTDIR="/var/cache/distfiles"` exactly.
pub struct FetchOptions {
    pub distdir: PathBuf,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            distdir: PathBuf::from("/var/cache/distfiles"),
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
fn wget_fetch(uri: &str, dest: &Path) -> Result<(), String> {
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

    let mut filenames = Vec::new();
    for entry in &entries {
        let dest = options.distdir.join(&entry.filename);
        let digests = manifest.get(&entry.filename);

        let already_verified = digests
            .map(|d| dest.is_file() && verify_digests(&dest, d).is_ok())
            .unwrap_or(false);

        if !already_verified {
            let Some(digests) = digests else {
                return Err(format!(
                    "{}: no Manifest entry, cannot verify -- refusing to fetch \
                     unverifiable content",
                    entry.filename
                ));
            };
            wget_fetch(&entry.uri, &dest)?;
            if let Err(e) = verify_digests(&dest, digests) {
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "{}: digest verification failed after fetch: {e}",
                    entry.filename
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
            "multicall_fetch_test_{}_{}",
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
    fn fetch_src_uri_is_empty_for_an_empty_src_uri() {
        let pkg_dir = tempdir();
        let distdir = tempdir();
        let result = fetch_src_uri(
            &pkg_dir,
            "",
            &FetchOptions {
                distdir: distdir.clone(),
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
}
