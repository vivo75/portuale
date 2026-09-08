//! The phase and merge shell scripts sent to the client.

use crate::remote::*;

// --- Client merge ------------------------------------------------------------------

/// Whether `MERGE_TYPE` reaches the phase env: only for EAPI 4+. An
/// unsupported or unparseable EAPI takes the exporting fallback row.
// Portage: lib/portage/eapi.py, eapi_exports_merge_type
pub(crate) fn eapi_exports_merge_type(eapi: &str) -> bool {
    // Portage tests the *whole* string against `_supported_eapis`;
    // anything that is not a plain supported number (e.g. `3-foo`)
    // takes the exporting fallback row.
    match eapi.trim().parse::<u32>() {
        Ok(n) => n >= 4,
        Err(_) => true,
    }
}

/// Locale variables the regen postinst run re-exports into the client
/// phase env. Only set variables are ever forwarded (never invented
/// defaults). `LANGUAGE` is not in Portage's whitelist -- it rides along
/// because the slice brief names it explicitly.
pub(crate) const REGEN_LOCALE_VARS: &[&str] = &[
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_PAPER",
];

/// The server process's own locale values for the client regen phase env,
/// as `(name, value)` pairs in `REGEN_LOCALE_VARS` order. Production
/// passes `|name| std::env::var(name).ok()`; tests inject a fake map.
pub(crate) fn collect_server_locale(get: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    REGEN_LOCALE_VARS
        .iter()
        .filter_map(|name| get(name).map(|value| (name.to_string(), value)))
        .collect()
}

/// Whether an EAPI runs posixish locale handling: true from EAPI 6 on.
/// An unparseable EAPI defaults to the modern rule.
// Portage: eapi.py, posixish_locale
pub(crate) fn eapi_is_posixish(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    // Anything reaching us parsed off an ebuild, so default an
    // unparseable value to the modern rule.
    digits.parse::<u64>().map(|n| n >= 6).unwrap_or(true)
}

/// The twelve locale categories a set `LC_ALL` fans out over. The
/// fan-out below iterates this list, not the forwarding set.
pub(crate) const PORTAGE_LOCALE_CATEGORIES: &[&str] = &[
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_ADDRESS",
    "LC_IDENTIFICATION",
    "LC_MEASUREMENT",
    "LC_NAME",
    "LC_PAPER",
    "LC_TELEPHONE",
];

/// Fan out a set `LC_ALL` over all twelve `PORTAGE_LOCALE_CATEGORIES`
/// for a posixish phase (itself disappearing), filling `LANG` only when
/// the server left it unset. `LANGUAGE` is never touched; a set-but-empty
/// `LC_ALL` is dropped without fanning out. Non-posixish EAPIs return
/// the pairs unchanged.
// Portage: portage/util/locale.py, split_LC_ALL
pub(crate) fn split_server_locale(
    locale: &[(String, String)],
    eapi: &str,
) -> Vec<(String, String)> {
    if !eapi_is_posixish(eapi) {
        return locale.to_vec();
    }
    let lc_all = locale
        .iter()
        .rev()
        .find(|(name, _)| name == "LC_ALL")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let mut out: Vec<(String, String)> = locale
        .iter()
        .filter(|(name, _)| name != "LC_ALL")
        .cloned()
        .collect();
    if lc_all.is_empty() {
        return out;
    }
    for name in PORTAGE_LOCALE_CATEGORIES {
        match out.iter_mut().find(|(n, _)| n == name) {
            Some(pair) => pair.1 = lc_all.clone(),
            None => out.push((name.to_string(), lc_all.clone())),
        }
    }
    if !out.iter().any(|(n, _)| n == "LANG") {
        out.push(("LANG".to_string(), lc_all));
    }
    out
}

/// Merge header: all values the flow needs, pre-quoted. The unit layout
/// mirrors the local merge (`image/`, `build-info/`, shipped `bin/`).
/// Client ledger destination + line baked into the merge driver; `None`
/// file skips the client record. `portuale_bin` is the resolved absolute
/// client path, exported for the whole driver (including the old hooks).
#[derive(Debug, Clone)]
pub(crate) struct LedgerSpec {
    pub file: String,
    pub line: String,
}
