// Real `${DISTDIR}/.mirror-cache.json` (`fetch.py::async_mirror_url`)
// and the URL helpers `async_mirror_url` uses to turn a mirror root plus
// a layout path into the URL (or local path) actually tried.
//
// The cache file is SHARED with real portage whenever both run against
// the same `DISTDIR`, so it must round-trip in both directions: real
// writes `json.dump({mirror_url: (time.time(), structure), ...})`, i.e.
// `{"http://distfiles.gentoo.org": [1788293350.3617969, [["filename-hash",
// "BLAKE2B", "8"]]]}`, and portuale writes exactly that shape back
// (Python's default `json.dump` separators and `ensure_ascii` escaping,
// entries in their original order, new ones appended -- a Python dict
// keeps insertion order).
//
// Pure: no I/O and no clock here; `portuale/src/fetch.rs` reads/writes the
// file and passes `now` in.

/// One cache entry: when the mirror's `layout.conf` was last read
/// (seconds since the epoch, real `time.time()`), and its `[structure]`.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    pub mirror_url: String,
    pub timestamp: f64,
    pub structure: Vec<Vec<String>>,
}

/// Real `ts >= time.time() - 86400`: an entry refreshed at least daily.
pub const MIRROR_CACHE_TTL_SECS: f64 = 86400.0;

/// Parses the cache file. Anything that isn't the real shape -- invalid
/// JSON, a non-object, an entry that isn't `[number, [[str...]...]]` --
/// yields an empty cache, as real's `except (OSError, ValueError): pass`
/// does for unreadable JSON (real would crash on a well-formed but
/// wrongly-shaped entry; starting over is the non-crashing equivalent).
pub fn parse_mirror_cache(text: &str) -> Vec<CacheEntry> {
    let mut parser = Json {
        chars: text.chars().collect(),
        pos: 0,
    };
    let Some(value) = parser.value() else {
        return Vec::new();
    };
    parser.skip_ws();
    if parser.pos != parser.chars.len() {
        return Vec::new();
    }
    let JsonValue::Object(members) = value else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for (mirror_url, value) in members {
        let JsonValue::Array(pair) = value else {
            return Vec::new();
        };
        let [JsonValue::Number(timestamp), JsonValue::Array(structure)] = pair.as_slice() else {
            return Vec::new();
        };
        let mut vals = Vec::new();
        for val in structure {
            let JsonValue::Array(args) = val else {
                return Vec::new();
            };
            let mut strings = Vec::new();
            for arg in args {
                let JsonValue::String(s) = arg else {
                    return Vec::new();
                };
                strings.push(s.clone());
            }
            vals.push(strings);
        }
        // Real `json.load` keeps the LAST duplicate key.
        entries.retain(|e: &CacheEntry| e.mirror_url != mirror_url);
        entries.push(CacheEntry {
            mirror_url,
            timestamp: *timestamp,
            structure: vals,
        });
    }
    entries
}

/// Real `json.dump(cache, f)` output for the cache.
pub fn serialize_mirror_cache(entries: &[CacheEntry]) -> String {
    let body: Vec<String> = entries
        .iter()
        .map(|entry| {
            let structure: Vec<String> = entry
                .structure
                .iter()
                .map(|val| {
                    let args: Vec<String> = val.iter().map(|s| json_string(s)).collect();
                    format!("[{}]", args.join(", "))
                })
                .collect();
            format!(
                "{}: [{}, [{}]]",
                json_string(&entry.mirror_url),
                python_float_repr(entry.timestamp),
                structure.join(", ")
            )
        })
        .collect();
    format!("{{{}}}", body.join(", "))
}

/// Real `cache[mirror_url] = (time.time(), structure)`: replace in
/// place (dict order unchanged) or append.
pub fn upsert_mirror_cache(entries: &mut Vec<CacheEntry>, entry: CacheEntry) {
    match entries
        .iter_mut()
        .find(|e| e.mirror_url == entry.mirror_url)
    {
        Some(existing) => *existing = entry,
        None => entries.push(entry),
    }
}

/// Python `repr(float)`: shortest round-trip digits, always with a
/// fractional part or exponent.
fn python_float_repr(value: f64) -> String {
    let s = format!("{value}");
    if s.contains(['.', 'e', 'E', 'i', 'N']) {
        s
    } else {
        format!("{s}.0")
    }
}

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

/// Real `urlparse(url).scheme`, lower-cased; empty when there is none.
pub fn url_scheme(url: &str) -> String {
    match url.split_once(':') {
        Some((scheme, _))
            if scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) =>
        {
            scheme.to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

/// Real `urlparse(url).hostname` (lower-cased, no userinfo, no port),
/// rendered the way real's `f".layout.conf.{hostname}"` does -- `None`
/// when there is no network location.
pub fn url_hostname(url: &str) -> String {
    let after_scheme = if url_scheme(url).is_empty() {
        url
    } else {
        &url[url.find(':').map_or(0, |i| i + 1)..]
    };
    let Some(rest) = after_scheme.strip_prefix("//") else {
        return "None".to_string();
    };
    let netloc = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        "None".to_string()
    } else {
        host.to_ascii_lowercase()
    }
}

/// Real `urllib.parse.quote(path)` (default `safe="/"`): percent-encode
/// every UTF-8 byte except ASCII letters, digits, `_.-~` and `/`.
pub fn url_quote(path: &str) -> String {
    let mut out = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'~' | b'/') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Real `async_mirror_url`'s final step: `path` is the layout path
/// (`MirrorLayout::get_path`), URL-quoted for `ftp`/`http`/`https`
/// mirrors only; a `/`-rooted mirror is a local directory
/// (`os.path.join(mirror_url, path)`), anything else gets
/// `/distfiles/` inserted.
pub fn mirror_file_url(mirror_url: &str, path: &str) -> String {
    let path = if matches!(url_scheme(mirror_url).as_str(), "ftp" | "http" | "https") {
        url_quote(path)
    } else {
        path.to_string()
    };
    if mirror_url.starts_with('/') {
        // `os.path.join`: an absolute second part replaces the first.
        if path.starts_with('/') {
            path
        } else if mirror_url.ends_with('/') {
            format!("{mirror_url}{path}")
        } else {
            format!("{mirror_url}/{path}")
        }
    } else {
        format!("{mirror_url}/distfiles/{path}")
    }
}

enum JsonValue {
    Object(Vec<(String, JsonValue)>),
    Array(Vec<JsonValue>),
    String(String),
    Number(f64),
    Other,
}

/// Just enough JSON for the cache file (strict RFC 8259 syntax plus
/// Python's `NaN`/`Infinity` tokens, which `json.dump` can emit).
struct Json {
    chars: Vec<char>,
    pos: usize,
}

impl Json {
    fn skip_ws(&mut self) {
        while self
            .chars
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_whitespace())
        {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.chars.get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<JsonValue> {
        self.skip_ws();
        match *self.chars.get(self.pos)? {
            '{' => {
                self.pos += 1;
                let mut members = Vec::new();
                if self.eat('}') {
                    return Some(JsonValue::Object(members));
                }
                loop {
                    self.skip_ws();
                    let JsonValue::String(key) = self.value()? else {
                        return None;
                    };
                    if !self.eat(':') {
                        return None;
                    }
                    members.push((key, self.value()?));
                    if self.eat('}') {
                        return Some(JsonValue::Object(members));
                    }
                    if !self.eat(',') {
                        return None;
                    }
                }
            }
            '[' => {
                self.pos += 1;
                let mut items = Vec::new();
                if self.eat(']') {
                    return Some(JsonValue::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    if self.eat(']') {
                        return Some(JsonValue::Array(items));
                    }
                    if !self.eat(',') {
                        return None;
                    }
                }
            }
            '"' => {
                self.pos += 1;
                let mut s = String::new();
                loop {
                    let c = *self.chars.get(self.pos)?;
                    self.pos += 1;
                    match c {
                        '"' => return Some(JsonValue::String(s)),
                        '\\' => {
                            let esc = *self.chars.get(self.pos)?;
                            self.pos += 1;
                            match esc {
                                '"' | '\\' | '/' => s.push(esc),
                                'b' => s.push('\u{8}'),
                                'f' => s.push('\u{c}'),
                                'n' => s.push('\n'),
                                'r' => s.push('\r'),
                                't' => s.push('\t'),
                                'u' => {
                                    let unit = self.hex4()?;
                                    if (0xd800..0xdc00).contains(&unit)
                                        && self.chars.get(self.pos) == Some(&'\\')
                                        && self.chars.get(self.pos + 1) == Some(&'u')
                                    {
                                        self.pos += 2;
                                        let low = self.hex4()?;
                                        let code = 0x10000
                                            + ((unit as u32 - 0xd800) << 10)
                                            + (low as u32).wrapping_sub(0xdc00);
                                        s.push(char::from_u32(code)?);
                                    } else {
                                        s.push(char::from_u32(unit as u32)?);
                                    }
                                }
                                _ => return None,
                            }
                        }
                        c => s.push(c),
                    }
                }
            }
            _ => {
                let start = self.pos;
                while self
                    .chars
                    .get(self.pos)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.'))
                {
                    self.pos += 1;
                }
                let token: String = self.chars[start..self.pos].iter().collect();
                match token.as_str() {
                    "" => None,
                    "true" | "false" | "null" => Some(JsonValue::Other),
                    "NaN" => Some(JsonValue::Number(f64::NAN)),
                    "Infinity" => Some(JsonValue::Number(f64::INFINITY)),
                    "-Infinity" => Some(JsonValue::Number(f64::NEG_INFINITY)),
                    _ => token.parse::<f64>().ok().map(JsonValue::Number),
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u16> {
        let digits: String = self.chars.get(self.pos..self.pos + 4)?.iter().collect();
        self.pos += 4;
        u16::from_str_radix(&digits, 16).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache file as real portage wrote it on this project's dev host
    /// (`/var/cache/distfiles/.mirror-cache.json`, 2026-09-14).
    const REAL_CACHE: &str = r#"{"http://gentoo.mirror.root.lu": [1770760517.2913396, [["filename-hash", "BLAKE2B", "8"]]], "https://ftp.fau.de/gentoo": [1767201153.5657716, [["filename-hash", "BLAKE2B", "8"]]], "http://distfiles.gentoo.org": [1788293350.3617969, [["filename-hash", "BLAKE2B", "8"]]]}"#;

    #[test]
    fn a_real_portage_cache_round_trips_byte_for_byte() {
        let entries = parse_mirror_cache(REAL_CACHE);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].mirror_url, "http://distfiles.gentoo.org");
        assert_eq!(entries[2].timestamp, 1788293350.3617969);
        assert_eq!(
            entries[2].structure,
            vec![vec![
                "filename-hash".to_string(),
                "BLAKE2B".into(),
                "8".into()
            ]]
        );
        assert_eq!(serialize_mirror_cache(&entries), REAL_CACHE);
    }

    #[test]
    fn upsert_replaces_in_place_and_appends_new_mirrors() {
        let mut entries = parse_mirror_cache(REAL_CACHE);
        upsert_mirror_cache(
            &mut entries,
            CacheEntry {
                mirror_url: "https://ftp.fau.de/gentoo".into(),
                timestamp: 1800000000.0,
                structure: vec![],
            },
        );
        upsert_mirror_cache(
            &mut entries,
            CacheEntry {
                mirror_url: "/srv/mirror".into(),
                timestamp: 1800000000.5,
                structure: vec![vec!["flat".into()]],
            },
        );
        assert_eq!(
            serialize_mirror_cache(&entries),
            r#"{"http://gentoo.mirror.root.lu": [1770760517.2913396, [["filename-hash", "BLAKE2B", "8"]]], "https://ftp.fau.de/gentoo": [1800000000.0, []], "http://distfiles.gentoo.org": [1788293350.3617969, [["filename-hash", "BLAKE2B", "8"]]], "/srv/mirror": [1800000000.5, [["flat"]]]}"#
        );
    }

    #[test]
    fn unreadable_or_wrongly_shaped_caches_are_empty() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"m": 1}"#,
            r#"{"m": [1, [["flat", 2]]]}"#,
            r#"{"m": [1, []]} trailing"#,
        ] {
            assert!(parse_mirror_cache(text).is_empty(), "{text:?}");
        }
        assert_eq!(parse_mirror_cache("{}"), vec![]);
    }

    #[test]
    fn json_strings_escape_like_python_ensure_ascii() {
        let entries = vec![CacheEntry {
            mirror_url: "http://mïrror/\"q\"\\".into(),
            timestamp: 1.25,
            structure: vec![],
        }];
        let text = serialize_mirror_cache(&entries);
        // Python: `json.dumps({'http://mïrror/"q"\\': (1.25, ())})`.
        assert_eq!(text, r#"{"http://m\u00efrror/\"q\"\\": [1.25, []]}"#);
        assert_eq!(parse_mirror_cache(&text), entries);
    }

    /// Expected values: real `urlparse(...).scheme/.hostname` and
    /// `urllib.parse.quote`.
    #[test]
    fn url_helpers_match_python_urllib() {
        assert_eq!(url_scheme("HTTPS://Host.Example/x"), "https");
        assert_eq!(url_scheme("/srv/mirror"), "");
        assert_eq!(
            url_hostname("http://distfiles.gentoo.org"),
            "distfiles.gentoo.org"
        );
        assert_eq!(
            url_hostname("https://user:pw@Mirror.Example:8080/gentoo"),
            "mirror.example"
        );
        assert_eq!(url_hostname("http://[::1]:8080/x"), "::1");
        assert_eq!(url_hostname("/srv/mirror"), "None");
        assert_eq!(url_quote("80/a b+c~d_e.f-g"), "80/a%20b%2Bc~d_e.f-g");
        assert_eq!(url_quote("2e/über.tgz"), "2e/%C3%BCber.tgz");
    }

    #[test]
    fn mirror_file_url_quotes_only_web_schemes_and_joins_local_dirs() {
        assert_eq!(
            mirror_file_url("http://distfiles.gentoo.org", "80/a b.tgz"),
            "http://distfiles.gentoo.org/distfiles/80/a%20b.tgz"
        );
        assert_eq!(
            mirror_file_url("rsync://mirror.example/gentoo", "80/a b.tgz"),
            "rsync://mirror.example/gentoo/distfiles/80/a b.tgz"
        );
        assert_eq!(
            mirror_file_url("/srv/mirror", "80/a b.tgz"),
            "/srv/mirror/80/a b.tgz"
        );
        assert_eq!(
            mirror_file_url("/srv/mirror/", "a.tgz"),
            "/srv/mirror/a.tgz"
        );
    }
}
