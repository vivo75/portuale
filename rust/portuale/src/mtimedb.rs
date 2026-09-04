// Real `/var/cache/edb/mtimedb` `resume` support (`_emerge/Scheduler.py::
// _save_resume_list` + `_emerge/actions.py`'s `--resume` handling): when a
// merge fails, the packages that still need merging are written to
// `mtimedb["resume"]["mergelist"]` (each entry `[type, root, cpv,
// operation]`) along with the original atom args
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
// v1 cuts: `--resume` only replays a *source* mergelist (a saved binary
// entry is merged as source). `myopts` records only the two flags that
// change how the mergelist is replayed -- `--oneshot` and `--onlydeps`
// (real portage stores every option); the build-time flags (`--usepkg`
// etc.) are the binary-entry-replay cut's concern.

use regex::Regex;
use std::path::{Path, PathBuf};

/// `(category, package, version)` -- one entry of a resume mergelist.
pub type ResumeCpv = (String, String, String);

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
        .map(|(cat, pkg, ver)| {
            format!(
                "\t\t\t[\n\t\t\t\t{},\n\t\t\t\t{},\n\t\t\t\t{},\n\t\t\t\t{}\n\t\t\t]",
                json_str("ebuild"),
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
/// `resume_backup` as given -- `None` for a key omits it entirely.
/// Removes the file outright when both are `None` (an empty object has
/// nothing real portage or portuale itself would ever read back).
fn write_sections(
    root: &Path,
    resume: Option<&Section>,
    resume_backup: Option<&Section>,
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
    if parts.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
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
    // Match `["ebuild", "<root>", "<cat/pkg-ver>", "merge"]`; the cpv is
    // split into `(cat, pkg, ver)` in Rust below (the version grammar is
    // too broad for a clean regex group).
    let entry_re =
        Regex::new(r#"\[\s*"ebuild"\s*,\s*"[^"]*"\s*,\s*"([^"]+)"\s*,\s*"merge"\s*\]"#).ok()?;

    let mut mergelist = Vec::new();
    for cap in entry_re.captures_iter(object) {
        let cpv = &cap[1];
        if let Some((cp, ver)) = split_cpv(cpv) {
            if let Some((cat, pkg)) = cp.split_once('/') {
                mergelist.push((cat.to_string(), pkg.to_string(), ver.to_string()));
            }
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

/// Writes `mtimedb["resume"]` for a failed merge: `favorites` (the atom
/// args) + `mergelist` (`["ebuild", <root>, "<cat/pkg-ver>", "merge"]`
/// per still-unmerged package) + `myopts` (the `--oneshot`/`--onlydeps`
/// flags, so `--resume` replays with the same world-recording
/// behaviour). No-op if `mergelist` is empty. Preserves an existing
/// `resume_backup` untouched -- real's own `mtimedb["resume"] = ...`
/// only ever assigns the `"resume"` key.
pub fn write_resume_list(
    root: &Path,
    favorites: &[&str],
    mergelist: &[ResumeCpv],
    opts: &ResumeOpts,
) -> Result<(), String> {
    if mergelist.is_empty() {
        return Ok(());
    }
    let resume = Section {
        favorites: favorites.iter().map(|s| s.to_string()).collect(),
        mergelist: mergelist.to_vec(),
        opts: *opts,
    };
    let backup = read_section(root, "resume_backup");
    write_sections(root, Some(&resume), backup.as_ref())
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
    let _ = write_sections(root, None, Some(&resume));
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
    let _ = write_sections(root, Some(&backup), None);
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
    let _ = write_sections(root, None, backup.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmproot() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mtimedb_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_then_read_round_trips_the_resume_list() {
        let root = tmproot();
        write_resume_list(
            &root,
            &["dev-libs/foo", "dev-libs/bar"],
            &[
                (
                    "dev-libs".to_string(),
                    "leaf-a".to_string(),
                    "1.0".to_string(),
                ),
                (
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
                    "dev-libs".to_string(),
                    "leaf-a".to_string(),
                    "1.0".to_string()
                ),
                (
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
    fn myopts_round_trips_onlydeps_and_defaults_to_none() {
        let root = tmproot();
        let cpv = &[("c".to_string(), "p".to_string(), "1".to_string())][..];
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
            ("dev-libs".to_string(), "a".to_string(), "1".to_string()),
            ("dev-libs".to_string(), "b".to_string(), "1".to_string()),
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
        let mergelist = vec![("dev-libs".to_string(), "a".to_string(), "1".to_string())];
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
            ("dev-libs".to_string(), "a".to_string(), "1".to_string()),
            ("dev-libs".to_string(), "b".to_string(), "1".to_string()),
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
            ("dev-libs".to_string(), "old-a".to_string(), "1".to_string()),
            ("dev-libs".to_string(), "old-b".to_string(), "1".to_string()),
        ];
        write_resume_list(&root, &[], &backup_list, &ResumeOpts::default()).unwrap();
        rotate_resume_to_backup(&root);
        assert!(read_section(&root, "resume").is_none());
        assert!(read_section(&root, "resume_backup").is_some());

        // A fresh failure writes a new "resume" -- the backup from the
        // *previous* abandoned run must survive untouched.
        let new_list = vec![("dev-libs".to_string(), "new".to_string(), "2".to_string())];
        write_resume_list(&root, &[], &new_list, &ResumeOpts::default()).unwrap();

        let resume = read_section(&root, "resume").expect("new resume written");
        assert_eq!(resume.mergelist, new_list);
        let backup = read_section(&root, "resume_backup").expect("old backup preserved");
        assert_eq!(backup.mergelist, backup_list);
        let _ = std::fs::remove_dir_all(&root);
    }
}
