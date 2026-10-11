// Native answer for the `has_utf8_ctype` QA probe.
//
// `bin/install-qa-check.d/90config-impl-decl` passes iff
// `locale.getlocale()[1]` prints exactly `UTF-8`. Answered with libc
// (`setlocale(LC_CTYPE)` + `nl_langinfo(CODESET)`): `LC_ALL`, when set
// and non-empty, disables PEP 538 coercion, while an unresolvable
// locale otherwise falls back to C and prints `UTF-8`.
// Portage: bin/install-qa-check.d/90config-impl-decl

use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStrExt;

/// The exact `-c` program string the QA check runs (D1 routing key).
pub(crate) const PROBE_PROGRAM: &str = "import locale; print(locale.getlocale()[1])";

/// Print what the Python probe prints for the current environment.
pub(crate) fn print_answer() {
    println!("{}", answer());
}

fn answer() -> String {
    if let Some(lc_all) = present_var("LC_ALL") {
        // No PEP 538 coercion under LC_ALL.
        if lc_all == "C" || lc_all == "POSIX" {
            return "None".to_string();
        }
        return match codeset_of(&lc_all) {
            Some(codeset) => map_codeset(&codeset),
            None => "None".to_string(),
        };
    }
    let effective = present_var("LC_CTYPE").or_else(|| present_var("LANG"));
    let Some(effective) = effective else {
        return "UTF-8".to_string();
    };
    if effective == "C" || effective == "POSIX" {
        return "UTF-8".to_string();
    }
    match codeset_of(&effective) {
        // Not installed: falls back to C, which PEP 538 coerces.
        None => "UTF-8".to_string(),
        Some(codeset) => map_codeset(&codeset),
    }
}

/// A set-but-empty variable counts as unset (glibc/Python treat `""` as
/// "no selection", so the next lower-precedence variable wins).
fn present_var(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

/// The `LC_CTYPE` codeset of the named locale, or `None` when it does
/// not resolve on this host (`setlocale` returns null).
fn codeset_of(name: &OsString) -> Option<String> {
    let bytes = name.as_bytes();
    let cstr = CString::new(bytes).ok()?;
    // SAFETY: `name` is a valid NUL-terminated string; `setlocale`
    // with an explicit name only queries this process's own locale.
    let ok = unsafe { libc::setlocale(libc::LC_CTYPE, cstr.as_ptr()) };
    if ok.is_null() {
        return None;
    }
    // SAFETY: `CODESET` makes `nl_langinfo` return a static NUL-
    // terminated string describing the just-selected locale.
    let codeset = unsafe { libc::nl_langinfo(libc::CODESET) };
    if codeset.is_null() {
        return None;
    }
    // SAFETY: `nl_langinfo(CODESET)` returns a valid C string.
    let s = unsafe { std::ffi::CStr::from_ptr(codeset) };
    Some(String::from_utf8_lossy(s.to_bytes()).into_owned())
}

/// Python's `locale.getlocale()[1]` spellings for the oracle's codesets;
/// anything else is printed as libc reports it.
fn map_codeset(codeset: &str) -> String {
    let norm: String = codeset
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect::<String>()
        .to_ascii_uppercase();
    match norm.as_str() {
        "UTF8" => "UTF-8".to_string(),
        "ISO88591" => "ISO8859-1".to_string(),
        _ => codeset.to_string(),
    }
}

#[cfg(test)]
mod table_tests {
    use std::path::Path;
    /// Every row of `fixtures/helpers/locale/table.tsv` (the oracle).
    /// The native answer runs as a child process with exactly that
    /// row's variables; rows resolving differently here are skipped.
    #[test]
    fn the_native_locale_answer_matches_every_oracle_row() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/locale");
        let table = std::fs::read_to_string(dir.join("table.tsv")).unwrap();
        let exe = crate::self_exe::built_portuale_bin();
        let mut rows = 0;
        let mut skipped = 0;
        for line in table.lines().skip(1) {
            if line.is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 7, "bad row: {line}");
            let (case, lc_all, lc_ctype, lang, python_stdout, locale_charmap, is_utf8) = (
                cols[0], cols[1], cols[2], cols[3], cols[4], cols[5], cols[6],
            );
            rows += 1;
            let mut cmd = std::process::Command::new(&exe);
            cmd.args(["__helper", "python", "-c", super::PROBE_PROGRAM])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent");
            for (name, value) in [("LC_ALL", lc_all), ("LC_CTYPE", lc_ctype), ("LANG", lang)] {
                // `-` is unset; an empty field is set-but-empty.
                if value != "-" {
                    cmd.env(name, value);
                }
            }
            // Skip rows this host resolves differently (see above).
            let mut charmap = std::process::Command::new("locale");
            charmap
                .arg("charmap")
                .env_clear()
                .env("PATH", "/usr/bin:/bin");
            for (name, value) in [("LC_ALL", lc_all), ("LC_CTYPE", lc_ctype), ("LANG", lang)] {
                if value != "-" {
                    charmap.env(name, value);
                }
            }
            if let Ok(charmap_out) = charmap.output() {
                let got = String::from_utf8_lossy(&charmap_out.stdout)
                    .trim()
                    .to_string();
                if got != locale_charmap {
                    println!(
                        "row {case}: skipped (host `locale charmap` is {got:?}, \
                         oracle says {locale_charmap:?})"
                    );
                    skipped += 1;
                    continue;
                }
            }
            let out = cmd.output().expect("portuale __helper spawns");
            assert_eq!(out.status.code(), Some(0), "row {case}: exit");
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            assert_eq!(stdout, python_stdout, "row {case}: answer");
            assert_eq!(stdout == "UTF-8", is_utf8 == "1", "row {case}: is_utf8 bit");
        }
        println!("locale table: {rows} rows, {skipped} skipped for host locale differences");
        assert!(rows > 0);
    }
}
