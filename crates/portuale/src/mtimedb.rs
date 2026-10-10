// Real `/var/cache/edb/mtimedb` `resume` support (`_emerge/Scheduler.py::
// _save_resume_list` + `_emerge/actions.py`'s `--resume` handling): the
// resolved plan's own mergelist is saved to `mtimedb["resume"]`
// **before the first merge runs** (real `Scheduler.merge()`'s own
// `_save_resume_list` call, #168), so a SIGKILL mid-merge still leaves a
// list for `emerge --resume`. Each successful merge then removes its own
// entry (`remove_merged_entry`, real `Scheduler.py:1595-1602`), deleting
// the key once the list empties; a clean failure re-saves the
// still-unmerged tail. Each entry is `[type, root, cpv,
// operation]` along with the original atom args
// (`mtimedb["resume"]["favorites"]`). `emerge --resume` reads them back
// and merges them in order; `emerge --resume --skipfirst` drops the first
// (the one that failed) before continuing.
//
// The file is real portage's JSON `mtimedb`. Portuale writes a
// real-compatible `{"resume": {...}, "resume_backup": {...}}` (hand-rolled,
// tab-indented like real `json.dumps(indent="\t", sort_keys=True)`); it
// does NOT preserve any other top-level keys an existing file had
// (`info`/`ldpath`/`updates` are `--sync`/`env-update` state portuale never
// manages). Reading extracts each section with a regex/brace-match rather
// than a full JSON parse -- enough for a portuale-written file, and
// tolerant of a real-portage-written one with the same shape.
//
// `resume_backup` rotation IS real now (`rotate_resume_to_backup`,
// `actions.py:664-672`): right before a fresh, non-`--resume` merge starts,
// an existing `resume` entry whose own mergelist has more than one item is
// preserved as `resume_backup` (replacing whatever backup existed before)
// rather than just being overwritten -- `emerge --resume` later promotes
// `resume_backup` back to `resume` when `resume` itself is absent
// (`actions.py:222-224`, `read_resume_list`'s own fallback), so an
// accidental fresh `emerge <atom>` doesn't destroy a still-recoverable
// resume point. `clear_resume_list` only ever removes `resume` itself
// (real `Scheduler.py:1599-1601`'s own `del mtimedb["resume"]` once the
// mergelist empties), leaving `resume_backup` untouched.
//
// `mtimedb["info"]` IS real now too: real `MtimeDB` (`util/mtimedb.py`)
// carries an `"info"` key mapping each absolute GNU-info directory to
// that directory's `st_mtime` (whole seconds) the last time real
// `chk_updated_info_files` (`util/_info_files.py`) regenerated its
// `dir` index -- or skipped regenerating because the mtime already
// matched. `read_info_mtimes`/`write_info_mtimes` round-trip that map
// through the same file without disturbing `resume`/`resume_backup`
// (real `commit()` persists the whole dict at once).
//
// Two deliberate narrowings, both documented where they bite:
//   - real's file always carries every `_MTIMEDBKEYS` key (`info`,
//     `ldpath`, `resume`, `resume_backup`, `starttime`, `updates`,
//     `version`); portuale only ever writes the sections it manages
//     (`resume`, `resume_backup`, now `info`) and still drops the rest
//     -- `ldpath`/`updates` are `--sync`/`env-update` state portuale
//     never manages, `starttime`/`version` real bookkeeping with no
//     reader here. An empty `info` map is omitted outright (real would
//     write `"info": {}`); the file is removed when nothing at all is
//     stored, as before.
//   - values are whole-second `i64`s (real `os.stat(...)[stat.ST_MTIME]`
//     is already an int). A foreign-written float (`123.0`) is accepted
//     on read and truncated; what portuale writes back is always an
//     integer, matching real's steady state.
//
// Binary-entry replay IS real now too: each mergelist item carries real
// portage's own `type` tag (`"ebuild"` or `"binary"`, `ResumeEntryKind`),
// so `emerge --resume` can dispatch a resumed binary package through
// `emerge_getbinpkg::run_merge_plan` the same way `--getbinpkg` itself
// does, rather than only ever replaying source entries. This also closes
// the real "build-time flags (`--usepkg` etc.)" gap `myopts` used to have
// no room for: real portage re-derives usepkg/getbinpkg preference from
// `myopts` and re-decides binary-vs-source at resume time, but portuale's
// own mergelist already records that *decision* directly (each entry's own
// `ResumeEntryKind`, fixed at the point the original run resolved it) --
// arguably more robust than re-deciding from restored flags, so `myopts`
// itself still only carries `--oneshot`/`--onlydeps` (the two flags that
// govern *world*-recording, unrelated to which entries are binary).
// Cut: a resumed binary entry always resolves from the local `$PKGDIR`
// (`ResumeEntryKind::Binary` maps to `GraphEntry::remote_binary: false`)
// -- real re-fetches from a remote binhost too if the original run would
// have; portuale doesn't try to re-derive "was this fetched remotely"
// from the failed run's own state.

use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Real portage's own `mergelist` entry `type` tag
/// (`["ebuild"|"binary", <root>, <cpv>, <action>]`) -- which kind of
/// merge `emerge --resume` replays this entry as.
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

/// The subset of `mtimedb["resume"]["myopts"]` that changes how
/// `--resume` replays the mergelist: `--oneshot` (don't add the
/// `favorites` to `world`) and `--onlydeps` (the target was never in the
/// mergelist, so nothing is world-recorded).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumeOpts {
    pub oneshot: bool,
    pub onlydeps: bool,
}

/// `(favorites, mergelist, myopts)` from `mtimedb["resume"]`.
pub type ResumeList = (Vec<String>, Vec<ResumeCpv>, ResumeOpts);

/// One resume section's own data (`resume` or `resume_backup` both have
/// this same shape) -- an owned counterpart to `ResumeList` so it can be
/// stored, compared, and re-serialized freely.
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

/// Real `json.dumps(..., sort_keys=True)` shape for one section's own
/// body (no outer `"resume": { ... }` key wrapper -- the caller adds
/// that) -- shared by every section a write ever produces.
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
    // Real `json.dumps(..., sort_keys=True)` -> myopts keys alphabetical
    // (`--onlydeps` < `--oneshot`), value `true`.
    let mut opt_pairs: Vec<String> = Vec::new();
    if section.opts.onlydeps {
        opt_pairs.push(format!("\t\t\t{}: true", json_str("--onlydeps")));
    }
    if section.opts.oneshot {
        opt_pairs.push(format!("\t\t\t{}: true", json_str("--oneshot")));
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

/// Writes the whole mtimedb file from scratch, with `resume` and/or
/// `resume_backup` as given plus the `info` dir-mtime memo -- `None`
/// for a section omits it entirely. Removes the file outright when
/// there is nothing to store (an empty object has nothing real portage
/// or portuale itself would ever read back).
fn write_sections(
    root: &Path,
    resume: Option<&Section>,
    resume_backup: Option<&Section>,
    info: Option<&BTreeMap<String, i64>>,
) -> Result<(), String> {
    let path = mtimedb_path(root);
    let mut parts = Vec::new();
    if let Some(s) = resume {
        parts.push(format!(
            "\t\"resume\": {{\n{}\n\t}}",
            format_section_body(root, s)
        ));
    }
    if let Some(s) = resume_backup {
        parts.push(format!(
            "\t\"resume_backup\": {{\n{}\n\t}}",
            format_section_body(root, s)
        ));
    }
    if let Some(mtimes) = info
        && !mtimes.is_empty()
    {
        // Real `json.dumps(..., indent="\t", sort_keys=True)` shape for
        // the flat `{path: mtime}` map (`BTreeMap` iterates sorted, like
        // real's `sort_keys`).
        let entries: Vec<String> = mtimes
            .iter()
            .map(|(k, v)| format!("\t\t{}: {v}", json_str(k)))
            .collect();
        parts.push(format!("\t\"info\": {{\n{}\n\t}}", entries.join(",\n")));
    }
    if parts.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // Real `sort_keys=True` orders the top-level sections too
    // (`info` < `resume` < `resume_backup`); the pushes above already
    // append in that order.
    let content = format!("{{\n{}\n}}\n", parts.join(",\n"));
    std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))
}

/// The `{...}` object immediately following `"<key>":` in `content`,
/// brace-depth-matched (quoted strings' own `{`/`}` don't count) --
/// unlike a naive "split on the key, take up to the first `}`", this
/// stays correct once a second top-level section (`resume_backup`
/// trailing after `resume`, or vice versa) follows in the same file.
fn extract_object<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\"");
    let after_key = content.split_once(&marker)?.1;
    // Only whitespace and a single `:` may separate the key from its
    // own value -- real JSON's own `"key": {...}` shape. Not "find the
    // first `{` anywhere later", which would skip straight past this
    // key's own (possibly array-shaped, e.g. `"favorites"`) value into
    // some sibling key's own object instead.
    let body = after_key.trim_start().strip_prefix(':')?.trim_start();
    if !body.starts_with('{') {
        return None;
    }
    let bytes = body.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&body[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parses one section's own object text (`extract_object`'s own
/// return) into a `Section`, or `None` if it's missing/malformed.
fn parse_section(object: &str) -> Option<Section> {
    // Match `["ebuild"|"binary", "<root>", "<cat/pkg-ver>", "merge"]`;
    // the cpv is split into `(cat, pkg, ver)` in Rust below (the version
    // grammar is too broad for a clean regex group).
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

    // `myopts` -- its own brace-matched object (portuale only ever
    // writes flat `"--flag": true` pairs, so a plain `{...}` suffices).
    let myopts_block = extract_object(object, "myopts").unwrap_or("");
    let opts = ResumeOpts {
        oneshot: myopts_block.contains("\"--oneshot\""),
        onlydeps: myopts_block.contains("\"--onlydeps\""),
    };

    Some(Section {
        favorites,
        mergelist,
        opts,
    })
}

/// Reads one top-level section (`"resume"` or `"resume_backup"`) from
/// the mtimedb file on disk, if present and well-formed.
fn read_section(root: &Path, key: &str) -> Option<Section> {
    let content = std::fs::read_to_string(mtimedb_path(root)).ok()?;
    parse_section(extract_object(&content, key)?)
}

/// Real `mtimedb["info"]`: absolute GNU-info directory -> that
/// directory's `st_mtime` (whole seconds) as real
/// `chk_updated_info_files` last left it. Empty when the file is
/// absent or carries no `info` section.
pub fn read_info_mtimes(root: &Path) -> BTreeMap<String, i64> {
    let Ok(content) = std::fs::read_to_string(mtimedb_path(root)) else {
        return BTreeMap::new();
    };
    let Some(object) = extract_object(&content, "info") else {
        return BTreeMap::new();
    };
    // `"path": 123` pairs (real `json.dumps` of `{str: int}`); a
    // foreign-written float (`123.0`) is accepted and truncated --
    // what portuale writes back is always an integer (see this
    // module's own doc comment).
    let pair_re = Regex::new(r#""((?:[^"\\]|\\.)*)"\s*:\s*(-?\d+(?:\.\d+)?)"#).ok();
    let Some(pair_re) = pair_re else {
        return BTreeMap::new();
    };
    let mut mtimes = BTreeMap::new();
    for cap in pair_re.captures_iter(object) {
        let raw = &cap[1];
        let mut key = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c == '\\'
                && let Some(e) = chars.next()
            {
                match e {
                    'n' => key.push('\n'),
                    't' => key.push('\t'),
                    'r' => key.push('\r'),
                    other => key.push(other),
                }
            } else {
                key.push(c);
            }
        }
        if let Ok(v) = cap[2].parse::<f64>() {
            mtimes.insert(key, v as i64);
        }
    }
    mtimes
}

/// Stores `mtimedb["info"]` (real `MtimeDB.commit()` persisting the
/// whole dict at once): rewrites the file with `info` replaced,
/// preserving any existing `resume`/`resume_backup` untouched. An empty
/// map stores nothing for `info` itself (the file keeps just the resume
/// sections, or is removed when there is nothing at all -- see
/// `write_sections`).
pub fn write_info_mtimes(root: &Path, info: &BTreeMap<String, i64>) -> Result<(), String> {
    let resume = read_section(root, "resume");
    let backup = read_section(root, "resume_backup");
    write_sections(root, resume.as_ref(), backup.as_ref(), Some(info))
}

/// Writes `mtimedb["resume"]` from `favorites` (the atom args), `mergelist`
/// (one merge item per package, tagged `"ebuild"` or `"binary"` with its
/// `<root>` and `"<cat/pkg-ver>"`) and `myopts` (the `--oneshot` /
/// `--onlydeps` flags, so `--resume` replays with the same
/// world-recording behaviour). Used both for the up-front save of the full
/// resolved plan (real `Scheduler.merge()`'s own `_save_resume_list`, #168)
/// and for re-saving the still-unmerged tail on a clean failure. Like
/// real's own `_save_resume_list` (`Scheduler.py:2398-2431`) the assignment
/// is unconditional: an empty `mergelist` (an all-noop plan whose entries
/// all resolved `nomerge` / already-installed) still overwrites any stale
/// list with the fresh empty one -- real reaches `Scheduler.merge()` for
/// such a plan (no `--ask` early return, no `--pretend` early return) and
/// commits the empty list. Callers on the *failure* path guard the empty
/// case themselves (a failure with nothing unmerged leaves the per-merge
/// shrink's own tail alone). An existing `resume_backup` is preserved
/// untouched -- real's own resume assignment only ever touches the
/// `"resume"` key.
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
    let backup = read_section(root, "resume_backup");
    let info = read_info_mtimes(root);
    write_sections(root, Some(&resume), backup.as_ref(), Some(&info))
}

/// Real `Scheduler.py:1595-1602`: after each successful merge (committed
/// at once, "so that --resume still works after being interrupted by
/// reboot, sigkill or similar"), the merged package leaves
/// `mtimedb["resume"]["mergelist"]`; once the list empties,
/// `del mtimedb["resume"]`. Removes the first entry equal to `entry`
/// (real's own `list.remove` semantics); a no-op when there is no
/// `resume` section at all or the entry isn't listed (an
/// already-installed no-op "merge" reports success without ever being
/// saved). An emptied list deletes the `resume` key outright (removing
/// the file when no `resume_backup` survives); a non-empty remainder is
/// re-committed with an existing `resume_backup` preserved untouched.
pub fn remove_merged_entry(root: &Path, entry: &ResumeCpv) {
    let Some(mut resume) = read_section(root, "resume") else {
        return;
    };
    let Some(pos) = resume.mergelist.iter().position(|e| e == entry) else {
        return;
    };
    resume.mergelist.remove(pos);
    let backup = read_section(root, "resume_backup");
    let resume_opt = if resume.mergelist.is_empty() {
        None
    } else {
        Some(&resume)
    };
    let _ = write_sections(
        root,
        resume_opt,
        backup.as_ref(),
        Some(&read_info_mtimes(root)),
    );
}

/// Real `actions.py:664-672`: right before a fresh, non-`--resume`
/// merge starts, an existing `resume` entry whose own mergelist has
/// more than one item is preserved as `resume_backup` (replacing
/// whatever backup existed before -- real's own unconditional
/// `mtimedb["resume_backup"] = mtimedb["resume"]`), and `resume` itself
/// is cleared. A single-item mergelist, or no `resume` entry at all, is
/// a no-op -- matching real's own `len(...) > 1` guard exactly (a
/// one-package resume list isn't worth preserving as a "you can still
/// get this back" backup).
pub fn rotate_resume_to_backup(root: &Path) {
    let Some(resume) = read_section(root, "resume") else {
        return;
    };
    if resume.mergelist.len() <= 1 {
        return;
    }
    let _ = write_sections(root, None, Some(&resume), Some(&read_info_mtimes(root)));
}

/// Reads back `(favorites, mergelist-cpvs, myopts)` from
/// `mtimedb["resume"]`, or `None` when there's nothing to resume.
/// Real `actions.py:220-225`: when `resume` itself is absent but
/// `resume_backup` is present, `resume_backup` is promoted to `resume`
/// (and removed from its own backup slot) rather than treated as
/// "nothing to resume".
pub fn read_resume_list(root: &Path) -> Option<ResumeList> {
    if let Some(s) = read_section(root, "resume") {
        return Some((s.favorites, s.mergelist, s.opts));
    }
    let backup = read_section(root, "resume_backup")?;
    let _ = write_sections(root, Some(&backup), None, Some(&read_info_mtimes(root)));
    Some((backup.favorites, backup.mergelist, backup.opts))
}

/// `cat/pkg-1.2.3-r1` -> `("cat/pkg", "1.2.3-r1")`. Splits at the last
/// `-` whose following char is a digit (real `pkgsplit` heuristic).
fn split_cpv(cpv: &str) -> Option<(&str, &str)> {
    let bytes = cpv.as_bytes();
    for (i, _) in cpv.match_indices('-') {
        if bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit()) {
            return Some((&cpv[..i], &cpv[i + 1..]));
        }
    }
    None
}

/// Clears `mtimedb["resume"]` after a successful `--resume` run -- real
/// `Scheduler.py:1599-1601`'s own `del mtimedb["resume"]` once the
/// mergelist empties. Only ever removes `resume` itself, leaving
/// `resume_backup` (if any) untouched -- a real, if stale-by-then,
/// recovery point real portage doesn't clear here either.
pub fn clear_resume_list(root: &Path) {
    let backup = read_section(root, "resume_backup");
    let _ = write_sections(root, None, backup.as_ref(), Some(&read_info_mtimes(root)));
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
        // Real `MtimeDB.commit()`: one file, every key at once -- an
        // `info` write must not drop `resume`, and vice versa.
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
    fn info_mtimes_accepts_a_foreign_float_and_reads_empty_as_empty() {
        let root = tmproot();
        // Absent file -> empty memo (real's own `setdefault("info", {})`
        // shape: nothing recorded yet).
        assert!(read_info_mtimes(&root).is_empty());

        std::fs::create_dir_all(mtimedb_path(&root).parent().unwrap()).unwrap();
        std::fs::write(
            mtimedb_path(&root),
            "{\n\t\"info\": {\n\t\t\"/usr/share/info\": 1700000000.0\n\t}\n}\n",
        )
        .unwrap();
        let mtimes = read_info_mtimes(&root);
        assert_eq!(mtimes.get("/usr/share/info"), Some(&1_700_000_000));

        // Clearing the memo stores nothing for `info` (no resume
        // sections either, so the file goes away entirely).
        write_info_mtimes(&root, &BTreeMap::new()).unwrap();
        assert!(!mtimedb_path(&root).exists());
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
                onlydeps: false
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_mixed_ebuild_and_binary_mergelist_round_trips_its_own_kind_tags() {
        // Real portage's own resume mergelist can hold both types
        // together (`[type, root, cpv, action]`, `type` is `"ebuild"`
        // or `"binary"`) -- portuale's own used to only ever write
        // `"ebuild"`.
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
            },
        )
        .unwrap();
        assert_eq!(
            read_resume_list(&root).unwrap().2,
            ResumeOpts {
                oneshot: false,
                onlydeps: true
            }
        );
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

        // "resume" itself is gone -- a fresh emerge has nothing left to
        // silently "resume" from.
        assert!(read_section(&root, "resume").is_none());
        // But it's recoverable: read_resume_list promotes resume_backup.
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

        // Real's own `len(...) > 1` guard: a single-package list isn't
        // worth preserving, so "resume" survives untouched.
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
        // Real `Scheduler.py:1595-1602`: after each successful merge the
        // merged package leaves `mtimedb["resume"]["mergelist"]` (committed,
        // so a SIGKILL still leaves the tail for `--resume`); once the
        // list empties, `del mtimedb["resume"]`.
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

        // After the last merge: the `resume` key is gone (and with no
        // backup to preserve, the file itself is gone too).
        remove_merged_entry(&root, &full[2]);
        assert!(read_resume_list(&root).is_none());
        assert!(!mtimedb_path(&root).exists());
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
        // The whole #168 driver lifecycle in one hermetic sequence: (d) a
        // stale multi-item list rotates to backup first; (a) the fresh run
        // saves its full mergelist before the first merge; (b) each
        // successful merge shrinks it to the remainder; (c) the last merge
        // deletes the key -- while the rotated backup survives the run.
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
        // #179: real `Scheduler._save_resume_list`
        // (`Scheduler.py:2398-2431`) assigns `mtimedb["resume"]`
        // unconditionally at merge start, so a run whose plan has
        // nothing to merge still commits the fresh (empty) list -- no
        // stale entry survives. The driver calls `write_resume_list`
        // with exactly the up-front-filtered mergelist (empty for an
        // all-noop plan), so the write itself must not no-op on empty.
        let root = tmproot();
        let stale = vec![(
            ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "stale".to_string(),
            "9.9".to_string(),
        )];
        write_resume_list(&root, &["dev-libs/stale"], &stale, &ResumeOpts::default()).unwrap();

        // A single-item stale list does not rotate (real's own
        // `len(...) > 1` guard, `actions.py:664-672`) -- the up-front
        // save overwrites it directly.
        rotate_resume_to_backup(&root);
        assert!(read_section(&root, "resume").is_some());

        write_resume_list(&root, &["dev-libs/noop"], &[], &ResumeOpts::default()).unwrap();
        // Real-equal on disk: the file still records the fresh run
        // (favorites present) with an empty mergelist -- and the stale
        // entry is gone. Readers treat an empty list as nothing to
        // resume (`parse_section` maps it to `None`), so behaviour
        // matches real's "nothing left to merge" too.
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
}
