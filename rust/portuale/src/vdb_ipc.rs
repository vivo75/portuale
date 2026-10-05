// The parent pipe (`PORTUALE_VDB_IPC`, feat#157, backlog #305 S6.3).
//
// redb allows one process per database file. While `mrg --vdb-backend=redb`
// merges, it holds the file read-write, so the native `portuale portageq`
// that an ebuild phase runs for `has_version` / `best_version` cannot open
// it. Real Portage has the same shape for its own helpers (`ebuild-ipc`: a
// phase asks the parent emerge over a FIFO in `${PORTAGE_BUILDDIR}/.ipc`);
// here the parent `mrg` answers over a Unix domain socket and exports its
// path in `PORTUALE_VDB_IPC`. Design: `docs/vdb_to_db.md` §10; plan step
// S6.3; owner question Q5 (default: the pipe for redb only, files and
// sqlite keep opening the database directly).
//
// # Split of the logic
//
// The child (`portageq.rs`) keeps every rule of
// `docs/evidence/305-s6-portageq.md` (arguments, strict parse, EAPI QA
// notice, USE-conditional evaluation, `dep_expand`, output and exit codes)
// and the config load for the profile's implicit IUSE. The parent only
// runs the two installed-package lookups those rules need, through the
// same functions the child uses on a files or sqlite root, against the
// backend registered for its ROOT:
//
// - `match`: `portage_repo::best_installed_match` (the resolver's own
//   installed-package matcher);
// - `categories`: `portageq::installed_categories_of` (the `dep_expand`
//   lookup for a bare package name, only reached outside a phase).
//
// # Protocol
//
// One connection, one request line, one reply line; fields are separated
// by TAB, and no field may hold a TAB or a newline.
//
// ```text
// match\t<eroot>\t<atom>\t<unevaluated atom or empty>\t<implicit IUSE, space separated>\n
//     -> ok\t<version>\n          (the highest matching installed version, PF's version part)
//     -> none\n                   (nothing matches)
// categories\t<eroot>\t<pn>\n
//     -> ok\t<cat> <cat> ...\n    (categories with <pn> installed; empty after the TAB)
// anything else, or a request the server cannot answer
//     -> err\t<message>\n
// ```
//
// `<eroot>` must name the ROOT the server serves (`portageq::same_root`);
// the client only uses the pipe for that root anyway. The implicit IUSE is
// sent only when the atom has USE dependencies (otherwise empty: the
// matcher never asks for it).
//
// # Lifecycle
//
// `mrg` starts the server (redb, not `--pretend`) right after it opened
// and registered the database, before `pretend::run`, and keeps the
// returned [`Server`] alive for the whole run. The socket lives in a
// private directory (mode 0700) `$TMPDIR/portuale-vdb-ipc-<pid>-<nanos>/s`
// (`/tmp` when that path is too long for a socket address). Dropping the
// [`Server`] stops the accept thread and removes the socket and the
// directory. Outside the unit tests a panic hook removes it too (the
// release profile aborts on panic, so `Drop` would not run). What a
// `SIGKILL` leaves behind is removed by the next server start (the
// directories of dead pids are swept, as `portage_util::TempDir` does).
//
// # Never blocking the merge
//
// The server thread only reads: every lookup is a read transaction of the
// registered handle, and redb readers never wait for the writer. A read
// sees the last committed state, never the merge's open write transaction
// nor its pending (`merging`) entry, which is exactly real `has_version`'s
// view during `pkg_preinst`: the old instance until the new one is
// published. Each connection is served on its own short-lived thread with
// a read and write timeout and a bounded request size, so a stuck or
// misbehaving client costs at most one idle thread for the timeout; a
// malformed request gets an `err` reply and the server keeps serving.
// The accept thread is not the merge thread.
//
// Consistency: one request makes several read calls (candidate list,
// `repository`, `IUSE`, `USE`), each its own snapshot. A commit landing
// between them can make one answer mix two committed states; the merge
// commits only at entry boundaries, and a phase asking while its own
// package's entry is being published cannot tell the order apart anyway.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The variable that carries the socket path to the children.
pub(crate) const IPC_VAR: &str = "PORTUALE_VDB_IPC";

/// The largest request line the server reads (the implicit IUSE of a real
/// profile is a few KiB).
const MAX_REQUEST: u64 = 1 << 20;
/// The largest reply line the client reads.
const MAX_REPLY: u64 = 1 << 20;
/// Per-connection read/write timeout in the server.
const SERVER_IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Read/write timeout in the client.
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(60);
/// Name prefix of the private socket directory.
const DIR_PREFIX: &str = "portuale-vdb-ipc-";
/// `sockaddr_un.sun_path` holds 108 bytes including the NUL.
const MAX_SOCKET_PATH: usize = 107;

/// A running server. Dropping it stops the accept thread and removes the
/// socket and its directory.
#[derive(Debug)]
pub(crate) struct Server {
    dir: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    /// Serve the database registered for `root` (`portage_vdb::for_root`)
    /// from a socket under `$TMPDIR`, or under `/tmp` when that path is
    /// too long for a socket address.
    pub(crate) fn start(root: &Path) -> std::io::Result<Server> {
        let tmp = std::env::temp_dir();
        match Server::start_in(&tmp, root) {
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput && tmp != Path::new("/tmp") => {
                Server::start_in(Path::new("/tmp"), root)
            }
            other => other,
        }
    }

    /// [`Server::start`] with the socket directory under `parent`.
    pub(crate) fn start_in(parent: &Path, root: &Path) -> std::io::Result<Server> {
        sweep_stale(parent);
        let dir = make_private_dir(parent)?;
        let socket = dir.join("s");
        if socket.as_os_str().len() > MAX_SOCKET_PATH {
            let _ = std::fs::remove_dir(&dir);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "socket path {} is too long for a Unix socket",
                    socket.display()
                ),
            ));
        }
        let listener = match UnixListener::bind(&socket) {
            Ok(l) => l,
            Err(e) => {
                let _ = std::fs::remove_dir(&dir);
                return Err(e);
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            let root = root.to_path_buf();
            std::thread::Builder::new()
                .name("vdb-ipc".into())
                .spawn(move || accept_loop(&listener, &root, &stop))
        };
        let thread = match thread {
            Ok(t) => t,
            Err(e) => {
                let _ = std::fs::remove_file(&socket);
                let _ = std::fs::remove_dir(&dir);
                return Err(e);
            }
        };
        cleanup::register(&dir);
        Ok(Server {
            dir,
            socket,
            stop,
            thread: Some(thread),
        })
    }

    /// The socket path (the value of `PORTUALE_VDB_IPC`).
    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking `accept`; it sees the flag and returns.
        if UnixStream::connect(&self.socket).is_ok()
            && let Some(t) = self.thread.take()
        {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.dir);
        cleanup::unregister(&self.dir);
    }
}

/// `mkdir <parent>/portuale-vdb-ipc-<pid>-<nanos>` with mode 0700 (never
/// an existing directory).
fn make_private_dir(parent: &Path) -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut last = None;
    for attempt in 0..16u128 {
        let dir = parent.join(format!(
            "{DIR_PREFIX}{}-{}",
            std::process::id(),
            nanos + attempt
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {
                // The umask can only remove bits; make sure of 0700.
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free socket directory name")))
}

/// Remove the socket directories of processes that no longer run (a
/// killed `mrg`). Only this user's directories are touched.
fn sweep_stale(parent: &Path) {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    // SAFETY: `geteuid` takes no arguments and returns a plain uid.
    let uid = unsafe { libc::geteuid() };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(DIR_PREFIX)) else {
            continue;
        };
        let Some(pid) = rest.split('-').next().and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid == std::process::id() || Path::new(&format!("/proc/{pid}")).exists() {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() && meta.uid() == uid {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn accept_loop(listener: &UnixListener, root: &Path, stop: &AtomicBool) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match stream {
            Ok(stream) => {
                let root = root.to_path_buf();
                let spawned = std::thread::Builder::new()
                    .name("vdb-ipc-conn".into())
                    .spawn(move || serve_connection(stream, &root));
                if let Err(e) = spawned {
                    eprintln!("mrg: {IPC_VAR}: cannot start a thread for a query: {e}");
                }
            }
            // EMFILE and friends: do not spin.
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Read one request line, write one reply line, close.
fn serve_connection(stream: UnixStream, root: &Path) {
    let _ = stream.set_read_timeout(Some(SERVER_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SERVER_IO_TIMEOUT));
    let mut line = Vec::new();
    let reply = match BufReader::new((&stream).take(MAX_REQUEST + 1)).read_until(b'\n', &mut line) {
        Err(e) => format!("err\tcannot read the request: {e}\n"),
        Ok(_) if line.len() as u64 > MAX_REQUEST => {
            format!("err\trequest longer than {MAX_REQUEST} bytes\n")
        }
        Ok(_) if line.last() != Some(&b'\n') => "err\trequest not terminated by a newline\n".into(),
        Ok(_) => match std::str::from_utf8(&line[..line.len() - 1]) {
            Ok(text) => answer(text, root),
            Err(_) => "err\trequest is not UTF-8\n".into(),
        },
    };
    let _ = (&stream).write_all(reply.as_bytes());
}

/// The reply line for one request line (without its newline).
pub(crate) fn answer(request: &str, root: &Path) -> String {
    let fields: Vec<&str> = request.split('\t').collect();
    let check_root = |eroot: &str| -> Result<(), String> {
        if crate::portageq::same_root(Path::new(eroot), root) {
            Ok(())
        } else {
            Err(format!(
                "this pipe serves the database of {}, not {eroot}",
                root.display()
            ))
        }
    };
    let result = match fields.as_slice() {
        ["match", eroot, atom, unevaluated, implicit] => check_root(eroot).and_then(|()| {
            if portage_dep::parse_atom(atom).is_none() {
                return Err(format!("invalid atom: {atom:?}"));
            }
            let implicit: HashSet<String> =
                implicit.split_whitespace().map(str::to_string).collect();
            let unevaluated = Some(*unevaluated).filter(|u| !u.is_empty());
            Ok(
                portage_repo::best_installed_match(root, atom, unevaluated, &|| implicit.clone())
                    .map_or_else(|| "none\n".to_string(), |v| format!("ok\t{v}\n")),
            )
        }),
        ["categories", eroot, pn] => check_root(eroot).and_then(|()| {
            if pn.is_empty() {
                return Err("empty package name".into());
            }
            Ok(format!(
                "ok\t{}\n",
                crate::portageq::installed_categories_of(root, pn).join(" ")
            ))
        }),
        [cmd @ ("match" | "categories"), ..] => {
            Err(format!("{cmd}: wrong number of fields ({})", fields.len()))
        }
        [cmd, ..] => Err(format!("unknown request {cmd:?}")),
        [] => Err("empty request".into()),
    };
    result.unwrap_or_else(|e| format!("err\t{}\n", e.replace(['\t', '\n'], " ")))
}

// ---------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------

/// A non-error reply.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Ok(String),
    None,
}

/// Send one request and read its reply. Every failure (cannot connect,
/// timeout, `err` reply, malformed reply) is an `Err` naming the cause.
pub(crate) fn request(socket: &Path, fields: &[&str]) -> Result<Reply, String> {
    if fields.iter().any(|f| f.contains(['\t', '\n'])) {
        return Err("a query field holds a TAB or a newline".into());
    }
    let mut stream = UnixStream::connect(socket).map_err(|e| {
        format!(
            "cannot reach the parent mrg over {IPC_VAR}={}: {e}",
            socket.display()
        )
    })?;
    let _ = stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT));
    let io = |e: std::io::Error| format!("query over {IPC_VAR}={} failed: {e}", socket.display());
    stream
        .write_all(format!("{}\n", fields.join("\t")).as_bytes())
        .map_err(io)?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut line = Vec::new();
    BufReader::new((&stream).take(MAX_REPLY))
        .read_until(b'\n', &mut line)
        .map_err(io)?;
    let Some(text) = line
        .strip_suffix(b"\n")
        .and_then(|l| std::str::from_utf8(l).ok())
    else {
        return Err(format!(
            "malformed or truncated reply over {IPC_VAR}={}",
            socket.display()
        ));
    };
    if text == "none" {
        Ok(Reply::None)
    } else if let Some(v) = text.strip_prefix("ok\t") {
        Ok(Reply::Ok(v.to_string()))
    } else if let Some(msg) = text.strip_prefix("err\t") {
        Err(format!("the parent mrg refused the query: {msg}"))
    } else {
        Err(format!(
            "malformed reply over {IPC_VAR}={}: {text:?}",
            socket.display()
        ))
    }
}

/// `best_installed_match` in the parent. `implicit_iuse` is only called
/// when the atom (or its unevaluated original) has USE dependencies.
pub(crate) fn query_match(
    socket: &Path,
    eroot: &Path,
    atom: &str,
    unevaluated: Option<&str>,
    implicit_iuse: &dyn Fn() -> HashSet<String>,
) -> Result<Option<String>, String> {
    let has_use_deps = std::iter::once(atom)
        .chain(unevaluated)
        .filter_map(portage_dep::parse_atom)
        .any(|a| a.use_deps.is_some_and(|d| !d.is_empty()));
    let implicit = if has_use_deps {
        let mut flags: Vec<String> = implicit_iuse().into_iter().collect();
        flags.sort();
        flags.join(" ")
    } else {
        String::new()
    };
    let eroot = eroot.to_string_lossy();
    match request(
        socket,
        &["match", &eroot, atom, unevaluated.unwrap_or(""), &implicit],
    )? {
        Reply::Ok(v) if !v.is_empty() => Ok(Some(v)),
        Reply::Ok(_) => Err(format!(
            "malformed reply over {IPC_VAR}={}: empty version",
            socket.display()
        )),
        Reply::None => Ok(None),
    }
}

/// `installed_categories_of` in the parent.
pub(crate) fn query_categories(
    socket: &Path,
    eroot: &Path,
    pn: &str,
) -> Result<Vec<String>, String> {
    let eroot = eroot.to_string_lossy();
    match request(socket, &["categories", &eroot, pn])? {
        Reply::Ok(v) => Ok(v.split_whitespace().map(str::to_string).collect()),
        Reply::None => Err(format!(
            "malformed reply over {IPC_VAR}={}: none",
            socket.display()
        )),
    }
}

// ---------------------------------------------------------------------
// Cleanup on panic (release builds abort, so `Drop` never runs)
// ---------------------------------------------------------------------

mod cleanup {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static LIVE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub(super) fn register(dir: &Path) {
        if let Ok(mut live) = LIVE.lock() {
            live.push(dir.to_path_buf());
        }
        install_hook();
    }

    pub(super) fn unregister(dir: &Path) {
        if let Ok(mut live) = LIVE.lock() {
            live.retain(|d| d != dir);
        }
    }

    /// Unit tests unwind (and run servers in parallel threads, where one
    /// test's expected panic must not remove another's socket): there
    /// `Drop` is enough, so no hook.
    #[cfg(not(test))]
    fn install_hook() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if let Ok(live) = LIVE.try_lock() {
                    for dir in live.iter() {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
                previous(info);
            }));
        });
    }

    #[cfg(test)]
    fn install_hook() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "vdb-redb")]
    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .unwrap()
    }

    /// Send raw bytes and read the raw reply.
    #[cfg(feature = "vdb-redb")]
    fn raw(socket: &Path, bytes: &[u8]) -> String {
        let mut s = UnixStream::connect(socket).unwrap();
        s.write_all(bytes).unwrap();
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    /// (a) The server answers the S6.2 query set exactly as the direct
    /// lookup does, on a redb copy of the fixture VDB registered under its
    /// own root key (the shape `mrg` has: the handle open read-write in
    /// this process, the queries on another thread). (c) The socket and
    /// its directory are gone after the drop. (d) Garbage gets an `err`
    /// reply and the server keeps serving.
    #[cfg(feature = "vdb-redb")]
    #[test]
    fn server_answers_like_the_direct_match_and_survives_garbage() {
        let fx = fixtures();
        let tmp = portage_util::TempDir::new("vdb_ipc");
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let db = std::sync::Arc::new(portage_vdb::RedbDb::open(tmp.join("vdb.redb")).unwrap());
        portage_vdb::copy_all(&portage_vdb::FilesDb::new(&fx), db.as_ref(), false).unwrap();
        portage_vdb::register(&root, db.clone());
        // Held read-write by this process, as under mrg.
        assert!(matches!(
            portage_vdb::RedbDb::open_readonly(tmp.join("vdb.redb")),
            Err(portage_vdb::Error::Busy { .. })
        ));

        let server = Server::start_in(&tmp, &root).unwrap();
        let socket = server.socket().to_path_buf();
        let dir = socket.parent().unwrap().to_path_buf();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );

        // `root` has no files VDB at all: a hit can only come from redb.
        assert!(!root.join("var/db/pkg").exists());
        let implicit = || HashSet::new();
        let cases: &[(&str, Option<&str>)] = &[
            ("dev-libs/keeper", None),
            ("dev-libs/nonexistent", None),
            (">=dev-libs/dualslotpkg-1.5", None),
            (">dev-libs/dualslotpkg-2.0", None),
            ("=dev-libs/unmergepkg-1.0", None),
            ("~dev-libs/unmergepkg-2.0", None),
            ("=dev-libs/unmergepkg-1*", None),
            ("dev-libs/dualslotpkg:1", None),
            ("dev-libs/dualslotpkg:3", None),
            ("dev-libs/r25lib:0/1", None),
            ("dev-libs/r25lib:0/2", None),
            ("dev-libs/newrepopkg::oldrepo", None),
            ("dev-libs/newrepopkg::testrepo", None),
            ("dev-libs/infoinstpkg[alpha]", None),
            ("dev-libs/infoinstpkg[beta]", None),
            ("dev-libs/infoinstpkg[-beta]", None),
            ("dev-libs/infoinstpkg[nosuchflag]", None),
            ("dev-libs/infoinstpkg[nosuchflag(+)]", None),
            // `[beta?]` evaluated with USE="" (the child's job), and the
            // unevaluated original sent along.
            ("dev-libs/infoinstpkg", Some("dev-libs/infoinstpkg[beta?]")),
            (
                "dev-libs/infoinstpkg",
                Some("dev-libs/infoinstpkg[nosuchflag?]"),
            ),
            ("dev-libs/unmergepkg", None),
            ("<dev-libs/unmergepkg-2", None),
            ("dev-libs/blk0y", None),
        ];
        let mut hits = 0;
        for (atom, unevaluated) in cases {
            let direct = portage_repo::best_installed_match(&root, atom, *unevaluated, &implicit);
            let piped = query_match(&socket, &root, atom, *unevaluated, &implicit).unwrap();
            assert_eq!(piped, direct, "{atom} {unevaluated:?}");
            hits += usize::from(direct.is_some());
        }
        assert!(hits > 10, "the redb copy answers (hits: {hits})");
        assert_eq!(
            query_match(&socket, &root, "dev-libs/unmergepkg", None, &implicit).unwrap(),
            Some("2.0".into())
        );
        assert_eq!(
            query_match(&socket, &root, "dev-libs/nonexistent", None, &implicit).unwrap(),
            None
        );
        // dep_expand's lookup.
        assert_eq!(
            query_categories(&socket, &root, "keeper").unwrap(),
            crate::portageq::installed_categories_of(&root, "keeper")
        );
        assert_eq!(
            query_categories(&socket, &root, "keeper").unwrap(),
            ["dev-libs"]
        );
        assert!(
            query_categories(&socket, &root, "no-such-pn")
                .unwrap()
                .is_empty()
        );
        // Another spelling of the root is the same root; another root is
        // refused.
        assert!(query_match(&socket, &root.join("."), "dev-libs/keeper", None, &implicit).is_ok());
        let e = query_match(&socket, &fx, "dev-libs/keeper", None, &implicit).unwrap_err();
        assert!(e.contains("this pipe serves"), "{e}");

        // (d) Garbage: an `err` reply each time, and the server still
        // answers afterwards.
        for bad in [
            &b"hello\n"[..],
            b"match\tonly-two\n",
            b"match\t/x\tnot an atom\t\t\n",
            b"\xff\xfe\n",
            b"match\t/\tdev-libs/keeper\t\t",
            b"\n",
        ] {
            let reply = raw(&socket, bad);
            assert!(reply.starts_with("err\t"), "{bad:?}: {reply:?}");
            assert!(reply.ends_with('\n'), "{bad:?}: {reply:?}");
        }
        let reply = raw(
            &socket,
            format!("match\t{}\tdev-libs/[[[\t\t\n", root.display()).as_bytes(),
        );
        assert!(reply.starts_with("err\tinvalid atom"), "{reply:?}");
        // Over-long request: refused once the bound is crossed. (Exactly
        // one byte over, so the server has read everything the client
        // sent: closing with unread input would reset the connection
        // before the client reads the reply.)
        let long = vec![b'a'; MAX_REQUEST as usize + 1];
        let reply = raw(&socket, &long);
        assert!(reply.starts_with("err\trequest longer"), "{reply:?}");
        // A client that connects and says nothing does not stop others.
        let idle = UnixStream::connect(&socket).unwrap();
        assert_eq!(
            query_match(&socket, &root, "dev-libs/keeper", None, &implicit).unwrap(),
            Some("1.0".into())
        );
        drop(idle);
        // The client refuses fields that would break the framing.
        assert!(request(&socket, &["match", "a\tb"]).is_err());

        // (c) Cleanup.
        drop(server);
        assert!(!socket.exists());
        assert!(!dir.exists());
        let e = query_match(&socket, &root, "dev-libs/keeper", None, &implicit).unwrap_err();
        assert!(e.contains("cannot reach the parent mrg"), "{e}");
        portage_vdb::register(&root, std::sync::Arc::new(portage_vdb::FilesDb::new(&root)));
    }

    /// The stale sweep removes a dead pid's directory and keeps a live
    /// one's.
    #[test]
    fn stale_directories_of_dead_processes_are_swept() {
        let tmp = portage_util::TempDir::new("vdb_ipc_sweep");
        let dead = tmp.join(format!("{DIR_PREFIX}{}-1", u32::MAX - 1));
        let live = tmp.join(format!("{DIR_PREFIX}{}-1", std::process::id()));
        std::fs::create_dir_all(dead.join("x")).unwrap();
        std::fs::create_dir_all(&live).unwrap();
        sweep_stale(&tmp);
        assert!(!dead.exists());
        assert!(live.exists());
    }

    /// A socket path longer than `sun_path` is refused before binding,
    /// leaving nothing behind.
    #[test]
    fn too_long_socket_path_is_refused() {
        let tmp = portage_util::TempDir::new("vdb_ipc_long");
        let deep = tmp.join("d".repeat(120));
        std::fs::create_dir_all(&deep).unwrap();
        let e = Server::start_in(&deep, Path::new("/")).err().unwrap();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read_dir(&deep).unwrap().count(), 0);
    }
}
