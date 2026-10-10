// Native `xpak-helper.py recompose` (#326 S5, D1/D3/D4/D7): a byte-level
// port of `bin/xpak-helper.py` (`main` + `command_recompose`) over
// `lib/portage/xpak.py` (`addtolist`, `xpak`, `xpak_mem`, `encodeint`,
// `tbz2.scan`, `tbz2.recompose_mem`), routed from the `python`
// dispatcher when the script argument's basename is `xpak-helper.py`.
// It is the inverse of the reader in `binpkg.rs`
// (`read_xpak_segment`/`parse_xpak_members`).
//
// Behaviour is real's, in real's order:
// - argv: no command -> argparse error (usage + `<prog>: error: missing
//   command argument`, rc 2); a command other than `recompose` -> rc 2
//   `invalid command: '<cmd>'`; `recompose` with argc != 2, arg 1 not a
//   regular file, arg 2 not a directory -> rc 1 with real's messages
//   (stderr is raw argv bytes, like Python's surrogateescape).
// - `xpak(dir)`: `addtolist` walks the tree (a real subdirectory is
//   listed as `sub/`, a symlink to a directory is skipped). Real's
//   UTF-8 checks there only fire on `bytes`, and its `os.walk` yields
//   `str`, so a non-UTF-8 name is NOT skipped: it is a surrogateescape
//   `str`, sorted by code point, and packed by `xpak_mem` with
//   `backslashreplace`. The invalid byte 0xff becomes the six bytes
//   `\udcff` (probed live 2026-10-08 on tmpfs). `CONTENTS` is skipped
//   and every other entry is read as raw bytes. Reading a `sub/` entry
//   dies in real with
//   `IsADirectoryError` (traceback, rc 1, binpkg untouched): reproduced
//   as rc 1, the binpkg untouched and a one-line message (the traceback
//   text is not reproduced).
// - `xpak_mem`: `XPAKPACK be32(indexlen) be32(datalen) index data
//   XPAKSTOP`, index records `be32(len(name)) name be32(datapos)
//   be32(size)`.
// - `recompose_mem`: `scan()` (last 16 bytes `XPAKSTOP be32(infosize)
//   STOP` -> `xpaksize = infosize + 8`, else 0); when `st_nlink > 1`
//   the hardlink is broken by a content-only copy (real
//   `util.file_copy.copyfile`) to a new file in the same directory,
//   renamed over. Real's `apply_stat_permissions(self.file, ...)` targets
//   the original path before the rename, so it changes nothing. The new
//   file ends up with mode `0o666 & ~umask` and the caller's owner, not
//   the old file's (probed live: a 0600 binpkg comes out 0644 under
//   umask 022). Then
//   `xpaksize` bytes are truncated from the end and `segment +
//   be32(len(segment)) + "STOP"` appended.

use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const DESCRIPTION: &str = "Perform metadata operations on a binary package.";

/// Raw bytes of an argv element, for messages that must echo them.
fn raw(arg: &OsString) -> &[u8] {
    arg.as_os_str().as_bytes()
}

fn err_bytes(parts: &[&[u8]]) {
    let mut buf = Vec::new();
    for p in parts {
        buf.extend_from_slice(p);
    }
    let _ = std::io::stderr().write_all(&buf);
}

fn basename_bytes(arg: &OsString) -> &[u8] {
    let b = raw(arg);
    match b.iter().rposition(|c| *c == b'/') {
        Some(i) => &b[i + 1..],
        None => b,
    }
}

/// Entry point: `script` is the `xpak-helper.py` element (real's
/// `sys.argv[0]`), `args` everything after it.
pub(crate) fn run(script: &OsString, args: &[OsString]) -> i32 {
    let prog = basename_bytes(script);
    // real passes `usage="usage: ..."` and argparse prefixes `usage: `
    // again: the doubled prefix is real's (checked against real).
    let usage = [b"usage: usage: ", prog, b" COMMAND [args]\n"].concat();
    // argparse: `-h`/`--help` prints the help and exits 0.
    if args.iter().any(|a| raw(a) == b"-h" || raw(a) == b"--help") {
        let help =
            format!("\n{DESCRIPTION}\n\noptions:\n  -h, --help  show this help message and exit\n");
        let _ = std::io::stdout().write_all(&[usage.as_slice(), help.as_bytes()].concat());
        return 0;
    }
    let parser_error = |msg: &[u8]| -> i32 {
        err_bytes(&[&usage, prog, b": error: ", msg, b"\n"]);
        2
    };
    let Some(command) = args.first() else {
        return parser_error(b"missing command argument");
    };
    if raw(command) != b"recompose" {
        return parser_error(&[b"invalid command: '", raw(command), b"'"].concat());
    }
    command_recompose(&args[1..])
}

fn command_recompose(args: &[OsString]) -> i32 {
    let usage: &[u8] = b"usage: recompose <binpkg_path> <metadata_dir>\n";
    if args.len() != 2 {
        err_bytes(&[
            usage,
            format!("2 arguments are required, got {}\n", args.len()).as_bytes(),
        ]);
        return 1;
    }
    let binpkg = Path::new(&args[0]);
    let metadata = Path::new(&args[1]);
    if !binpkg.is_file() {
        err_bytes(&[
            usage,
            b"Argument 1 is not a regular file: '",
            raw(&args[0]),
            b"'\n",
        ]);
        return 1;
    }
    if !metadata.is_dir() {
        err_bytes(&[
            usage,
            b"Argument 2 is not a directory: '",
            raw(&args[1]),
            b"'\n",
        ]);
        return 1;
    }
    match xpak(metadata).and_then(|segment| recompose_mem(binpkg, &segment)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("xpak-helper.py: {e}");
            1
        }
    }
}

/// Real `encodeint`.
fn encodeint(n: usize) -> [u8; 4] {
    (n as u32).to_be_bytes()
}

/// Real `addtolist`: the relative names under `root`, as raw bytes (`sub/`
/// for a real subdirectory, then its contents).
fn addtolist(root: &Path, rel: &[u8], list: &mut Vec<Vec<u8>>) -> std::io::Result<()> {
    let dir = root.join(std::ffi::OsStr::from_bytes(rel));
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = [rel, entry.file_name().as_bytes()].concat();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let sub = [name.as_slice(), b"/"].concat();
            list.push(sub.clone());
            addtolist(root, &sub, list)?;
        } else if file_type.is_symlink() && entry.path().is_dir() {
            // os.walk: a symlink to a directory is in `dirs`, not
            // recursed into (followlinks=False), never listed.
        } else {
            list.push(name);
        }
    }
    Ok(())
}

/// A name as the code points of Python's surrogateescape `str`: each
/// byte of an invalid UTF-8 sequence becomes U+DC80+byte. Real sorts
/// these `str`s, so this is the sort key.
fn py_code_points(name: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(name.len());
    for chunk in name.utf8_chunks() {
        out.extend(chunk.valid().chars().map(u32::from));
        out.extend(chunk.invalid().iter().map(|b| 0xDC00 + u32::from(*b)));
    }
    out
}

/// Real `xpak_mem`'s `k.encode("utf-8", "backslashreplace")` of a
/// surrogateescape name: valid UTF-8 kept, each invalid byte written as
/// the text `\udcXX`.
fn py_name_bytes(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    for chunk in name.utf8_chunks() {
        out.extend_from_slice(chunk.valid().as_bytes());
        for b in chunk.invalid() {
            out.extend_from_slice(format!("\\udc{b:02x}").as_bytes());
        }
    }
    out
}

/// Real `xpak(rootdir)` returning the segment.
fn xpak(root: &Path) -> Result<Vec<u8>, String> {
    let mut list = Vec::new();
    addtolist(root, b"", &mut list).map_err(|e| format!("{}: {e}", root.display()))?;
    list.sort_by_cached_key(|name| py_code_points(name));
    let mut members: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for name in list {
        if name == b"CONTENTS" {
            continue;
        }
        let path = root.join(std::ffi::OsStr::from_bytes(&name));
        let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        members.push((py_name_bytes(&name), data));
    }
    Ok(xpak_mem(&members))
}

/// Real `xpak_mem`.
fn xpak_mem(members: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut index = Vec::new();
    let mut data = Vec::new();
    for (name, value) in members {
        index.extend_from_slice(&encodeint(name.len()));
        index.extend_from_slice(name);
        index.extend_from_slice(&encodeint(data.len()));
        index.extend_from_slice(&encodeint(value.len()));
        data.extend_from_slice(value);
    }
    let mut out = Vec::with_capacity(index.len() + data.len() + 24);
    out.extend_from_slice(b"XPAKPACK");
    out.extend_from_slice(&encodeint(index.len()));
    out.extend_from_slice(&encodeint(data.len()));
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    out.extend_from_slice(b"XPAKSTOP");
    out
}

/// Real `tbz2.scan()`'s `xpaksize`: `infosize + 8` when the last 16
/// bytes are `XPAKSTOP be32(infosize) STOP`, else 0.
fn scan_xpaksize(file: &Path) -> std::io::Result<u64> {
    let mut f = std::fs::File::open(file)?;
    let len = f.seek(SeekFrom::End(0))?;
    if len < 16 {
        return Ok(0);
    }
    let mut trailer = [0u8; 16];
    f.seek(SeekFrom::End(-16))?;
    f.read_exact(&mut trailer)?;
    if &trailer[12..16] != b"STOP" || &trailer[0..8] != b"XPAKSTOP" {
        return Ok(0);
    }
    let infosize = u32::from_be_bytes([trailer[8], trailer[9], trailer[10], trailer[11]]) as u64;
    Ok(infosize + 8)
}

/// Real `recompose_mem`'s hardlink break: a content-only copy into a new
/// file in the same directory (mode `0o666 & ~umask`, the caller's
/// owner, as real's `copyfile` leaves it), renamed over.
fn break_hardlink(file: &Path) -> std::io::Result<()> {
    let dir = file.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let base = file
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    let mut n = 0u32;
    let (tmp, mut dst): (PathBuf, std::fs::File) = loop {
        let mut name = base.clone().into_vec();
        name.extend_from_slice(format!(".{}{n}", std::process::id()).as_bytes());
        let candidate = dir.join(OsString::from_vec(name));
        // Default creation mode 0o666, filtered by the umask.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => break (candidate, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        // Content only: `std::fs::copy` would also copy the permissions.
        std::io::copy(&mut std::fs::File::open(file)?, &mut dst)?;
        drop(dst);
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Real `tbz2.recompose_mem(xpdata)`.
fn recompose_mem(file: &Path, segment: &[u8]) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}: {e}", file.display());
    let meta = std::fs::metadata(file).map_err(fail)?;
    let xpaksize = scan_xpaksize(file).map_err(fail)?;
    if meta.nlink() > 1 {
        break_hardlink(file).map_err(fail)?;
    }
    let len = std::fs::metadata(file).map_err(fail)?.len();
    if xpaksize > len {
        return Err(format!(
            "{}: Invalid argument (xpak trailer size exceeds the file)",
            file.display()
        ));
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(file)
        .map_err(fail)?;
    let keep = len - xpaksize;
    f.set_len(keep).map_err(fail)?;
    f.seek(SeekFrom::Start(keep)).map_err(fail)?;
    let mut tail = Vec::with_capacity(segment.len() + 8);
    tail.extend_from_slice(segment);
    tail.extend_from_slice(&encodeint(segment.len()));
    tail.extend_from_slice(b"STOP");
    f.write_all(&tail).map_err(fail)?;
    f.flush().map_err(fail)
}

#[cfg(test)]
mod tests {
    use portage_util::TempDir;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    /// The `portuale` binary next to the test binary (child processes).
    fn portuale_exe() -> PathBuf {
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        exe
    }

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/xpak")
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let dest = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_tree(&e.path(), &dest);
            } else {
                std::fs::copy(e.path(), dest).unwrap();
            }
        }
    }

    /// `portuale __helper python xpak-helper.py recompose <args>` with
    /// no Portage checkout reachable.
    fn run_helper(args: &[std::ffi::OsString]) -> std::process::Output {
        std::process::Command::new(portuale_exe())
            .args(["__helper", "python", "xpak-helper.py"])
            .args(args)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .output()
            .expect("portuale __helper spawns")
    }

    /// One oracle case (`pmtest fixtures/helpers/xpak/<case>`, generated
    /// by running the real `bin/xpak-helper.py`): asserts rc, stderr and
    /// the resulting bytes.
    fn check_case(case: &str) {
        let dir = fixtures().join(case);
        let tmp = TempDir::new("xpak-oracle");
        let work = tmp.path().join(case);
        std::fs::create_dir_all(&work).unwrap();
        let tbz2 = work.join("pkg.tbz2");
        let bi = work.join("bi");
        std::fs::copy(dir.join("in.tbz2"), &tbz2).unwrap();
        copy_tree(&dir.join("bi"), &bi);
        let other = work.join("other.tbz2");
        if case == "hardlinked" {
            std::fs::hard_link(&tbz2, &other).unwrap();
        }
        let argv: Vec<std::ffi::OsString> = std::fs::read_to_string(dir.join("argv"))
            .unwrap()
            .lines()
            .map(|l| match l {
                "@TBZ2@" => tbz2.clone().into_os_string(),
                "@BI@" => bi.clone().into_os_string(),
                other => other.into(),
            })
            .collect();
        let mut args = vec![std::ffi::OsString::from("recompose")];
        args.extend(argv);
        let out = run_helper(&args);

        let rc: i32 = std::fs::read_to_string(dir.join("rc.txt"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(out.status.code(), Some(rc), "{case}: rc, {out:?}");
        assert!(out.stdout.is_empty(), "{case}: stdout {out:?}");
        let expected_err = std::fs::read_to_string(dir.join("stderr.txt"))
            .unwrap()
            .replace("@TMP@", tmp.path().to_str().unwrap());
        if case != "nested" {
            // The error cases echo `@TMP@/<case>/...`: the fixture
            // generator used the same `<scratch>/<case>/` layout.
            assert_eq!(
                String::from_utf8_lossy(&out.stderr),
                expected_err,
                "{case}: stderr"
            );
        } else {
            assert!(!out.stderr.is_empty(), "{case}: a message on stderr");
        }
        let got = std::fs::read(&tbz2).unwrap();
        let want = std::fs::read(dir.join("out.tbz2")).unwrap();
        assert!(got == want, "{case}: out.tbz2 differs from the oracle");
        if case == "hardlinked" {
            assert_eq!(
                std::fs::read(&other).unwrap(),
                std::fs::read(dir.join("other-link.tbz2")).unwrap(),
                "{case}: the other link must keep the input bytes"
            );
            assert_eq!(std::fs::metadata(&tbz2).unwrap().nlink(), 1);
            assert_eq!(std::fs::metadata(&other).unwrap().nlink(), 1);
        }
    }

    macro_rules! oracle_case {
        ($name:ident, $case:literal) => {
            #[test]
            fn $name() {
                check_case($case);
            }
        };
    }
    oracle_case!(oracle_fresh, "fresh");
    oracle_case!(oracle_re_recompose, "re-recompose");
    oracle_case!(oracle_small_payload, "small-payload");
    oracle_case!(oracle_raw_bytes, "raw-bytes");
    oracle_case!(oracle_hardlinked, "hardlinked");
    oracle_case!(oracle_nested, "nested");
    oracle_case!(oracle_error_argc, "error-argc");
    oracle_case!(oracle_error_not_file, "error-not-file");
    oracle_case!(oracle_error_not_dir, "error-not-dir");

    #[test]
    fn nested_leaves_the_binpkg_unchanged() {
        let dir = fixtures().join("nested");
        let tmp = TempDir::new("xpak-nested");
        let tbz2 = tmp.path().join("pkg.tbz2");
        let bi = tmp.path().join("bi");
        std::fs::copy(dir.join("in.tbz2"), &tbz2).unwrap();
        copy_tree(&dir.join("bi"), &bi);
        let out = run_helper(&["recompose".into(), tbz2.clone().into(), bi.into()]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert_eq!(
            std::fs::read(&tbz2).unwrap(),
            std::fs::read(dir.join("in.tbz2")).unwrap()
        );
    }

    #[test]
    fn argparse_errors_exit_2() {
        let none = std::process::Command::new(portuale_exe())
            .args(["__helper", "python", "xpak-helper.py"])
            .output()
            .unwrap();
        assert_eq!(none.status.code(), Some(2));
        assert_eq!(
            String::from_utf8_lossy(&none.stderr),
            "usage: usage: xpak-helper.py COMMAND [args]\nxpak-helper.py: error: missing command argument\n"
        );
        let bad = run_helper(&["frob".into()]);
        assert_eq!(bad.status.code(), Some(2));
        let text = String::from_utf8_lossy(&bad.stderr);
        assert!(
            text.contains("xpak-helper.py: error: invalid command: 'frob'"),
            "{text}"
        );
    }

    /// The writer is the inverse of `binpkg.rs`'s reader.
    #[test]
    fn recompose_round_trips_through_the_binpkg_reader() {
        let tmp = TempDir::new("xpak-rt");
        let bi = tmp.path().join("bi");
        std::fs::create_dir_all(&bi).unwrap();
        let files: [(&str, &[u8]); 4] = [
            ("SLOT", b"0\n"),
            ("PF", b"foo-1.0\n"),
            ("environment.bz2", &[0xff, 0x00, 0x42, 0x5a]),
            ("EMPTY", b""),
        ];
        for (n, v) in files {
            std::fs::write(bi.join(n), v).unwrap();
        }
        std::fs::write(bi.join("CONTENTS"), b"obj /x\n").unwrap();
        let tbz2 = tmp.path().join("pkg.tbz2");
        std::fs::write(&tbz2, b"BZh-payload").unwrap();
        let out = run_helper(&["recompose".into(), tbz2.clone().into(), bi.clone().into()]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        let seg = crate::binpkg::read_xpak_segment(&tbz2).unwrap();
        let got: std::collections::BTreeMap<String, Vec<u8>> =
            crate::binpkg::parse_xpak_members(&seg)
                .unwrap()
                .into_iter()
                .map(|(k, v)| (k, v.to_vec()))
                .collect();
        let want: std::collections::BTreeMap<String, Vec<u8>> = files
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect();
        assert_eq!(got, want);
        assert!(std::fs::read(&tbz2).unwrap().starts_with(b"BZh-payload"));
    }

    /// A non-UTF-8 name is packed, not skipped: real's `str` from
    /// `os.walk` is surrogateescape, sorted by code point (U+DCFF before
    /// U+E000, the reverse of byte order) and encoded with
    /// backslashreplace. Expected bytes are real `xpak-helper.py`'s on
    /// this input (probed live 2026-10-08, tmpfs; /var/tmp is utf8only
    /// zfs here and refuses the name).
    #[test]
    fn non_utf8_names_are_packed_like_portage_surrogateescape() {
        let tmp = if Path::new("/dev/shm").is_dir() {
            TempDir::new_in(Path::new("/dev/shm"), "xpak-nonutf8")
        } else {
            TempDir::new("xpak-nonutf8")
        };
        let bi = tmp.path().join("bi");
        std::fs::create_dir_all(&bi).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let bad = bi.join(std::ffi::OsStr::from_bytes(b"A\xff"));
        if std::fs::write(&bad, b"1").is_err() {
            eprintln!("skipped: the filesystem refuses non-UTF-8 names");
            return;
        }
        std::fs::write(bi.join("A\u{e000}"), b"2").unwrap();
        let tbz2 = tmp.path().join("pkg.tbz2");
        std::fs::write(&tbz2, b"BZhxxxxxxxxxxxxxxxxxxxxxx").unwrap();
        let out = run_helper(&["recompose".into(), tbz2.clone().into(), bi.into()]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        let got = std::fs::read(&tbz2).unwrap();
        let want: &[u8] = b"BZhxxxxxxxxxxxxxxxxxxxxxxXPAKPACK\x00\x00\x00#\x00\x00\x00\x02\
            \x00\x00\x00\x07A\\udcff\x00\x00\x00\x00\x00\x00\x00\x01\
            \x00\x00\x00\x04A\xee\x80\x80\x00\x00\x00\x01\x00\x00\x00\x01\
            12XPAKSTOP\x00\x00\x00=STOP";
        assert_eq!(got, want);
    }

    /// Breaking the hardlink leaves the file with the caller's creation
    /// mode, not the old one: real `copyfile` copies content only, and
    /// its `apply_stat_permissions` hits the original path before the
    /// rename (probed live: 0600 in, 0644 out under umask 022).
    #[test]
    fn broken_hardlink_gets_the_creation_mode_like_portage() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new("xpak-mode");
        let bi = tmp.path().join("bi");
        std::fs::create_dir_all(&bi).unwrap();
        std::fs::write(bi.join("PF"), b"m-1\n").unwrap();
        let tbz2 = tmp.path().join("pkg.tbz2");
        std::fs::write(&tbz2, b"BZhxxxxxxxxxxxxxxxxxxxxxx").unwrap();
        std::fs::set_permissions(&tbz2, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&tbz2, tmp.path().join("other.tbz2")).unwrap();
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("umask 022; exec \"$0\" __helper python xpak-helper.py recompose \"$1\" \"$2\"")
            .arg(portuale_exe())
            .arg(&tbz2)
            .arg(&bi)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        let mode = std::fs::metadata(&tbz2).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o644);
        let other = std::fs::metadata(tmp.path().join("other.tbz2")).unwrap();
        assert_eq!(other.permissions().mode() & 0o7777, 0o600);
    }

    /// The two real `package` scenarios re-run in a child process with
    /// no Portage checkout and no real Python reachable, so the native
    /// `xpak-helper.py` is the only thing that can pack the metadata.
    fn rerun_without_checkout(test: &str) {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .output()
            .unwrap();
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{test}: {text}");
        assert!(text.contains("1 passed"), "{test} did not run: {text}");
    }

    #[test]
    fn real_package_xpak_works_without_the_checkout() {
        rerun_without_checkout(
            "ebuild_package::tests::real_package_builds_a_real_xpak_tbz2_and_a_real_packages_entry",
        );
    }

    #[test]
    fn real_multi_instance_xpak_works_without_the_checkout() {
        rerun_without_checkout(
            "ebuild_package::tests::real_multi_instance_xpak_uses_the_dot_xpak_extension_and_scans_back",
        );
    }
}
