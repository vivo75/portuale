// Native `filter-bash-environment`: keeps only the wanted variables.
//
// A byte-level port of `filter_bash_environment()`: stdin streams to
// stdout over raw bytes with no UTF-8 decoding; per line, first match
// wins (multi-line-quote continuation, then assignments and bare declares
// outside here-docs and functions, then here-doc and function lines).
// Python `re` on bytes becomes `regex::bytes::Regex` with `(?-u)`,
// `\A` for `.match` anchoring and `\n?\z` for a trailing `$`.
// Portage: bin/filter-bash-environment.py, filter_bash_environment

use std::borrow::Cow;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::ffi::OsStrExt;
use std::sync::OnceLock;

/// Translate raw PATTERN bytes: ASCII passes through, non-ASCII becomes
/// `\xHH`. `None` when a `\`-escape of a non-ASCII byte diverges.
fn rust_pattern_source(joined: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(joined.len());
    let mut escaped = false;
    for &b in joined {
        if escaped {
            if b >= 0x80 {
                return None;
            }
            out.push(b as char);
            escaped = false;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            out.push('\\');
        } else if b < 0x80 {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    Some(out)
}
