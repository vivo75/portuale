// Port of `lib/portage/versions.py` (`ververify`, `vercmp`), kept
// structurally close to the original for diffing.
//
// Components wider than `i128` compare as decimal strings, mirroring
// Python's unbounded `int`, so no width of component can miscompare.

use regex::Regex;
use std::cmp::Ordering;
use std::sync::OnceLock;

fn ver_regexp() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d+)((?:\.\d+)*)([a-z]?)((?:_(?:pre|p|beta|alpha|rc)\d*)*)(?:-r(\d+))?$")
            .unwrap()
    })
}

fn suffix_regexp() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(alpha|beta|rc|pre|p)(\d*)$").unwrap())
}

fn cmp_bignum(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}
