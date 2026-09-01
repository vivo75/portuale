use super::*;

use portage_util::TempDir;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// `usize -> u32` narrowing that panics instead of silently wrapping;
/// the fixture-making sizes are far below `u32::MAX`.
fn xpak_u32(len: usize) -> u32 {
    u32::try_from(len).expect("xpak length exceeds u32::MAX")
}

/// Build an XPAK segment and append it to prefix bytes, the way a
/// `.tbz2` is `[tarball][XPAK trailer]`.
// Portage: xpak.py, xpak_mem
fn make_xpak_binpkg(prefix: &[u8], entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut index = Vec::new();
    let mut data = Vec::new();
    for (name, value) in entries {
        index.extend_from_slice(&xpak_u32(name.len()).to_be_bytes());
        index.extend_from_slice(name.as_bytes());
        index.extend_from_slice(&xpak_u32(data.len()).to_be_bytes());
        index.extend_from_slice(&xpak_u32(value.len()).to_be_bytes());
        data.extend_from_slice(value);
    }
    let mut segment = Vec::new();
    segment.extend_from_slice(b"XPAKPACK");
    segment.extend_from_slice(&xpak_u32(index.len()).to_be_bytes());
    segment.extend_from_slice(&xpak_u32(data.len()).to_be_bytes());
    segment.extend_from_slice(&index);
    segment.extend_from_slice(&data);
    segment.extend_from_slice(b"XPAKSTOP");

    let mut out = prefix.to_vec();
    out.extend_from_slice(&segment);
    out.extend_from_slice(&xpak_u32(segment.len()).to_be_bytes());
    out.extend_from_slice(b"STOP");
    out
}

/// One member of a crafted test container.
enum GpkgEntry<'a> {
    File(&'a [u8]),
    Symlink(&'a str),
    Fifo,
    /// `fs::hard_link` to an earlier member: GNU `tar` records it as
    /// `LNKTYPE`.
    Hardlink(&'a str),
    CharDevice {
        major: u32,
        minor: u32,
    },
}

fn blake2b_hex(bytes: &[u8]) -> String {
    use blake2::Digest as _;
    blake2::Blake2b512::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const GPG_PASSPHRASE: &str = "GentooTest";

/// Clear-sign `data` for the `Manifest`; detached-sign it for a member.
// Portage: gpkg.py, checksum_helper
fn gpg_sign(home: &Path, key: &str, data: &[u8], clearsign: bool) -> Vec<u8> {
    use std::io::Write;
    let mut cmd = Command::new("/usr/bin/gpg");
    cmd.args(["--homedir"])
        .arg(home)
        .args([
            "--batch",
            "--no-tty",
            "--yes",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            GPG_PASSPHRASE,
            "--digest-algo",
            "SHA512",
            "--local-user",
            key,
        ])
        .args(if clearsign {
            &["--clearsign"][..]
        } else {
            &["--armor", "--detach-sig"][..]
        })
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().expect("gpg signs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(data)
        .expect("gpg stdin");
    let output = child.wait_with_output().expect("gpg runs");
    assert!(
        output.status.success(),
        "gpg sign failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

// The in-process inner metadata.tar reader.

/// One inner-tar member for the unit tests, written through `tar::Builder`.
struct InnerTestEntry<'a> {
    name: &'a str,
    entry_type: tar::EntryType,
    link_name: &'a str,
    data: &'a [u8],
}

fn inner_entry<'a>(name: &'a str, data: &'a [u8]) -> InnerTestEntry<'a> {
    InnerTestEntry {
        name,
        entry_type: tar::EntryType::Regular,
        link_name: "",
        data,
    }
}
mod extract;
mod inner;
mod metadata;
