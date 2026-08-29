// ANSI colour for `emerge --pretend` output -- increment 2 of the `-pv`
// real-`output.py` layout + colour buildout (increment 1 landed the
// bracket layout, see `pretend.rs::attr_display_field`).
//
// This ports the small slice of `lib/portage/output.py` the pretend
// renderer needs: the RGB-name -> ANSI-code table (`output.py:30-92`),
// `colorize()` (`output.py:383-392`), the handful of `_styles` entries
// (`output.py:126-154`), and `nc_len()` (`output.py:249-251`). It does
// NOT port `/etc/portage/color.map` / `PORTAGE_COLORMAP` parsing -- the
// real default colour map is hardcoded, the same deliberate "optional
// config not modelled" cut the rest of this pilot makes.

/// Real `output.py`'s own `esc_seq` + the `rgb_ansi_colors[i]` ->
/// `ansi_codes[i]` mapping (`output.py:68-92`): `ansi_codes` is
/// `[30m, 30;01m, 31m, 31;01m, …]`, paired positionally with the 16
/// `0xRRGGBB` names. Only the names the pretend renderer actually uses
/// are given here.
fn code(name: &str) -> &'static str {
    match name {
        "reset" => "\x1b[39;49;00m",
        // rgb_ansi_colors / ansi_codes pairs
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
        "red" => "\x1b[31;01m",       // 0xFF5555
        _ => "",
    }
}

/// Real `output.py:126-154`'s `_styles` map, narrowed to the keys the
/// pretend renderer's `colorize()` calls reach. Each maps to exactly one
/// colour-name (real `_styles` values are 1-tuples for all of these).
fn style(key: &str) -> &'static str {
    match key {
        "BAD" => "red",
        "WARN" => "yellow",
        "GOOD" => "green",
        "PKG_MERGE" => "darkgreen",
        "PKG_MERGE_SYSTEM" => "darkgreen",
        "PKG_MERGE_WORLD" => "green",
        "PKG_BINARY_MERGE" => "purple",
        "PKG_BINARY_MERGE_SYSTEM" => "purple",
        "PKG_BINARY_MERGE_WORLD" => "fuchsia",
        "PKG_UNINSTALL" => "red",
        "PKG_NOMERGE" => "teal",
        "PKG_NOMERGE_SYSTEM" => "teal",
        "PKG_NOMERGE_WORLD" => "blue",
        "PKG_BLOCKER" => "red",
        "PKG_BLOCKER_SATISFIED" => "teal",
        _ => "",
    }
}

/// Real `output.py:249-251`'s `nc_len`: the visible length of a string,
/// with ANSI SGR sequences (`\x1b` … `m`) removed first. Real portage's
/// pattern is `re.sub(esc_seq + "^m]+m", "", mystr)` i.e. `\x1b[^m]+m`;
/// this is the same, sans regex. Used for `--columns` padding so a
/// coloured line lines up the same as an uncoloured one.
pub fn nc_len(s: &str) -> usize {
    let mut n = 0usize;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for e in chars.by_ref() {
                if e == 'm' {
                    break;
                }
            }
        } else {
            n += 1;
        }
    }
    n
}

/// Real `output.py`'s own gate for whether colour is emitted, ported from
/// `lib/_emerge/actions.py:2816-2828` + `lib/portage/util/no_color`:
///
/// 1. off, then on unless `no_color` -- `NO_COLOR` set to anything, or
///    `NOCOLOR` = `yes`/`true`;
/// 2. an explicit `--color y|n` overrides everything;
/// 3. otherwise also off when `TERM=dumb` or stdout is not a tty.
///
/// `color_opt` is `Some(true)`/`Some(false)` for `--color y`/`--color n`,
/// `None` when the flag was not given.
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

/// A colouriser bound to a resolved `havecolor` value -- real portage's
/// module-global `havecolor` plus its `colorize()` function, together.
/// When `enabled` is false every method returns its input unchanged, so
/// callers never branch on colour themselves.
#[derive(Clone, Copy)]
pub struct Colorizer {
    pub enabled: bool,
}

impl Colorizer {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Real `colorize(key, text)` (`output.py:383-392`): a `codes` key
    /// wraps directly, a `_styles` key resolves to its colour-name(s)
    /// first; either way `text` is followed by `codes["reset"]`. `key`
    /// here is always a `_styles` key or a bare colour name.
    pub fn c(&self, key: &str, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let name = if code(key).is_empty() {
            style(key)
        } else {
            key
        };
        let seq = code(name);
        if seq.is_empty() {
            return text.to_string();
        }
        format!("{seq}{text}{}", code("reset"))
    }

    /// Real `Display.pkgprint` (`output.py:265-292`), the merge-list case
    /// only (`pkg_info.merge` is always true for every bracket entry this
    /// pilot prints): pick the palette entry from `built` (binary) +
    /// `system` + `world`. `system` wins over `world`, exactly as real.
    pub fn pkgprint(&self, text: &str, binary: bool, system: bool, world: bool) -> String {
        let key = match (binary, system, world) {
            (true, true, _) => "PKG_BINARY_MERGE_SYSTEM",
            (true, false, true) => "PKG_BINARY_MERGE_WORLD",
            (true, false, false) => "PKG_BINARY_MERGE",
            (false, true, _) => "PKG_MERGE_SYSTEM",
            (false, false, true) => "PKG_MERGE_WORLD",
            (false, false, false) => "PKG_MERGE",
        };
        self.c(key, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorize_matches_real_portage_escape_codes() {
        let c = Colorizer::new(true);
        // Real `colorize("green", "N")` = codes["green"] + "N" + reset.
        assert_eq!(c.c("green", "N"), "\x1b[32;01mN\x1b[39;49;00m");
        // A `_styles` key resolves to its colour name first.
        assert_eq!(c.c("PKG_MERGE", "x"), "\x1b[32mx\x1b[39;49;00m");
        assert_eq!(c.c("PKG_MERGE_WORLD", "x"), "\x1b[32;01mx\x1b[39;49;00m");
        assert_eq!(c.c("WARN", "~"), "\x1b[33;01m~\x1b[39;49;00m");
        assert_eq!(c.c("BAD", "#"), "\x1b[31;01m#\x1b[39;49;00m");
    }

    #[test]
    fn disabled_colorizer_is_identity() {
        let c = Colorizer::new(false);
        assert_eq!(c.c("green", "N"), "N");
        assert_eq!(c.pkgprint("ebuild", false, false, true), "ebuild");
    }

    #[test]
    fn pkgprint_palette_system_wins_over_world() {
        let c = Colorizer::new(true);
        assert_eq!(
            c.pkgprint("p", false, true, true),
            "\x1b[32mp\x1b[39;49;00m"
        ); // SYSTEM
        assert_eq!(
            c.pkgprint("p", false, false, true),
            "\x1b[32;01mp\x1b[39;49;00m"
        ); // WORLD
        assert_eq!(
            c.pkgprint("p", false, false, false),
            "\x1b[32mp\x1b[39;49;00m"
        ); // plain
        assert_eq!(
            c.pkgprint("p", true, false, false),
            "\x1b[35mp\x1b[39;49;00m"
        ); // BINARY
        assert_eq!(
            c.pkgprint("p", true, false, true),
            "\x1b[35;01mp\x1b[39;49;00m"
        ); // BINARY_WORLD
    }

    #[test]
    fn nc_len_strips_ansi() {
        assert_eq!(nc_len("plain"), 5);
        assert_eq!(nc_len("\x1b[32;01mN\x1b[39;49;00m"), 1);
        assert_eq!(
            nc_len("[\x1b[32mebuild\x1b[39;49;00m  \x1b[32;01mN\x1b[39;49;00m    ]"),
            15
        );
    }

    #[test]
    fn resolve_havecolor_explicit_wins() {
        // `--color y` / `--color n` override everything, including a
        // non-tty stdout (the test process's own).
        assert!(resolve_havecolor(Some(true)));
        assert!(!resolve_havecolor(Some(false)));
        // No flag + piped stdout (the test harness) -> off.
        assert!(!resolve_havecolor(None));
    }
}
