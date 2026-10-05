// The parent pipe (`PORTUALE_VDB_IPC`): installed-package lookups while
// `mrg` holds a redb database read-write.
//
// redb allows one writer process per database file, so the native
// `portuale portageq` that an ebuild phase runs for `has_version` /
// `best_version` cannot open it mid-merge. The parent `mrg` answers over a
// Unix domain socket and exports its path in `PORTUALE_VDB_IPC`.
// Portage: ebuild-ipc (a phase asks the parent emerge over a FIFO).
//
// One connection, one request line, one reply line; fields are separated
// by TAB, and no field may hold a TAB or a newline.
//
// ```text
// match\t<eroot>\t<atom>\t<unevaluated atom or empty>\t<implicit IUSE>\n
//     -> ok\t<version>\n          (the highest matching installed version)
//     -> none\n                   (nothing matches)
// categories\t<eroot>\t<pn>\n
//     -> ok\t<cat> <cat> ...\n    (categories with <pn> installed)
// anything else, or a request the server cannot answer
//     -> err\t<message>\n
// ```
//
// `<eroot>` must name the ROOT the server serves; the implicit IUSE is sent
// only when the atom has USE dependencies.
//
// `mrg` starts the server (redb, not `--pretend`) before `pretend::run` and
// keeps the returned [`Server`] alive for the whole run. Dropping it stops
// the accept thread and removes the socket and its directory.
//
// The server thread only reads, and each connection is served on its own
// short-lived thread with timeouts, so a stuck client never blocks the
// merge. A read sees the last committed state, never the merge's open write
// transaction nor its pending entry.

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

/// The largest request line the server reads (the implicit IUSE of a
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

    pub(super) fn unregister(dir: &Path) {
        if let Ok(mut live) = LIVE.lock() {
            live.retain(|d| d != dir);
        }
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
}
