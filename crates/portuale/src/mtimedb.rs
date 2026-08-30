//! `/var/cache/edb/mtimedb` persistence for three sections: the `resume`
//! mergelist (`emerge --resume`), its `resume_backup` rotation, and the
//! `info` dir-mtime memo. The file is Portage's JSON mtimedb shape
//! (hand-rolled, tab-indented); reading extracts each section with a
//! regex/brace-match rather than a full JSON parse.
// Portage: Scheduler.py, _save_resume_list; actions.py (--resume handling); util/mtimedb.py, MtimeDB

use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One mergelist entry's `type` tag (`"ebuild"` or `"binary"`): which kind
/// of merge `emerge --resume` replays this entry as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeEntryKind {
    Ebuild,
    Binary,
}

/// `(kind, category, package, version)` -- one entry of a resume
/// mergelist.
pub type ResumeCpv = (ResumeEntryKind, String, String, String);

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
