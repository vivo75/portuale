// Native `filter-bash-environment` (#326 S3, D1/D3/D4/D7): a byte-level
// port of `bin/filter-bash-environment.py`'s `filter_bash_environment()`
// (all 164 lines of it), routed from the `python` dispatcher (D1): a
// first argument whose basename is `filter-bash-environment.py` comes
// here with the remaining args. The vendored script itself is untouched
// (the call site stays upstream's; in D2 `real` mode
// `PORTAGE_PYTHON=/usr/bin/python` still runs it).
//
// Behaviour is real's, in real's order, over raw bytes: stdin is split
// exactly like Python binary-file iteration (lines end at `\n`
// inclusive; the last line may lack it), stdin streams to stdout
// (`BufReader`/`BufWriter`), and there is no UTF-8 decoding anywhere.
// Per line, first match wins:
// - a multi-line-quote continuation line (written with `\x01` stripped
//   when its variable is kept; it clears the quote when
//   `have_end_quote` says so);
// - then, only when not in a here-doc or function, a variable
//   assignment (a kept line gets the `declare -r` rewrite and the
//   `\x01` strip) or a `declare` without assignment (a kept line gets
//   the rewrite but NO `\x01` strip);
// - then a here-doc delimiter line, a here-doc start line, a function
//   body line, a function start line, else pass-through.
// The end-quote search skips the start quote exactly like real:
// `line[end(name)+2..]` (past `=` and the opening quote).
//
// Regex translation (Python `re` on bytes -> `regex::bytes::Regex`):
// - `(?-u)` everywhere, so `\w \s \d \W` are ASCII-only, like Python
//   bytes patterns (on bytes Python never matches non-ASCII word
//   bytes). `.` never matches `\n` in either engine; `(?s)` is NOT set.
// - Python `$` (no MULTILINE) matches at end or just before a final
//   `\n`. Every line here carries its `\n`, so each trailing `$` is
//   written `\n?\z`, consistently, including the runtime here-doc
//   delimiter `^DELIM$` -> `\AD ELIM\n?\z` (DELIM is `\w+`, so, as in
//   real, nothing is escaped).
// - Python `.match` anchors at the start only: every anchored pattern
//   gets `\A` (`close_quote_re.search` stays unanchored).
//
// | Python (bytes, `.match` unless noted) | Rust (`regex::bytes`)             |
// | `.*\s<<[-]?(\w+)$`                    | `(?-u)\A.*\s<<[-]?(\w+)\n?\z`      |
// | `^[-\w]+\s*\(\)\s*$`                  | `(?-u)\A[-\w]+\s*\(\)\s*\n?\z`     |
// | `^\}$`                                | `(?-u)\A\}\n?\z`                   |
// | `(^|^declare\s+-\S+\s+|^declare\s+    | `(?-u)\A(?:declare\s+-\S+\s+       |
// |   |^export\s+)([^=\s]+)=("|\')?.*$`   |   |declare\s+|export\s+)?          |
// |                                       |   ([^=\s]+)=(["'])?.*\n?\z`       |
// |   (groups 1=prefix 2=name 3=quote)    |   (groups 1=name 2=quote)          |
// | `(\\"|"|\')\s*$` (`.search`)          | `(?-u)(\\"|["'])\s*\n?\z` (search) |
// | `^declare\s+-(\S*)r(\S*)\s+`          | `(?-u)\Adeclare\s+-(\S*)r(\S*)\s+` |
// | `^declare(\s+-\S+)?\s+([^=\s]+)\s*$`  | `(?-u)\Adeclare(?:\s+-\S+)?\s+     |
// |   (groups 1=flags 2=name)             |   ([^=\s]+)\s*\n?\z` (group 1=name)|
// | runtime `^DELIM$`                     | `(?-u)\ADELIM\n?\z`                |
// | var `^(t1|..\|\d.*|.*\W.*)$`          | `(?-u)\A(?:t1|..\|\d.*|.*\W.*)\n?\z`|
// |   (`.match(name)`)                    |   (`is_match(name)`)                |
//
// The PATTERN argument uses Python regex syntax (real joins
// `argv[1].split()` -- an ASCII-whitespace split, empties dropped --
// plus `\d.*` and `.*\W.*` with `|`). Accepted: everything
// `regex::bytes` compiles with identical meaning to Python `re` on
// bytes -- literal names, `.`, `*`/`+`/`?`/`{m,n}`, `.*`, `\d \D \s
// \S \w \W`, `[...]`, `|` (real joins the tokens with it),
// `(...)`/`(?:...)`/`(?P<name>...)`, `^ $ \A \z \b \B`, `\\` escapes,
// `(?i)`-style scoped flags with Python-identical meaning. Loud
// failure (stderr message + exit 1, empty stdout, the
// `error-bad-pattern` rc): anything the Rust engine rejects
// (lookarounds, backrefs, `\Z`, `\C`, octal escapes -- all
// Python-`re`-only), plus anything Rust would accept with a different
// meaning than Python-on-bytes: `\p`/`\P` (no Python `re` equivalent)
// and `(?u`/`(?U` flag runs outside a character class (bytes patterns
// cannot set UNICODE in Python; Rust would flip `\w` et al. to
// Unicode); a `[` inside a class (`[a[b]]`, POSIX `[[:alpha:]]`: Rust
// nests, Python reads it literally) and `&&`/`--`/`~~` inside a class
// (Rust set operators, Python literals or a bad range); `(?<name>`
// (a Rust named group, a Python error). PATTERN bytes outside ASCII become `\xHH` escapes (a
// literal-byte match in both engines); a `\`-escape of a non-ASCII
// byte is a bad escape in Python, so it fails loudly too.
//
// argv (real's `__main__`): `-h`/`--help` anywhere in the args after
// the script -> `usage: <script-basename> PATTERN` on stdout, exit 0;
// anything but exactly one PATTERN arg -> the usage plus
// `Exactly one PATTERN argument required.` on stderr, exit 2.

use std::borrow::Cow;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::ffi::OsStrExt;
use std::sync::OnceLock;

/// Entry from the `python` dispatcher: `script` is the argv element
/// whose basename is `filter-bash-environment.py` (real's
/// `sys.argv[0]`, the usage basename), `args` is everything after it
/// (real's `sys.argv[1:]`). Returns the process exit code; stdout stays
/// empty on every non-zero exit (the failure paths return before the
/// filter runs, as in real, where `re.compile` throws first).
pub(crate) fn run(script: &OsString, args: &[OsString]) -> i32 {
    let usage = format!(
        "usage: {} PATTERN",
        String::from_utf8_lossy(basename_of(script))
    );
    if args.iter().any(|a| {
        let b = a.as_os_str().as_bytes();
        b == b"-h" || b == b"--help"
    }) {
        println!("{usage}");
        return 0;
    }
    if args.len() != 1 {
        eprintln!("{usage}");
        eprintln!("Exactly one PATTERN argument required.");
        return 2;
    }
    let pattern = match compile_var_pattern(args[0].as_os_str().as_bytes()) {
        Ok(re) => re,
        Err(message) => {
            eprintln!("portuale: filter-bash-environment: {message}");
            return 1;
        }
    };
    let stdin = std::io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    if let Err(e) = run_filter(&pattern, &mut input, &mut output) {
        eprintln!("portuale: filter-bash-environment: I/O error: {e}");
        return 1;
    }
    if let Err(e) = output.flush() {
        eprintln!("portuale: filter-bash-environment: I/O error: {e}");
        return 1;
    }
    0
}

/// The raw basename of a script argv element (bytes after the last
/// `/`), like `os.path.basename`.
fn basename_of(arg: &OsString) -> &[u8] {
    let bytes = arg.as_os_str().as_bytes();
    match bytes.iter().rposition(|b| *b == b'/') {
        Some(i) => &bytes[i + 1..],
        None => bytes,
    }
}

/// Real's `__main__` pattern build: `argv[1].split()` (an
/// ASCII-whitespace split, empties dropped) plus the appended `\d.*`
/// (drop digit-leading names: not valid bash) and `.*\W.*` (drop names
/// with non-bash characters), compiled as `^(...)$`. Returns the
/// loud-failure message when the tokens have no Python-bytes
/// equivalent (see the module docs).
fn compile_var_pattern(raw: &[u8]) -> Result<regex::bytes::Regex, String> {
    let mut alts: Vec<&[u8]> = raw
        .split(|b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
        .filter(|t| !t.is_empty())
        .collect();
    alts.push(b"\\d.*");
    alts.push(b".*\\W.*");
    let joined = alts.join(&b"|"[..]);
    if pattern_is_divergent(&joined) {
        return Err("PATTERN uses regex syntax with no Python-bytes equivalent".to_string());
    }
    let Some(body) = rust_pattern_source(&joined) else {
        return Err("PATTERN uses regex syntax with no Python-bytes equivalent".to_string());
    };
    let src = format!("(?-u)\\A(?:{body})\\n?\\z");
    regex::bytes::Regex::new(&src).map_err(|e| format!("invalid PATTERN ({e})"))
}

/// True when the joined token bytes would compile in Rust with a
/// different meaning than Python `re` on bytes (the module docs list
/// the cases): `\p`/`\P` anywhere an escape is active, and a `u`/`U`
/// flag inside an inline `(?...)` group outside a character class.
fn pattern_is_divergent(joined: &[u8]) -> bool {
    let mut escaped = false;
    let mut in_class = false;
    let mut i = 0;
    while i < joined.len() {
        let c = joined[i];
        if escaped {
            if c == b'p' || c == b'P' {
                return true;
            }
            escaped = false;
            i += 1;
            continue;
        }
        if in_class {
            match c {
                b'\\' => escaped = true,
                // Rust nests classes (`[a[b]]`, POSIX `[[:alpha:]]`) and
                // has set operators; Python reads all of these literally.
                b'[' => return true,
                b'&' | b'-' | b'~' if joined.get(i + 1) == Some(&c) => return true,
                b']' => in_class = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match c {
            b'\\' => escaped = true,
            b'[' => {
                in_class = true;
                // A `]` right after `[` or `[^` is a literal in both.
                if joined.get(i + 1) == Some(&b'^') {
                    i += 1;
                }
                if joined.get(i + 1) == Some(&b']') {
                    i += 1;
                }
            }
            // `(?<name>` is a named group in Rust, an error in Python
            // (which spells it `(?P<name>`); `(?<=`/`(?<!` are
            // lookbehinds Rust rejects anyway.
            b'(' if joined[i..].starts_with(b"(?<")
                && !matches!(joined.get(i + 3), Some(b'=' | b'!')) =>
            {
                return true;
            }
            // `(?P<name>` / `(?P=name)` mean the same in both engines;
            // anything else opening with letters is an inline-flag run,
            // where `u`/`U` diverges.
            b'(' if joined[i..].starts_with(b"(?") && joined.get(i + 2) != Some(&b'P') => {
                let mut j = i + 2;
                while j < joined.len() && joined[j].is_ascii_alphabetic() {
                    if joined[j] == b'u' || joined[j] == b'U' {
                        return true;
                    }
                    j += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Translate raw PATTERN bytes into the Rust pattern string: ASCII
/// bytes pass through (every regex metacharacter is ASCII, with the
/// same meaning in both engines); each non-ASCII byte becomes a
/// `\xHH` escape (a literal-byte match in both). `None` when the bytes
/// cannot mean the same in both engines: a `\`-escape of a non-ASCII
/// byte is a bad escape in Python `re`.
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

/// The static line patterns (the translation table lives in the module
/// docs). Group numbering differs from real's where a prefix group
/// became non-capturing; the uses below name the new numbers.
struct LineRes {
    here_doc: regex::bytes::Regex,
    func_start: regex::bytes::Regex,
    func_end: regex::bytes::Regex,
    var_assign: regex::bytes::Regex,
    close_quote: regex::bytes::Regex,
    readonly: regex::bytes::Regex,
    var_declare: regex::bytes::Regex,
}

fn res() -> &'static LineRes {
    static CELL: OnceLock<LineRes> = OnceLock::new();
    CELL.get_or_init(|| LineRes {
        here_doc: regex::bytes::Regex::new(r"(?-u)\A.*\s<<[-]?(\w+)\n?\z").unwrap(),
        func_start: regex::bytes::Regex::new(r"(?-u)\A[-\w]+\s*\(\)\s*\n?\z").unwrap(),
        func_end: regex::bytes::Regex::new(r"(?-u)\A\}\n?\z").unwrap(),
        var_assign: regex::bytes::Regex::new(
            r#"(?-u)\A(?:declare\s+-\S+\s+|declare\s+|export\s+)?([^=\s]+)=(["'])?.*\n?\z"#,
        )
        .unwrap(),
        close_quote: regex::bytes::Regex::new(r#"(?-u)(\\"|["'])\s*\n?\z"#).unwrap(),
        readonly: regex::bytes::Regex::new(r"(?-u)\Adeclare\s+-(\S*)r(\S*)\s+").unwrap(),
        var_declare: regex::bytes::Regex::new(r"(?-u)\Adeclare(?:\s+-\S+)?\s+([^=\s]+)\s*\n?\z")
            .unwrap(),
    })
}

/// Real's `have_end_quote`: the line ends (modulo trailing whitespace)
/// with the opening quote byte. `\\"` (backslash-quote) at the end is
/// NOT the end quote -- group 1 is then two bytes, never equal to the
/// one-byte quote.
fn have_end_quote(quote: u8, line: &[u8]) -> bool {
    res()
        .close_quote
        .captures(line)
        .is_some_and(|caps| caps.get(1).is_some_and(|m| m.as_bytes() == [quote]))
}

/// Real's `filter_declare_readonly_opt`: drop the `r` from a
/// `declare -<opts>r<opts>` flag run (`declare -ar` -> `declare -a`,
/// bare `declare -r` -> `declare `).
fn filter_declare_readonly_opt(line: &[u8]) -> Cow<'_, [u8]> {
    let Some(caps) = res().readonly.captures(line) else {
        return Cow::Borrowed(line);
    };
    let end = caps.get(0).unwrap().end();
    let mut opts = Vec::new();
    // `(\S*)` always participates, so both groups are present.
    opts.extend_from_slice(caps.get(1).unwrap().as_bytes());
    opts.extend_from_slice(caps.get(2).unwrap().as_bytes());
    let mut out = Vec::with_capacity(line.len());
    if opts.is_empty() {
        out.extend_from_slice(b"declare ");
    } else {
        out.extend_from_slice(b"declare -");
        out.extend_from_slice(&opts);
        out.extend_from_slice(b" ");
    }
    out.extend_from_slice(&line[end..]);
    Cow::Owned(out)
}

/// Real's `line.replace(b"\1", b"")` (bug 222091: bash multiplies
/// `\x01` bytes on every env save).
fn strip_ctrl_a(line: &[u8]) -> Cow<'_, [u8]> {
    if !line.contains(&1) {
        return Cow::Borrowed(line);
    }
    Cow::Owned(line.iter().copied().filter(|b| *b != 1).collect())
}

/// Real builds the delimiter matcher as `re.compile(b"^" + group +
/// b"$")` (nothing escaped); the group is `\w+`, so it is regex-safe
/// by construction.
fn here_doc_delimiter(delim: &[u8]) -> regex::bytes::Regex {
    let mut src = String::from("(?-u)\\A");
    src.push_str(std::str::from_utf8(delim).expect("here-doc delimiter is \\w+ (ASCII)"));
    src.push_str("\\n?\\z");
    regex::bytes::Regex::new(&src).expect("here-doc delimiter is \\w+ (regex-safe)")
}

/// Real's `filter_bash_environment`, in real's order (see the module
/// docs). Lines are raw byte slices ending at `\n` inclusive, except a
/// final line without one.
fn run_filter(
    pattern: &regex::bytes::Regex,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> std::io::Result<()> {
    let r = res();
    let mut here_doc_delim: Option<regex::bytes::Regex> = None;
    let mut in_func = false;
    // The open multi-line quote byte and whether its variable matched.
    let mut multi: Option<(u8, bool)> = None;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let line: &[u8] = &buf;
        // Multi-line quoted value first.
        if let Some((quote, filter_this)) = multi {
            if !filter_this {
                output.write_all(&strip_ctrl_a(line))?;
            }
            if have_end_quote(quote, line) {
                multi = None;
            }
            continue;
        }
        // Assignments and bare declares only outside here-docs and
        // function bodies.
        if here_doc_delim.is_none() && !in_func {
            if let Some(caps) = r.var_assign.captures(line) {
                let name = caps.get(1).unwrap();
                let quote = caps.get(2).map(|m| m.as_bytes()[0]);
                let filter_this = pattern.is_match(name.as_bytes());
                // Skip the start quote when searching for the end
                // quote, so a newline right after it does not read as
                // an immediate end: past `=` and the opening quote.
                if let Some(q) = quote {
                    let rest = &line[name.end() + 2..];
                    if !have_end_quote(q, rest) {
                        multi = Some((q, filter_this));
                    }
                }
                if !filter_this {
                    let line = filter_declare_readonly_opt(line);
                    output.write_all(&strip_ctrl_a(&line))?;
                }
                continue;
            }
            if let Some(caps) = r.var_declare.captures(line) {
                let filter_this = pattern.is_match(caps.get(1).unwrap().as_bytes());
                if !filter_this {
                    let line = filter_declare_readonly_opt(line);
                    // No `\x01` strip on this path, like real.
                    output.write_all(&line)?;
                }
                continue;
            }
        }
        // Here-documents before functions, so doc content cannot read
        // as a function end.
        if let Some(delim) = &here_doc_delim {
            if delim.is_match(line) {
                here_doc_delim = None;
            }
            output.write_all(line)?;
            continue;
        }
        if let Some(caps) = r.here_doc.captures(line) {
            here_doc_delim = Some(here_doc_delimiter(caps.get(1).unwrap().as_bytes()));
            output.write_all(line)?;
            continue;
        }
        if in_func {
            if r.func_end.is_match(line) {
                in_func = false;
            }
            output.write_all(line)?;
            continue;
        }
        if r.func_start.is_match(line) {
            in_func = true;
            output.write_all(line)?;
            continue;
        }
        output.write_all(line)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::Path;
    use std::process::Stdio;

    /// The built `portuale` binary next to this test binary (child
    /// processes, never in-process: the helper reads stdin and the
    /// failure paths replace the process image in the transition rows,
    /// so only a real process boundary matches the shim's call shape).
    fn portuale_exe() -> std::path::PathBuf {
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        exe
    }

    /// Run the native filter exactly like the shim would:
    /// `portuale __helper python filter-bash-environment.py <args>`,
    /// stdin bytes verbatim. (The `args` fixture file bytes ARE
    /// argv[1]: no trailing newline is added, none is present, so this
    /// matches `"$(cat args)"` byte for byte.)
    fn run_native(pattern: &[u8], stdin_bytes: &[u8]) -> std::process::Output {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        // A pipe could deadlock past one pipe buffer (both sides stream),
        // so stdin goes through a temp file while stdout/stderr stream.
        let tmp = portage_util::TempDir::new("helper-filter-env-stdin");
        let input_path = tmp.join("in");
        std::fs::write(&input_path, stdin_bytes).unwrap();
        let input_file = std::fs::File::open(&input_path).unwrap();
        std::process::Command::new(portuale_exe())
            .args(["__helper", "python", "filter-bash-environment.py"])
            .arg(OsStr::from_bytes(pattern))
            .stdin(Stdio::from(input_file))
            .output()
            .expect("portuale __helper spawns")
    }

    /// Read a case side that is raw (`stem`) or bzip2-compressed
    /// (`stem.bz2`, for inputs over 64 KiB).
    fn read_case_side(dir: &Path, stem: &str) -> Vec<u8> {
        let compressed = dir.join(format!("{stem}.bz2"));
        if compressed.is_file() {
            let file = std::fs::File::open(&compressed).unwrap();
            let mut decoder = bzip2::read::BzDecoder::new(file);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).unwrap();
            out
        } else {
            std::fs::read(dir.join(stem)).unwrap()
        }
    }

    /// 1-based number of the first line where two byte strings differ
    /// (lines split at `\n`, a missing trailing newline still counts as
    /// a final line), for the failure report.
    fn first_diff_line(got: &[u8], want: &[u8]) -> Option<usize> {
        fn first_line(b: &[u8]) -> (Option<&[u8]>, &[u8]) {
            match b.iter().position(|&c| c == b'\n') {
                Some(i) => (Some(&b[..=i]), &b[i + 1..]),
                None if b.is_empty() => (None, b),
                None => (Some(b), &b[b.len()..]),
            }
        }
        let (mut g, mut w) = (got, want);
        let mut n = 1;
        loop {
            let (gl, gr) = first_line(g);
            let (wl, wr) = first_line(w);
            match (gl, wl) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return Some(n);
                    }
                    n += 1;
                    g = gr;
                    w = wr;
                }
                (None, None) => return None,
                _ => return Some(n),
            }
        }
    }

    /// #326 S3/D4: every `fixtures/helpers/filter-env/<case>` oracle --
    /// run the native filter as a child process (the shim's call shape)
    /// and assert stdout is BYTE-IDENTICAL, rc matches, and (for rc 0)
    /// stderr is empty. `error-bad-pattern` pins only rc and empty
    /// stdout (the Python traceback text is not reproduced).
    #[test]
    fn native_filter_matches_every_fixture_case_byte_identical() {
        let cases = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/filter-env");
        let mut names: Vec<_> = std::fs::read_dir(&cases)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 64, "fixture case count changed (D4 oracle)");
        for name in &names {
            let dir = cases.join(name);
            let pattern = std::fs::read(dir.join("args")).unwrap();
            let stdin_bytes = read_case_side(&dir, "in");
            let expected_out = read_case_side(&dir, "out");
            let expected_rc: i32 = std::fs::read_to_string(dir.join("rc.txt"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let out = run_native(&pattern, &stdin_bytes);
            assert_eq!(out.status.code(), Some(expected_rc), "case {name}: rc");
            if name == "error-bad-pattern" {
                assert!(
                    out.stdout.is_empty(),
                    "case {name}: stdout must stay empty on pattern failure"
                );
                continue;
            }
            assert!(
                out.stdout == expected_out,
                "case {name}: stdout differs at line {}",
                first_diff_line(&out.stdout, &expected_out).unwrap_or(0)
            );
            if expected_rc == 0 {
                assert!(
                    out.stderr.is_empty(),
                    "case {name}: stderr must stay empty on success"
                );
            } else {
                let expected_stderr = std::fs::read(dir.join("stderr.txt")).unwrap();
                assert_eq!(out.stderr, expected_stderr, "case {name}: stderr");
            }
            println!("case {name}: ok");
        }
        println!("filter-env fixtures: {}/{} pass", names.len(), names.len());
    }

    /// `$` (no MULTILINE) matches at end OR just before a final `\n`:
    /// the function-end and here-doc-delimiter patterns accept both
    /// spellings and reject a non-delimiter line either way.
    #[test]
    fn dollar_matches_before_a_final_newline() {
        let r = res();
        assert!(r.func_end.is_match(b"}\n"));
        assert!(r.func_end.is_match(b"}"));
        assert!(!r.func_end.is_match(b"}x\n"));
        assert!(!r.func_end.is_match(b"} "));
        let delim = here_doc_delimiter(b"EOF");
        assert!(delim.is_match(b"EOF\n"));
        assert!(delim.is_match(b"EOF"));
        assert!(!delim.is_match(b"EOFx\n"));
        assert!(!delim.is_match(b"EOF \n"));
    }

    /// `\w` is ASCII-only on both sides: a here-doc redirect into a
    /// UTF-8 word is NOT a here-doc start (the `(\w+)$` cannot reach
    /// the line end past the non-ASCII bytes), while a plain ASCII
    /// delimiter is.
    #[test]
    fn word_class_is_ascii_only() {
        let r = res();
        assert!(r.here_doc.is_match(b"cat <<EOF\n"));
        assert!(r.here_doc.is_match(b"cat <<EOF"));
        assert!(!r.here_doc.is_match("cat <<caf\u{e9}\n".as_bytes()));
        assert!(!r.here_doc.is_match("cat <<caf\u{e9}".as_bytes()));
        // `<<-` (strip-tabs) variant still matches ASCII delimiters.
        assert!(r.here_doc.is_match(b"cat <<-EOF\n"));
    }

    /// Real appends `.*\W.*` to the alternation, and `\W` is ASCII-only
    /// on bytes: a name with a non-ASCII byte matches (every non-ASCII
    /// byte is a non-word byte), an ASCII clean name does not.
    #[test]
    fn non_ascii_name_bytes_match_dot_star_backslash_capital_w() {
        let pattern = compile_var_pattern(b"MY_KEEP").unwrap();
        // Listed names match (they are dropped); clean ASCII names do not.
        assert!(pattern.is_match(b"MY_KEEP"));
        assert!(!pattern.is_match(b"OTHER_VAR"));
        // Real's appended `\d.*` drops digit-leading names.
        assert!(pattern.is_match(b"9LIVES"));
        // Real's appended `.*\W.*` drops names with non-bash
        // characters, and `\W` is ASCII-only on bytes: a name with a
        // non-ASCII byte matches (every non-ASCII byte is a non-word
        // byte), so real drops it (see the `edge-utf8-name` fixture).
        assert!(pattern.is_match("caf\u{e9}".as_bytes()));
        assert!(pattern.is_match(b"caf\xff"));
        // A literal multi-token pattern still matches exactly.
        let pattern = compile_var_pattern(b"BASH_FUNC_.* ___.*").unwrap();
        assert!(pattern.is_match(b"BASH_FUNC_foo"));
        assert!(pattern.is_match(b"___save_x"));
        assert!(!pattern.is_match(b"BASH_FUNCX"));
    }

    /// A last line without trailing newline round-trips byte-identical
    /// (kept: no newline added; dropped: no output), like Python
    /// binary-file iteration.
    #[test]
    fn a_last_line_without_newline_round_trips() {
        let pattern = compile_var_pattern(b"D").unwrap();
        let mut out = Vec::new();
        run_filter(&pattern, &mut &b"declare -x MY_KEEP=\"y\""[..], &mut out).unwrap();
        assert_eq!(out, b"declare -x MY_KEEP=\"y\"");
        let mut out = Vec::new();
        run_filter(&pattern, &mut &b"declare -x D=\"v\""[..], &mut out).unwrap();
        assert!(out.is_empty());
        // Mixed: a newline-terminated line then a bare tail.
        let mut out = Vec::new();
        run_filter(
            &pattern,
            &mut &b"declare -x MY_KEEP=\"x\"\ndeclare -x D=\"v\""[..],
            &mut out,
        )
        .unwrap();
        assert_eq!(out, b"declare -x MY_KEEP=\"x\"\n");
    }

    /// `-h`/`--help` anywhere prints the usage to stdout and exits 0.
    #[test]
    fn help_flag_prints_usage_and_exits_zero() {
        let exe = portuale_exe();
        for argv in [
            vec!["__helper", "python", "filter-bash-environment.py", "-h"],
            vec!["__helper", "python", "filter-bash-environment.py", "--help"],
            vec![
                "__helper",
                "python",
                "filter-bash-environment.py",
                "D",
                "--help",
            ],
        ] {
            let out = std::process::Command::new(&exe)
                .args(&argv)
                .output()
                .expect("portuale __helper spawns");
            assert_eq!(out.status.code(), Some(0), "{argv:?}");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "usage: filter-bash-environment.py PATTERN\n",
                "{argv:?}"
            );
        }
    }

    /// Anything but exactly one PATTERN arg prints the usage plus the
    /// `Exactly one PATTERN argument required.` line to stderr, exit 2.
    #[test]
    fn wrong_argc_prints_usage_to_stderr_and_exits_two() {
        let exe = portuale_exe();
        for argv in [
            vec!["__helper", "python", "filter-bash-environment.py"],
            vec!["__helper", "python", "filter-bash-environment.py", "A", "B"],
        ] {
            let out = std::process::Command::new(&exe)
                .args(&argv)
                .output()
                .expect("portuale __helper spawns");
            assert_eq!(out.status.code(), Some(2), "{argv:?}");
            assert_eq!(
                String::from_utf8_lossy(&out.stderr),
                "usage: filter-bash-environment.py PATTERN\n\
                 Exactly one PATTERN argument required.\n",
                "{argv:?}"
            );
            assert!(out.stdout.is_empty(), "{argv:?}");
        }
    }

    /// An uncompilable PATTERN fails loudly: exit 1, empty stdout (the
    /// Python traceback text is not reproduced -- only the rc pins the
    /// `error-bad-pattern` fixture).
    #[test]
    fn bad_pattern_exits_one_with_empty_stdout() {
        let out = run_native(b"(", b"declare -x MY_KEEP=\"k\"\n");
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(!out.stderr.is_empty());
        // A divergent-but-compilable pattern fails the same way.
        let out = run_native(b"a\\pb", b"declare -x MY_KEEP=\"k\"\n");
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
    }

    /// Patterns Rust compiles with a meaning Python `re` does not share
    /// are refused (Python 3, probed live 2026-10-08: `[[:alpha:]]`
    /// matches `a]`, `[a&&b]` matches `&`, `[a~~b]` is the class
    /// {a,~,b}, `(?<n>a)` is "unknown extension"); the Python-identical
    /// shapes that look similar are still accepted.
    #[test]
    fn patterns_with_rust_only_meaning_are_refused() {
        for p in [
            &b"[[:alpha:]]"[..],
            b"[a[b]c]",
            b"[a&&b]",
            b"[a--z]",
            b"[a~~b]",
            b"(?<n>a)",
            b"X (?u)\\w",
        ] {
            assert!(
                compile_var_pattern(p).is_err(),
                "{}",
                String::from_utf8_lossy(p)
            );
        }
        for p in [
            &b"[]a]"[..],
            b"[^]a]",
            b"[a-z_]*",
            b"(?P<n>a)",
            b"(?i)abc",
            b"A\\[B",
            b"BASH_.* __.*",
        ] {
            assert!(
                compile_var_pattern(p).is_ok(),
                "{}",
                String::from_utf8_lossy(p)
            );
        }
        // `[]a]` means {], a} in both engines.
        let re = compile_var_pattern(b"[]a]").unwrap();
        assert!(re.is_match(b"]"));
        assert!(re.is_match(b"a"));
    }
}
