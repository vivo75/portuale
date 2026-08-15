//! `FilesDb`'s write side: the merge's `-MERGING-<pf>` entry, the
//! `counter` store, the preserved-libs registry and the config-protect
//! memory.
//!
//! The code `portuale::ebuild_merge` ran before, moved without changing a
//! syscall. Nothing is atomic and nothing is locked: [`FilesTxn`] applies
//! every call when it is made and [`WriteTxn::commit`] is only an ordering
//! point.
//!
//! The on-disk formats of the registry and the config memory live here
//! too, because the database backends store the parsed values and a
//! converter must read and write the `files` bytes: the registry is
//! Portage's `PreservedLibsRegistry.store()` JSON, the config memory its
//! `grabdict`/`writedict` `"path value\n"` lines.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::files::{is_absent, parse_metadata_text};
use crate::{
    ConfigMemory, Counter, EntryKey, Error, METADATA_FILE_FIELDS, METADATA_FILE_FORMAT_VERSION,
    MetadataStamp, PreservedLibs, PreservedLibsEntry, Result, World, WorldSets,
};

/// The counter store under a root.
// Portage: lib/portage/const.py, CACHE_PATH
const COUNTER_PATH: &str = "var/cache/edb/counter";
/// The preserved-libs registry under a root.
const PRESERVED_LIBS_PATH: &str = "var/lib/portage/preserved_libs_registry";
/// The config-protect memory under a root.
const CONFIG_MEMORY_PATH: &str = "var/lib/portage/config";
/// The world file under a root.
// Portage: lib/portage/const.py, WORLD_FILE
const WORLD_PATH: &str = "var/lib/portage/world";
/// The world-sets file under a root.
const WORLD_SETS_PATH: &str = "var/lib/portage/world_sets";

fn mtime_ns_of(st: &fs::Metadata) -> i128 {
    st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128
}

/// Set the access and modification time of a file or directory to `ns`
/// (nanoseconds since the epoch) with `futimens` on a read-only
/// descriptor. Never touches the directory entry of `path` itself.
fn set_mtime_ns(path: &Path, ns: i128) -> Result<()> {
    let secs = ns.div_euclid(1_000_000_000);
    let nanos = ns.rem_euclid(1_000_000_000) as u32;
    let bad = || Error::Invalid(format!("{}: mtime {ns} out of range", path.display()));
    let whole = u64::try_from(secs.unsigned_abs()).map_err(|_| bad())?;
    let t = if secs >= 0 {
        UNIX_EPOCH.checked_add(Duration::new(whole, nanos))
    } else {
        UNIX_EPOCH
            .checked_sub(Duration::new(whole, 0))
            .and_then(|t| t.checked_add(Duration::new(0, nanos)))
    }
    .ok_or_else(bad)?;
    let f = fs::File::open(path).map_err(|e| Error::io(path, e))?;
    f.set_times(fs::FileTimes::new().set_accessed(t).set_modified(t))
        .map_err(|e| Error::io(path, e))
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o7777))
        .map_err(|e| Error::io(path, e))
}

/// `body` with every `#dir_mtime=` line removed and a final newline.
fn without_stamp(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    for line in body.split_inclusive(|&b| b == b'\n') {
        if !line.starts_with(b"#dir_mtime=") {
            out.extend_from_slice(line);
        }
    }
    if !out.is_empty() && out.last() != Some(&b'\n') {
        out.push(b'\n');
    }
    out
}

/// `std::fs::create_dir_all` of `path`'s parent, error on the parent.
/// Portage's `lockdir(<vdb>)`: a blocking exclusive `flock(2)` on
/// `<parent>/.<basename>.portage_lockfile`, unlinked again on release.
/// Because a waiter may hold an fd on the removed inode, the acquirer
/// re-checks that the path still names the file it locked and starts over
/// when it does not. Scoped here to one counter tick.
// Portage: lib/portage/locks.py, lockdir
struct VdbLock {
    _file: fs::File,
    path: PathBuf,
}

impl VdbLock {
    fn acquire(vdb: &Path) -> Result<Self> {
        use std::os::unix::io::AsRawFd as _;
        let parent = vdb.parent().unwrap_or_else(|| Path::new("."));
        let base = vdb
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let path = parent.join(format!(".{base}.portage_lockfile"));
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        loop {
            let file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| Error::io(&path, e))?;
            // SAFETY: `flock` takes only the fd, which `file` keeps valid.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(Error::io(path, std::io::Error::last_os_error()));
            }
            let (Ok(held), Ok(named)) = (file.metadata(), fs::metadata(&path)) else {
                continue; // unlinked by the previous holder: start over
            };
            if held.ino() == named.ino() && held.dev() == named.dev() {
                return Ok(Self { _file: file, path });
            }
        }
    }
}

impl Drop for VdbLock {
    /// Unlink while still holding the lock, then the fd closes and the
    /// lock is released.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Portage's `write_atomic`: follow symlinks, write a temporary file in
/// the same directory, copy the mode of the file it replaces, then
/// `rename` over the target.
// Portage: lib/portage/util/__init__.py, atomic_ofstream
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write as _;
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    create_parent(&target)?;
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let base = target
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut n = 0u32;
    let (tmp, mut file) = loop {
        let tmp = parent.join(format!("{base}.{}.{n}", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(Error::io(tmp, e)),
        }
    };
    let result = (|| {
        file.write_all(data)?;
        if let Ok(st) = fs::metadata(&target) {
            fs::set_permissions(&tmp, fs::Permissions::from_mode(st.mode() & 0o7777))?;
        }
        drop(file);
        fs::rename(&tmp, &target)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(Error::io(target, e));
    }
    Ok(())
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    Ok(())
}

/// A minimal JSON string-literal reader for the registry: narrow by
/// design, not a general parser.
fn parse_json_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{C}'),
                'u' => {
                    let hex: String = (0..4).map(|_| chars.next()).collect::<Option<String>>()?;
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
}

fn skip_json_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
}

fn parse_json_string_array(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> Option<Vec<String>> {
    skip_json_ws(chars);
    if chars.next()? != '[' {
        return None;
    }
    let mut out = Vec::new();
    skip_json_ws(chars);
    if chars.peek() == Some(&']') {
        chars.next();
        return Some(out);
    }
    loop {
        skip_json_ws(chars);
        out.push(parse_json_string(chars)?);
        skip_json_ws(chars);
        match chars.next()? {
            ',' => continue,
            ']' => return Some(out),
            _ => return None,
        }
    }
}

/// Parses exactly the shape Portage's `PreservedLibsRegistry.store()`
/// writes: `{"cp:slot": [cpv, counter, [paths...]], ...}`. `None` on any
/// deviation; readers treat that like a missing file.
pub fn parse_preserved_libs(text: &str) -> Option<BTreeMap<String, PreservedLibsEntry>> {
    let mut chars = text.chars().peekable();
    let mut entries = BTreeMap::new();
    skip_json_ws(&mut chars);
    if chars.next()? != '{' {
        return None;
    }
    skip_json_ws(&mut chars);
    if chars.peek() == Some(&'}') {
        chars.next();
        return Some(entries);
    }
    loop {
        skip_json_ws(&mut chars);
        let key = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ':' {
            return None;
        }
        skip_json_ws(&mut chars);
        if chars.next()? != '[' {
            return None;
        }
        skip_json_ws(&mut chars);
        let cpv = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ',' {
            return None;
        }
        skip_json_ws(&mut chars);
        let counter = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ',' {
            return None;
        }
        let paths = parse_json_string_array(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ']' {
            return None;
        }
        entries.insert(
            key,
            PreservedLibsEntry {
                cpv,
                counter,
                paths,
            },
        );
        skip_json_ws(&mut chars);
        match chars.next()? {
            ',' => continue,
            '}' => return Some(entries),
            _ => return None,
        }
    }
}

fn json_quote(s: &str) -> String {
    // Portage's `json.dumps(..., ensure_ascii=False)` quoting.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Portage's `store()` JSON layout (sorted keys); `{}` when empty, no
/// trailing newline.
pub fn format_preserved_libs(entries: &BTreeMap<String, PreservedLibsEntry>) -> String {
    if entries.is_empty() {
        return String::from("{}");
    }
    let mut out = String::from("{\n");
    let n = entries.len();
    for (i, (key, entry)) in entries.iter().enumerate() {
        out.push_str(&format!("\t{}: [\n", json_quote(key)));
        out.push_str(&format!("\t\t{},\n", json_quote(&entry.cpv)));
        out.push_str(&format!("\t\t{},\n", json_quote(&entry.counter)));
        if entry.paths.is_empty() {
            out.push_str("\t\t[]\n");
        } else {
            out.push_str("\t\t[\n");
            for (j, p) in entry.paths.iter().enumerate() {
                out.push_str(&format!("\t\t\t{}", json_quote(p)));
                out.push_str(if j + 1 < entry.paths.len() {
                    ",\n"
                } else {
                    "\n"
                });
            }
            out.push_str("\t\t]\n");
        }
        out.push_str("\t]");
        out.push_str(if i + 1 < n { ",\n" } else { "\n" });
    }
    out.push('}');
    out
}
