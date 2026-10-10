// Native `gpkg-helper.py compress` (#326 S4, D1/D3/D4/D7): a port of
// `bin/gpkg-helper.py` (`main` + `command_compose`) over
// `lib/portage/gpkg.py` (`gpkg.compress`, `_add_metadata`,
// `_add_manifest`, `_add_signature`, `_record_checksum`,
// `_generate_metadata_from_dir`, `_check_pre_image_files`,
// `_get_tar_format_from_stats`, `_get_binary_cmd`, `_create_tarinfo`,
// `tar_stream_writer`, `checksum_helper`), routed from the `python`
// dispatcher when the script argument's basename is `gpkg-helper.py`.
// It writes what the reader in `binpkg.rs` (`read_outer_members`,
// `verify_gpkg_manifest`) reads.
//
// Behaviour is real's, in real's order:
// - argv: no command -> argparse error (rc 2); a command other than
//   `compress` -> rc 2 `invalid command: '<cmd>'`; `compress` with argc !=
//   4, arg 3 or 4 not a directory -> rc 1 with real's messages (the usage
//   line there says `compose`, as real's does). The category of the first
//   argument is dropped (`basename.split("/", 1)[-1]`).
// - Order of work: the metadata dict (`os.walk` order of the metadata
//   dir, first-insertion position and last value per file name, names
//   that are not UTF-8 skipped), then the image pre-scan (a non-UTF-8
//   name there is an error, rc 1, and no artifact is created), then the
//   container file, `gpkg-1`, the compressor command (resolved from the
//   settings *after* the container exists, like real), the metadata
//   member (+ `.sig`), the image member (+ `.sig`), the `Manifest`, and
//   the two zero blocks plus padding to a 10240-byte record.
// - Tar headers are rendered by hand as `tarfile.TarInfo.tobuf` does
//   (the `tar` crate cannot reproduce them: USTAR directory names keep a
//   trailing `/`, the name/prefix split is `_posix_split_name`'s, GNU
//   `L`/`K` extension blocks are real's, uname/gname are looked up from
//   the uid/gid and are empty without an entry, numbers overflow into
//   base-256 only in GNU format). The container format (`gpkg-1`, the
//   `.sig` members, `Manifest`) is USTAR, GNU when the basename has 154
//   characters or more or the image is 8000000000 bytes or more; the
//   `metadata.tar` member header is always USTAR and the `image.tar`
//   member header carries the image format (oracle: `long-basename` has
//   USTAR stream members in a GNU container, `longname` the reverse).
//   The image tar is GNU when a file name or link target
//   is 100 bytes or more, a directory path is 155 bytes or more, or one
//   file is 8000000000 bytes or more (`_get_tar_format_from_stats`).
// - The image tar is `TarFile.add(root, "image")`: lstat, a sorted
//   walk, a regular file seen again with `st_nlink > 1` becomes a type 1
//   hardlink to the first arcname, sockets are skipped, `st_mode` keeps
//   its setuid/setgid/sticky bits.
// - Compressors and the signing command are spawned from the settings
//   in the process environment (`BINPKG_COMPRESS`,
//   `BINPKG_COMPRESS_FLAGS[_<NAME>]`, `PORTAGE_BZIP2_COMMAND`,
//   `MAKEOPTS`, `FEATURES`, `BINPKG_GPG_*`) over `make.globals`'
//   defaults, with `varexpand` + `shlex.split` and `{JOBS}` from
//   `makeopts_to_job_count`, like `_get_binary_cmd` and
//   `checksum_helper`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};

use blake2::Blake2b512;
use sha2::{Digest, Sha512};

const DESCRIPTION: &str = "Perform metadata operations on a binary package.";

/// `portage.const.HASHING_BLOCKSIZE`.
const HASHING_BLOCKSIZE: usize = 32768;
const BLOCKSIZE: u64 = 512;
const RECORDSIZE: u64 = 20 * 512;
const LENGTH_NAME: usize = 100;
const LENGTH_LINK: usize = 100;
const LENGTH_PREFIX: usize = 155;

/// Why `compress` stopped. `Compressor` is the one exception the real
/// helper catches (`CompressorOperationFailed` -> `eerror` + rc 1);
/// everything else is an uncaught Python exception (rc 1, traceback).
#[derive(Debug)]
enum Failure {
    Compressor,
    Msg(String),
}

impl From<String> for Failure {
    fn from(msg: String) -> Self {
        Failure::Msg(msg)
    }
}

fn io_msg(what: &str, e: std::io::Error) -> Failure {
    Failure::Msg(format!("{what}: {e}"))
}

// ---------------------------------------------------------------------
// Entry point and argv
// ---------------------------------------------------------------------

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

/// Entry point: `script` is the `gpkg-helper.py` element (real's
/// `sys.argv[0]`), `args` everything after it.
pub(crate) fn run(script: &OsString, args: &[OsString]) -> i32 {
    let prog = basename_bytes(script);
    // real passes `usage="usage: ..."` and argparse prefixes `usage: `
    // again: the doubled prefix is real's.
    let usage = [b"usage: usage: ", prog, b" COMMAND [args]\n"].concat();
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
    if raw(command) != b"compress" {
        return parser_error(&[b"invalid command: '", raw(command), b"'"].concat());
    }
    command_compose(&args[1..])
}

fn command_compose(args: &[OsString]) -> i32 {
    // The usage line names `compose` (real's own text).
    let usage: &[u8] = b"usage: compose <package_cpv> <binpkg_path> <metadata_dir> <image_dir>\n";
    if args.len() != 4 {
        err_bytes(&[
            usage,
            format!("4 arguments are required, got {}\n", args.len()).as_bytes(),
        ]);
        return 1;
    }
    let metadata_dir = Path::new(&args[2]);
    let image_dir = Path::new(&args[3]);
    if !metadata_dir.is_dir() {
        err_bytes(&[
            usage,
            b"Argument 3 is not a directory: '",
            raw(&args[2]),
            b"'\n",
        ]);
        return 1;
    }
    if !image_dir.is_dir() {
        err_bytes(&[
            usage,
            b"Argument 4 is not a directory: '",
            raw(&args[3]),
            b"'\n",
        ]);
        return 1;
    }
    let settings = Settings::from_env();
    match compress(
        &settings,
        raw(&args[0]),
        Path::new(&args[1]),
        metadata_dir,
        image_dir,
    ) {
        Ok(()) => 0,
        Err(Failure::Compressor) => {
            err_bytes(&[b" * Compressor Operation Failed\n"]);
            1
        }
        Err(Failure::Msg(msg)) => {
            err_bytes(&[msg.as_bytes(), b"\n"]);
            1
        }
    }
}

// ---------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------

/// The slice of `portage.settings` real reads: the process environment
/// over the `make.globals` defaults of the keys below. (The Python helper
/// also folds `make.conf`/profile into `settings`; here the caller,
/// `invoke_dyn_package`, exports the resolved values into the
/// environment, backlog #180.)
struct Settings {
    vars: HashMap<String, String>,
}

impl Settings {
    fn from_env() -> Self {
        let mut vars: HashMap<String, String> = HashMap::new();
        for (k, v) in std::env::vars_os() {
            vars.insert(
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            );
        }
        // cnf/make.globals:39,47,50,105
        for (k, v) in [
            ("BINPKG_COMPRESS", "zstd"),
            (
                "BINPKG_GPG_SIGNING_BASE_COMMAND",
                "/usr/bin/flock /run/lock/portage-binpkg-gpg.lock /usr/bin/gpg --sign --armor [PORTAGE_CONFIG]",
            ),
            ("BINPKG_GPG_SIGNING_DIGEST", "SHA512"),
            ("PORTAGE_BZIP2_COMMAND", "bzip2"),
        ] {
            vars.entry(k.to_string()).or_insert_with(|| v.to_string());
        }
        Settings { vars }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    /// `settings.features`: the incremental `FEATURES` stack.
    fn has_feature(&self, token: &str) -> bool {
        let mut features: Vec<&str> = Vec::new();
        for t in self.get("FEATURES").unwrap_or("").split_whitespace() {
            if t == "-*" {
                features.clear();
            } else if let Some(neg) = t.strip_prefix('-') {
                features.retain(|f| *f != neg);
            } else {
                features.push(t);
            }
        }
        features.contains(&token)
    }
}

/// `_get_binary_cmd(compression, "compress")`.
fn compression_cmd(settings: &Settings, compression: &str) -> Result<Vec<String>, Failure> {
    let Some(template) = crate::ebuild_package::compress_template(compression) else {
        return Err(Failure::Msg(format!(
            "portage.exception.InvalidCompressionMethod: {compression}"
        )));
    };
    let mut template = template.to_string();
    let upper = compression.to_uppercase();
    if settings
        .get(&format!("BINPKG_COMPRESS_FLAGS_{upper}"))
        .is_some()
    {
        template = template.replace(
            "${BINPKG_COMPRESS_FLAGS}",
            &format!("${{BINPKG_COMPRESS_FLAGS_{upper}}}"),
        );
    }
    let jobs =
        crate::ebuild_package::makeopts_to_job_count(settings.get("MAKEOPTS").unwrap_or("1"));
    let template = template.replace("{JOBS}", &jobs);
    let cmd: Vec<String> = portage_fetch::expand_and_split(&template, &settings.vars)
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect();
    if cmd.is_empty() {
        return Err(Failure::Msg(format!(
            "portage.exception.CompressorNotFound: {compression}"
        )));
    }
    if !crate::ebuild_package::find_binary(&cmd[0]) {
        return Err(Failure::Msg(format!(
            "portage.exception.CompressorNotFound: {}",
            cmd[0]
        )));
    }
    Ok(cmd)
}

/// The signing command of `checksum_helper(SIGNING)`.
fn signing_command(settings: &Settings, detached: bool) -> Result<Vec<String>, Failure> {
    let base = settings
        .get("BINPKG_GPG_SIGNING_BASE_COMMAND")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Failure::Msg(
                "portage.exception.CommandNotFound: GnuPG signing command is not set".into(),
            )
        })?;
    // f-strings of unset settings print `None`.
    let none = |k: &str| settings.get(k).unwrap_or("None").to_string();
    let command = base.replace(
        "[PORTAGE_CONFIG]",
        &format!(
            "--homedir {} --digest-algo {} --local-user {} {} --batch --no-tty",
            none("BINPKG_GPG_SIGNING_GPG_HOME"),
            none("BINPKG_GPG_SIGNING_DIGEST"),
            none("BINPKG_GPG_SIGNING_KEY"),
            if detached {
                "--detach-sig"
            } else {
                "--clear-sign"
            },
        ),
    );
    let cmd: Vec<String> = portage_fetch::expand_and_split(&command, &settings.vars)
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect();
    if cmd.is_empty() {
        return Err(Failure::Msg(
            "portage.exception.CommandNotFound: GnuPG signing command is not set".into(),
        ));
    }
    Ok(cmd)
}

// ---------------------------------------------------------------------
// tarfile.TarInfo.tobuf
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fmt {
    Ustar,
    Gnu,
}

const POSIX_MAGIC: &[u8; 8] = b"ustar\x0000";
const GNU_MAGIC: &[u8; 8] = b"ustar  \x00";

#[derive(Clone)]
struct Info {
    name: Vec<u8>,
    mode: u32,
    uid: i128,
    gid: i128,
    size: u64,
    mtime: i64,
    typeflag: u8,
    linkname: Vec<u8>,
    uname: Vec<u8>,
    gname: Vec<u8>,
    devmajor: u64,
    devminor: u64,
}

impl Info {
    /// A fresh `tarfile.TarInfo(name)`: mode 0644, owner 0:0, no names.
    fn new(name: Vec<u8>, mtime: i64) -> Self {
        Info {
            name,
            mode: 0o644,
            uid: 0,
            gid: 0,
            size: 0,
            mtime,
            typeflag: b'0',
            linkname: Vec::new(),
            uname: Vec::new(),
            gname: Vec::new(),
            devmajor: 0,
            devminor: 0,
        }
    }
}

/// `tarfile.itn`.
fn itn(n: i128, digits: usize, fmt: Fmt) -> Result<Vec<u8>, String> {
    let limit8 = 8i128.pow(digits as u32 - 1);
    if (0..limit8).contains(&n) {
        let mut s = format!("{:0width$o}", n, width = digits - 1).into_bytes();
        s.push(0);
        return Ok(s);
    }
    let limit256 = 256i128.pow(digits as u32 - 1);
    if fmt == Fmt::Gnu && (-limit256..limit256).contains(&n) {
        let (marker, mut v) = if n >= 0 {
            (0o200u8, n)
        } else {
            (0o377u8, 256i128.pow(digits as u32) + n)
        };
        let mut tail = vec![0u8; digits - 1];
        for i in (0..digits - 1).rev() {
            tail[i] = (v & 0xff) as u8;
            v >>= 8;
        }
        let mut s = vec![marker];
        s.extend(tail);
        return Ok(s);
    }
    Err("overflow in number field".to_string())
}

/// `tarfile.stn`: truncate to `length`, pad with NULs.
fn stn(s: &[u8], length: usize) -> Vec<u8> {
    let mut out = s[..s.len().min(length)].to_vec();
    out.resize(length, 0);
    out
}

/// `TarInfo._create_header`.
#[allow(clippy::too_many_arguments)]
fn create_header(
    info: &Info,
    mode: u32,
    magic: &[u8; 8],
    prefix: &[u8],
    fmt: Fmt,
) -> Result<Vec<u8>, String> {
    let (devmajor, devminor) = if matches!(info.typeflag, b'3' | b'4') {
        (
            itn(i128::from(info.devmajor), 8, fmt)?,
            itn(i128::from(info.devminor), 8, fmt)?,
        )
    } else {
        (vec![0u8; 8], vec![0u8; 8])
    };
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    buf.extend(stn(&info.name, 100));
    buf.extend(itn(i128::from(mode & 0o7777), 8, fmt)?);
    buf.extend(itn(info.uid, 8, fmt)?);
    buf.extend(itn(info.gid, 8, fmt)?);
    buf.extend(itn(i128::from(info.size), 12, fmt)?);
    buf.extend(itn(i128::from(info.mtime), 12, fmt)?);
    buf.extend_from_slice(b"        ");
    buf.push(info.typeflag);
    buf.extend(stn(&info.linkname, 100));
    buf.extend_from_slice(magic);
    buf.extend(stn(&info.uname, 32));
    buf.extend(stn(&info.gname, 32));
    buf.extend(devmajor);
    buf.extend(devminor);
    buf.extend(stn(prefix, 155));
    buf.resize(512, 0);
    let sum: u32 = buf.iter().map(|b| u32::from(*b)).sum();
    let chk = format!("{sum:06o}\0");
    buf[148..155].copy_from_slice(chk.as_bytes());
    Ok(buf)
}

/// `TarInfo._create_gnu_long_header`.
fn gnu_long_header(name: &[u8], typeflag: u8) -> Result<Vec<u8>, String> {
    let mut payload = name.to_vec();
    payload.push(0);
    let mut info = Info::new(b"././@LongLink".to_vec(), 0);
    info.mode = 0;
    info.typeflag = typeflag;
    info.size = payload.len() as u64;
    let mut out = create_header(&info, 0, GNU_MAGIC, b"", Fmt::Ustar)?;
    out.extend(payload.iter());
    let rem = payload.len() % 512;
    if rem != 0 {
        out.resize(out.len() + 512 - rem, 0);
    }
    Ok(out)
}

/// `TarInfo._posix_split_name`.
fn posix_split_name(name: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let components: Vec<&[u8]> = name.split(|b| *b == b'/').collect();
    for i in 1..components.len() {
        let prefix = components[..i].join(&b'/');
        let rest = components[i..].join(&b'/');
        if prefix.len() <= LENGTH_PREFIX && rest.len() <= LENGTH_NAME {
            return Ok((prefix, rest));
        }
    }
    Err("name is too long".to_string())
}

/// `TarInfo.tobuf(format)`: the header block(s) of one member.
fn tobuf(info: &Info, fmt: Fmt) -> Result<Vec<u8>, String> {
    let mut info = info.clone();
    // get_info(): a directory name always ends with `/`.
    if info.typeflag == b'5' && !info.name.ends_with(b"/") {
        info.name.push(b'/');
    }
    let mode = info.mode & 0o7777;
    match fmt {
        Fmt::Ustar => {
            if info.linkname.len() > LENGTH_LINK {
                return Err("linkname is too long".to_string());
            }
            let mut prefix = Vec::new();
            if info.name.len() > LENGTH_NAME {
                let (p, n) = posix_split_name(&info.name)?;
                prefix = p;
                info.name = n;
            }
            create_header(&info, mode, POSIX_MAGIC, &prefix, Fmt::Ustar)
        }
        Fmt::Gnu => {
            let mut buf = Vec::new();
            if info.linkname.len() > LENGTH_LINK {
                buf.extend(gnu_long_header(&info.linkname, b'K')?);
            }
            if info.name.len() > LENGTH_NAME {
                buf.extend(gnu_long_header(&info.name, b'L')?);
            }
            buf.extend(create_header(&info, mode, GNU_MAGIC, b"", Fmt::Gnu)?);
            Ok(buf)
        }
    }
}

// ---------------------------------------------------------------------
// Checksums and the Manifest
// ---------------------------------------------------------------------

/// `checksum_helper.libs` for `MANIFEST2_HASH_DEFAULTS`.
struct Hashes {
    blake2b: Blake2b512,
    sha512: Sha512,
}

impl Hashes {
    fn new() -> Self {
        Hashes {
            blake2b: Blake2b512::new(),
            sha512: Sha512::new(),
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.blake2b.update(data);
        self.sha512.update(data);
    }

    /// `[("BLAKE2B", hex), ("SHA512", hex)]`.
    fn finish(self) -> [(&'static str, String); 2] {
        let hex = |d: &[u8]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        [
            ("BLAKE2B", hex(&self.blake2b.finalize())),
            ("SHA512", hex(&self.sha512.finalize())),
        ]
    }
}

/// `gpkg.checksums` and `_record_checksum`.
#[derive(Default)]
struct Checksums {
    records: Vec<Vec<String>>,
}

impl Checksums {
    fn record(&mut self, member_name: &[u8], size: u64, digests: [(&'static str, String); 2]) {
        let file_name = member_name
            .rsplit(|b| *b == b'/')
            .next()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        self.records.retain(|c| c[1] != file_name);
        let mut rec = vec!["DATA".to_string(), file_name, size.to_string()];
        for (algo, hex) in digests {
            rec.push(algo.to_string());
            rec.push(hex);
        }
        self.records.push(rec);
    }

    fn manifest(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for r in &self.records {
            out.extend(r.join(" ").into_bytes());
            out.push(b'\n');
        }
        out
    }
}

fn digest_of(data: &[u8]) -> [(&'static str, String); 2] {
    let mut h = Hashes::new();
    h.update(data);
    h.finish()
}

// ---------------------------------------------------------------------
// Signing (checksum_helper with gpg_operation=SIGNING)
// ---------------------------------------------------------------------

struct Gpg {
    child: std::process::Child,
    stdin: Option<ChildStdin>,
}

fn spawn_gpg(settings: &Settings, detached: bool) -> Result<Gpg, Failure> {
    let cmd = signing_command(settings, detached)?;
    let mut command = Command::new(&cmd[0]);
    command
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // `env["GPG_TTY"] = os.ttyname(sys.stdout.fileno())`, when stdout is
    // a terminal.
    let tty = unsafe { libc::ttyname(1) };
    if !tty.is_null() {
        let name = unsafe { std::ffi::CStr::from_ptr(tty) };
        command.env("GPG_TTY", std::ffi::OsStr::from_bytes(name.to_bytes()));
    }
    let mut child = command
        .spawn()
        .map_err(|e| io_msg(&format!("cannot run {}", cmd[0]), e))?;
    let stdin = child.stdin.take();
    Ok(Gpg { child, stdin })
}

impl Gpg {
    /// `checksum_helper.finish()`: EOF, wait, collect the output; a
    /// failed run is `GPGException("GnuPG signing failed")`.
    fn finish(mut self) -> Result<Vec<u8>, Failure> {
        drop(self.stdin.take());
        let out = self
            .child
            .wait_with_output()
            .map_err(|e| io_msg("gpg", e))?;
        if !out.status.success() {
            let mut msg = String::from("Binary package is not usable (signing failed):\n");
            for line in String::from_utf8_lossy(&out.stderr).lines() {
                msg.push_str(&format!("\t{line}\n"));
            }
            msg.push_str("portage.exception.GPGException: GnuPG signing failed");
            return Err(Failure::Msg(msg));
        }
        Ok(out.stdout)
    }
}

// ---------------------------------------------------------------------
// The container and the stream writer
// ---------------------------------------------------------------------

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

struct Container {
    file: File,
    fmt: Fmt,
}

impl Container {
    /// `container.addfile(tarinfo, fileobj)` at the end of the file.
    fn add_file(&mut self, mut info: Info, data: &[u8]) -> Result<(), Failure> {
        info.size = data.len() as u64;
        let header = tobuf(&info, self.fmt)?;
        self.file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&header))
            .and_then(|_| self.file.write_all(data))
            .map_err(|e| io_msg("container", e))?;
        let rem = data.len() % 512;
        if rem != 0 {
            self.file
                .write_all(&vec![0u8; 512 - rem])
                .map_err(|e| io_msg("container", e))?;
        }
        Ok(())
    }

    /// `container.close()`: two zero blocks, then pad to a record.
    fn close(mut self) -> Result<(), Failure> {
        let end = self
            .file
            .seek(SeekFrom::End(0))
            .map_err(|e| io_msg("container", e))?;
        let mut tail = vec![0u8; (BLOCKSIZE * 2) as usize];
        let total = end + BLOCKSIZE * 2;
        let rem = total % RECORDSIZE;
        if rem != 0 {
            tail.resize(tail.len() + (RECORDSIZE - rem) as usize, 0);
        }
        self.file
            .write_all(&tail)
            .and_then(|_| self.file.flush())
            .map_err(|e| io_msg("container", e))
    }
}

/// Where the compressed (or raw) stream of one member goes: the
/// container file, the digests, and the signer's stdin
/// (`tar_stream_writer` + `checksum_helper.update`).
struct Sink {
    file: File,
    hashes: Hashes,
    gpg: Option<ChildStdin>,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write_all(buf)?;
        self.hashes.update(buf);
        if let Some(g) = self.gpg.as_mut() {
            g.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Streamed {
    size: u64,
    digests: [(&'static str, String); 2],
    signature: Option<Vec<u8>>,
}

/// `tar_stream_writer` as used by `_add_metadata` (header format USTAR)
/// and `compress` (header format `image_tar_format`: the *container*
/// format only governs `gpkg-1`, the `.sig` members and `Manifest`,
/// which go through `container.addfile`): write the member header, stream `produce`'s tar through the compressor into
/// the container (hashing and signing as it goes), then pad and rewrite
/// the header with the final size.
#[allow(clippy::too_many_arguments)]
fn stream_member(
    container: &mut Container,
    settings: &Settings,
    name: &[u8],
    mtime: i64,
    header_fmt: Fmt,
    cmd: Option<&[String]>,
    sign: bool,
    produce: impl FnOnce(&mut dyn Write) -> Result<(), Failure>,
) -> Result<Streamed, Failure> {
    let mut info = Info::new(name.to_vec(), mtime);
    let begin = container
        .file
        .seek(SeekFrom::End(0))
        .map_err(|e| io_msg("container", e))?;
    // The checksum helper (and its gpg) is made before the writer.
    let mut gpg = if sign {
        Some(spawn_gpg(settings, true)?)
    } else {
        None
    };
    let header = tobuf(&info, header_fmt)?;
    container
        .file
        .write_all(&header)
        .map_err(|e| io_msg("container", e))?;
    let header_size = header.len() as u64;

    let sink = Sink {
        file: container
            .file
            .try_clone()
            .map_err(|e| io_msg("container", e))?,
        hashes: Hashes::new(),
        gpg: gpg.as_mut().and_then(|g| g.stdin.take()),
    };

    let sink = match cmd {
        None => {
            let mut sink = sink;
            produce(&mut sink)?;
            sink
        }
        Some(cmd) => {
            let mut proc = Command::new(&cmd[0])
                .args(&cmd[1..])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| io_msg(&format!("cannot run {}", cmd[0]), e))?;
            let mut stdout = proc.stdout.take().expect("piped");
            let reader = std::thread::spawn(move || {
                let mut sink = sink;
                let mut buf = vec![0u8; HASHING_BLOCKSIZE];
                let result = loop {
                    match stdout.read(&mut buf) {
                        Ok(0) => break Ok(()),
                        Ok(n) => {
                            if let Err(e) = sink.write_all(&buf[..n]) {
                                // keep draining so the compressor ends
                                let _ = std::io::copy(&mut stdout, &mut std::io::sink());
                                break Err(e);
                            }
                        }
                        Err(e) => break Err(e),
                    }
                };
                (sink, result)
            });
            let mut stdin = proc.stdin.take().expect("piped");
            let produced = produce(&mut stdin);
            drop(stdin);
            let (sink, read_result) = reader.join().expect("reader thread");
            let status = proc.wait().map_err(|e| io_msg(&cmd[0], e))?;
            // A dead compressor shows as a broken pipe in `produce`:
            // real's `CompressorOperationFailed("PIPE broken")`.
            if !status.success() {
                return Err(Failure::Compressor);
            }
            produced?;
            read_result.map_err(|e| io_msg("container", e))?;
            sink
        }
    };

    let Sink {
        hashes,
        gpg: gpg_in,
        ..
    } = sink;
    drop(gpg_in);
    let signature = match gpg.take() {
        Some(g) => Some(g.finish()?),
        None => None,
    };

    // Size of the stream, padding, final header.
    let end = container
        .file
        .seek(SeekFrom::End(0))
        .map_err(|e| io_msg("container", e))?;
    let size = end - begin - header_size;
    let rem = size % BLOCKSIZE;
    if rem != 0 {
        container
            .file
            .write_all(&vec![0u8; (BLOCKSIZE - rem) as usize])
            .map_err(|e| io_msg("container", e))?;
    }
    info.size = size;
    let header = tobuf(&info, header_fmt)?;
    container
        .file
        .seek(SeekFrom::Start(begin))
        .and_then(|_| container.file.write_all(&header))
        .and_then(|_| container.file.seek(SeekFrom::End(0)))
        .map_err(|e| io_msg("container", e))?;
    Ok(Streamed {
        size,
        digests: hashes.finish(),
        signature,
    })
}

/// `_add_signature`: the `.sig` member plus its Manifest record.
fn add_signature(
    container: &mut Container,
    checksums: &mut Checksums,
    member_name: &[u8],
    signature: &[u8],
) -> Result<(), Failure> {
    let name = [member_name, b".sig"].concat();
    container.add_file(Info::new(name.clone(), now()), signature)?;
    checksums.record(&name, signature.len() as u64, digest_of(signature));
    Ok(())
}

// ---------------------------------------------------------------------
// The image tar: TarFile.add
// ---------------------------------------------------------------------

fn name_of(kind: NameKind, id: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 1024];
    loop {
        let (rc, found) = unsafe {
            match kind {
                NameKind::User => {
                    let mut pwd: libc::passwd = std::mem::zeroed();
                    let mut res: *mut libc::passwd = std::ptr::null_mut();
                    let rc = libc::getpwuid_r(
                        id,
                        &mut pwd,
                        buf.as_mut_ptr().cast(),
                        buf.len(),
                        &mut res,
                    );
                    let name = if res.is_null() {
                        None
                    } else {
                        Some(std::ffi::CStr::from_ptr(pwd.pw_name).to_bytes().to_vec())
                    };
                    (rc, name)
                }
                NameKind::Group => {
                    let mut grp: libc::group = std::mem::zeroed();
                    let mut res: *mut libc::group = std::ptr::null_mut();
                    let rc = libc::getgrgid_r(
                        id,
                        &mut grp,
                        buf.as_mut_ptr().cast(),
                        buf.len(),
                        &mut res,
                    );
                    let name = if res.is_null() {
                        None
                    } else {
                        Some(std::ffi::CStr::from_ptr(grp.gr_name).to_bytes().to_vec())
                    };
                    (rc, name)
                }
            }
        };
        if rc == libc::ERANGE {
            let bigger = buf.len() * 2;
            buf.resize(bigger, 0);
            continue;
        }
        // `pwd.getpwuid` KeyError -> `''`.
        return found.unwrap_or_default();
    }
}

#[derive(Clone, Copy)]
enum NameKind {
    User,
    Group,
}

/// A tar stream under construction: counts what it wrote so the end
/// padding can be computed (`TarFile.offset`).
struct Counted<'w> {
    w: &'w mut dyn Write,
    offset: u64,
}

impl Counted<'_> {
    fn put(&mut self, buf: &[u8]) -> Result<(), Failure> {
        self.w.write_all(buf).map_err(|e| io_msg("tar stream", e))?;
        self.offset += buf.len() as u64;
        Ok(())
    }

    /// `TarFile.close()`: two zero blocks, then pad to a record.
    fn finish_tar(&mut self) -> Result<(), Failure> {
        let mut tail = vec![0u8; (BLOCKSIZE * 2) as usize];
        let rem = (self.offset + BLOCKSIZE * 2) % RECORDSIZE;
        if rem != 0 {
            tail.resize(tail.len() + (RECORDSIZE - rem) as usize, 0);
        }
        self.put(&tail)
    }
}

struct ImageTar<'w> {
    out: Counted<'w>,
    fmt: Fmt,
    inodes: HashMap<(u64, u64), Vec<u8>>,
    unames: HashMap<u32, Vec<u8>>,
    gnames: HashMap<u32, Vec<u8>>,
}

impl<'w> ImageTar<'w> {
    fn new(out: &'w mut dyn Write, fmt: Fmt) -> Self {
        ImageTar {
            out: Counted { w: out, offset: 0 },
            fmt,
            inodes: HashMap::new(),
            unames: HashMap::new(),
            gnames: HashMap::new(),
        }
    }

    fn write(&mut self, buf: &[u8]) -> Result<(), Failure> {
        self.out.put(buf)
    }

    /// `TarFile.add(path, arcname)` (recursive).
    fn add(&mut self, path: &Path, arcname: &[u8]) -> Result<(), Failure> {
        let md =
            std::fs::symlink_metadata(path).map_err(|e| io_msg(&path.display().to_string(), e))?;
        let ftype = md.mode() & libc::S_IFMT;
        let mut info = Info::new(arcname.to_vec(), md.mtime());
        info.typeflag = match ftype {
            libc::S_IFREG => {
                let key = (md.dev(), md.ino());
                match self.inodes.get(&key) {
                    Some(first) if md.nlink() > 1 && first.as_slice() != arcname => {
                        info.linkname = first.clone();
                        b'1'
                    }
                    _ => {
                        if md.ino() != 0 {
                            self.inodes.insert(key, arcname.to_vec());
                        }
                        b'0'
                    }
                }
            }
            libc::S_IFDIR => b'5',
            libc::S_IFIFO => b'6',
            libc::S_IFLNK => {
                info.linkname = std::fs::read_link(path)
                    .map_err(|e| io_msg(&path.display().to_string(), e))?
                    .into_os_string()
                    .into_vec();
                b'2'
            }
            libc::S_IFCHR => b'3',
            libc::S_IFBLK => b'4',
            // sockets: `gettarinfo` returns None, nothing is added
            _ => return Ok(()),
        };
        info.mode = md.mode() & 0o7777;
        info.uid = i128::from(md.uid());
        info.gid = i128::from(md.gid());
        info.size = if info.typeflag == b'0' { md.size() } else { 0 };
        info.uname = self
            .unames
            .entry(md.uid())
            .or_insert_with(|| name_of(NameKind::User, md.uid()))
            .clone();
        info.gname = self
            .gnames
            .entry(md.gid())
            .or_insert_with(|| name_of(NameKind::Group, md.gid()))
            .clone();
        if matches!(info.typeflag, b'3' | b'4') {
            info.devmajor = u64::from(libc::major(md.rdev()));
            info.devminor = u64::from(libc::minor(md.rdev()));
        }
        match info.typeflag {
            b'0' => {
                let mut f = File::open(path).map_err(|e| io_msg(&path.display().to_string(), e))?;
                let header = tobuf(&info, self.fmt)?;
                self.write(&header)?;
                let mut left = info.size;
                let mut buf = vec![0u8; 64 * 1024];
                while left > 0 {
                    let want = buf.len().min(left as usize);
                    let n = f
                        .read(&mut buf[..want])
                        .map_err(|e| io_msg(&path.display().to_string(), e))?;
                    if n == 0 {
                        return Err(Failure::Msg("OSError: unexpected end of data".into()));
                    }
                    self.write(&buf[..n])?;
                    left -= n as u64;
                }
                let rem = info.size % BLOCKSIZE;
                if rem != 0 {
                    self.write(&vec![0u8; (BLOCKSIZE - rem) as usize])?;
                }
            }
            b'5' => {
                let header = tobuf(&info, self.fmt)?;
                self.write(&header)?;
                let mut names: Vec<Vec<u8>> = std::fs::read_dir(path)
                    .map_err(|e| io_msg(&path.display().to_string(), e))?
                    .map(|e| e.map(|e| e.file_name().into_vec()))
                    .collect::<Result<_, _>>()
                    .map_err(|e| io_msg(&path.display().to_string(), e))?;
                // `sorted(os.listdir(name))`: the names are UTF-8 (the
                // pre-scan refused anything else), so byte order is
                // code-point order.
                names.sort();
                for name in names {
                    let child = path.join(std::ffi::OsStr::from_bytes(&name));
                    let child_arc = [arcname, b"/", name.as_slice()].concat();
                    self.add(&child, &child_arc)?;
                }
            }
            _ => {
                let header = tobuf(&info, self.fmt)?;
                self.write(&header)?;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(), Failure> {
        self.out.finish_tar()
    }
}

// ---------------------------------------------------------------------
// _check_pre_image_files / _generate_metadata_from_dir
// ---------------------------------------------------------------------

/// Python's `UnicodeEncodeError` text for a surrogate-escaped name.
fn utf8_error(name: &[u8]) -> Option<String> {
    if std::str::from_utf8(name).is_ok() {
        return None;
    }
    let mut points: Vec<(u32, bool)> = Vec::new();
    for chunk in name.utf8_chunks() {
        points.extend(chunk.valid().chars().map(|c| (u32::from(c), false)));
        points.extend(
            chunk
                .invalid()
                .iter()
                .map(|b| (0xDC00 + u32::from(*b), true)),
        );
    }
    let first = points.iter().position(|(_, bad)| *bad)?;
    let last = first + points[first..].iter().take_while(|(_, bad)| *bad).count() - 1;
    Some(if first == last {
        format!(
            "'utf-8' codec can't encode character '\\u{:04x}' in position {first}: surrogates not allowed",
            points[first].0
        )
    } else {
        format!(
            "'utf-8' codec can't encode characters in position {first}-{last}: surrogates not allowed"
        )
    })
}

/// The five numbers `_check_pre_image_files` returns.
struct ImageStats {
    max_prefix_length: usize,
    max_name_length: usize,
    max_link_length: usize,
    max_file_size: u64,
    total_size: u64,
}

/// `os.walk(root)` + the bookkeeping of `_check_pre_image_files`.
/// `rel` is the path of `dir` below the image root (empty for the root).
fn scan_image(dir: &Path, rel: &[u8], stats: &mut ImageStats) -> Result<(), Failure> {
    // `os.walk` swallows a listing error.
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut dirs: Vec<(Vec<u8>, PathBuf)> = Vec::new();
    let mut files: Vec<(Vec<u8>, PathBuf)> = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().into_vec();
        let path = entry.path();
        // `entry.is_dir()` follows symlinks.
        if std::fs::metadata(&path)
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            dirs.push((name, path));
        } else {
            files.push((name, path));
        }
    }
    let rel_of = |name: &[u8]| -> Vec<u8> {
        if rel.is_empty() {
            name.to_vec()
        } else {
            [rel, b"/", name].concat()
        }
    };
    // "image/" is the arcname prefix real counts (`image_prefix_length`).
    let counted = |rel_path: &[u8]| "image/".len() + rel_path.len();
    for (name, path) in &dirs {
        if let Some(msg) = utf8_error(name) {
            err_bytes(&[format!("\n*** {msg}\n\n").as_bytes()]);
            return Err(Failure::Msg(format!("UnicodeEncodeError: {msg}")));
        }
        let rel_path = rel_of(name);
        stats.max_prefix_length = stats.max_prefix_length.max(counted(&rel_path));
        let is_link = std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_link && let Ok(target) = std::fs::read_link(path) {
            stats.max_link_length = stats.max_link_length.max(target.as_os_str().len());
        }
    }
    for (name, path) in &files {
        if let Some(msg) = utf8_error(name) {
            err_bytes(&[format!("\n*** {msg}\n\n").as_bytes()]);
            return Err(Failure::Msg(format!("UnicodeEncodeError: {msg}")));
        }
        stats.max_name_length = stats.max_name_length.max(name.len());
        let path_length = counted(&rel_of(name));
        let md =
            std::fs::symlink_metadata(path).map_err(|e| io_msg(&path.display().to_string(), e))?;
        let link_length = if md.file_type().is_symlink() {
            std::fs::read_link(path)
                .map_err(|e| io_msg(&path.display().to_string(), e))?
                .as_os_str()
                .len()
        } else if md.nlink() > 1 {
            path_length
        } else {
            0
        };
        stats.max_link_length = stats.max_link_length.max(link_length);
        if md.file_type().is_symlink() {
            continue;
        }
        stats.total_size += md.size();
        stats.max_file_size = stats.max_file_size.max(md.size());
    }
    for (name, path) in &dirs {
        let is_link = std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if !is_link {
            scan_image(path, &rel_of(name), stats)?;
        }
    }
    Ok(())
}

/// `_get_tar_format_from_stats`.
fn tar_formats(stats: &ImageStats) -> (Fmt, Fmt) {
    let container = if stats.total_size < 8_000_000_000 {
        Fmt::Ustar
    } else {
        Fmt::Gnu
    };
    let mut image = if stats.max_file_size < 8_000_000_000 {
        Fmt::Ustar
    } else {
        Fmt::Gnu
    };
    if stats.max_prefix_length >= 155
        || stats.max_name_length >= 100
        || stats.max_link_length >= 100
    {
        image = Fmt::Gnu;
    }
    (container, image)
}

/// `_generate_metadata_from_dir`: a Python dict in insertion order.
/// A Python dict of file name -> contents, in insertion order.
type MetadataDict = Vec<(Vec<u8>, Vec<u8>)>;

fn generate_metadata_from_dir(dir: &Path) -> Result<MetadataDict, Failure> {
    fn walk(
        dir: &Path,
        out: &mut MetadataDict,
        index: &mut HashMap<Vec<u8>, usize>,
    ) -> Result<(), Failure> {
        let Ok(read) = std::fs::read_dir(dir) else {
            return Ok(());
        };
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut files: Vec<(Vec<u8>, PathBuf)> = Vec::new();
        for entry in read.flatten() {
            let path = entry.path();
            if std::fs::metadata(&path)
                .map(|m| m.is_dir())
                .unwrap_or(false)
            {
                dirs.push(path);
            } else {
                files.push((entry.file_name().into_vec(), path));
            }
        }
        for (name, path) in files {
            if std::str::from_utf8(&name).is_err() {
                continue;
            }
            let data = std::fs::read(&path).map_err(|e| io_msg(&path.display().to_string(), e))?;
            match index.get(&name) {
                Some(i) => out[*i].1 = data,
                None => {
                    index.insert(name.clone(), out.len());
                    out.push((name, data));
                }
            }
        }
        for path in dirs {
            let is_link = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if !is_link {
                walk(&path, out, index)?;
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, &mut out, &mut HashMap::new())?;
    Ok(out)
}

// ---------------------------------------------------------------------
// gpkg.compress
// ---------------------------------------------------------------------

/// `len(str)` of a surrogate-escaped name: one per code point, one per
/// undecodable byte.
fn py_len(name: &[u8]) -> usize {
    name.utf8_chunks()
        .map(|c| c.valid().chars().count() + c.invalid().len())
        .sum()
}

fn compress(
    settings: &Settings,
    basename: &[u8],
    binpkg_path: &Path,
    metadata_dir: &Path,
    image_dir: &Path,
) -> Result<(), Failure> {
    // `gpkg.__init__`
    let basename: &[u8] = match basename.iter().position(|b| *b == b'/') {
        Some(i) => &basename[i + 1..],
        None => basename,
    };
    let compression: Option<&str> = match settings.get("BINPKG_COMPRESS") {
        None | Some("") | Some("none") => None,
        Some(c) => Some(c),
    };
    let create_signature = settings.has_feature("binpkg-signing");
    // `self.ext_list`; an unknown method is refused later, by
    // `_get_compression_cmd`, once the container and `gpkg-1` exist.
    let ext = match compression {
        Some("gzip") => ".gz",
        Some("bzip2") => ".bz2",
        Some("lz4") => ".lz4",
        Some("lzip") => ".lz",
        Some("lzop") => ".lzo",
        Some("xz") => ".xz",
        Some("zstd") => ".zst",
        _ => "",
    };

    // helper: `_generate_metadata_from_dir`, then `compress()`
    let metadata = generate_metadata_from_dir(metadata_dir)?;
    let mut stats = ImageStats {
        max_prefix_length: 0,
        max_name_length: 0,
        max_link_length: 0,
        max_file_size: 0,
        total_size: 0,
    };
    scan_image(image_dir, b"", &mut stats)?;
    let (mut container_fmt, image_fmt) = tar_formats(&stats);
    // Long CPV
    if py_len(basename) >= 154 {
        container_fmt = Fmt::Gnu;
    }

    let file =
        File::create(binpkg_path).map_err(|e| io_msg(&binpkg_path.display().to_string(), e))?;
    let mut container = Container {
        file,
        fmt: container_fmt,
    };
    let mut checksums = Checksums::default();

    // `os.path.join(self.basename, name)`; an empty basename joins to
    // the bare name, and `_create_tarinfo` then refuses.
    let member = |name: &str| -> Vec<u8> {
        if basename.is_empty() {
            name.as_bytes().to_vec()
        } else {
            [basename, b"/", name.as_bytes()].concat()
        }
    };

    // gpkg version
    let version_name = member("gpkg-1");
    container.add_file(Info::new(version_name.clone(), now()), b"")?;
    checksums.record(&version_name, 0, digest_of(b""));

    let cmd = match compression {
        Some(c) => Some(compression_cmd(settings, c)?),
        None => None,
    };
    if basename.is_empty() {
        return Err(Failure::Msg(
            "portage.exception.InvalidBinaryPackageFormat: No basename or prefix specified".into(),
        ));
    }

    // metadata
    let metadata_name = member(&format!("metadata.tar{ext}"));
    let streamed = stream_member(
        &mut container,
        settings,
        &metadata_name,
        now(),
        Fmt::Ustar,
        cmd.as_deref(),
        create_signature,
        |w| {
            let mut out = Counted { w, offset: 0 };
            for (name, data) in &metadata {
                let mut info = Info::new([b"metadata/".as_slice(), name].concat(), now());
                info.size = data.len() as u64;
                out.put(&tobuf(&info, Fmt::Ustar)?)?;
                out.put(data)?;
                let rem = data.len() % 512;
                if rem != 0 {
                    out.put(&vec![0u8; 512 - rem])?;
                }
            }
            out.finish_tar()
        },
    )?;
    checksums.record(&metadata_name, streamed.size, streamed.digests);
    if let Some(sig) = streamed.signature {
        add_signature(&mut container, &mut checksums, &metadata_name, &sig)?;
    }

    // image
    let image_name = member(&format!("image.tar{ext}"));
    let streamed = stream_member(
        &mut container,
        settings,
        &image_name,
        now(),
        image_fmt,
        cmd.as_deref(),
        create_signature,
        |w| {
            let mut tar = ImageTar::new(w, image_fmt);
            tar.add(image_dir, b"image")?;
            tar.finish()
        },
    )?;
    checksums.record(&image_name, streamed.size, streamed.digests);
    if let Some(sig) = streamed.signature {
        add_signature(&mut container, &mut checksums, &image_name, &sig)?;
    }

    // `_add_manifest`
    let mut manifest = checksums.manifest();
    if create_signature {
        let mut gpg = spawn_gpg(settings, false)?;
        if let Some(stdin) = gpg.stdin.as_mut() {
            stdin.write_all(&manifest).map_err(|e| io_msg("gpg", e))?;
        }
        // `manifest.seek(0); manifest.write(gpg_output)` with the size
        // `manifest.tell()`: the member is exactly gpg's output.
        manifest = gpg.finish()?;
    }
    container.add_file(Info::new(member("Manifest"), now()), &manifest)?;
    container.close()
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::os::unix::fs::PermissionsExt;

    // ---- fixture plumbing --------------------------------------------

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

    /// pmtest's `fixtures/helpers/gpkg/` (oracles generated by running the
    /// real `bin/gpkg-helper.py compress`, see its README).
    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/gpkg")
    }

    /// pmtest's `fixtures/helpers/gpg-keyring/` (portage's own test
    /// keyring: trusted key 0x5D90EA06352177F6, untrusted
    /// 0x8812797DDF1DD192, passphrase GentooTest).
    fn keyring() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/gpg-keyring")
    }

    fn have(tool: &str) -> bool {
        crate::ebuild_package::find_binary(tool)
    }

    /// Scratch space for the oracle runs: tmpfs, never `/var/tmp` (zfs
    /// here: it refuses non-UTF-8 names and hands `readdir` back in hash
    /// order, which would change the metadata member order the helper
    /// sees; tmpfs order is stable for a fixed creation order).
    fn shm_scratch(tag: &str) -> Option<TempDir> {
        let shm = Path::new("/dev/shm");
        if !shm.is_dir() {
            eprintln!("skipped: no /dev/shm tmpfs for the readdir-order oracle");
            return None;
        }
        Some(TempDir::new_in(shm, tag))
    }

    /// `materialise.py`: `%XX` unescaping (uppercase hex only).
    fn unesc(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        let hex = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        };
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && i + 2 < b.len()
                && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
            {
                out.push(h * 16 + l);
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    /// `materialise.py` ported to Rust (so the test needs no python3):
    /// entries are created in manifest order, then a deepest-first pass
    /// applies ownership (root only: `lchown`, else every entry stays
    /// owned by the invoker), mode (not for symlinks; after the chown,
    /// which clears setuid) and the pinned mtime. Hardlinks share their
    /// target's inode and are skipped in the final pass.
    fn materialise(manifest: &Path, dest: &Path) {
        const MTIME: i64 = 1_700_000_000;
        std::fs::create_dir_all(dest).unwrap();
        let is_root = unsafe { libc::geteuid() } == 0;
        let mut entries: Vec<(char, u32, u32, u32, PathBuf)> = Vec::new();
        for line in std::fs::read_to_string(manifest).unwrap().lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let t: Vec<&str> = line.split(' ').collect();
            assert!(
                t.len() >= 5,
                "{}: too few fields: {line}",
                manifest.display()
            );
            let typ = t[0].chars().next().unwrap();
            let mode = if t[1] == "----" {
                0
            } else {
                u32::from_str_radix(t[1], 8).unwrap()
            };
            let (uid, gid): (u32, u32) = (t[2].parse().unwrap(), t[3].parse().unwrap());
            let rel = unesc(t[4]);
            let mut extra = &t[5..];
            let mut target = None;
            let mut content = None;
            if !extra.is_empty() && (extra[0] == "->" || extra[0] == "=>") {
                target = Some(unesc(extra[1]));
                extra = &extra[2..];
            }
            if !extra.is_empty() && extra[0].starts_with('|') {
                content = Some(unesc(&extra.join(" ")[1..]));
            }
            let path = if rel == b"." {
                dest.to_path_buf()
            } else {
                dest.join(std::ffi::OsStr::from_bytes(&rel))
            };
            match typ {
                'd' => std::fs::create_dir_all(&path).unwrap(),
                'f' => std::fs::write(&path, content.unwrap_or_default()).unwrap(),
                'l' => {
                    std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(&target.unwrap()), &path)
                        .unwrap()
                }
                'h' => std::fs::hard_link(
                    dest.join(std::ffi::OsStr::from_bytes(&target.unwrap())),
                    &path,
                )
                .unwrap(),
                other => panic!("bad manifest type {other:?}"),
            }
            entries.push((typ, mode, uid, gid, path));
        }
        // deepest first (stable, like `sorted(..., reverse=True)`)
        entries.sort_by_key(|e| {
            std::cmp::Reverse(
                e.4.as_os_str()
                    .as_bytes()
                    .iter()
                    .filter(|b| **b == b'/')
                    .count(),
            )
        });
        let ft = filetime::FileTime::from_unix_time(MTIME, 0);
        for (typ, mode, uid, gid, path) in &entries {
            if *typ == 'h' {
                continue;
            }
            if is_root {
                let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::lchown(c.as_ptr(), *uid, *gid) }, 0);
            }
            if *typ != 'l' {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(*mode)).unwrap();
            }
            filetime::set_symlink_file_times(path, ft, ft).unwrap();
        }
    }

    // ---- the D3 field dump (`dump_fields.py`, format v1) ---------------

    fn esc(b: &[u8]) -> String {
        b.iter()
            .map(|c| {
                if c.is_ascii_alphanumeric() || b"._/+-".contains(c) {
                    (*c as char).to_string()
                } else {
                    format!("%{c:02X}")
                }
            })
            .collect()
    }

    fn nts(b: &[u8]) -> &[u8] {
        match b.iter().position(|c| *c == 0) {
            Some(p) => &b[..p],
            None => b,
        }
    }

    /// `tarfile.nti`.
    fn nti(b: &[u8]) -> i128 {
        if b[0] == 0o200 || b[0] == 0o377 {
            let mut n: i128 = 0;
            for byte in &b[1..] {
                n = (n << 8) + i128::from(*byte);
            }
            if b[0] == 0o377 {
                n = -(256i128.pow(b.len() as u32 - 1) - n);
            }
            n
        } else {
            let s = std::str::from_utf8(nts(b)).unwrap().trim();
            if s.is_empty() {
                0
            } else {
                i128::from_str_radix(s, 8).unwrap()
            }
        }
    }

    struct Member {
        name: Vec<u8>,
        pre: String,
        fmt: &'static str,
        typeflag: u8,
        mode: i128,
        uid: i128,
        gid: i128,
        uname: Vec<u8>,
        gname: Vec<u8>,
        size: i128,
        linkname: Vec<u8>,
        devmajor: i128,
        devminor: i128,
        data: Vec<u8>,
    }

    /// Read a tar as `tarfile` does (own parser: the oracle side must not
    /// share code with the writer under test).
    fn parse_tar(data: &[u8]) -> Vec<Member> {
        let mut out = Vec::new();
        let mut o = 0usize;
        let (mut pre, mut longname, mut longlink) =
            (String::new(), None::<Vec<u8>>, None::<Vec<u8>>);
        while o + 512 <= data.len() {
            let blk = &data[o..o + 512];
            if blk.iter().all(|b| *b == 0) {
                break;
            }
            let t = blk[156];
            let size = nti(&blk[124..136]) as usize;
            let padded = size.div_ceil(512) * 512;
            if matches!(t, b'L' | b'K' | b'x' | b'g') {
                pre.push(t as char);
                let payload = nts(&data[o + 512..o + 512 + size]).to_vec();
                match t {
                    b'L' => longname = Some(payload),
                    b'K' => longlink = Some(payload),
                    _ => {}
                }
                o += 512 + padded;
                continue;
            }
            let mut name = longname.take().unwrap_or_else(|| {
                let n = nts(&blk[0..100]).to_vec();
                let prefix = nts(&blk[345..500]);
                if prefix.is_empty() {
                    n
                } else {
                    [prefix, b"/", n.as_slice()].concat()
                }
            });
            if t == b'5' {
                while name.ends_with(b"/") {
                    name.pop();
                }
            }
            let magic = &blk[257..265];
            let fmt = if magic == b"ustar\x0000" {
                "USTAR"
            } else if magic == b"ustar  \x00" {
                "GNU"
            } else if magic == [0u8; 8] {
                "V7"
            } else {
                "UNKNOWN"
            };
            out.push(Member {
                name,
                pre: std::mem::take(&mut pre),
                fmt,
                typeflag: t,
                mode: nti(&blk[100..108]),
                uid: nti(&blk[108..116]),
                gid: nti(&blk[116..124]),
                uname: nts(&blk[265..297]).to_vec(),
                gname: nts(&blk[297..329]).to_vec(),
                size: size as i128,
                linkname: longlink
                    .take()
                    .unwrap_or_else(|| nts(&blk[157..257]).to_vec()),
                devmajor: nti(&blk[329..337]),
                devminor: nti(&blk[337..345]),
                data: data[o + 512..o + 512 + size].to_vec(),
            });
            o += 512 + padded;
        }
        out
    }

    fn decompress(base: &str, data: &[u8]) -> Vec<u8> {
        let tool: Option<(&str, &[&str])> = if base.ends_with(".zst") {
            Some(("zstd", &["-dc"]))
        } else if base.ends_with(".xz") {
            Some(("xz", &["-dc"]))
        } else if base.ends_with(".bz2") {
            Some(("bzip2", &["-dc"]))
        } else if base.ends_with(".gz") {
            Some(("gzip", &["-dc"]))
        } else {
            None
        };
        let Some((tool, args)) = tool else {
            return data.to_vec();
        };
        let mut child = Command::new(tool)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = data.to_vec();
        let feeder = std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let out = child.wait_with_output().unwrap();
        feeder.join().unwrap();
        assert!(out.status.success(), "{tool} -d failed");
        out.stdout
    }

    fn hex_of(d: &[u8]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn dump_tar(data: &[u8], label: &str, out: &mut Vec<String>) {
        out.push(format!(
            "{label}.archive size={} mod10240={}",
            data.len(),
            data.len() % 10240
        ));
        for m in parse_tar(data) {
            let mut name = m.name.clone();
            if m.typeflag == b'5' {
                name.push(b'/');
            }
            let mut line = format!(
                "{label}.member name={} type={} fmt={} pre={} mode={:04o} uid={} gid={} uname={} gname={} size={} linkname={} devmajor={} devminor={}",
                esc(&name),
                m.typeflag as char,
                m.fmt,
                if m.pre.is_empty() { "-" } else { &m.pre },
                m.mode,
                m.uid,
                m.gid,
                esc(&m.uname),
                esc(&m.gname),
                m.size,
                if m.linkname.is_empty() {
                    "-".to_string()
                } else {
                    esc(&m.linkname)
                },
                m.devmajor,
                m.devminor,
            );
            if matches!(m.typeflag, b'0' | 0 | b'7') {
                line.push_str(&format!(
                    " sha256={}",
                    hex_of(&sha2::Sha256::digest(&m.data))
                ));
            }
            out.push(line);
        }
    }

    fn dump_manifest(text: &str, out: &mut Vec<String>) {
        let is_crc = |l: &str| {
            l.len() == 5
                && l.starts_with('=')
                && l[1..]
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
        };
        let is_b64 = |l: &str| {
            l.len() >= 20
                && l.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/' || c == b'=')
        };
        for ln in text.lines() {
            let t: Vec<&str> = ln.split_whitespace().collect();
            if t.first() == Some(&"DATA") {
                let mut pairs: Vec<(&str, &str)> = (3..t.len().saturating_sub(1))
                    .step_by(2)
                    .map(|i| (t[i], t[i + 1]))
                    .collect();
                pairs.sort();
                let algos: Vec<String> = pairs
                    .iter()
                    .map(|(a, d)| format!("{a}:{}hex", d.len()))
                    .collect();
                let base = t[1];
                let vol = if base.ends_with(".sig") {
                    "signature"
                } else if base.starts_with("metadata.tar") {
                    "mtime"
                } else if base.starts_with("image.tar") {
                    "compressor"
                } else {
                    "none"
                };
                if vol == "mtime" || vol == "signature" {
                    out.push(format!(
                        "manifest.data name={} volatile={vol} algos={}",
                        esc(base.as_bytes()),
                        algos.join(",")
                    ));
                } else {
                    let digests: Vec<String> =
                        pairs.iter().map(|(a, d)| format!("{a}={d}")).collect();
                    out.push(format!(
                        "manifest.data name={} volatile={vol} size={} {}",
                        esc(base.as_bytes()),
                        t[2],
                        digests.join(" ")
                    ));
                }
            } else if ln.starts_with("-----BEGIN PGP SIGNATURE") {
                out.push("manifest.pgp-signature volatile=signature (block marker)".to_string());
            } else if ln.starts_with("-----") {
                out.push(format!("manifest.pgp-armor {ln}"));
            } else if is_crc(ln) {
                out.push("manifest.pgp-crc volatile=signature".to_string());
            } else if is_b64(ln) {
                if !out
                    .last()
                    .is_some_and(|l| l.starts_with("manifest.pgp-base64"))
                {
                    out.push(
                        "manifest.pgp-base64 volatile=signature (consecutive lines collapsed)"
                            .to_string(),
                    );
                }
            } else if !ln.trim().is_empty() {
                out.push(format!("manifest.other {}", esc(ln.as_bytes())));
            }
        }
    }

    /// `dump_fields.py <container>`: the D3 field dump of a gpkg
    /// container, mtime never printed.
    fn dump_fields(path: &Path) -> String {
        let raw = std::fs::read(path).unwrap();
        let mut out = vec![
            "# gpkg field dump v1 (fixtures/helpers/gpkg/README.md); mtime never printed"
                .to_string(),
            format!("container.archive size=~ mod10240={}", raw.len() % 10240),
        ];
        let mut inner: Vec<(String, Vec<u8>)> = Vec::new();
        let mut manifest = None;
        for m in parse_tar(&raw) {
            let name = String::from_utf8_lossy(&m.name).into_owned();
            let base = name
                .split_once('/')
                .map(|(_, b)| b)
                .unwrap_or(&name)
                .to_string();
            let size = if base == "gpkg-1" {
                format!("size={}", m.size)
            } else {
                "size=~".to_string()
            };
            out.push(format!(
                "container.member name={} fmt={} pre={} type={} mode={:04o} uid={} gid={} uname={} gname={} linkname={} devmajor={} devminor={} {size}",
                esc(&m.name),
                m.fmt,
                if m.pre.is_empty() { "-" } else { &m.pre },
                m.typeflag as char,
                m.mode,
                m.uid,
                m.gid,
                esc(&m.uname),
                esc(&m.gname),
                if m.linkname.is_empty() { "-".to_string() } else { esc(&m.linkname) },
                m.devmajor,
                m.devminor,
            ));
            if (base.starts_with("metadata.tar") || base.starts_with("image.tar"))
                && !base.ends_with(".sig")
            {
                inner.push((base, m.data));
            } else if base == "Manifest" {
                manifest = Some(String::from_utf8(m.data).unwrap());
            }
        }
        for (base, data) in inner {
            let label = base.split('.').next().unwrap().to_string();
            dump_tar(&decompress(&base, &data), &label, &mut out);
        }
        if let Some(m) = manifest {
            dump_manifest(&m, &mut out);
        }
        out.join("\n") + "\n"
    }

    // ---- host owners ---------------------------------------------------

    #[derive(Clone, PartialEq, Debug)]
    struct Owner {
        uid: u32,
        gid: u32,
        uname: String,
        gname: String,
    }

    /// The owner the generator's user run recorded, read from the case
    /// README's `/etc/passwd (user run)` / `/etc/group (user run)` lines.
    fn generator_owner(readme: &str) -> Owner {
        let field = |label: &str| -> Vec<String> {
            readme
                .lines()
                .find_map(|l| l.trim().strip_prefix(label))
                .unwrap_or_else(|| panic!("README has no {label:?} line"))
                .trim()
                .split(':')
                .map(str::to_string)
                .collect()
        };
        let pw = field("/etc/passwd (user run):");
        let gr = field("/etc/group  (user run):");
        Owner {
            uid: pw[2].parse().unwrap(),
            gid: pw[3].parse().unwrap(),
            uname: pw[0].clone(),
            gname: gr[0].clone(),
        }
    }

    fn host_owner() -> Owner {
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        Owner {
            uid,
            gid,
            uname: String::from_utf8_lossy(&name_of(NameKind::User, uid)).into_owned(),
            gname: String::from_utf8_lossy(&name_of(NameKind::Group, gid)).into_owned(),
        }
    }

    /// The user-run oracle records the generator's own owner (uid 1000,
    /// gid 100, `vivo`, `users`) on every image entry it owned. Those
    /// entries are owned by whoever runs the test here, so the expected
    /// values of the `image.member` lines are rewritten to the host
    /// owner. The container and metadata lines (uid 0, empty names, set
    /// by the writer, not by the filesystem) are never touched.
    fn rewrite_owners(expected: &str, from: &Owner, to: &Owner) -> String {
        let old = format!(
            " uid={} gid={} uname={} gname={} size=",
            from.uid,
            from.gid,
            esc(from.uname.as_bytes()),
            esc(from.gname.as_bytes())
        );
        let new = format!(
            " uid={} gid={} uname={} gname={} size=",
            to.uid,
            to.gid,
            esc(to.uname.as_bytes()),
            esc(to.gname.as_bytes())
        );
        expected
            .lines()
            .map(|l| {
                if l.starts_with("image.member ") {
                    l.replace(&old, &new)
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    // ---- running one oracle case ----------------------------------------

    struct CaseRun {
        work: TempDir,
        out: PathBuf,
        rc: i32,
        stderr: String,
    }

    /// Stage a case like the generator did and run
    /// `portuale __helper python gpkg-helper.py <args>` as a CHILD
    /// PROCESS with the case's `env` file and the `@..@` tokens
    /// substituted. `None` = skipped (tool or tmpfs missing).
    /// `key` overrides `BINPKG_GPG_SIGNING_KEY`.
    fn run_case(case: &str, comp: &str, key: Option<&str>) -> Option<CaseRun> {
        let case_dir = fixtures().join(case);
        let comp_dir = case_dir.join(comp);
        if !have(comp) {
            eprintln!("skipped {case}/{comp}: no {comp}");
            return None;
        }
        let signed = std::fs::read_to_string(comp_dir.join("env"))
            .unwrap()
            .contains("binpkg-signing");
        if signed && !(have("gpg") && have("flock")) {
            eprintln!("skipped {case}/{comp}: needs gpg and flock");
            return None;
        }
        let work = shm_scratch("gpkg-case")?;
        let w = work.path().to_path_buf();
        let metadata = w.join("metadata");
        std::fs::create_dir_all(&metadata).unwrap();
        // sorted creation order, as `generate-gpkg.sh` stages it
        let mut names: Vec<_> = std::fs::read_dir(fixtures().join("metadata"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        for n in names {
            std::fs::copy(fixtures().join("metadata").join(&n), metadata.join(&n)).unwrap();
        }
        let image = w.join("image");
        materialise(&case_dir.join("image.manifest"), &image);
        let gnupg = w.join("gnupg");
        if signed {
            copy_tree(&keyring(), &gnupg);
            std::fs::set_permissions(&gnupg, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let out = w.join("out.gpkg.tar");
        let subst = |s: &str| -> OsString {
            match s {
                "@OUT@" => out.clone().into_os_string(),
                "@METADATA@" => metadata.clone().into_os_string(),
                "@IMAGE@" => image.clone().into_os_string(),
                other => other
                    .replace("@SCRATCH@", w.to_str().unwrap())
                    .replace("@GNUPGHOME@", gnupg.to_str().unwrap())
                    .into(),
            }
        };
        let args: Vec<OsString> = std::fs::read_to_string(comp_dir.join("args"))
            .unwrap()
            .lines()
            .map(&subst)
            .collect();
        let mut cmd = Command::new(portuale_exe());
        cmd.args(["__helper", "python", "gpkg-helper.py"])
            .args(&args)
            .current_dir(&w)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("HOME", w.join("home"))
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python");
        for line in std::fs::read_to_string(comp_dir.join("env"))
            .unwrap()
            .lines()
        {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line.split_once('=').unwrap();
            let v = if k == "BINPKG_GPG_SIGNING_KEY" {
                key.unwrap_or(v).to_string()
            } else {
                v.to_string()
            };
            cmd.env(k, subst(&v));
        }
        let output = cmd.output().expect("portuale __helper spawns");
        if signed {
            let _ = Command::new("gpgconf")
                .arg("--homedir")
                .arg(&gnupg)
                .args(["--kill", "all"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        Some(CaseRun {
            work,
            out,
            rc: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let dest = to.join(e.file_name());
            if e.file_type().unwrap().is_symlink() {
                std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), dest).unwrap();
            } else if e.file_type().unwrap().is_dir() {
                copy_tree(&e.path(), &dest);
            } else {
                std::fs::copy(e.path(), dest).unwrap();
            }
        }
    }

    /// Compare one `<case>/<comp>/` directory against the oracle.
    /// Returns false when skipped.
    fn check_oracle(case: &str, comp: &str) -> bool {
        let comp_dir = fixtures().join(case).join(comp);
        let Some(run) = run_case(case, comp, None) else {
            return false;
        };
        // As root the oracle's root run applies when it differs from the
        // user run (owners/names), else the user run's files.
        let as_root = unsafe { libc::geteuid() } == 0;
        let root_oracle = as_root && comp_dir.join("rc.root.txt").exists();
        let (rc_file, fields_file) = if root_oracle {
            ("rc.root.txt", "out.root.fields")
        } else {
            ("rc.txt", "out.fields")
        };
        let want_rc: i32 = std::fs::read_to_string(comp_dir.join(rc_file))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(run.rc, want_rc, "{case}/{comp}: rc, stderr: {}", run.stderr);
        if want_rc != 0 {
            // real's refusal: rc 1 and no artifact
            assert!(!run.out.exists(), "{case}/{comp}: an artifact was left");
            let want_err = std::fs::read_to_string(comp_dir.join("stderr.txt")).unwrap();
            let needle = want_err
                .lines()
                .find(|l| l.starts_with("UnicodeEncodeError: "))
                .map(|l| l.trim_start_matches("UnicodeEncodeError: ").to_string())
                .expect("oracle stderr names the UnicodeEncodeError");
            assert!(
                run.stderr.contains(&needle),
                "{case}/{comp}: stderr lacks {needle:?}: {}",
                run.stderr
            );
            eprintln!("oracle {case}/{comp}: ok (refused)");
            return true;
        }
        let mut want = std::fs::read_to_string(comp_dir.join(fields_file)).unwrap();
        if !root_oracle {
            let readme = std::fs::read_to_string(fixtures().join(case).join("README")).unwrap();
            want = rewrite_owners(&want, &generator_owner(&readme), &host_owner());
        }
        let got = dump_fields(&run.out);
        if got != want {
            let g: Vec<&str> = got.lines().collect();
            let wl: Vec<&str> = want.lines().collect();
            let mut diff = String::new();
            for i in 0..g.len().max(wl.len()) {
                if g.get(i) != wl.get(i) {
                    diff.push_str(&format!(
                        "line {}:\n  got:  {}\n  want: {}\n",
                        i + 1,
                        g.get(i).unwrap_or(&"<none>"),
                        wl.get(i).unwrap_or(&"<none>")
                    ));
                }
            }
            panic!("{case}/{comp}: field dump differs from the oracle\n{diff}");
        }
        eprintln!("oracle {case}/{comp}: ok");
        true
    }

    macro_rules! oracle_case {
        ($name:ident, $case:literal, [$($comp:literal),+]) => {
            #[test]
            fn $name() {
                for comp in [$($comp),+] {
                    check_oracle($case, comp);
                }
            }
        };
    }
    oracle_case!(oracle_plain, "plain", ["zstd", "xz", "bzip2", "gzip"]);
    oracle_case!(oracle_special, "special", ["zstd", "xz", "bzip2", "gzip"]);
    oracle_case!(oracle_empty, "empty", ["zstd", "xz", "bzip2", "gzip"]);
    oracle_case!(oracle_longname_gnu_image_tar, "longname", ["zstd"]);
    oracle_case!(oracle_basename_154_gnu_container, "long-basename", ["zstd"]);
    oracle_case!(oracle_basename_153_stays_ustar, "basename-153", ["zstd"]);
    oracle_case!(oracle_non_utf8_name_is_refused, "non-utf8", ["zstd"]);
    oracle_case!(oracle_signed, "signed", ["zstd"]);

    // ---- unit tests ------------------------------------------------------

    /// The owner rewrite touches only `image.member` lines carrying the
    /// generator's owner.
    #[test]
    fn owner_rewrite_changes_only_image_lines_with_the_generator_owner() {
        let from = Owner {
            uid: 1000,
            gid: 100,
            uname: "vivo".into(),
            gname: "users".into(),
        };
        let to = Owner {
            uid: 4242,
            gid: 4243,
            uname: String::new(),
            gname: String::new(),
        };
        let text = "container.member name=a uid=0 gid=0 uname= gname= linkname=- size=~\n\
                    image.member name=image/ type=5 mode=0755 uid=1000 gid=100 uname=vivo gname=users size=0 linkname=-\n\
                    image.member name=image/x type=0 mode=0644 uid=0 gid=0 uname=root gname=root size=1 linkname=-\n";
        let got = rewrite_owners(text, &from, &to);
        assert!(
            got.contains("uid=4242 gid=4243 uname= gname= size=0"),
            "{got}"
        );
        assert!(got.contains("container.member name=a uid=0 gid=0"), "{got}");
        assert!(
            got.contains("uid=0 gid=0 uname=root gname=root size=1"),
            "{got}"
        );
        let readme = "  /etc/passwd (user run): vivo:x:1000:100::/home/vivo:/bin/bash\n  /etc/group  (user run): users:x:100:\n";
        assert_eq!(generator_owner(readme), from);
    }

    /// `tarfile.itn`: octal, GNU base-256 overflow, USTAR overflow error.
    #[test]
    fn itn_matches_tarfile() {
        assert_eq!(itn(0o644, 8, Fmt::Ustar).unwrap(), b"0000644\0");
        assert!(itn(8i128.pow(7), 8, Fmt::Ustar).is_err());
        assert_eq!(
            itn(8i128.pow(7), 8, Fmt::Gnu).unwrap(),
            [0o200, 0, 0, 0, 0, 0x20, 0, 0]
        );
        assert_eq!(
            itn(-1, 12, Fmt::Gnu).unwrap(),
            [0o377, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255]
        );
    }

    /// `_posix_split_name` and the 100/155 limits.
    #[test]
    fn ustar_names_split_like_posix_split_name() {
        let long = format!("{}/{}", "d".repeat(120), "f".repeat(50));
        let (prefix, name) = posix_split_name(long.as_bytes()).unwrap();
        assert_eq!((prefix.len(), name.len()), (120, 50));
        let too_long = format!("{}/{}", "d".repeat(160), "f".repeat(50));
        assert!(posix_split_name(too_long.as_bytes()).is_err());
        let mut info = Info::new(vec![b'a'; 101], 0);
        info.typeflag = b'0';
        assert!(tobuf(&info, Fmt::Ustar).is_err(), "no slash to split at");
        // GNU: an `L` block in front only beyond 100 bytes.
        info.name = vec![b'a'; 100];
        assert_eq!(tobuf(&info, Fmt::Gnu).unwrap().len(), 512);
        info.name = vec![b'a'; 101];
        assert_eq!(tobuf(&info, Fmt::Gnu).unwrap().len(), 512 * 3);
    }

    /// `_get_tar_format_from_stats`: the 100/155 thresholds are `>=`.
    #[test]
    fn tar_formats_follow_the_thresholds() {
        let base = ImageStats {
            max_prefix_length: 154,
            max_name_length: 99,
            max_link_length: 99,
            max_file_size: 7_999_999_999,
            total_size: 7_999_999_999,
        };
        assert_eq!(tar_formats(&base), (Fmt::Ustar, Fmt::Ustar));
        for (field, which) in [("prefix", 0), ("name", 1), ("link", 2)] {
            let mut s = ImageStats { ..base };
            match which {
                0 => s.max_prefix_length = 155,
                1 => s.max_name_length = 100,
                _ => s.max_link_length = 100,
            }
            assert_eq!(tar_formats(&s), (Fmt::Ustar, Fmt::Gnu), "{field}");
        }
        let mut big = ImageStats { ..base };
        big.max_file_size = 8_000_000_000;
        big.total_size = 8_000_000_000;
        assert_eq!(tar_formats(&big), (Fmt::Gnu, Fmt::Gnu));
    }

    fn run_helper(args: &[&str]) -> std::process::Output {
        Command::new(portuale_exe())
            .args(["__helper", "python", "gpkg-helper.py"])
            .args(args)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .output()
            .expect("portuale __helper spawns")
    }

    /// argv errors are real's (`gpkg-helper.py`): argparse rc 2, the
    /// `compose` usage rc 1.
    #[test]
    fn argv_errors_are_portages() {
        let none = run_helper(&[]);
        assert_eq!(none.status.code(), Some(2));
        assert_eq!(
            String::from_utf8_lossy(&none.stderr),
            "usage: usage: gpkg-helper.py COMMAND [args]\ngpkg-helper.py: error: missing command argument\n"
        );
        let bad = run_helper(&["frob"]);
        assert_eq!(bad.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&bad.stderr)
                .contains("gpkg-helper.py: error: invalid command: 'frob'")
        );
        let argc = run_helper(&["compress", "a", "b"]);
        assert_eq!(argc.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&argc.stderr),
            "usage: compose <package_cpv> <binpkg_path> <metadata_dir> <image_dir>\n4 arguments are required, got 2\n"
        );
        let tmp = TempDir::new("gpkg-argv");
        let dir = tmp.path().to_str().unwrap();
        let not3 = run_helper(&["compress", "a", "b", "/nonexistent-meta", dir]);
        assert_eq!(not3.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&not3.stderr)
                .ends_with("Argument 3 is not a directory: '/nonexistent-meta'\n")
        );
        let not4 = run_helper(&["compress", "a", "b", dir, "/nonexistent-image"]);
        assert_eq!(not4.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&not4.stderr)
                .ends_with("Argument 4 is not a directory: '/nonexistent-image'\n")
        );
    }

    /// An unknown `BINPKG_COMPRESS` is refused by `_get_compression_cmd`,
    /// after the container and its `gpkg-1` header exist: real (probed
    /// live, Portage 3.0.82.2, `BINPKG_COMPRESS=foo`) exits 1 with
    /// `InvalidCompressionMethod: foo` and leaves a 512-byte file holding
    /// only `pkg-1/gpkg-1`.
    #[test]
    fn unknown_compression_fails_after_gpkg_1_like_portage() {
        let tmp = TempDir::new("gpkg-badcomp");
        let (meta, image) = (tmp.path().join("m"), tmp.path().join("i"));
        std::fs::create_dir_all(&meta).unwrap();
        std::fs::create_dir_all(&image).unwrap();
        std::fs::write(meta.join("PF"), "x\n").unwrap();
        std::fs::write(image.join("f"), "y\n").unwrap();
        let out = tmp.path().join("out.gpkg.tar");
        let run = Command::new(portuale_exe())
            .args([
                "__helper",
                "python",
                "gpkg-helper.py",
                "compress",
                "cat/pkg-1",
            ])
            .args([&out, &meta, &image])
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .env("BINPKG_COMPRESS", "foo")
            .output()
            .expect("portuale __helper spawns");
        assert_eq!(run.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&run.stderr).contains("InvalidCompressionMethod: foo"),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        let left = std::fs::read(&out).expect("the partial container is left");
        assert_eq!(left.len(), 512);
        assert_eq!(nts(&left[..100]), b"pkg-1/gpkg-1");
    }

    // ---- round trips through the container reader -------------------------

    /// A listing of a tree: path -> (kind, payload), plus the hardlink
    /// groups, for comparing an extracted image with the materialised one.
    fn tree_listing(root: &Path) -> (Vec<String>, Vec<Vec<String>>) {
        fn walk(
            root: &Path,
            dir: &Path,
            lines: &mut Vec<String>,
            inodes: &mut HashMap<(u64, u64), Vec<String>>,
        ) {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            for n in names {
                let p = dir.join(&n);
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                let md = std::fs::symlink_metadata(&p).unwrap();
                if md.file_type().is_symlink() {
                    lines.push(format!(
                        "{rel} -> {}",
                        std::fs::read_link(&p).unwrap().display()
                    ));
                } else if md.is_dir() {
                    lines.push(format!("{rel}/"));
                    walk(root, &p, lines, inodes);
                } else {
                    lines.push(format!(
                        "{rel} {}",
                        hex_of(&sha2::Sha256::digest(std::fs::read(&p).unwrap()))
                    ));
                    if md.nlink() > 1 {
                        inodes.entry((md.dev(), md.ino())).or_default().push(rel);
                    }
                }
            }
        }
        let mut lines = Vec::new();
        let mut inodes = HashMap::new();
        walk(root, root, &mut lines, &mut inodes);
        let mut groups: Vec<Vec<String>> = inodes.into_values().collect();
        groups.sort();
        (lines, groups)
    }

    fn unsigned_verify() -> crate::binpkg::GpgVerify {
        crate::binpkg::GpgVerify {
            verify_signature: true,
            request_signature: false,
            base_command: crate::binpkg::DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string(),
            gpg_home: "/nonexistent-gnupg".to_string(),
        }
    }

    /// The native output is accepted by the container reader, its
    /// metadata is read back, the Manifest verifies, and extracting the
    /// image gives back the materialised tree (types, contents, link
    /// targets, hardlink groups; modes depend on the umask and are not
    /// compared). Also: the build-info extracted verbatim equals the
    /// metadata dir that went in.
    fn round_trip(case: &str) {
        let Some(run) = run_case(case, "zstd", None) else {
            return;
        };
        assert_eq!(run.rc, 0, "{}", run.stderr);
        let (members, marker) = crate::binpkg::read_outer_members(&run.out).expect("outer members");
        assert!(marker, "gpkg-1 marker");
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["gpkg-1", "metadata.tar.zst", "image.tar.zst", "Manifest"]
        );
        let meta = crate::binpkg::read_gpkg_metadata(&run.out).expect("metadata");
        let category = std::fs::read_to_string(fixtures().join("metadata/CATEGORY")).unwrap();
        assert_eq!(
            meta.get("CATEGORY").map(|s| s.trim()),
            Some(category.trim())
        );
        crate::binpkg::verify_gpkg_manifest(&run.out, &unsigned_verify())
            .expect("Manifest verifies");

        let image_out = run.work.path().join("extracted-image");
        let bi_out = run.work.path().join("extracted-build-info");
        crate::binpkg::extract_binpkg(
            &run.out,
            &image_out,
            &bi_out,
            &unsigned_verify(),
            &crate::ebuild_merge::XattrPolicy::default(),
        )
        .expect("extract");
        assert_eq!(
            tree_listing(&image_out),
            tree_listing(&run.work.path().join("image")),
            "{case}: extracted image differs from the materialised tree"
        );
        for e in std::fs::read_dir(fixtures().join("metadata")).unwrap() {
            let name = e.unwrap().file_name();
            assert_eq!(
                std::fs::read(bi_out.join(&name)).unwrap(),
                std::fs::read(fixtures().join("metadata").join(&name)).unwrap(),
                "build-info/{}",
                name.to_string_lossy()
            );
        }
    }

    #[test]
    fn plain_output_round_trips_through_the_reader() {
        round_trip("plain");
    }

    #[test]
    fn special_output_round_trips_through_the_reader() {
        round_trip("special");
    }

    /// Signed: the oracle ran with the untrusted key 0x8812... (the field
    /// dump does not depend on the key); acceptance by
    /// `verify_gpkg_manifest` needs the keyring's TRUSTED key
    /// 0x5D90EA06352177F6, so this run signs with that one. Every member
    /// has its `.sig`, the clear-signed Manifest and each detached
    /// signature verify (request-signature = nothing may be unsigned).
    #[test]
    fn signed_output_is_accepted_by_verify_gpkg_manifest() {
        let Some(run) = run_case("signed", "zstd", Some("0x5D90EA06352177F6")) else {
            return;
        };
        assert_eq!(run.rc, 0, "{}", run.stderr);
        let (members, _) = crate::binpkg::read_outer_members(&run.out).unwrap();
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "gpkg-1",
                "metadata.tar.zst",
                "metadata.tar.zst.sig",
                "image.tar.zst",
                "image.tar.zst.sig",
                "Manifest"
            ]
        );
        let gpg = crate::binpkg::GpgVerify {
            verify_signature: true,
            request_signature: true,
            base_command: crate::binpkg::DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string(),
            gpg_home: run.work.path().join("gnupg").display().to_string(),
        };
        let verdict = crate::binpkg::verify_gpkg_manifest(&run.out, &gpg);
        let _ = Command::new("gpgconf")
            .arg("--homedir")
            .arg(run.work.path().join("gnupg"))
            .args(["--kill", "all"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        verdict.expect("the signed container verifies against the trusted key");

        // The oracle's own key (0x8812...) is the keyring's UNTRUSTED one:
        // its signatures are valid but `verify_gpkg_manifest` refuses
        // them (`TRUST_UNDEFINED`), as real's `_check_gpg_status` does.
        let Some(untrusted) = run_case("signed", "zstd", None) else {
            return;
        };
        assert_eq!(untrusted.rc, 0, "{}", untrusted.stderr);
        let gpg = crate::binpkg::GpgVerify {
            gpg_home: untrusted.work.path().join("gnupg").display().to_string(),
            ..gpg
        };
        let refused = crate::binpkg::verify_gpkg_manifest(&untrusted.out, &gpg);
        let _ = Command::new("gpgconf")
            .arg("--homedir")
            .arg(untrusted.work.path().join("gnupg"))
            .args(["--kill", "all"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let refused = refused.expect_err("the untrusted key's signature is refused");
        eprintln!("untrusted-key verdict: {refused}");
    }

    /// The metadata dict semantics of `_generate_metadata_from_dir`: a
    /// duplicate file name keeps its first position and its last value, a
    /// non-UTF-8 name is skipped, a symlink to a directory is not walked.
    #[test]
    fn metadata_dict_semantics() {
        let Some(tmp) = shm_scratch("gpkg-meta") else {
            return;
        };
        let meta = tmp.path().join("meta");
        std::fs::create_dir_all(meta.join("sub")).unwrap();
        std::fs::write(meta.join("A"), b"top").unwrap();
        std::fs::write(meta.join("sub/A"), b"inner").unwrap();
        std::fs::write(meta.join("sub/B"), b"b").unwrap();
        std::fs::write(
            meta.join(std::ffi::OsStr::from_bytes(b"bad\xff")),
            b"skipped",
        )
        .unwrap();
        std::os::unix::fs::symlink("sub", meta.join("linkdir")).unwrap();
        let got = generate_metadata_from_dir(&meta).unwrap();
        // top-level files first, then the subdirectory; `A` is last-wins
        assert_eq!(
            got,
            vec![
                (b"A".to_vec(), b"inner".to_vec()),
                (b"B".to_vec(), b"b".to_vec())
            ]
        );
    }

    // ---- no checkout: the package scenarios as child test processes ----

    /// Re-run one `ebuild_package` test in a child process with no
    /// Portage checkout and no real Python reachable, so the native
    /// `gpkg-helper.py` is the only thing that can build the gpkg.
    fn rerun_without_checkout(test: &str) {
        let out = Command::new(std::env::current_exe().unwrap())
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
    fn real_package_gpkg_works_without_the_checkout() {
        rerun_without_checkout(
            "ebuild_package::tests::real_package_with_gpkg_format_builds_a_real_gpkg_tar_this_pilots_reader_round_trips",
        );
    }

    #[test]
    fn real_multi_instance_gpkg_works_without_the_checkout() {
        rerun_without_checkout(
            "ebuild_package::tests::real_package_with_binpkg_multi_instance_writes_the_cat_pn_subdir_layout",
        );
    }

    #[test]
    fn quickpkg_from_vdb_gpkg_works_without_the_checkout() {
        rerun_without_checkout(
            "ebuild_package::tests::quickpkg_from_vdb_with_gpkg_format_builds_a_readable_gpkg",
        );
    }

    /// `MAKEOPTS` set only in `make.conf` (not in the calling env) reaches
    /// the native helper: `invoke_dyn_package` exports the file side, and
    /// `{JOBS}` of `zstd -T{JOBS}` becomes 5 (real: `makeopts_to_job_count`
    /// of the helper's own `portage.settings`). A logging `zstd` stands in
    /// for the compressor. Child process, no checkout, no real Python.
    #[test]
    fn make_conf_makeopts_reaches_the_native_compressor() {
        if !have("zstd") {
            eprintln!("skipped: no zstd");
            return;
        }
        let tmp = TempDir::new("gpkg-makeopts");
        let w = tmp.path();
        let fakebin = w.join("fakebin");
        std::fs::create_dir_all(&fakebin).unwrap();
        let log = w.join("zstd.log");
        let script = fakebin.join("zstd");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"zstd $*\" >> {}\nexec /usr/bin/zstd \"$@\"\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // a config root: the fixtures' etc/ (make.conf + repos.conf) with
        // MAKEOPTS appended and the repo locations made absolute
        let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .unwrap();
        let cfg = w.join("cfg");
        copy_tree(&fixtures_root.join("etc"), &cfg.join("etc"));
        let repos_conf = cfg.join("etc/portage/repos.conf");
        for entry in std::fs::read_dir(&repos_conf).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap().replace(
                "location = ",
                &format!("location = {}/", fixtures_root.display()),
            );
            std::fs::write(&path, text).unwrap();
        }
        let profile = cfg.join("etc/portage/make.profile");
        std::fs::remove_file(&profile).unwrap();
        std::os::unix::fs::symlink(fixtures_root.join("repo/profiles/default"), &profile).unwrap();
        let make_conf = cfg.join("etc/portage/make.conf");
        let mut text = std::fs::read_to_string(&make_conf).unwrap();
        text.push_str("\nMAKEOPTS=\"-j5\"\n");
        std::fs::write(&make_conf, text).unwrap();
        for d in ["tmp", "pkgdir", "root"] {
            std::fs::create_dir_all(w.join(d)).unwrap();
        }
        let out = Command::new(portuale_exe())
            .arg("ebuild")
            .arg(fixtures_root.join("repo/dev-libs/packagepkg/packagepkg-1.0.ebuild"))
            .arg("package")
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/local/bin:/usr/bin:/bin", fakebin.display()),
            )
            .env("HOME", w)
            .env("PORTAGE_CONFIGROOT", &cfg)
            .env("PKGDIR", w.join("pkgdir"))
            .env("PORTAGE_TMPDIR", w.join("tmp"))
            .env("ROOT", w.join("root"))
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .output()
            .unwrap();
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{text}");
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(logged.contains("zstd -T5"), "zstd log: {logged:?}\n{text}");
        assert!(
            w.join("pkgdir/dev-libs/packagepkg-1.0.gpkg.tar").is_file(),
            "{text}"
        );
    }
}
