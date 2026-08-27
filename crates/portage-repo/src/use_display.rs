//! USE flag display columns.

use crate::*;

/// The installed version's recorded `USE` / `IUSE` (both bare flag
/// names, `USE` already intersected with `IUSE`), for the `*`/`%`
/// diff markers. `None` for a `New` entry -> no markers.
pub(crate) struct InstalledUseState {
    pub(crate) old_use: HashSet<String>,
    pub(crate) old_iuse: HashSet<String>,
}

/// Natural sort key: split on runs of digits and compare those as
/// numbers, so `python3_9` sorts before `python3_12`.
/// Portage: output_helpers.py, _alnum_sort_key
///
/// A digit run longer than `u128` holds is kept as `Str`, still
/// deterministic.
pub fn alnum_sort_key(s: &str) -> Vec<AlnumKey> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        match rest.find(|c: char| c.is_ascii_digit()) {
            None => {
                out.push(AlnumKey::Str(rest.to_string()));
                break;
            }
            Some(start) => {
                out.push(AlnumKey::Str(rest[..start].to_string()));
                let digits_end = rest[start..]
                    .find(|c: char| !c.is_ascii_digit())
                    .map(|i| start + i)
                    .unwrap_or(rest.len());
                let digits = &rest[start..digits_end];
                out.push(match digits.parse::<u128>() {
                    Ok(n) => AlnumKey::Num(n),
                    Err(_) => AlnumKey::Str(digits.to_string()),
                });
                rest = &rest[digits_end..];
            }
        }
    }
    out
}

/// One segment of an `alnum_sort_key`: a non-digit run (`Str`) or a
/// digit run as a number (`Num`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlnumKey {
    Str(String),
    Num(u128),
}
pub(crate) struct BuildUseExpandDisplayArgs<'a> {
    pub(crate) use_flags_display: &'a [(String, bool)],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) installed: Option<&'a InstalledUseState>,
    pub(crate) forced: &'a HashSet<String>,
    pub(crate) all_flags: bool,
    pub(crate) reinst_flags: &'a HashSet<String>,
}
