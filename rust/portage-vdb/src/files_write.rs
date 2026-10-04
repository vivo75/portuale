//! `FilesDb`'s write side (S1.4): the merge's `-MERGING-<pf>` entry, the
//! `counter` store, the preserved-libs registry and the config-protect
//! memory.
//!
//! Every function here is the code `portuale::ebuild_merge` ran before
//! S1.4, moved without changing a syscall: the same paths (joined, never
//! canonicalised), the same `std::fs` calls in the same order, the same
//! error texts (`"<path>: <io error>"` via [`Error::Io`]). Nothing is
//! atomic and nothing is locked, exactly as before: [`FilesTxn`] applies
//! every call when it is made and [`WriteTxn::commit`] is only an ordering
//! point (module doc of the crate, item 20).
//!
//! The on-disk formats of the registry and the config memory live here
//! too, because the database backends store the parsed values and a
//! converter must read and write the `files` bytes (S2.2): the registry is
//! real `PreservedLibsRegistry.store()`'s `json.dumps(..., ensure_ascii=
//! False, indent="\t", sort_keys=True)`, the config memory real
//! `grabdict`/`writedict`'s `"path value\n"` lines.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use crate::files::{FilesDb, is_absent, todo_step};
use crate::{
    ConfigMemory, Counter, EntryImage, EntryKey, Error, METADATA_FILE_FIELDS,
    METADATA_FILE_FORMAT_VERSION, PreservedLibs, PreservedLibsEntry, Result, World, WorldSets,
    WriteTxn,
};

/// Real `const.py` `CACHE_PATH` + `vartree.py:420`: the counter store
/// under a root.
const COUNTER_PATH: &str = "var/cache/edb/counter";
/// Real `PRIVATE_PATH` + `PreservedLibsRegistry`'s hardcoded file name.
const PRESERVED_LIBS_PATH: &str = "var/lib/portage/preserved_libs_registry";
/// Real `PRIVATE_PATH` + `CONFIG_MEMORY_FILE` (`vardbapi._conf_mem_file`).
const CONFIG_MEMORY_PATH: &str = "var/lib/portage/config";
/// Real `const.py` `WORLD_FILE` under a root.
const WORLD_PATH: &str = "var/lib/portage/world";
/// Real `const.py` `WORLD_SETS_FILE` under a root.
const WORLD_SETS_PATH: &str = "var/lib/portage/world_sets";

impl FilesDb {
    /// `<vdb>/<category>/-MERGING-<pf>`, no I/O.
    pub(crate) fn pending_dir(&self, key: &EntryKey) -> PathBuf {
        self.vdb_path().join(&key.category).join(format!(
            "{}{}",
            portage_util::MERGING_IDENTIFIER,
            key.pf
        ))
    }

    /// `<root>/<rel>` for a D4 store; [`Error::Unsupported`] on a bare
    /// VDB directory ([`FilesDb::open_vdb_dir`] has no stores).
    fn store_path(&self, rel: &str) -> Result<PathBuf> {
        match self.root() {
            Some(root) => Ok(root.join(rel)),
            None => Err(Error::Unsupported(format!(
                "{rel}: a bare VDB directory ({}) has no root to hold it",
                self.vdb_path().display()
            ))),
        }
    }

    /// `<root>/<rel>` for a D4 store read; `None` on a bare VDB directory
    /// (the read then sees an empty store).
    fn store_path_opt(&self, rel: &str) -> Option<PathBuf> {
        self.root().map(|root| root.join(rel))
    }

    /// [`InstalledDb::read_pending_file`](crate::InstalledDb::read_pending_file):
    /// one `open` of `-MERGING-<pf>/<name>`, no existence `stat`. A
    /// missing pending entry and a missing file are both `Ok(None)`.
    pub(crate) fn read_pending(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        let path = self.pending_dir(key).join(name);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if is_absent(&e) => Ok(None),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// The `counter` file as `next_counter` reads it: `None` when it is
    /// missing, unreadable or not an integer after trimming.
    pub(crate) fn read_counter(&self) -> Option<Counter> {
        let path = self.store_path_opt(COUNTER_PATH)?;
        fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .map(Counter)
    }

    /// Moved from `pretend::read_world_atoms`: one `read_to_string` of
    /// `<root>/var/lib/portage/world`; a missing file (`NotFound` only) is
    /// an empty world, any other failure is [`Error::Io`]. Lines are
    /// trimmed; blank, `#` and `@` lines are dropped.
    pub(crate) fn read_world(&self) -> Result<World> {
        let Some(path) = self.store_path_opt(WORLD_PATH) else {
            return Ok(World::default());
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(World::default()),
            Err(e) => return Err(Error::io(path, e)),
        };
        Ok(World {
            atoms: text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
                .map(String::from)
                .collect(),
        })
    }

    /// Moved from `pretend::read_world_sets`: as [`FilesDb::read_world`],
    /// keeping only `@` lines and stripping every leading `@`.
    pub(crate) fn read_world_sets(&self) -> Result<WorldSets> {
        let Some(path) = self.store_path_opt(WORLD_SETS_PATH) else {
            return Ok(WorldSets::default());
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WorldSets::default());
            }
            Err(e) => return Err(Error::io(path, e)),
        };
        Ok(WorldSets {
            sets: text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && l.starts_with('@'))
                .map(|l| l.trim_start_matches('@').to_string())
                .collect(),
        })
    }

    /// The registry as read (moved from `ebuild_merge::read_plib_registry`):
    /// one read; a missing, unreadable, non-UTF-8 or unparsable file is an
    /// empty registry (real `load()`'s graceful degrade). Not pruned: the
    /// `lstat`-based `pruneNonExisting` stays with the caller (N6).
    pub(crate) fn read_preserved_libs(&self) -> PreservedLibs {
        let entries = self
            .store_path_opt(PRESERVED_LIBS_PATH)
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| parse_preserved_libs(&text))
            .unwrap_or_default();
        PreservedLibs {
            loaded: entries.clone(),
            entries,
        }
    }

    /// The config memory as read (moved from `ebuild_merge::read_cfgfiledict`):
    /// one read; missing or unreadable is empty.
    pub(crate) fn read_config_memory(&self) -> ConfigMemory {
        let mut entries = BTreeMap::new();
        if let Some(text) = self
            .store_path_opt(CONFIG_MEMORY_PATH)
            .and_then(|path| fs::read_to_string(path).ok())
        {
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    entries.insert(key.to_string(), value.to_string());
                }
            }
        }
        ConfigMemory { entries }
    }
}

/// `files`' [`WriteTxn`]: every call goes straight to the filesystem, in
/// call order; [`WriteTxn::commit`] does nothing and dropping the
/// transaction undoes nothing.
pub(crate) struct FilesTxn<'a> {
    pub(crate) db: &'a FilesDb,
    /// `var/lib/portage` was already created by an earlier world write of
    /// this transaction, so the next one skips its `create_dir_all` (the
    /// `emerge --deselect` rewrite of both files made it once).
    portage_dir_made: bool,
}

impl<'a> FilesTxn<'a> {
    pub(crate) fn new(db: &'a FilesDb) -> Self {
        FilesTxn {
            db,
            portage_dir_made: false,
        }
    }

    /// The shared body of `set_world` / `set_world_sets`, moved from
    /// `pretend`: `create_dir_all` of the parent (once per transaction),
    /// then the lines joined by `\n` with a trailing `\n` unless there
    /// are none, one plain `std::fs::write`.
    fn write_world_store(&mut self, rel: &str, lines: Vec<String>) -> Result<()> {
        let path = self.db.store_path(rel)?;
        if !self.portage_dir_made {
            create_parent(&path)?;
            self.portage_dir_made = true;
        }
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        fs::write(&path, body).map_err(|e| Error::io(path, e))
    }
}

/// `std::fs::create_dir_all` of `path`'s parent, error on the parent.
fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    Ok(())
}

impl WriteTxn for FilesTxn<'_> {
    /// Moved from `ebuild_merge::create_vdb_tmp` (real `treewalk()`'s
    /// `self.dbdir = self.dbtmpdir; self.delete(); ensure_dirs(...)`):
    /// `stat` the pending dir, `remove_dir_all` a stale one a killed merge
    /// left, then `create_dir_all` it (and the category dir). The category
    /// dir of a removed stale entry is not `rmdir`ed (real does; S0.1 row
    /// 3).
    fn begin_entry(&mut self, key: &EntryKey) -> Result<()> {
        let tmp_dir = self.db.pending_dir(key);
        if tmp_dir.exists() {
            fs::remove_dir_all(&tmp_dir).map_err(|e| Error::io(&tmp_dir, e))?;
        }
        fs::create_dir_all(&tmp_dir).map_err(|e| Error::io(&tmp_dir, e))?;
        Ok(())
    }

    /// `std::fs::write` into the pending dir (truncate in place, mode from
    /// the umask), as `populate_vdb_tmp` / `write_vdb_tmp_contents` did.
    fn put_entry_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        let path = self.db.pending_dir(key).join(name);
        fs::write(&path, data).map_err(|e| Error::io(path, e))
    }

    /// `std::fs::copy(src, <pending>/<name>)` (mode copied, mtime not), as
    /// `populate_vdb_tmp` did. The error names `src`, like before.
    fn copy_entry_file(&mut self, key: &EntryKey, name: &str, src: &Path) -> Result<()> {
        fs::copy(src, self.db.pending_dir(key).join(name)).map_err(|e| Error::io(src, e))?;
        Ok(())
    }

    /// Moved from `ebuild_merge::next_counter`: read the `counter` file
    /// (`-1` when missing or unparsable), add one, `create_dir_all` its
    /// parent, `std::fs::write` the number with no newline. **No lock and
    /// no max over the entries' `COUNTER`s**, unlike real
    /// `counter_tick_core` (`vartree.py:1304-1397`); plan §0.7 keeps that
    /// in S1 and files it as a residue.
    fn next_counter(&mut self) -> Result<Counter> {
        let counter_path = self.db.store_path(COUNTER_PATH)?;
        let previous = self.db.read_counter().map_or(-1, |c| c.0);
        let next = previous + 1;
        create_parent(&counter_path)?;
        fs::write(&counter_path, next.to_string()).map_err(|e| Error::io(&counter_path, e))?;
        Ok(Counter(next))
    }

    /// Moved from `ebuild_merge::write_consolidated_metadata_file` (real
    /// `_write_metadata_file` + `_stamp_metadata_file`, `vartree.py:
    /// 188-229`, `not delete_individual`): every [`METADATA_FILE_FIELDS`]
    /// file in the pending dir that reads as UTF-8, whitespace-normalised,
    /// sorted, under `#format=1`, written with `std::fs::write`; then a
    /// `stat` of the dir and `#dir_mtime=<st_mtime_ns>` appended in a
    /// second open. No-op when no field file exists.
    fn seal_entry(&mut self, key: &EntryKey) -> Result<()> {
        let dbdir = self.db.pending_dir(key);
        let mut data: Vec<(String, String)> = Vec::new();
        for &field in METADATA_FILE_FIELDS {
            let path = dbdir.join(field);
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            data.push((
                field.to_string(),
                raw.split_whitespace().collect::<Vec<_>>().join(" "),
            ));
        }
        if data.is_empty() {
            return Ok(());
        }
        data.sort();

        let metadata_path = dbdir.join("metadata");
        let mut body = format!("#format={METADATA_FILE_FORMAT_VERSION}\n");
        for (k, v) in &data {
            body.push_str(&format!("{k}={v}\n"));
        }
        fs::write(&metadata_path, &body).map_err(|e| Error::io(&metadata_path, e))?;

        // Real appends `#dir_mtime=` in a separate `open(..., "a")` step,
        // *after* the body is on disk, so `st_mtime_ns` reflects the body
        // write (the last dir change) and the append itself -- no new dir
        // entry -- leaves it untouched.
        let st = fs::metadata(&dbdir).map_err(|e| Error::io(&dbdir, e))?;
        let dir_mtime_ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&metadata_path)
            .map_err(|e| Error::io(&metadata_path, e))?;
        use std::io::Write as _;
        writeln!(f, "#dir_mtime={dir_mtime_ns}").map_err(|e| Error::io(&metadata_path, e))?;
        Ok(())
    }

    /// Moved from `ebuild_merge::publish_vdb_tmp` (real `self.dbdir =
    /// self.dbpkgdir; self.delete(); _movefile(dbtmpdir, dbpkgdir)`):
    /// `stat` the live `<pf>`, `remove_dir_all` it when present (the
    /// same-`pf` reinstall), then `rename` the pending dir onto it. Both
    /// sit in the same category dir, so the rename is atomic and keeps the
    /// pending dir's own mtime (and so the `metadata` stamp). Rename
    /// errors name the live path, like before.
    fn finish_entry(&mut self, key: &EntryKey) -> Result<()> {
        let tmp_dir = self.db.pending_dir(key);
        let final_dir = self.db.vdb_path().join(&key.category).join(&key.pf);
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir).map_err(|e| Error::io(&final_dir, e))?;
        }
        fs::rename(&tmp_dir, &final_dir).map_err(|e| Error::io(&final_dir, e))?;
        Ok(())
    }

    fn insert_entry(&mut self, _image: &EntryImage) -> Result<()> {
        todo_step("insert_entry", "S2.2")
    }

    /// Moved from `ebuild_unmerge::delete_vdb_dir` (real `dblink.delete()`):
    /// `remove_dir_all` of the live entry (its error is returned), then a
    /// best-effort `remove_dir` of the category directory, ignored when
    /// another entry still lives there (N14).
    fn delete_entry(&mut self, key: &EntryKey) -> Result<()> {
        let dir = self.db.vdb_path().join(&key.category).join(&key.pf);
        fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        if let Some(cat_dir) = dir.parent() {
            let _ = fs::remove_dir(cat_dir);
        }
        Ok(())
    }

    /// Moved from `remove_from_contents`' two `std::fs::write` calls: a
    /// plain `std::fs::write` of `<entry>/<name>` (truncate in place, no
    /// temporary file, no rename), so the entry directory's mtime and the
    /// `metadata` stamp are untouched.
    fn replace_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        let path = self
            .db
            .vdb_path()
            .join(&key.category)
            .join(&key.pf)
            .join(name);
        fs::write(&path, data).map_err(|e| Error::io(path, e))
    }

    /// Moved from `pretend` (`update_world_file`, `deselect_from_world`,
    /// `run_deselect`): the caller sorts and de-duplicates. `create_dir_all`
    /// of `var/lib/portage` (skipped when this transaction already did
    /// it), the atoms joined by newlines plus a trailing newline (an empty
    /// list writes an empty file), a plain `std::fs::write`: no temporary
    /// file, no lock.
    fn set_world(&mut self, world: &World) -> Result<()> {
        self.write_world_store(WORLD_PATH, world.atoms.clone())
    }

    /// As [`WriteTxn::set_world`], each name written as `@<name>`.
    fn set_world_sets(&mut self, sets: &WorldSets) -> Result<()> {
        self.write_world_store(
            WORLD_SETS_PATH,
            sets.sets.iter().map(|n| format!("@{n}")).collect(),
        )
    }

    /// Moved from `ebuild_merge::write_plib_registry`: nothing at all
    /// (not even the parent directory) when `entries == loaded` (real
    /// `store()`'s `_data == _data_orig`, backlog #167); otherwise the
    /// `json.dumps` bytes ([`format_preserved_libs`]), `create_dir_all` of
    /// the parent and a plain `std::fs::write` (not real's
    /// `atomic_ofstream`).
    fn set_preserved_libs(&mut self, libs: &PreservedLibs) -> Result<()> {
        if libs.entries == libs.loaded {
            return Ok(());
        }
        let out = format_preserved_libs(&libs.entries);
        let path = self.db.store_path(PRESERVED_LIBS_PATH)?;
        create_parent(&path)?;
        fs::write(&path, out).map_err(|e| Error::io(path, e))
    }

    /// Moved from `ebuild_merge::write_cfgfiledict`: `create_dir_all` of
    /// the parent, then `"<path> <md5>\n"` per entry in key order with a
    /// plain `std::fs::write`, unconditionally.
    fn set_config_memory(&mut self, memory: &ConfigMemory) -> Result<()> {
        let path = self.db.store_path(CONFIG_MEMORY_PATH)?;
        create_parent(&path)?;
        let mut text = String::new();
        for (k, v) in &memory.entries {
            text.push_str(&format!("{k} {v}\n"));
        }
        fs::write(&path, text).map_err(|e| Error::io(path, e))
    }

    fn set_counter(&mut self, _counter: Counter) -> Result<()> {
        todo_step("set_counter", "S2.2")
    }

    /// Nothing to do: every call above was applied when it was made.
    fn commit(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

/// A minimal JSON string-literal reader (the `\"`/`\\`/`\/`/`\n`/`\t`/
/// `\r`/`\b`/`\f`/`\uXXXX` escapes real `json.dumps` may emit for a
/// path), used only by [`parse_preserved_libs`]: narrow by design, not a
/// general JSON parser. Moved from `ebuild_merge::parse_json_string`.
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

/// Parses exactly the shape real `PreservedLibsRegistry.store()` writes:
/// `{"cp:slot": [cpv, counter, [paths...]], ...}`. `None` on any
/// deviation; readers treat that like a missing file (real `load()`'s
/// graceful degrade to `{}`). Moved from `ebuild_merge::parse_plib_registry`.
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
    // Real `json.dumps(..., ensure_ascii=False)`: `"` and `\` escaped,
    // C0 controls as the short forms (`\b \t \n \f \r`) or `\u00xx`,
    // everything else (including non-ASCII and DEL) raw UTF-8.
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

/// Real `store()`'s `json.dumps(..., ensure_ascii=False, indent="\t",
/// sort_keys=True)` layout (the `BTreeMap` keeps keys sorted). An empty
/// dict is exactly `{}`, and there is no trailing newline, byte for byte
/// Python's output. Moved from `ebuild_merge::write_plib_registry`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstalledDb;

    fn scratch(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("portage-vdb-s14-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// The merge sequence on `files`: pending entry hidden from readers,
    /// the counter ticks from a missing store to 0, the stamp is the last
    /// write and survives the rename, and a same-`pf` live entry is
    /// replaced.
    #[test]
    fn merge_sequence_publishes_a_stamped_entry() {
        let root = scratch("merge");
        let db = FilesDb::new(&root);
        let key = EntryKey::new("dev-libs", "a-1.0");
        let live = root.join("var/db/pkg/dev-libs/a-1.0");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("OLD"), b"x").unwrap();
        let src = root.join("SLOT.src");
        fs::write(&src, b"0\n").unwrap();

        let mut txn = db.begin_write().unwrap();
        txn.begin_entry(&key).unwrap();
        // A stale pending entry is wiped by the next begin_entry.
        txn.put_entry_file(&key, "STALE", b"").unwrap();
        txn.begin_entry(&key).unwrap();
        txn.copy_entry_file(&key, "SLOT", &src).unwrap();
        assert_eq!(txn.next_counter().unwrap(), Counter(0));
        assert_eq!(txn.next_counter().unwrap(), Counter(1));
        txn.put_entry_file(&key, "COUNTER", b"1").unwrap();
        txn.put_entry_file(&key, "CONTENTS", b"obj /x 0 0\n")
            .unwrap();
        txn.seal_entry(&key).unwrap();
        assert_eq!(
            db.read_pending_file(&key, "COUNTER").unwrap().as_deref(),
            Some(&b"1"[..])
        );
        assert_eq!(db.read_pending_file(&key, "STALE").unwrap(), None);
        assert_eq!(db.category_entries("dev-libs").unwrap(), vec!["a-1.0"]);
        txn.finish_entry(&key).unwrap();
        txn.commit().unwrap();

        assert!(!live.join("OLD").exists());
        assert_eq!(db.read_pending_file(&key, "COUNTER").unwrap(), None);
        assert_eq!(db.counter().unwrap(), Some(Counter(1)));
        let meta = fs::read_to_string(live.join("metadata")).unwrap();
        assert!(meta.starts_with("#format=1\nCOUNTER=1\nSLOT=0\n#dir_mtime="));
        // The stamp is valid: aux_get serves from the snapshot.
        assert_eq!(db.aux_get(&key, "SLOT").unwrap().as_deref(), Some("0"));
        let st = fs::metadata(&live).unwrap();
        let ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;
        assert!(meta.ends_with(&format!("#dir_mtime={ns}\n")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn seal_without_fields_writes_nothing() {
        let root = scratch("seal-empty");
        let db = FilesDb::new(&root);
        let key = EntryKey::new("dev-libs", "b-1.0");
        let mut txn = db.begin_write().unwrap();
        txn.begin_entry(&key).unwrap();
        txn.put_entry_file(&key, "CONTENTS", b"").unwrap();
        txn.seal_entry(&key).unwrap();
        assert_eq!(db.read_pending_file(&key, "metadata").unwrap(), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn world_stores_round_trip_alone_in_a_world_only_transaction() {
        let root = scratch("world");
        let db = FilesDb::new(&root);
        assert_eq!(db.world().unwrap(), World::default());
        assert_eq!(db.world_sets().unwrap(), WorldSets::default());
        let mut txn = db.begin_write().unwrap();
        txn.set_world(&World {
            atoms: vec!["dev-libs/a".into(), "dev-libs/b:2".into()],
        })
        .unwrap();
        txn.commit().unwrap();
        let path = root.join("var/lib/portage/world");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "dev-libs/a\ndev-libs/b:2\n"
        );
        assert!(!root.join("var/lib/portage/world_sets").exists());
        // Reading drops blanks, comments and `@` lines.
        fs::write(&path, "# c\n\n @x \n dev-libs/a \n").unwrap();
        assert_eq!(db.world().unwrap().atoms, vec!["dev-libs/a".to_string()]);
        let mut txn = db.begin_write().unwrap();
        txn.set_world_sets(&WorldSets {
            sets: vec!["one".into(), "two".into()],
        })
        .unwrap();
        txn.set_world(&World::default()).unwrap();
        txn.commit().unwrap();
        let sets = root.join("var/lib/portage/world_sets");
        assert_eq!(fs::read_to_string(&sets).unwrap(), "@one\n@two\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        fs::write(&sets, "dev-libs/a\n# c\n@@one\n@two\n").unwrap();
        assert_eq!(
            db.world_sets().unwrap().sets,
            vec!["one".to_string(), "two".to_string()]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn preserved_libs_round_trip_and_skip_unchanged() {
        let root = scratch("plib");
        let db = FilesDb::new(&root);
        assert_eq!(db.preserved_libs().unwrap(), PreservedLibs::default());
        let mut libs = db.preserved_libs().unwrap();
        let mut txn = db.begin_write().unwrap();
        // Unchanged: not even the parent directory is created.
        txn.set_preserved_libs(&libs).unwrap();
        assert!(!root.join("var/lib/portage").exists());
        libs.entries.insert(
            "dev-libs/a:0".to_string(),
            PreservedLibsEntry {
                cpv: "dev-libs/a-1.0".to_string(),
                counter: "3".to_string(),
                paths: vec!["/usr/lib/a\"b.so".to_string()],
            },
        );
        txn.set_preserved_libs(&libs).unwrap();
        let back = db.preserved_libs().unwrap();
        assert_eq!(back.entries, libs.entries);
        assert_eq!(back.loaded, back.entries);
        assert_eq!(format_preserved_libs(&BTreeMap::new()), "{}");
        fs::write(root.join(PRESERVED_LIBS_PATH), b"not json").unwrap();
        assert!(db.preserved_libs().unwrap().entries.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn config_memory_round_trips() {
        let root = scratch("confmem");
        let db = FilesDb::new(&root);
        assert!(db.config_memory().unwrap().entries.is_empty());
        let mut mem = ConfigMemory::default();
        mem.entries.insert("/etc/a".to_string(), "abc".to_string());
        let mut txn = db.begin_write().unwrap();
        txn.set_config_memory(&mem).unwrap();
        assert_eq!(
            fs::read_to_string(root.join(CONFIG_MEMORY_PATH)).unwrap(),
            "/etc/a abc\n"
        );
        assert_eq!(db.config_memory().unwrap(), mem);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bare_vdb_dir_has_no_stores() {
        let root = scratch("bare");
        let db = FilesDb::open_vdb_dir(&root);
        assert_eq!(db.counter().unwrap(), None);
        assert!(db.preserved_libs().unwrap().entries.is_empty());
        let mut txn = db.begin_write().unwrap();
        assert!(matches!(txn.next_counter(), Err(Error::Unsupported(_))));
        let _ = fs::remove_dir_all(&root);
    }

    fn put_live(root: &Path, cat: &str, pf: &str, files: &[(&str, &str)]) {
        let dir = root.join("var/db/pkg").join(cat).join(pf);
        fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            fs::write(dir.join(name), body).unwrap();
        }
    }

    /// `delete_entry` removes the entry, and the category only when it is
    /// then empty; a missing entry is the `remove_dir_all` error.
    #[test]
    fn delete_entry_removes_the_empty_category_only() {
        let root = scratch("delete");
        put_live(&root, "dev-libs", "a-1", &[("CONTENTS", "")]);
        put_live(&root, "dev-libs", "b-1", &[("CONTENTS", "")]);
        let db = FilesDb::new(&root);
        let vdb = root.join("var/db/pkg");
        let mut txn = db.begin_write().unwrap();
        txn.delete_entry(&EntryKey::new("dev-libs", "a-1")).unwrap();
        assert!(!vdb.join("dev-libs/a-1").exists());
        assert!(vdb.join("dev-libs").is_dir(), "b-1 keeps the category");
        txn.delete_entry(&EntryKey::new("dev-libs", "b-1")).unwrap();
        assert!(!vdb.join("dev-libs").exists(), "empty category is removed");
        let err = txn
            .delete_entry(&EntryKey::new("dev-libs", "b-1"))
            .unwrap_err();
        assert!(
            err.to_string()
                .starts_with(&format!("{}: ", vdb.join("dev-libs/b-1").display()))
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// `replace_file` rewrites in place (same inode) and leaves the entry
    /// directory's mtime alone.
    #[test]
    fn replace_file_rewrites_in_place() {
        use std::os::unix::fs::MetadataExt as _;
        let root = scratch("replace");
        put_live(&root, "dev-libs", "a-1", &[("CONTENTS", "old old old\n")]);
        let dir = root.join("var/db/pkg/dev-libs/a-1");
        let (ino, mtime) = {
            let f = fs::metadata(dir.join("CONTENTS")).unwrap();
            let d = fs::metadata(&dir).unwrap();
            (f.ino(), (d.mtime(), d.mtime_nsec()))
        };
        let db = FilesDb::new(&root);
        let mut txn = db.begin_write().unwrap();
        txn.replace_file(&EntryKey::new("dev-libs", "a-1"), "CONTENTS", b"new\n")
            .unwrap();
        assert_eq!(fs::read(dir.join("CONTENTS")).unwrap(), b"new\n");
        assert_eq!(fs::metadata(dir.join("CONTENTS")).unwrap().ino(), ino);
        let d = fs::metadata(&dir).unwrap();
        assert_eq!((d.mtime(), d.mtime_nsec()), mtime);
        let _ = fs::remove_dir_all(&root);
    }

    /// `read_file_all` lists every live entry (a missing file is `None`),
    /// skips `-MERGING-`; `owners` matches with one leading `/` ignored,
    /// in listing then `CONTENTS` order; `categories` lists directories.
    #[test]
    fn bulk_reads_and_owners_scan_the_tree() {
        let root = scratch("bulk");
        put_live(
            &root,
            "dev-libs",
            "a-1",
            &[
                ("NEEDED.ELF.2", "n\n"),
                ("CONTENTS", "dir /usr\nobj /usr/x abc 1\nfoo /usr/y\n"),
            ],
        );
        put_live(
            &root,
            "dev-libs",
            "b-1",
            &[("CONTENTS", "sym /usr/x -> y 1\n")],
        );
        put_live(
            &root,
            "dev-libs",
            "-MERGING-c-1",
            &[("NEEDED.ELF.2", "p\n")],
        );
        put_live(&root, "app-misc", "d-1", &[]);
        fs::write(root.join("var/db/pkg/stray"), "").unwrap();
        let db = FilesDb::new(&root);
        assert_eq!(db.categories().unwrap(), ["app-misc", "dev-libs"]);
        let all = db.read_file_all("NEEDED.ELF.2").unwrap();
        assert_eq!(
            all,
            vec![
                (EntryKey::new("app-misc", "d-1"), None),
                (EntryKey::new("dev-libs", "a-1"), Some(b"n\n".to_vec())),
                (EntryKey::new("dev-libs", "b-1"), None),
            ]
        );
        let owned = db
            .owners(&[
                b"/usr/x".as_slice(),
                b"/usr/y".as_slice(),
                b"/usr".as_slice(),
            ])
            .unwrap();
        assert_eq!(
            owned,
            vec![
                (b"/usr".to_vec(), EntryKey::new("dev-libs", "a-1")),
                (b"/usr/x".to_vec(), EntryKey::new("dev-libs", "a-1")),
                (b"/usr/x".to_vec(), EntryKey::new("dev-libs", "b-1")),
            ],
            "`foo` is not a recorded kind, so /usr/y is unowned"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
