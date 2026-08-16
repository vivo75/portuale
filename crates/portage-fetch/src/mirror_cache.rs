// Portage's `${DISTDIR}/.mirror-cache.json` (`fetch.py::async_mirror_url`)
// and the URL helpers `async_mirror_url` uses to turn a mirror root plus
// a layout path into the URL (or local path) actually tried.
//
// The cache file is shared with Portage whenever both run against
// the same `DISTDIR`, so it must round-trip in both directions: Portage
// writes `json.dump({mirror_url: (time.time(), structure), ...})`, i.e.
// `{"http://distfiles.gentoo.org": [1788293350.3617969, [["filename-hash",
// "BLAKE2B", "8"]]]}`, and portuale writes exactly that shape back
// (Python's default `json.dump` separators and `ensure_ascii` escaping,
// entries in their original order, new ones appended -- a Python dict
// keeps insertion order).
//
// Pure: no I/O and no clock here; `portuale/src/fetch.rs` reads/writes the
// file and passes `now` in.

/// Python `json.dumps(str)` with the default `ensure_ascii=True`.
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
