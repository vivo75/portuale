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

/// Assemble a plain-outer-tar gpkg container from `<prefix>/<name>`
/// members plus a `Manifest` body, in gpkg member order.
fn build_gpkg(prefix: &str, members: &[(&str, &[u8])], manifest: Option<&str>) -> PathBuf {
    let scratch = ScratchDir::new("gpkg-build").unwrap();
    // leak the scratch dir for the caller's test lifetime
    let root = scratch.path().to_path_buf();
    std::mem::forget(scratch);
    let pkgdir = root.join(prefix);
    fs::create_dir_all(&pkgdir).unwrap();
    let mut argv: Vec<String> = vec![
        "-cf".into(),
        lossy(&root.join("out.gpkg.tar")),
        "-C".into(),
        lossy(&root),
    ];
    for (name, bytes) in members {
        fs::write(pkgdir.join(name), bytes).unwrap();
        argv.push(format!("{prefix}/{name}"));
    }
    if let Some(body) = manifest {
        fs::write(pkgdir.join("Manifest"), body).unwrap();
        argv.push(format!("{prefix}/Manifest"));
    }
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    run_tar(&refs).unwrap();
    root.join("out.gpkg.tar")
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

/// `build_gpkg`'s typed sibling: stage one filesystem object per entry
/// and let GNU `tar` write the headers.
fn build_gpkg_entries(entries: &[(&str, GpkgEntry)]) -> PathBuf {
    let scratch = ScratchDir::new("gpkg-build-typed").unwrap();
    // leak the scratch dir for the caller's test lifetime
    let root = scratch.path().to_path_buf();
    std::mem::forget(scratch);
    let mut argv: Vec<String> = vec![
        "-cf".into(),
        lossy(&root.join("out.gpkg.tar")),
        "-C".into(),
        lossy(&root),
    ];
    for (name, entry) in entries {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        match entry {
            GpkgEntry::File(bytes) => fs::write(&path, bytes).unwrap(),
            GpkgEntry::Symlink(target) => std::os::unix::fs::symlink(target, &path).unwrap(),
            GpkgEntry::Fifo => {
                let cstr = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                    .expect("test path has no NUL");
                // SAFETY: `mkfifo` takes a valid NUL-terminated path
                // and a mode, and returns a plain int.
                assert_eq!(unsafe { libc::mkfifo(cstr.as_ptr(), 0o644) }, 0);
            }
            GpkgEntry::Hardlink(target) => fs::hard_link(root.join(target), &path).unwrap(),
            GpkgEntry::CharDevice { major, minor } => {
                let cstr = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                    .expect("test path has no NUL");
                let dev = libc::makedev(*major, *minor);
                // SAFETY: `mknod` takes a valid NUL-terminated path, a
                // mode/type and a device number, and returns a plain
                // int. Root-only, like cell e itself.
                assert_eq!(
                    unsafe { libc::mknod(cstr.as_ptr(), libc::S_IFCHR | 0o666, dev) },
                    0
                );
            }
        }
        argv.push(name.to_string());
    }
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    run_tar(&refs).unwrap();
    root.join("out.gpkg.tar")
}

fn blake2b_hex(bytes: &[u8]) -> String {
    use blake2::Digest as _;
    blake2::Blake2b512::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn sha512_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha512::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn data_line(name: &str, bytes: &[u8]) -> String {
    format!(
        "DATA {name} {} BLAKE2B {} SHA512 {}\n",
        bytes.len(),
        blake2b_hex(bytes),
        sha512_hex(bytes)
    )
}

const GPG_PASSPHRASE: &str = "GentooTest";

/// Best-effort `gpg-agent` shutdown for a test homedir.
/// Verify-only tests never start an agent and don't need this.
fn kill_gpg_agent(home: &Path) {
    let _ = Command::new("gpgconf")
        .args(["--homedir"])
        .arg(home)
        .args(["--kill", "all"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

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

/// Copy a `.gpkg.tar` fixture to a tempdir, unpack its outer tar, let
/// `mutate` alter the unpacked tree, and repack. Packed-byte flips
/// would only corrupt the tar framing.
fn repack_gpkg_with_mutation(src: &Path, tag: &str, mutate: impl Fn(&Path)) -> PathBuf {
    let root = TempDir::new(&format!("portuale-gpg-{tag}")).keep();
    let outer = root.join("outer");
    fs::create_dir_all(&outer).unwrap();
    run_tar(&["-xf", &lossy(src), "-C", &lossy(&outer)]).unwrap();
    mutate(&outer);
    let mut argv: Vec<String> = vec![
        "-cf".into(),
        lossy(&root.join("out.gpkg.tar")),
        "-C".into(),
        lossy(&outer),
    ];
    // List the prefix's files, not the prefix directory itself: GNU
    // tar would also write a `<prefix>/` directory member, which the
    // one-level structure check rejects. The signed fixture is flat.
    for prefix_dir in read_dir_sorted(&outer).unwrap() {
        let prefix = prefix_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for file in read_dir_sorted(&prefix_dir).unwrap() {
            argv.push(format!(
                "{prefix}/{}",
                file.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    run_tar(&refs).unwrap();
    root.join("out.gpkg.tar")
}

/// Build a signed container around a fixed payload whose Manifest
/// is clear-signed with `manifest_key`, and whose payload `.sig`
/// sidecar is present only when `with_sidecar` (signed with
/// `member_key`, defaulting to the manifest key).
fn build_signed_gpkg(
    home: &Path,
    manifest_key: &str,
    with_sidecar: bool,
    member_key: Option<&str>,
) -> PathBuf {
    let payload: &[u8] = b"signed payload bytes";
    let manifest = format!(
        "{}{}",
        data_line("gpkg-1", b""),
        data_line("payload.bin", payload)
    );
    let signed_manifest = gpg_sign(home, manifest_key, manifest.as_bytes(), true);
    let mut members: Vec<(&str, Vec<u8>)> =
        vec![("gpkg-1", b"".to_vec()), ("payload.bin", payload.to_vec())];
    if with_sidecar {
        let sig = gpg_sign(home, member_key.unwrap_or(manifest_key), payload, false);
        members.push(("payload.bin.sig", sig));
    }
    let scratch = ScratchDir::new("gpkg-signed-build").unwrap();
    let root = scratch.path().to_path_buf();
    std::mem::forget(scratch);
    let prefix = "sigtest-1.0";
    let pkgdir = root.join(prefix);
    fs::create_dir_all(&pkgdir).unwrap();
    let mut argv: Vec<String> = vec![
        "-cf".into(),
        lossy(&root.join("out.gpkg.tar")),
        "-C".into(),
        lossy(&root),
    ];
    let member_refs: Vec<(String, Vec<u8>)> = members
        .into_iter()
        .map(|(n, b)| (format!("{prefix}/{n}"), b))
        .collect();
    for (name, bytes) in &member_refs {
        fs::write(root.join(name), bytes).unwrap();
        argv.push(name.clone());
    }
    fs::write(pkgdir.join("Manifest"), &signed_manifest).unwrap();
    argv.push(format!("{prefix}/Manifest"));
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    run_tar(&refs).unwrap();
    root.join("out.gpkg.tar")
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

mod digest;
mod extract;
mod inner;
mod manifest;
mod metadata;
