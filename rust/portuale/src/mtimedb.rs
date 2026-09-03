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
// real-compatible `{"resume": {...}}` (hand-rolled, tab-indented like
// real `json.dumps(indent="\t", sort_keys=True)`); it does NOT preserve
// any other top-level keys an existing file had (`info`/`ldpath`/
// `updates` are `--sync`/`env-update` state portuale never manages).
// Reading extracts the two `resume` arrays with a regex rather than a
// full JSON parse -- enough for a portuale-written file, and tolerant of a
// real-portage-written one with the same shape.
//
// v1 cuts: no `resume_backup` rotation (a cleared list is just deleted);
// `--resume` only replays a *source* mergelist (a saved binary entry is
// merged as source). `myopts` records only the two flags that change
// how the mergelist is replayed -- `--oneshot` and `--onlydeps` (real
// portage stores every option); the build-time flags (`--usepkg` etc.)
// are the binary-entry-replay cut's concern.

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

/// Writes `mtimedb["resume"]` for a failed merge: `favorites` (the atom
/// args) + `mergelist` (`["ebuild", <root>, "<cat/pkg-ver>", "merge"]`
/// per still-unmerged package) + `myopts` (the `--oneshot`/`--onlydeps`
/// flags, so `--resume` replays with the same world-recording
/// behaviour). No-op if `mergelist` is empty.
pub fn write_resume_list(
    root: &Path,
    favorites: &[&str],
    mergelist: &[ResumeCpv],
    opts: &ResumeOpts,
) -> Result<(), String> {
    if mergelist.is_empty() {
        return Ok(());
    }
    let path = mtimedb_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let root_str = root.display().to_string();
    let favs: Vec<String> = favorites
        .iter()
        .map(|f| format!("\t\t\t{}", json_str(f)))
        .collect();
    let merges: Vec<String> = mergelist
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
    if opts.onlydeps {
        opt_pairs.push(format!("\t\t\t{}: true", json_str("--onlydeps")));
    }
    if opts.oneshot {
        opt_pairs.push(format!("\t\t\t{}: true", json_str("--oneshot")));
    }
    let myopts = if opt_pairs.is_empty() {
        "{}".to_string()
    } else {
        format!("{{\n{}\n\t\t}}", opt_pairs.join(",\n"))
    };
    let content = format!(
        "{{\n\t\"resume\": {{\n\t\t\"favorites\": [\n{}\n\t\t],\n\t\t\"mergelist\": [\n{}\n\t\t],\n\t\t\"myopts\": {myopts}\n\t}}\n}}\n",
        favs.join(",\n"),
        merges.join(",\n")
    );
    std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads back `(favorites, mergelist-cpvs, myopts)` from
/// `mtimedb["resume"]`, or `None` when there's nothing to resume.
pub fn read_resume_list(root: &Path) -> Option<ResumeList> {
    let content = std::fs::read_to_string(mtimedb_path(root)).ok()?;
    // Only look inside the "resume" object.
    let resume = content.split("\"resume\"").nth(1)?;

    // Match `["ebuild", "<root>", "<cat/pkg-ver>", "merge"]`; the cpv is
    // split into `(cat, pkg, ver)` in Rust below (the version grammar is
    // too broad for a clean regex group).
    let entry_re =
        Regex::new(r#"\[\s*"ebuild"\s*,\s*"[^"]*"\s*,\s*"([^"]+)"\s*,\s*"merge"\s*\]"#).ok()?;

    let mut mergelist = Vec::new();
    for cap in entry_re.captures_iter(resume) {
        let cpv = &cap[1];
        // Split `cat/pkg-ver` -> the last `-<version>` where the version
        // starts with a digit (real `catpkgsplit` shape).
        if let Some((cp, ver)) = split_cpv(cpv) {
            if let Some((cat, pkg)) = cp.split_once('/') {
                mergelist.push((cat.to_string(), pkg.to_string(), ver.to_string()));
            }
        }
    }
    if mergelist.is_empty() {
        return None;
    }

    let fav_block = resume
        .split("\"favorites\"")
        .nth(1)
        .and_then(|s| s.split('[').nth(1))
        .and_then(|s| s.split(']').next())
        .unwrap_or("");
    let favorites: Vec<String> = Regex::new(r#""([^"]+)""#)
        .ok()?
        .captures_iter(fav_block)
        .map(|c| c[1].to_string())
        .collect();

    // `myopts` -- the object after `"myopts"`, up to its closing brace
    // (portuale only ever writes flat `"--flag": true` pairs, so the
    // first `}` closes it).
    let myopts_block = resume
        .split("\"myopts\"")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .unwrap_or("");
    let opts = ResumeOpts {
        oneshot: myopts_block.contains("\"--oneshot\""),
        onlydeps: myopts_block.contains("\"--onlydeps\""),
    };

    Some((favorites, mergelist, opts))
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

/// Clears `mtimedb["resume"]` after a successful `--resume` run -- the
/// portuale just removes the file (real rotates it to `resume_backup`).
pub fn clear_resume_list(root: &Path) {
    let _ = std::fs::remove_file(mtimedb_path(root));
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
    fn empty_mergelist_writes_nothing_and_reads_none() {
        let root = tmproot();
        write_resume_list(&root, &["x/y"], &[], &ResumeOpts::default()).unwrap();
        assert!(!mtimedb_path(&root).exists());
        assert!(read_resume_list(&root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clear_removes_the_file() {
        let root = tmproot();
        write_resume_list(
            &root,
            &[],
            &[("c".to_string(), "p".to_string(), "1".to_string())],
            &ResumeOpts::default(),
        )
        .unwrap();
        assert!(mtimedb_path(&root).exists());
        clear_resume_list(&root);
        assert!(!mtimedb_path(&root).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn split_cpv_splits_at_the_version() {
        assert_eq!(
            split_cpv("dev-libs/foo-1.2.3-r1"),
            Some(("dev-libs/foo", "1.2.3-r1"))
        );
        assert_eq!(split_cpv("cat/pkg-name-2.0"), Some(("cat/pkg-name", "2.0")));
        assert_eq!(split_cpv("cat/pkg"), None);
    }
}
