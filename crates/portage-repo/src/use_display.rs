//! USE flag display columns.

use crate::*;

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
