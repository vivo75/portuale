// ANSI colour for `emerge --pretend` output: the slice of Portage's
// `output.py` the pretend renderer needs (colour table, `colorize`,
// `_styles` keys, `nc_len`, `color.map` parsing, `PORTAGE_COLORMAP`).
// Portage: lib/portage/output.py

use std::collections::HashMap;

/// The `esc_seq` + `rgb_ansi_colors[i]` -> `ansi_codes[i]` mapping:
/// `ansi_codes` is `[30m, 30;01m, 31m, 31;01m, …]`, paired positionally
/// with the 16 `0xRRGGBB` names.
// Portage: output.py, esc_seq
fn code(name: &str) -> &'static str {
    match name {
        "normal" => "\x1b[0m",
        "reset" => "\x1b[39;49;00m",
        "bold" => "\x1b[01m",
        // `rgb_ansi_colors` / `ansi_codes` pairs.
        "black" => "\x1b[30m",        // 0x000000
        "darkgray" => "\x1b[30;01m",  // 0x555555
        "darkred" => "\x1b[31m",      // 0xAA0000
        "red" => "\x1b[31;01m",       // 0xFF5555
        "darkgreen" => "\x1b[32m",    // 0x00AA00
        "green" => "\x1b[32;01m",     // 0x55FF55
        "brown" => "\x1b[33m",        // 0xAA5500
        "yellow" => "\x1b[33;01m",    // 0xFFFF55
        "darkblue" => "\x1b[34m",     // 0x0000AA
        "blue" => "\x1b[34;01m",      // 0x5555FF
        "purple" => "\x1b[35m",       // 0xAA00AA
        "fuchsia" => "\x1b[35;01m",   // 0xFF55FF
        "teal" => "\x1b[36m",         // 0x00AAAA
        "turquoise" => "\x1b[36;01m", // 0x55FFFF
        "lightgray" => "\x1b[37m",    // 0xAAAAAA
        "white" => "\x1b[37;01m",     // 0xFFFFFF
        _ => "",
    }
}

/// The `_styles` map, narrowed to the keys the pretend renderer's
/// `colorize()` calls reach. Each maps to exactly one colour-name.
// Portage: output.py, _styles
fn style(key: &str) -> &'static str {
    match key {
        "BAD" => "red",
        "WARN" => "yellow",
        "GOOD" => "green",
        // Used by the `PORTAGE_COLORMAP` phase-env export.
        "NORMAL" => "normal",
        "HILITE" => "teal",
        "BRACKET" => "blue",
        // The `EOutput` `e*` prefixes for the elog `echo` module.
        "ERR" => "red",
        "INFO" => "darkgreen",
        "LOG" => "green",
        "QAWARN" => "brown",
        "PKG_MERGE" => "darkgreen",
        "PKG_MERGE_SYSTEM" => "darkgreen",
        "PKG_MERGE_WORLD" => "green",
        "PKG_BINARY_MERGE" => "purple",
        "PKG_BINARY_MERGE_SYSTEM" => "purple",
        "PKG_BINARY_MERGE_WORLD" => "fuchsia",
        "PKG_UNINSTALL" => "red",
        "UNMERGE_WARN" => "red",
        "INFORM" => "darkgreen",
        "MERGE_LIST_PROGRESS" => "yellow",
        "PKG_NOMERGE" => "teal",
        "PKG_NOMERGE_SYSTEM" => "teal",
        "PKG_NOMERGE_WORLD" => "blue",
        "PKG_BLOCKER" => "red",
        "PKG_BLOCKER_SATISFIED" => "teal",
        // `UserQuery.query`'s own default `[Yes/No]` colours.
        "PROMPT_CHOICE_DEFAULT" => "green",
        "PROMPT_CHOICE_OTHER" => "red",
        _ => "",
    }
}

/// Every `_styles` key name, so `color_map_overrides` can validate a
/// `color.map` `KEY`. Kept in sync with `style()` above.
const STYLE_KEYS: &[&str] = &[
    "BAD",
    "WARN",
    "GOOD",
    "HILITE",
    "BRACKET",
    "NORMAL",
    "ERR",
    "INFO",
    "LOG",
    "QAWARN",
    "PKG_MERGE",
    "PKG_MERGE_SYSTEM",
    "PKG_MERGE_WORLD",
    "PKG_BINARY_MERGE",
    "PKG_BINARY_MERGE_SYSTEM",
    "PKG_BINARY_MERGE_WORLD",
    "PKG_UNINSTALL",
    "UNMERGE_WARN",
    "INFORM",
    "MERGE_LIST_PROGRESS",
    "PKG_NOMERGE",
    "PKG_NOMERGE_SYSTEM",
    "PKG_NOMERGE_WORLD",
    "PKG_BLOCKER",
    "PKG_BLOCKER_SATISFIED",
    "PROMPT_CHOICE_DEFAULT",
    "PROMPT_CHOICE_OTHER",
];

/// Every `codes` colour-name -- the RHS-names a `color.map` value may
/// reference and the LHS-names it may override.
const CODE_NAMES: &[&str] = &[
    "normal",
    "reset",
    "bold",
    "black",
    "darkgray",
    "red",
    "darkred",
    "green",
    "darkgreen",
    "yellow",
    "brown",
    "blue",
    "darkblue",
    "fuchsia",
    "purple",
    "turquoise",
    "teal",
    "white",
    "lightgray",
];

/// Parse `<config_root>/etc/portage/color.map` and return every
/// override as `key -> final escape sequence`. A line is `KEY = VALUE`
/// (`#` starts a comment); `KEY` must be a known `_styles`/`codes`
/// name; `VALUE` is a raw ANSI code or a space-separated list of
/// `codes` names. A malformed line is skipped with a warning, never
/// fatal. Cached once per process.
// Portage: output.py, _parse_color_map
fn color_map_overrides() -> &'static HashMap<String, String> {
    use std::sync::OnceLock;
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut out = HashMap::new();
        let config_root = std::env::var("PORTAGE_CONFIGROOT").unwrap_or_else(|_| "/".to_string());
        let path = std::path::Path::new(&config_root).join("etc/portage/color.map");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return out;
        };
        let is_ansi = |v: &str| {
            !v.is_empty()
                && v.ends_with('m')
                && v[..v.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ';')
        };
        fn strip_quotes(t: &str) -> &str {
            let b = t.as_bytes();
            if b.len() >= 2 && (b[0] == b'\'' || b[0] == b'"') && b[0] == b[b.len() - 1] {
                &t[1..t.len() - 1]
            } else {
                t
            }
        }
        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                eprintln!(
                    "'{}', line {lineno}: expected exactly one occurrence of '=' operator",
                    path.display()
                );
                continue;
            };
            let k = strip_quotes(k.trim());
            let v = strip_quotes(v.trim());
            if !STYLE_KEYS.contains(&k) && !CODE_NAMES.contains(&k) {
                eprintln!(
                    "'{}', line {lineno}: Unknown variable: '{k}'",
                    path.display()
                );
                continue;
            }
            let seq = if is_ansi(v) {
                format!("\x1b[{v}")
            } else {
                let mut s = String::new();
                let mut bad = false;
                for name in v.split_whitespace() {
                    let c = code(name);
                    if c.is_empty() && name != "normal" {
                        bad = true;
                        break;
                    }
                    s.push_str(if name == "normal" { "\x1b[0m" } else { c });
                }
                if bad {
                    eprintln!("'{}', line {lineno}: Undefined: '{v}'", path.display());
                    continue;
                }
                s
            };
            out.insert(k.to_string(), seq);
        }
        out
    })
}

/// The `PORTAGE_COLORMAP` phase-env value: bash source the ebuild
/// helpers `eval` so `elog`/`einfo`/`ewarn` use the same (possibly
/// `color.map`-overridden) colours `emerge` itself does.
// Portage: output.py, colormap
pub fn phase_colormap_export() -> String {
    [
        "BAD", "BRACKET", "ERR", "GOOD", "HILITE", "INFO", "LOG", "NORMAL", "QAWARN", "WARN",
    ]
    .iter()
    .map(|k| format!("PORTAGE_COLOR_{k}=$'{}'", ansi_escape(&resolved_code(k))))
    .collect::<Vec<_>>()
    .join("\n")
}

/// `\x1b` -> `\E` for a bash `$'...'` literal.
fn ansi_escape(seq: &str) -> String {
    seq.replace('\x1b', "\\E")
}

/// The escape sequence for `name`, honouring `color.map` overrides:
/// an override for `name` itself wins; else the hardcoded `codes`
/// entry; else a `_styles` key resolves to its colour-name (which may
/// itself be overridden).
fn resolved_code(name: &str) -> String {
    if let Some(v) = color_map_overrides().get(name) {
        return v.clone();
    }
    let c = code(name);
    if !c.is_empty() {
        return c.to_string();
    }
    let s = style(name);
    if s.is_empty() {
        return String::new();
    }
    color_map_overrides()
        .get(s)
        .cloned()
        .unwrap_or_else(|| code(s).to_string())
}

/// Whether colour is emitted:
/// 1. off, then on unless `no_color` -- `NO_COLOR` set, or `NOCOLOR`
///    = `yes`/`true`;
/// 2. an explicit `--color y|n` overrides everything;
/// 3. otherwise also off when `TERM=dumb` or stdout is not a tty.
///
/// `color_opt` is `Some(true)`/`Some(false)` for `--color y`/`--color n`,
/// `None` when the flag was not given.
// Portage: actions.py, no_color
pub fn resolve_havecolor(color_opt: Option<bool>) -> bool {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let no_color = env("NO_COLOR").is_some()
        || matches!(
            env("NOCOLOR")
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("yes") | Some("true")
        );
    let mut havecolor = !no_color;
    match color_opt {
        Some(v) => havecolor = v,
        None => {
            use std::io::IsTerminal;
            if env("TERM").as_deref() == Some("dumb") || !std::io::stdout().is_terminal() {
                havecolor = false;
            }
        }
    }
    havecolor
}

/// A colouriser bound to a resolved `havecolor` value. When `enabled`
/// is false every method returns its input unchanged, so callers never
/// branch on colour themselves.
#[derive(Clone, Copy)]
pub struct Colorizer {
    pub enabled: bool,
}

impl Colorizer {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// `colorize(key, text)`: a `codes` key wraps directly, a `_styles`
    /// key resolves to its colour-name first; either way `text` is
    /// followed by `codes["reset"]`.
    // Portage: output.py, colorize
    pub fn c(&self, key: &str, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let seq = resolved_code(key);
        if seq.is_empty() {
            return text.to_string();
        }
        format!("{seq}{text}{}", resolved_code("reset"))
    }
}
