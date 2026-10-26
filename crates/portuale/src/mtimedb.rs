//! `/var/cache/edb/mtimedb` persistence: the `resume` mergelist
//! (`emerge --resume`), its `resume_backup` rotation, the `info`
//! dir-mtime memo, and the `updates` per-file mtime record. The file is
//! Portage's JSON mtimedb shape (tab-indented, sorted keys, no trailing
//! newline). Every load normalizes the top level the way `MtimeDB._load`
//! does, and every write commits the whole normalized object at once,
//! skipping the write when nothing changed. Nested sections keep their
//! raw text: only the top level is normalized.
// Portage: lib/portage/util/mtimedb.py, MtimeDB

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// One mergelist entry's `type` tag (`"ebuild"` or `"binary"`): which kind
/// of merge `emerge --resume` replays this entry as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeEntryKind {
    Ebuild,
    Binary,
}

impl ResumeEntryKind {
    fn as_json_str(self) -> &'static str {
        match self {
            ResumeEntryKind::Ebuild => "ebuild",
            ResumeEntryKind::Binary => "binary",
        }
    }
}

/// `(kind, category, package, version)` -- one entry of a resume
/// mergelist.
pub type ResumeCpv = (ResumeEntryKind, String, String, String);

/// The subset of `mtimedb["resume"]["myopts"]` that changes how `--resume`
/// replays the mergelist: `--oneshot` and `--onlydeps` govern
/// world-recording; `--quiet` and `--quiet-build` govern the display;
/// `--merge-wait-scope` governs the scheduler barriers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumeOpts {
    pub oneshot: bool,
    pub onlydeps: bool,
    pub quiet: bool,
    /// `--quiet-build`: `Some(true)` bare/`=y`, `Some(false)` `=n`.
    pub quiet_build: Option<bool>,
    /// `--merge-wait-scope`, as given; `None` (unset) replays as `deep`.
    pub merge_wait_scope: Option<crate::pretend::MergeWaitScope>,
}

/// `(favorites, mergelist, myopts)` from `mtimedb["resume"]`.
pub type ResumeList = (Vec<String>, Vec<ResumeCpv>, ResumeOpts);

/// One resume section's data (`resume` and `resume_backup` share the shape).
#[derive(Clone)]
struct Section {
    favorites: Vec<String>,
    mergelist: Vec<ResumeCpv>,
    opts: ResumeOpts,
}

/// `<root>/var/cache/edb/mtimedb`.
pub fn mtimedb_path(root: &Path) -> PathBuf {
    root.join("var/cache/edb/mtimedb")
}

/// What [`load_mtimedb`] hands back: the unknown-key warnings for the
/// entry point to print.
pub struct MtimeDbLoad {
    /// One `Deleting invalid mtimedb key: <k>` line per unknown key,
    /// sorted so repeated runs print the same order.
    pub warnings: Vec<String>,
}

/// The top level after `_load` normalization: defaults filled, `old`
/// folded into `updates`, `cur` gone, unknown keys dropped. Sections
/// portuale never parses (`ldpath`, `resume`, `resume_backup`) are kept
/// as raw text and splice back verbatim on every write.
struct NormalizedState {
    starttime: i64,
    version: String,
    info: BTreeMap<String, i64>,
    ldpath_raw: String,
    updates: BTreeMap<String, i64>,
    resume_raw: Option<String>,
    resume_backup_raw: Option<String>,
}

/// Roots whose load warnings already went out this run: every op loads
/// the file fresh (so an in-process setup write, a spawned run, and an
/// in-process read all see the same bytes), while the warnings print
/// only for the first load of each root.
static WARNED: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

fn warned() -> &'static Mutex<BTreeSet<PathBuf>> {
    WARNED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Loads and normalizes `root`'s file, returning the state plus the
/// unknown-key warnings. Only the first call per root per run reports
/// warnings, so the entry point prints each key exactly once; the later
/// `read_*`/`write_*` calls load silently and never warn again.
pub fn load_mtimedb(root: &Path) -> MtimeDbLoad {
    let (_, warnings) = load_normalized(root);
    let warnings = if warned().lock().unwrap().insert(root.to_path_buf()) {
        warnings
    } else {
        Vec::new()
    };
    MtimeDbLoad { warnings }
}

/// The normalized state for `root`, loaded fresh and silently: every
/// `read_*`/`write_*` call re-reads the file, so each op is a
/// read-modify-write off current bytes.
fn silent_load(root: &Path) -> NormalizedState {
    let (state, _) = load_normalized(root);
    warned().lock().unwrap().insert(root.to_path_buf());
    state
}

/// Runs `f` over a fresh load, committing when it changed; the
/// `Result` comes back for fallible callers.
fn mutate_result(root: &Path, f: impl FnOnce(&mut NormalizedState)) -> Result<(), String> {
    let (mut state, _) = load_normalized(root);
    let before = render_full(&state);
    f(&mut state);
    commit(root, &before, &mut state)
}

/// Runs `f` over a fresh load, committing when it changed. Swallowed
/// errors stay silent like Portage's `__write_to_disk` (`except
/// OSError: pass`); fallible callers use [`mutate_result`].
fn mutate(root: &Path, f: impl FnOnce(&mut NormalizedState)) {
    if mutate_result(root, f).is_err() {
        // Portage: lib/portage/util/mtimedb.py, __write_to_disk
    }
}

/// Renders the state, writing it back only when it differs from the
/// render before the mutation; a write stamps the portuale version first.
///
/// # Errors
///
/// Returns a message when the atomic rewrite fails.
fn commit(root: &Path, before: &str, state: &mut NormalizedState) -> Result<(), String> {
    if render_full(state) == before {
        return Ok(());
    }
    state.version = format!("portuale-{}", env!("CARGO_PKG_VERSION"));
    let rendered = render_full(state);
    write_content(root, &rendered)?;
    Ok(())
}

/// The valid top-level keys: everything else is dropped with a warning.
fn is_known_key(key: &str) -> bool {
    matches!(
        key,
        "info" | "ldpath" | "resume" | "resume_backup" | "starttime" | "updates" | "version"
    )
}

/// Reads the file into a normalized state plus warnings: `old` replaces
/// `updates`, `cur` is dropped, defaults are filled, and unknown keys
/// are dropped with Portage's warning.
fn load_normalized(root: &Path) -> (NormalizedState, Vec<String>) {
    let raw = read_raw_objects(root);
    let updates = raw
        .get("old")
        .or_else(|| raw.get("updates"))
        .filter(|v| is_object(v))
        .map(|v| parse_flat_mtime_map(v))
        .unwrap_or_default();
    let info = raw
        .get("info")
        .filter(|v| is_object(v))
        .map(|v| parse_flat_mtime_map(v))
        .unwrap_or_default();
    let mut warnings: Vec<String> = raw
        .keys()
        .filter(|k| !is_known_key(k) && k.as_str() != "old" && k.as_str() != "cur")
        .map(|k| format!("Deleting invalid mtimedb key: {k}"))
        .collect();
    warnings.sort();
    let state = NormalizedState {
        starttime: raw
            .get("starttime")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0),
        version: raw
            .get("version")
            .map(String::as_str)
            .map(parse_version_string)
            .unwrap_or_default(),
        info,
        ldpath_raw: raw
            .get("ldpath")
            .cloned()
            .unwrap_or_else(|| "{}".to_string()),
        updates,
        resume_raw: raw.get("resume").cloned(),
        resume_backup_raw: raw.get("resume_backup").cloned(),
    };
    (state, warnings)
}

/// Whether a raw top-level value is an object (the only shape the parsed
/// sections accept; anything else reads back as the default).
fn is_object(raw: &str) -> bool {
    raw.trim_start().starts_with('{')
}

/// The string value of a raw JSON string slice, unescaped; anything else
/// reads back as the default.
fn parse_version_string(raw: &str) -> String {
    let text = raw.trim();
    if !text.starts_with('"') {
        return String::new();
    }
    let Some(end) = string_end(text.as_bytes(), 0) else {
        return String::new();
    };
    unescape_json_string(&text[1..end - 1])
}

/// The `json.dumps` escape mapping for one string's contents.
fn unescape_json_string(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(e) => out.push(e),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The whole file in `json.dumps(sort_keys=True)` shape: every default
/// key always present, `resume` keys only when set, no trailing newline.
fn render_full(state: &NormalizedState) -> String {
    let mut parts = Vec::new();
    parts.push(render_flat_full("info", &state.info));
    parts.push(format!("\t\"ldpath\": {}", state.ldpath_raw));
    if let Some(raw) = &state.resume_raw {
        parts.push(format!("\t\"resume\": {raw}"));
    }
    if let Some(raw) = &state.resume_backup_raw {
        parts.push(format!("\t\"resume_backup\": {raw}"));
    }
    parts.push(format!("\t\"starttime\": {}", state.starttime));
    parts.push(render_flat_full("updates", &state.updates));
    parts.push(format!("\t\"version\": {}", json_str(&state.version)));
    format!("{{\n{}\n}}", parts.join(",\n"))
}

/// One flat `{path: mtime}` section, or `{}` when empty (Portage renders
/// an empty dict inline, never omits the key).
fn render_flat_full(key: &str, mtimes: &BTreeMap<String, i64>) -> String {
    if mtimes.is_empty() {
        format!("\t\"{key}\": {{}}")
    } else {
        format_flat_section(key, mtimes)
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One section's body in `json.dumps(sort_keys=True)` shape (no outer key
/// wrapper; the caller adds that). Shared by every section a write produces.
fn format_section_body(root: &Path, section: &Section) -> String {
    let root_str = root.display().to_string();
    let favs: Vec<String> = section
        .favorites
        .iter()
        .map(|f| format!("\t\t\t{}", json_str(f)))
        .collect();
    let merges: Vec<String> = section
        .mergelist
        .iter()
        .map(|(kind, cat, pkg, ver)| {
            format!(
                "\t\t\t[\n\t\t\t\t{},\n\t\t\t\t{},\n\t\t\t\t{},\n\t\t\t\t{}\n\t\t\t]",
                json_str(kind.as_json_str()),
                json_str(&root_str),
                json_str(&format!("{cat}/{pkg}-{ver}")),
                json_str("merge")
            )
        })
        .collect();
    // `myopts` keys in `json.dumps(sort_keys=True)` order.
    let mut opt_pairs: Vec<String> = Vec::new();
    if let Some(scope) = section.opts.merge_wait_scope {
        opt_pairs.push(format!(
            "\t\t\t{}: \"{}\"",
            json_str("--merge-wait-scope"),
            scope.as_str()
        ));
    }
    if section.opts.oneshot {
        opt_pairs.push(format!("\t\t\t{}: true", json_str("--oneshot")));
    }
    if section.opts.onlydeps {
        opt_pairs.push(format!("\t\t\t{}: true", json_str("--onlydeps")));
    }
    if section.opts.quiet {
        opt_pairs.push(format!("\t\t\t{}: true", json_str("--quiet")));
    }
    match section.opts.quiet_build {
        Some(true) => opt_pairs.push(format!("\t\t\t{}: \"y\"", json_str("--quiet-build"))),
        Some(false) => opt_pairs.push(format!("\t\t\t{}: \"n\"", json_str("--quiet-build"))),
        None => {}
    }
    let myopts = if opt_pairs.is_empty() {
        "{}".to_string()
    } else {
        format!("{{\n{}\n\t\t}}", opt_pairs.join(",\n"))
    };
    format!(
        "\t\t\"favorites\": [\n{}\n\t\t],\n\t\t\"mergelist\": [\n{}\n\t\t],\n\t\t\"myopts\": {myopts}",
        favs.join(",\n"),
        merges.join(",\n")
    )
}

/// Writes the whole mtimedb file at once. The write is atomic: a
/// same-directory temporary renamed over the target, with the file mode
/// forced to `0644` and the `portage` group applied best-effort, so a
/// crash never leaves a half-written record behind.
///
/// # Errors
///
/// Returns a message when the directory, the temporary, or the rename fails.
fn write_content(root: &Path, content: &str) -> Result<(), String> {
    let path = mtimedb_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("mtimedb");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut n = 0u32;
    let (tmp, mut file) = loop {
        let candidate = parent.join(format!(".{base}.tmp.{}.{n}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(format!("{}: {e}", candidate.display())),
        }
    };
    let result = (|| {
        use std::io::Write as _;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        drop(file);
        set_mtimedb_mode(&tmp)?;
        chown_portage_gid(&tmp);
        std::fs::rename(&tmp, &path)
            .map_err(|e| format!("{} -> {}: {e}", tmp.display(), path.display()))
    })();
    if result.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    result?;
    Ok(())
}

/// `apply_secpass_permissions(mode=0o644)`: the record stays
/// world-readable regardless of the writer's umask.
// Portage: lib/portage/util/__init__.py, apply_secpass_permissions
fn set_mtimedb_mode(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Best-effort `portage`-group ownership of a fresh record
/// (`apply_secpass_permissions(gid=portage_gid)`): a writer outside the
/// group cannot chown, so a failure is silently kept, never fatal.
// Portage: lib/portage/util/__init__.py, apply_secpass_permissions
fn chown_portage_gid(path: &Path) {
    let Some(gid) = portage_gid() else {
        return;
    };
    let Ok(name) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return;
    };
    // SAFETY: `name` is a NUL-terminated path that outlives the call;
    // `(uid_t)-1` keeps the owner, `gid` is a looked-up group.
    unsafe {
        libc::chown(name.as_ptr(), u32::MAX, gid);
    }
}

/// The `portage` group's gid, or `None` when the group is unknown.
fn portage_gid() -> Option<u32> {
    let name = std::ffi::CString::new("portage").ok()?;
    // SAFETY: `name` is a valid NUL-terminated string; `entry`, the
    // scratch buffer and `result` stay alive for the call.
    unsafe {
        let mut entry: libc::group = std::mem::zeroed();
        let mut result: *mut libc::group = std::ptr::null_mut();
        let mut bufsize = libc::sysconf(libc::_SC_GETGR_R_SIZE_MAX);
        if bufsize <= 0 {
            bufsize = 16384;
        }
        let mut buf = vec![0u8; bufsize as usize];
        let rc = libc::getgrnam_r(
            name.as_ptr(),
            &mut entry,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        );
        if rc == 0 && !result.is_null() {
            Some((*result).gr_gid)
        } else {
            None
        }
    }
}

/// Raw value slices for every top-level key on disk, in file order.
/// Keys portuale never writes (`ldpath`, `starttime`, `version`,
/// anything else Portage stores) splice back verbatim through
/// `preserved_parts`, so no write drops what it does not own. Values
/// may be objects, arrays, strings or bare numbers/literals; the scan
/// is string-aware, so braces inside quoted strings never count.
// Portage: lib/portage/util/mtimedb.py, MtimeDB
fn read_raw_objects(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(content) = std::fs::read_to_string(mtimedb_path(root)) else {
        return out;
    };
    let bytes = content.as_bytes();
    let mut i = skip_ws(bytes, 0);
    if bytes.get(i) != Some(&b'{') {
        return out;
    }
    i = skip_ws(bytes, i + 1);
    loop {
        i = skip_ws(bytes, i);
        if bytes.get(i) == Some(&b'}') {
            return out;
        }
        if bytes.get(i) != Some(&b'"') {
            return out;
        }
        let Some(close) = string_end(bytes, i) else {
            return out;
        };
        let Some(key) = content.get(i + 1..close - 1).map(str::to_string) else {
            return out;
        };
        i = skip_ws(bytes, close);
        if bytes.get(i) != Some(&b':') {
            return out;
        }
        i = skip_ws(bytes, i + 1);
        let Some(end) = raw_value_end(bytes, i) else {
            return out;
        };
        let Some(value) = content.get(i..end) else {
            return out;
        };
        out.insert(key, value.to_string());
        i = skip_ws(bytes, end);
        match bytes.get(i) {
            Some(b',') => i += 1,
            Some(b'}') => return out,
            _ => return out,
        }
    }
}

/// Offset just past the closing quote of the string opening at `open`
/// (which must itself be a `"`), honouring backslash escapes.
fn string_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// First non-blank offset at or after `i`.
fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Offset just past the JSON value starting at `start`: an object or
/// array (brace-matched, string-aware), a quoted string, or a bare
/// number/literal up to the next delimiter.
fn raw_value_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start)? {
        b'"' => string_end(bytes, start),
        b'{' | b'[' => {
            let mut depth = 0i32;
            let mut i = start;
            let mut in_string = false;
            let mut escape = false;
            while i < bytes.len() {
                let b = bytes[i];
                if in_string {
                    if escape {
                        escape = false;
                    } else if b == b'\\' {
                        escape = true;
                    } else if b == b'"' {
                        in_string = false;
                    }
                    i += 1;
                    continue;
                }
                match b {
                    b'"' => {
                        in_string = true;
                        i += 1;
                    }
                    b'{' | b'[' => {
                        depth += 1;
                        i += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        i += 1;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
            None
        }
        _ => {
            let mut i = start;
            while i < bytes.len() && !matches!(bytes[i], b',' | b'}' | b']') {
                i += 1;
            }
            while i > start && bytes[i - 1].is_ascii_whitespace() {
                i -= 1;
            }
            (i > start).then_some(i)
        }
    }
}

/// Fresh `"resume"` object text from a parsed section (stamps the
/// current root into the mergelist entries -- only used for lists this
/// run itself wrote; preserved ones splice back verbatim).
fn render_resume_object(root: &Path, section: &Section) -> String {
    format!("{{\n{}\n\t}}", format_section_body(root, section))
}

/// The `{...}` object immediately following `"<key>":` in `content`,
/// brace-depth-matched (quoted strings' `{`/`}` don't count), so a second
/// top-level section following in the same file can't confuse it.
fn extract_object<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\"");
    let after_key = content.split_once(&marker)?.1;
    // Only whitespace and a single `:` may separate the key from its value;
    // otherwise this key's own (possibly array-shaped) value could be skipped
    // in favour of a sibling key's object.
    let body = after_key.trim_start().strip_prefix(':')?.trim_start();
    if !body.starts_with('{') {
        return None;
    }
    let start = content.len() - body.len();
    let end = raw_value_end(content.as_bytes(), start)?;
    body.get(..end - start)
}

/// Parses one section's object text into a `Section`, or `None` if it is
/// missing/malformed.
fn parse_section(object: &str) -> Option<Section> {
    // Match `["ebuild"|"binary", "<root>", "<cat/pkg-ver>", "merge"]`; the
    // cpv is split into `(cat, pkg, ver)` below.
    let entry_re =
        Regex::new(r#"\[\s*"(ebuild|binary)"\s*,\s*"[^"]*"\s*,\s*"([^"]+)"\s*,\s*"merge"\s*\]"#)
            .ok()?;

    let mut mergelist = Vec::new();
    for cap in entry_re.captures_iter(object) {
        let kind = if &cap[1] == "binary" {
            ResumeEntryKind::Binary
        } else {
            ResumeEntryKind::Ebuild
        };
        let cpv = &cap[2];
        if let Some((cp, ver)) = split_cpv(cpv)
            && let Some((cat, pkg)) = cp.split_once('/')
        {
            mergelist.push((kind, cat.to_string(), pkg.to_string(), ver.to_string()));
        }
    }
    if mergelist.is_empty() {
        return None;
    }

    let fav_block = extract_object(object, "favorites")
        .or_else(|| object.split("\"favorites\"").nth(1))
        .and_then(|s| s.split('[').nth(1))
        .and_then(|s| s.split(']').next())
        .unwrap_or("");
    let favorites: Vec<String> = Regex::new(r#""([^"]+)""#)
        .ok()?
        .captures_iter(fav_block)
        .map(|c| c[1].to_string())
        .collect();

    // `myopts` is a flat `"--flag": true` object.
    let myopts_block = extract_object(object, "myopts").unwrap_or("");
    let opts = ResumeOpts {
        oneshot: myopts_block.contains("\"--oneshot\""),
        onlydeps: myopts_block.contains("\"--onlydeps\""),
        quiet: myopts_block.contains("\"--quiet\""),
        quiet_build: parse_quiet_build(myopts_block),
        merge_wait_scope: parse_merge_wait_scope(myopts_block),
    };

    Some(Section {
        favorites,
        mergelist,
        opts,
    })
}

/// `--merge-wait-scope` from a `myopts` block: Portage stores the scope
/// word as a string (`"deep"`); absent means the `deep` default.
fn parse_merge_wait_scope(myopts_block: &str) -> Option<crate::pretend::MergeWaitScope> {
    let after = myopts_block.split("\"--merge-wait-scope\"").nth(1)?;
    let value = after.trim_start().strip_prefix(':')?.trim_start();
    let word = value.strip_prefix('"')?.split('"').next()?;
    crate::pretend::MergeWaitScope::parse(word)
}

/// `--quiet-build` from a `myopts` block: Portage stores bare/`y` as
/// `true`/`"y"` and `n` as `false`/`"n"`.
fn parse_quiet_build(myopts_block: &str) -> Option<bool> {
    let after = myopts_block.split("\"--quiet-build\"").nth(1)?;
    let value = after.trim_start().strip_prefix(':')?.trim_start();
    let token: String = value
        .chars()
        .take_while(|c| !matches!(c, ',' | '}' | '\n'))
        .collect();
    Some(!matches!(token.trim(), "false" | "\"n\""))
}

/// Reads one top-level section (`"resume"` or `"resume_backup"`) from
/// a fresh load, if present and well-formed.
#[cfg(test)]
fn read_section(root: &Path, key: &str) -> Option<Section> {
    let state = silent_load(root);
    let raw = match key {
        "resume" => state.resume_raw.as_deref()?,
        "resume_backup" => state.resume_backup_raw.as_deref()?,
        _ => return None,
    };
    parse_section(raw)
}

/// One flat `{path: mtime}` section body in `json.dumps(sort_keys=True)`
/// shape (keys sorted via the `BTreeMap` iteration order). Shared by the
/// `info` and `updates` sections.
fn format_flat_section(key: &str, mtimes: &BTreeMap<String, i64>) -> String {
    let entries: Vec<String> = mtimes
        .iter()
        .map(|(k, v)| format!("\t\t{}: {v}", json_str(k)))
        .collect();
    format!("\t\"{key}\": {{\n{}\n\t}}", entries.join(",\n"))
}

/// `"path": 123` pairs from a flat section object; a foreign-written
/// float (`123.0`) is accepted and truncated, while what portuale
/// writes back is always an integer.
fn parse_flat_mtime_map(object: &str) -> BTreeMap<String, i64> {
    static PAIR_RE: OnceLock<Regex> = OnceLock::new();
    let pair_re = PAIR_RE.get_or_init(|| {
        Regex::new(r#""((?:[^"\\]|\\.)*)"\s*:\s*(-?\d+(?:\.\d+)?)"#).expect("static regex is valid")
    });
    let mut mtimes = BTreeMap::new();
    for cap in pair_re.captures_iter(object) {
        let key = unescape_json_string(&cap[1]);
        if let Ok(v) = cap[2].parse::<f64>() {
            mtimes.insert(key, v as i64);
        }
    }
    mtimes
}

/// Reads one flat `{path: mtime}` top-level section (`"info"` or
/// `"updates"`) from a fresh load. Empty when the file is absent or
/// carries no such section.
fn read_flat_mtimes(root: &Path, key: &str) -> BTreeMap<String, i64> {
    let state = silent_load(root);
    match key {
        "info" => state.info.clone(),
        "updates" => state.updates.clone(),
        _ => BTreeMap::new(),
    }
}

/// `mtimedb["info"]`: absolute GNU-info directory -> that directory's
/// `st_mtime` (whole seconds) as `chk_updated_info_files` last left it.
/// Empty when the file is absent or carries no `info` section.
pub fn read_info_mtimes(root: &Path) -> BTreeMap<String, i64> {
    read_flat_mtimes(root, "info")
}

/// `mtimedb["updates"]`: updates-file path -> that file's `st_mtime`
/// (whole seconds) as the last global-updates run left it. Empty when
/// the file is absent or carries no `updates` section.
// Portage: lib/portage/_global_updates.py (`prev_mtimes`); util/mtimedb.py
pub fn read_updates_mtimes(root: &Path) -> BTreeMap<String, i64> {
    read_flat_mtimes(root, "updates")
}

/// Stores one flat section, committing when it changed.
///
/// # Errors
///
/// Returns a message when the atomic rewrite fails.
fn set_flat_section(root: &Path, key: &str, mtimes: &BTreeMap<String, i64>) -> Result<(), String> {
    mutate_result(root, |state| match key {
        "info" => state.info = mtimes.clone(),
        "updates" => state.updates = mtimes.clone(),
        _ => {}
    })
}

/// Stores `mtimedb["info"]`: commits the whole normalized object with
/// `info` replaced. An unchanged map skips the write.
///
/// # Errors
///
/// Returns a message when the atomic rewrite fails.
pub fn write_info_mtimes(root: &Path, info: &BTreeMap<String, i64>) -> Result<(), String> {
    set_flat_section(root, "info", info)
}

/// Stores `mtimedb["updates"]`: commits the whole normalized object
/// with `updates` replaced. An unchanged map skips the write.
// Portage: lib/portage/_global_updates.py (`prev_mtimes.update` + commit)
///
/// # Errors
///
/// Returns a message when the atomic rewrite fails.
pub fn write_updates_mtimes(root: &Path, updates: &BTreeMap<String, i64>) -> Result<(), String> {
    set_flat_section(root, "updates", updates)
}

/// Writes `mtimedb["resume"]` from the atom args, one merge item per package
/// (tagged `"ebuild"` or `"binary"` with its `<root>` and `"<cat/pkg-ver>"`)
/// and the `--oneshot` / `--onlydeps` flags. Used for the up-front save of
/// the full resolved plan and for re-saving the still-unmerged tail on a
/// clean failure. The assignment is unconditional: an empty `mergelist`
/// still overwrites any stale list. An existing `resume_backup` is preserved
/// untouched.
///
/// # Errors
///
/// Returns a message when the atomic rewrite fails.
pub fn write_resume_list(
    root: &Path,
    favorites: &[&str],
    mergelist: &[ResumeCpv],
    opts: &ResumeOpts,
) -> Result<(), String> {
    let resume = Section {
        favorites: favorites.iter().map(|s| s.to_string()).collect(),
        mergelist: mergelist.to_vec(),
        opts: *opts,
    };
    let object = render_resume_object(root, &resume);
    mutate_result(root, |state| {
        state.resume_raw = Some(object.clone());
    })
}

/// After each successful merge the merged package leaves
/// `mtimedb["resume"]["mergelist"]`; once the list empties, the `resume` key
/// is deleted. Removes the first entry equal to `entry`; a no-op when there
/// is no `resume` section or the entry isn't listed.
pub fn remove_merged_entry(root: &Path, entry: &ResumeCpv) {
    mutate(root, |state| {
        let Some(raw) = state.resume_raw.clone() else {
            return;
        };
        let Some(mut resume) = parse_section(&raw) else {
            return;
        };
        let Some(pos) = resume.mergelist.iter().position(|e| e == entry) else {
            return;
        };
        resume.mergelist.remove(pos);
        state.resume_raw = if resume.mergelist.is_empty() {
            None
        } else {
            Some(render_resume_object(root, &resume))
        };
    });
}

/// Right before a fresh, non-`--resume` merge starts, an existing `resume`
/// entry with more than one item is preserved as `resume_backup` (replacing
/// whatever backup existed) and `resume` itself is cleared. A single-item
/// list, or no `resume` entry, is a no-op.
pub fn rotate_resume_to_backup(root: &Path) {
    mutate(root, |state| {
        let Some(raw) = state.resume_raw.clone() else {
            return;
        };
        let Some(resume) = parse_section(&raw) else {
            return;
        };
        if resume.mergelist.len() <= 1 {
            return;
        }
        state.resume_raw = None;
        state.resume_backup_raw = Some(raw);
    });
}

/// Reads back `(favorites, mergelist-cpvs, myopts)` from `mtimedb["resume"]`,
/// or `None` when there's nothing to resume. When `resume` is absent but
/// `resume_backup` is present, the backup is promoted to `resume`.
pub fn read_resume_list(root: &Path) -> Option<ResumeList> {
    let state = silent_load(root);
    if let Some(raw) = &state.resume_raw
        && let Some(s) = parse_section(raw)
    {
        return Some((s.favorites, s.mergelist, s.opts));
    }
    let backup = state.resume_backup_raw.clone()?;
    let parsed = parse_section(&backup)?;
    mutate(root, |state| {
        state.resume_raw = Some(backup.clone());
        state.resume_backup_raw = None;
    });
    Some((parsed.favorites, parsed.mergelist, parsed.opts))
}

/// `cat/pkg-1.2.3-r1` -> `("cat/pkg", "1.2.3-r1")`. Splits at the last `-`
/// whose following char is a digit.
fn split_cpv(cpv: &str) -> Option<(&str, &str)> {
    let bytes = cpv.as_bytes();
    for (i, _) in cpv.match_indices('-') {
        if bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit()) {
            return Some((&cpv[..i], &cpv[i + 1..]));
        }
    }
    None
}

/// Clears `mtimedb["resume"]` after a successful `--resume` run. Only ever
/// removes `resume` itself, leaving `resume_backup` (if any) untouched.
pub fn clear_resume_list(root: &Path) {
    mutate(root, |state| {
        state.resume_raw = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn tmproot() -> std::path::PathBuf {
        TempDir::new("mtimedb_test").keep()
    }

    #[test]
    fn info_mtimes_round_trip_without_disturbing_the_resume_list() {
        // One file, every key at once: an `info` write must not drop
        // `resume`, and vice versa.
        let root = tmproot();
        write_resume_list(
            &root,
            &["dev-libs/a"],
            &[(
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "a".to_string(),
                "1".to_string(),
            )],
            &ResumeOpts::default(),
        )
        .unwrap();

        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        mtimes.insert("/opt/pkg/info".to_string(), 1_700_000_001);
        write_info_mtimes(&root, &mtimes).unwrap();

        assert_eq!(read_info_mtimes(&root), mtimes);
        // The resume list survived the info write.
        assert_eq!(read_resume_list(&root).unwrap().0, vec!["dev-libs/a"]);

        // And a resume write preserves the info memo.
        write_resume_list(
            &root,
            &["dev-libs/b"],
            &[(
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "b".to_string(),
                "2".to_string(),
            )],
            &ResumeOpts::default(),
        )
        .unwrap();
        assert_eq!(read_info_mtimes(&root), mtimes);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn updates_mtimes_round_trip_without_disturbing_other_sections() {
        // The `updates` record shares the file with `resume` and `info`;
        // every writer preserves the others.
        let root = tmproot();
        let mut updates = BTreeMap::new();
        updates.insert("/repo/profiles/updates/2Q-2024".to_string(), 1_700_000_002);
        write_updates_mtimes(&root, &updates).unwrap();
        assert_eq!(read_updates_mtimes(&root), updates);

        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();
        assert_eq!(read_updates_mtimes(&root), updates);

        write_resume_list(
            &root,
            &["dev-libs/a"],
            &[(
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "a".to_string(),
                "1".to_string(),
            )],
            &ResumeOpts::default(),
        )
        .unwrap();
        assert_eq!(read_updates_mtimes(&root), updates);
        assert_eq!(read_info_mtimes(&root), mtimes);

        // Sections serialize in `sort_keys` order.
        let text = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        let at = |k: &str| text.find(k).unwrap();
        assert!(at("\"info\"") < at("\"resume\"") && at("\"resume\"") < at("\"updates\""));

        // Clearing the record stores nothing for `updates` itself.
        write_updates_mtimes(&root, &BTreeMap::new()).unwrap();
        assert!(read_updates_mtimes(&root).is_empty());
        assert_eq!(read_resume_list(&root).unwrap().0, vec!["dev-libs/a"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn managed_top_level_keys_splice_back_verbatim_while_unknown_ones_drop() {
        // Portage's mtimedb carries keys portuale never manages (`ldpath`,
        // `starttime`: `lib/portage/util/mtimedb.py`, `_MTIMEDBKEYS`): an
        // `info` or `resume` write keeps them byte-identical, while an
        // unknown key is dropped with the load warning instead. A commit
        // stamps the portuale version over the loaded one.
        let root = tmproot();
        std::fs::create_dir_all(mtimedb_path(&root).parent().unwrap()).unwrap();
        std::fs::write(
            mtimedb_path(&root),
            "{\n\t\"bogus\": 1,\n\t\"info\": {},\n\t\"ldpath\": {\n\t\t\"/lib\": 1700000001\n\t},\n\t\"starttime\": 0,\n\t\"version\": \"3.0.83\"\n}",
        )
        .unwrap();
        let fragments = [
            "\"ldpath\": {\n\t\t\"/lib\": 1700000001\n\t}",
            "\"starttime\": 0",
        ];

        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();
        let after_info = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        for fragment in fragments {
            assert!(
                after_info.contains(fragment),
                "an `info` write must keep {fragment}: {after_info}"
            );
        }
        assert!(
            !after_info.contains("bogus"),
            "an `info` write must drop the stray key: {after_info}"
        );
        assert_eq!(read_info_mtimes(&root), mtimes);

        write_resume_list(
            &root,
            &[],
            &[(
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "a".to_string(),
                "1".to_string(),
            )],
            &ResumeOpts::default(),
        )
        .unwrap();
        let after_resume = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        for fragment in fragments {
            assert!(
                after_resume.contains(fragment),
                "a `resume` write must keep {fragment}: {after_resume}"
            );
        }
        assert!(
            after_resume.contains(&format!(
                "\"version\": \"portuale-{}\"",
                env!("CARGO_PKG_VERSION")
            )),
            "a commit stamps the portuale version: {after_resume}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn writes_replace_the_file_atomically_with_fixed_mode() {
        // Portage's `atomic_ofstream` (same-directory temporary renamed
        // over the target) plus `apply_secpass_permissions(mode=0o644)`
        // (`lib/portage/util/__init__.py`, `util/mtimedb.py`): the write
        // lands whole, world-readable whatever the umask, and no
        // temporary survives it.
        let scratch = TempDir::new("mtimedb_test");
        let root = scratch.path().to_path_buf();
        std::fs::create_dir_all(mtimedb_path(&root).parent().unwrap()).unwrap();
        std::fs::write(mtimedb_path(&root), "{}\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(mtimedb_path(&root), std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(mtimedb_path(&root))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644,
                "the rewrite forces 0644 over the old 0600"
            );
        }
        assert_eq!(read_info_mtimes(&root), mtimes);
        let leftovers: Vec<_> = std::fs::read_dir(mtimedb_path(&root).parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "no rewrite temporary survives");
    }

    #[test]
    fn mtimedb_file_carries_the_portage_gid_when_permitted() {
        // `apply_secpass_permissions(gid=portage_gid)`: the record is
        // group-owned by `portage` whenever the writer may chown it; a
        // writer outside the group keeps its own gid, never failing.
        let Some(gid) = portage_gid() else {
            return;
        };
        // SAFETY: `geteuid`/`getgroups` take no other arguments.
        let permitted = unsafe { libc::geteuid() } == 0 || {
            let mut groups = vec![0 as libc::gid_t; 64];
            let n = unsafe { libc::getgroups(groups.len() as libc::c_int, groups.as_mut_ptr()) };
            n > 0 && groups[..n as usize].contains(&gid)
        };
        if !permitted {
            return;
        }
        let scratch = TempDir::new("mtimedb_test");
        let root = scratch.path().to_path_buf();
        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(mtimedb_path(&root)).unwrap().gid(),
                gid,
                "the record is group-owned by portage"
            );
        }
    }

    #[test]
    fn info_mtimes_accepts_a_foreign_float_and_reads_empty_as_empty() {
        let root = tmproot();
        // Absent file -> empty memo: nothing recorded yet.
        assert!(read_info_mtimes(&root).is_empty());

        std::fs::create_dir_all(mtimedb_path(&root).parent().unwrap()).unwrap();
        std::fs::write(
            mtimedb_path(&root),
            "{\n\t\"info\": {\n\t\t\"/usr/share/info\": 1700000000.0\n\t}\n}\n",
        )
        .unwrap();
        let mtimes = read_info_mtimes(&root);
        assert_eq!(mtimes.get("/usr/share/info"), Some(&1_700_000_000));

        // Clearing the memo stores `"info": {}` for `info` itself: like
        // Portage's `commit`, the file keeps every default key.
        write_info_mtimes(&root, &BTreeMap::new()).unwrap();
        assert!(read_info_mtimes(&root).is_empty());
        let text = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        assert!(
            text.contains("\"info\": {}")
                && text.contains("\"ldpath\": {}")
                && text.contains("\"starttime\": 0")
                && text.contains("\"updates\": {}"),
            "a clearing write keeps the default keys: {text}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_then_read_round_trips_the_resume_list() {
        let root = tmproot();
        write_resume_list(
            &root,
            &["dev-libs/foo", "dev-libs/bar"],
            &[
                (
                    ResumeEntryKind::Ebuild,
                    "dev-libs".to_string(),
                    "leaf-a".to_string(),
                    "1.0".to_string(),
                ),
                (
                    ResumeEntryKind::Ebuild,
                    "dev-libs".to_string(),
                    "leaf-b".to_string(),
                    "2.3-r1".to_string(),
                ),
            ],
            &ResumeOpts {
                oneshot: true,
                onlydeps: false,
                ..Default::default()
            },
        )
        .unwrap();

        let (favs, merges, opts) = read_resume_list(&root).expect("a resume list");
        assert_eq!(favs, vec!["dev-libs/foo", "dev-libs/bar"]);
        assert_eq!(
            merges,
            vec![
                (
                    ResumeEntryKind::Ebuild,
                    "dev-libs".to_string(),
                    "leaf-a".to_string(),
                    "1.0".to_string()
                ),
                (
                    ResumeEntryKind::Ebuild,
                    "dev-libs".to_string(),
                    "leaf-b".to_string(),
                    "2.3-r1".to_string()
                ),
            ]
        );
        assert_eq!(
            opts,
            ResumeOpts {
                oneshot: true,
                onlydeps: false,
                ..Default::default()
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_mixed_ebuild_and_binary_mergelist_round_trips_its_own_kind_tags() {
        // A resume mergelist can hold both entry types together.
        let root = tmproot();
        let mergelist = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "src-pkg".to_string(),
                "1.0".to_string(),
            ),
            (
                ResumeEntryKind::Binary,
                "dev-libs".to_string(),
                "bin-pkg".to_string(),
                "2.0".to_string(),
            ),
        ];
        write_resume_list(&root, &[], &mergelist, &ResumeOpts::default()).unwrap();

        let (_, merges, _) = read_resume_list(&root).expect("a resume list");
        assert_eq!(merges, mergelist);
        assert_eq!(merges[0].0, ResumeEntryKind::Ebuild);
        assert_eq!(merges[1].0, ResumeEntryKind::Binary);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn myopts_round_trips_onlydeps_and_defaults_to_none() {
        let root = tmproot();
        let cpv = &[(
            ResumeEntryKind::Ebuild,
            "c".to_string(),
            "p".to_string(),
            "1".to_string(),
        )][..];
        write_resume_list(&root, &[], cpv, &ResumeOpts::default()).unwrap();
        assert_eq!(read_resume_list(&root).unwrap().2, ResumeOpts::default());

        write_resume_list(
            &root,
            &[],
            cpv,
            &ResumeOpts {
                oneshot: false,
                onlydeps: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            read_resume_list(&root).unwrap().2,
            ResumeOpts {
                oneshot: false,
                onlydeps: true,
                ..Default::default()
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quiet_options_round_trip_and_keys_sort_like_python() {
        let root = tmproot();
        let cpv = &[(
            ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "a".to_string(),
            "1".to_string(),
        )][..];
        for quiet_build in [Some(true), Some(false), None] {
            let opts = ResumeOpts {
                oneshot: true,
                onlydeps: true,
                quiet: true,
                quiet_build,
                merge_wait_scope: None,
            };
            write_resume_list(&root, &[], cpv, &opts).unwrap();
            assert_eq!(read_resume_list(&root).unwrap().2, opts);
        }
        // A saved scope round-trips and sorts first in `myopts` (own
        // root: this write must not disturb the flag soup below).
        let scope_root = tmproot();
        let opts = ResumeOpts {
            oneshot: true,
            merge_wait_scope: Some(crate::pretend::MergeWaitScope::System),
            ..ResumeOpts::default()
        };
        write_resume_list(&scope_root, &[], cpv, &opts).unwrap();
        assert_eq!(read_resume_list(&scope_root).unwrap().2, opts);
        let scope_text = std::fs::read_to_string(mtimedb_path(&scope_root)).unwrap();
        assert!(scope_text.contains("\"--merge-wait-scope\": \"system\""));
        let at = |k: &str| scope_text.find(k).unwrap();
        assert!(at("--merge-wait-scope") < at("--oneshot"));
        let _ = std::fs::remove_dir_all(&scope_root);
        // `json.dumps(sort_keys=True)`: `--oneshot` < `--onlydeps` < `--quiet`.
        let text = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        let at = |k: &str| text.find(k).unwrap();
        assert!(at("--oneshot") < at("--onlydeps") && at("--onlydeps") < at("--quiet\""));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rotate_to_backup_preserves_a_multi_item_resume_list() {
        let root = tmproot();
        let mergelist = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "a".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "b".to_string(),
                "1".to_string(),
            ),
        ];
        write_resume_list(&root, &["dev-libs/a"], &mergelist, &ResumeOpts::default()).unwrap();

        rotate_resume_to_backup(&root);

        // "resume" itself is gone, but it stays recoverable via the backup.
        assert!(read_section(&root, "resume").is_none());
        let (favs, merges, _) = read_resume_list(&root).expect("promoted from resume_backup");
        assert_eq!(favs, vec!["dev-libs/a"]);
        assert_eq!(merges, mergelist);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rotate_to_backup_ignores_a_single_item_resume_list() {
        let root = tmproot();
        let mergelist = vec![(
            ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "a".to_string(),
            "1".to_string(),
        )];
        write_resume_list(&root, &[], &mergelist, &ResumeOpts::default()).unwrap();

        rotate_resume_to_backup(&root);

        // A single-package list isn't worth preserving, so "resume"
        // survives untouched.
        assert!(read_section(&root, "resume_backup").is_none());
        assert_eq!(read_resume_list(&root).unwrap().1, mergelist);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clear_resume_list_leaves_resume_backup_alone() {
        let root = tmproot();
        let mergelist = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "a".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "b".to_string(),
                "1".to_string(),
            ),
        ];
        write_resume_list(&root, &[], &mergelist, &ResumeOpts::default()).unwrap();
        rotate_resume_to_backup(&root);
        assert!(read_section(&root, "resume_backup").is_some());

        // A brand-new resume list, then cleared (as if it merged fully).
        write_resume_list(&root, &[], &mergelist, &ResumeOpts::default()).unwrap();
        clear_resume_list(&root);

        assert!(read_section(&root, "resume").is_none());
        assert!(
            read_section(&root, "resume_backup").is_some(),
            "clear_resume_list must not touch resume_backup"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_resume_list_preserves_an_existing_resume_backup() {
        let root = tmproot();
        let backup_list = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "old-a".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "old-b".to_string(),
                "1".to_string(),
            ),
        ];
        write_resume_list(&root, &[], &backup_list, &ResumeOpts::default()).unwrap();
        rotate_resume_to_backup(&root);
        assert!(read_section(&root, "resume").is_none());
        assert!(read_section(&root, "resume_backup").is_some());

        // A fresh failure writes a new "resume" -- the backup from the
        // *previous* abandoned run must survive untouched.
        let new_list = vec![(
            ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "new".to_string(),
            "2".to_string(),
        )];
        write_resume_list(&root, &[], &new_list, &ResumeOpts::default()).unwrap();

        let resume = read_section(&root, "resume").expect("new resume written");
        assert_eq!(resume.mergelist, new_list);
        let backup = read_section(&root, "resume_backup").expect("old backup preserved");
        assert_eq!(backup.mergelist, backup_list);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_merged_entry_shrinks_the_list_and_deletes_it_when_empty() {
        // After each successful merge the merged package leaves the list
        // (committed, so a SIGKILL still leaves the tail); once the list
        // empties, the `resume` key is deleted.
        let root = tmproot();
        let full = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "resume-a".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "resume-b".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Binary,
                "dev-libs".to_string(),
                "resume-c".to_string(),
                "2".to_string(),
            ),
        ];
        write_resume_list(&root, &["dev-libs/resume-a"], &full, &ResumeOpts::default()).unwrap();

        // After package 1 of 3 merges: the remaining 2.
        remove_merged_entry(&root, &full[0]);
        assert_eq!(read_resume_list(&root).unwrap().1, full[1..]);

        // After package 2 of 3: the remaining 1.
        remove_merged_entry(&root, &full[1]);
        assert_eq!(read_resume_list(&root).unwrap().1, full[2..]);

        // After the last merge: the `resume` key is gone, while the
        // file itself keeps every default key (Portage's `commit`
        // persists the whole dict, never unlinks the file).
        remove_merged_entry(&root, &full[2]);
        assert!(read_resume_list(&root).is_none());
        let text = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        assert!(
            !text.contains("\"resume\""),
            "no `resume` key may survive the last merge: {text}"
        );
        assert!(
            text.contains("\"updates\": {}"),
            "the default keys stay: {text}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_merged_entry_keeps_the_backup_and_ignores_unknown_entries() {
        let root = tmproot();
        let old = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "old-a".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "old-b".to_string(),
                "1".to_string(),
            ),
        ];
        write_resume_list(&root, &[], &old, &ResumeOpts::default()).unwrap();
        rotate_resume_to_backup(&root);
        let fresh = vec![(
            ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "new".to_string(),
            "2".to_string(),
        )];
        write_resume_list(&root, &["dev-libs/new"], &fresh, &ResumeOpts::default()).unwrap();

        // Removing an entry that is only in the backup (not in `resume`)
        // leaves the file byte-identical.
        let before = std::fs::read(mtimedb_path(&root)).unwrap();
        remove_merged_entry(&root, &old[0]);
        assert_eq!(std::fs::read(mtimedb_path(&root)).unwrap(), before);

        // Removing the last fresh entry deletes `resume` but keeps the
        // rotated backup recoverable.
        remove_merged_entry(&root, &fresh[0]);
        assert!(read_section(&root, "resume").is_none());
        let (_, back, _) = read_resume_list(&root).expect("backup promoted");
        assert_eq!(back, old);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_merged_entry_without_any_resume_file_is_a_noop() {
        let root = tmproot();
        let ghost = (
            ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "ghost".to_string(),
            "1".to_string(),
        );
        remove_merged_entry(&root, &ghost);
        assert!(!mtimedb_path(&root).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn driver_sequence_rotates_first_then_saves_up_front_and_shrinks_per_merge() {
        // The whole driver lifecycle in one hermetic sequence: (d) a stale
        // multi-item list rotates to backup first; (a) the fresh run saves
        // its full mergelist before the first merge; (b) each successful
        // merge shrinks it; (c) the last merge deletes the key -- while the
        // rotated backup survives the run.
        let root = tmproot();
        let stale = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "stale-a".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "stale-b".to_string(),
                "1".to_string(),
            ),
        ];
        write_resume_list(&root, &["stale"], &stale, &ResumeOpts::default()).unwrap();

        rotate_resume_to_backup(&root);
        assert!(read_section(&root, "resume").is_none());

        let full = vec![
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "run-a".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "run-b".to_string(),
                "1".to_string(),
            ),
            (
                ResumeEntryKind::Ebuild,
                "dev-libs".to_string(),
                "run-c".to_string(),
                "1".to_string(),
            ),
        ];
        write_resume_list(&root, &["fav"], &full, &ResumeOpts::default()).unwrap();
        assert_eq!(read_resume_list(&root).unwrap().1, full);

        remove_merged_entry(&root, &full[0]);
        assert_eq!(read_resume_list(&root).unwrap().1, full[1..]);

        remove_merged_entry(&root, &full[1]);
        remove_merged_entry(&root, &full[2]);
        assert!(read_section(&root, "resume").is_none());
        assert!(read_section(&root, "resume_backup").is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn up_front_save_of_an_all_noop_plan_overwrites_a_stale_list_with_empty() {
        // The up-front save assigns `mtimedb["resume"]` unconditionally at
        // merge start, so a run whose plan has nothing to merge still
        // commits the fresh (empty) list -- no stale entry survives. The
        // driver passes exactly the up-front-filtered mergelist (empty for
        // an all-noop plan), so the write itself must not no-op on empty.
        let root = tmproot();
        let stale = vec![(
            ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "stale".to_string(),
            "9.9".to_string(),
        )];
        write_resume_list(&root, &["dev-libs/stale"], &stale, &ResumeOpts::default()).unwrap();

        // A single-item stale list does not rotate -- the up-front save
        // overwrites it directly.
        rotate_resume_to_backup(&root);
        assert!(read_section(&root, "resume").is_some());

        write_resume_list(&root, &["dev-libs/noop"], &[], &ResumeOpts::default()).unwrap();
        // On disk the file still records the fresh run (favorites present)
        // with an empty mergelist -- and the stale entry is gone. Readers
        // treat an empty list as nothing to resume, so behaviour matches
        // "nothing left to merge" too.
        let content = std::fs::read_to_string(mtimedb_path(&root)).expect("mtimedb written");
        assert!(
            !content.contains("stale-9.9"),
            "no stale 1-item resume may survive: {content}"
        );
        assert!(
            content.contains("dev-libs/noop"),
            "the fresh run's favorites are recorded: {content}"
        );
        assert!(read_resume_list(&root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A fresh root's first commit carries every default key in Portage's
    /// own serialization (`sort_keys`, tab indent, no trailing newline),
    /// with `version` as `portuale-<crate version>`.
    /// Portage 3.0.83 `lib/portage/util/mtimedb.py:83-86, 105`
    /// (`setdefault` fills, `__write_to_disk` stamps `portage.VERSION`);
    /// bytes shape from S0 (a) (`t2/a-mtimedb`).
    #[test]
    fn fresh_file_has_portage_default_keys() {
        let scratch = TempDir::new("mtimedb_test");
        let root = scratch.path().to_path_buf();
        let mut updates = BTreeMap::new();
        updates.insert("/repo/profiles/updates/2Q-2024".to_string(), 1_700_000_002);
        write_updates_mtimes(&root, &updates).unwrap();

        let version = format!("portuale-{}", env!("CARGO_PKG_VERSION"));
        let expected = format!(
            "{{\n\t\"info\": {{}},\n\t\"ldpath\": {{}},\n\t\"starttime\": 0,\n\t\"updates\": {{\n\t\t\"/repo/profiles/updates/2Q-2024\": 1700000002\n\t}},\n\t\"version\": \"{version}\"\n}}"
        );
        assert_eq!(
            std::fs::read_to_string(mtimedb_path(&root)).unwrap(),
            expected
        );
    }

    /// An unknown top-level key is dropped with Portage's warning, not
    /// spliced back.
    /// Portage 3.0.83 `lib/portage/util/mtimedb.py:88-90`
    /// (`Deleting invalid mtimedb key: <k>`); S0 (b) (`t2/b-p.err`,
    /// `t2/b-mtimedb`).
    #[test]
    fn stray_key_is_dropped_with_portage_warning() {
        let scratch = TempDir::new("mtimedb_test");
        let root = scratch.path().to_path_buf();
        std::fs::create_dir_all(mtimedb_path(&root).parent().unwrap()).unwrap();
        std::fs::write(
            mtimedb_path(&root),
            "{\n\t\"bogus\": 1,\n\t\"updates\": {}\n}",
        )
        .unwrap();

        assert_eq!(
            load_mtimedb(&root).warnings,
            vec!["Deleting invalid mtimedb key: bogus".to_string()],
            "the load reports Portage's warning"
        );

        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();

        let text = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        assert!(
            !text.contains("bogus"),
            "the stray key is dropped on write: {text}"
        );
        assert_eq!(read_info_mtimes(&root), mtimes);
    }

    /// `old` becomes `updates` and `cur` is dropped, the way `_load`
    /// normalizes them before any default is filled.
    /// Portage 3.0.83 `lib/portage/util/mtimedb.py:77-81`.
    #[test]
    fn old_key_becomes_updates_and_cur_is_dropped() {
        let scratch = TempDir::new("mtimedb_test");
        let root = scratch.path().to_path_buf();
        std::fs::create_dir_all(mtimedb_path(&root).parent().unwrap()).unwrap();
        std::fs::write(
            mtimedb_path(&root),
            "{\n\t\"cur\": {\"a\": 1},\n\t\"old\": {\"/x/updates/1Q-2020\": 111},\n\t\"updates\": {}\n}",
        )
        .unwrap();

        assert_eq!(
            read_updates_mtimes(&root),
            BTreeMap::from([("/x/updates/1Q-2020".to_string(), 111)]),
            "`old` reads back as `updates`"
        );

        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();
        let text = std::fs::read_to_string(mtimedb_path(&root)).unwrap();
        assert!(
            !text.contains("\"old\"") && !text.contains("\"cur\""),
            "`old`/`cur` are gone after a write: {text}"
        );
        assert!(text.contains("/x/updates/1Q-2020"));
    }

    /// `commit` writes only when the state changed: rewriting identical
    /// content leaves the file (and its mtime) alone.
    /// Portage 3.0.83 `lib/portage/util/mtimedb.py:94-101`; S0 (c)
    /// (repeat run, mtime and md5 unchanged).
    #[test]
    fn unchanged_state_is_not_rewritten() {
        let scratch = TempDir::new("mtimedb_test");
        let root = scratch.path().to_path_buf();
        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();

        let path = mtimedb_path(&root);
        let before = std::fs::read(&path).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();

        write_info_mtimes(&root, &mtimes).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            old,
            "an unchanged write must not touch the file"
        );
    }

    /// A load followed by `read_*` and `write_*` calls reports the
    /// warning exactly once for that root: only the first load of each
    /// root reports warnings, per root.
    /// Portage 3.0.83 `lib/_emerge/actions.py:3380-3391` (one `MtimeDB`
    /// per root, each `_load` warning once).
    #[test]
    fn warning_printed_once_per_root_per_run() {
        let scratch = TempDir::new("mtimedb_test");
        let root = scratch.path().join("one");
        let other = scratch.path().join("two");
        for dir in [&root, &other] {
            std::fs::create_dir_all(mtimedb_path(dir).parent().unwrap()).unwrap();
            std::fs::write(mtimedb_path(dir), "{\n\t\"bogus\": 1\n}").unwrap();
        }

        assert_eq!(
            load_mtimedb(&root).warnings,
            vec!["Deleting invalid mtimedb key: bogus".to_string()]
        );
        // Reads and writes reuse the load: no warning again.
        assert!(read_updates_mtimes(&root).is_empty());
        let mut mtimes = BTreeMap::new();
        mtimes.insert("/usr/share/info".to_string(), 1_700_000_000);
        write_info_mtimes(&root, &mtimes).unwrap();
        assert!(load_mtimedb(&root).warnings.is_empty());
        // A read before any load silently loads: the later load is quiet.
        assert!(read_updates_mtimes(&other).is_empty());
        assert!(load_mtimedb(&other).warnings.is_empty());
    }
}
