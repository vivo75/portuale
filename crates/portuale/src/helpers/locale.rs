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
