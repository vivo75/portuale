//! One parser for `CONTENTS` lines, shared by every backend and caller.
//!
//! Portage splits each line with `_contents_re`: `obj` takes the path up
//! to the last two whitespace-separated fields (`md5`, `mtime`),
//! `sym` cuts at the last ` -> ` before the trailing `mtime`, and
//! `dir`/`dev`/`fif` take the rest of the line, so paths with spaces
//! survive. A line that matches nothing is skipped. Every previous
//! whitespace-`split` site mis-parsed those paths (see #357).

/// A parsed `CONTENTS` line, borrowing the line it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentsLine<'a> {
    Obj {
        path: &'a str,
        md5: &'a str,
        mtime: i64,
    },
    Sym {
        path: &'a str,
        target: &'a str,
        mtime: i64,
    },
    Dir {
        path: &'a str,
    },
    Dev {
        path: &'a str,
    },
    Fif {
        path: &'a str,
    },
    /// Portage's regex rejects `bin`, but portuale-written vdbs may hold
    /// it; kept so those rows still parse instead of silently vanishing.
    Bin {
        path: &'a str,
    },
}

impl<'a> ContentsLine<'a> {
    /// The recorded kind word (`"obj"`, `"sym"`, `"dir"`, `"dev"`,
    /// `"fif"` or `"bin"`).
    pub fn kind(&self) -> &'static str {
        match self {
            ContentsLine::Obj { .. } => "obj",
            ContentsLine::Sym { .. } => "sym",
            ContentsLine::Dir { .. } => "dir",
            ContentsLine::Dev { .. } => "dev",
            ContentsLine::Fif { .. } => "fif",
            ContentsLine::Bin { .. } => "bin",
        }
    }

    /// The recorded path, exactly as written (leading `/` included).
    pub fn path(&self) -> &'a str {
        match self {
            ContentsLine::Obj { path, .. }
            | ContentsLine::Sym { path, .. }
            | ContentsLine::Dir { path }
            | ContentsLine::Dev { path }
            | ContentsLine::Fif { path }
            | ContentsLine::Bin { path } => path,
        }
    }
}

/// The trailing ` <mtime>` of an `obj`/`sym` line: the head before it and
/// the numeric value. `None` when the last field is not all digits.
fn split_mtime(rest: &str) -> Option<(&str, i64)> {
    let (head, tail) = rest.rsplit_once(' ')?;
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((head, tail.parse::<i64>().ok()?))
}

/// Parses one `CONTENTS` line; `None` when it matches nothing (skipped,
/// like Portage skipping a non-matching line). `obj` splits at the last
/// two spaces, `sym` strips the mtime and cuts at the last ` -> ` (so a
/// target holding ` -> ` mis-splits exactly like Portage's greedy
/// regex: path keeps the extra ` -> x`), and `dir`/`dev`/`fif`/`bin`
/// take the rest of the line.
// Portage: lib/portage/dbapi/vartree.py, _contents_re
pub fn parse_line(line: &str) -> Option<ContentsLine<'_>> {
    // Portage reads the file in text mode (universal newlines); a stray
    // `\r` is a line ending, never part of a path.
    let line = line.strip_suffix('\r').unwrap_or(line);
    if let Some(rest) = line.strip_prefix("obj ") {
        let (rest, mtime) = split_mtime(rest)?;
        let (path, md5) = rest.rsplit_once(' ')?;
        if path.is_empty() || md5.is_empty() {
            return None;
        }
        Some(ContentsLine::Obj { path, md5, mtime })
    } else if let Some(rest) = line.strip_prefix("sym ") {
        let (rest, mtime) = split_mtime(rest)?;
        let (path, target) = rest.rsplit_once(" -> ")?;
        if path.is_empty() || target.is_empty() {
            return None;
        }
        Some(ContentsLine::Sym {
            path,
            target,
            mtime,
        })
    } else if let Some(path) = line.strip_prefix("dir ") {
        (!path.is_empty()).then_some(ContentsLine::Dir { path })
    } else if let Some(path) = line.strip_prefix("dev ") {
        (!path.is_empty()).then_some(ContentsLine::Dev { path })
    } else if let Some(path) = line.strip_prefix("fif ") {
        (!path.is_empty()).then_some(ContentsLine::Fif { path })
    } else if let Some(path) = line.strip_prefix("bin ") {
        (!path.is_empty()).then_some(ContentsLine::Bin { path })
    } else {
        None
    }
}

/// `(kind, path)` of every parsable line of `text`, in order; malformed
/// lines are skipped.
pub fn paths(text: &str) -> impl Iterator<Item = (&str, &str)> + '_ {
    text.lines()
        .filter_map(|line| parse_line(line).map(|entry| (entry.kind(), entry.path())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected values follow Portage 3.0.82.2's `_contents_re`
    /// (`vartree.py:1951-1958`): spaces split from the right for `obj`,
    /// at the last ` -> ` for `sym`, and not at all for the rest.
    #[test]
    fn spaces_survive_in_every_kind() {
        assert_eq!(
            parse_line("obj /usr/share/t355/a b.txt daa8075d6ac5ff8d0c6d4650adb4ef29 1792105216"),
            Some(ContentsLine::Obj {
                path: "/usr/share/t355/a b.txt",
                md5: "daa8075d6ac5ff8d0c6d4650adb4ef29",
                mtime: 1792105216,
            })
        );
        assert_eq!(
            parse_line("sym /usr/share/t355/c d -> a b.txt 1792105216"),
            Some(ContentsLine::Sym {
                path: "/usr/share/t355/c d",
                target: "a b.txt",
                mtime: 1792105216,
            })
        );
        assert_eq!(
            parse_line("dir /usr/share/t355/sp dir"),
            Some(ContentsLine::Dir {
                path: "/usr/share/t355/sp dir"
            })
        );
        assert_eq!(
            parse_line("dev /dev/with space"),
            Some(ContentsLine::Dev {
                path: "/dev/with space"
            })
        );
        assert_eq!(
            parse_line("fif /run/with space"),
            Some(ContentsLine::Fif {
                path: "/run/with space"
            })
        );
    }

    /// Portage's greedy `sym (.+) -> (.+)` keeps the extra ` -> x` in
    /// the path (S0 cell S3 pins the line
    /// `sym /usr/share/t355/arrow -> x -> y <mtime>`). This looks wrong;
    /// it is Portage, so it stays.
    #[test]
    fn a_target_holding_arrow_mis_splits_like_portage() {
        assert_eq!(
            parse_line("sym /usr/share/t355/arrow -> x -> y 1792105238"),
            Some(ContentsLine::Sym {
                path: "/usr/share/t355/arrow -> x",
                target: "y",
                mtime: 1792105238,
            })
        );
    }

    #[test]
    fn a_trailing_space_path_is_kept_verbatim() {
        assert_eq!(
            parse_line("obj /a  md5 7"),
            Some(ContentsLine::Obj {
                path: "/a ",
                md5: "md5",
                mtime: 7,
            })
        );
    }

    #[test]
    fn malformed_lines_are_skipped() {
        for line in [
            "",
            "junk",
            "obj",
            "obj /only-path",
            "obj /p md5",
            "sym /p -> t",
            "sym /p t 1",
            "sym /p ->  1",
            "sym  -> t 1",
            "dir",
            "dir ",
            "foo /usr/z",
            " bin /b",
            "OBJ /x y 1",
        ] {
            assert_eq!(parse_line(line), None, "{line:?} must not parse");
        }
    }

    #[test]
    fn a_non_numeric_mtime_is_rejected() {
        assert_eq!(parse_line("obj /p md5 notanumber"), None);
        assert_eq!(parse_line("obj /p md5 -1"), None);
        assert_eq!(parse_line("obj /p md5 "), None);
        assert_eq!(parse_line("sym /p -> t x"), None);
    }

    #[test]
    fn crlf_and_legacy_kinds_parse() {
        assert_eq!(
            parse_line("obj /usr/share d 1\r"),
            Some(ContentsLine::Obj {
                path: "/usr/share",
                md5: "d",
                mtime: 1,
            })
        );
        assert_eq!(parse_line("bin /b"), Some(ContentsLine::Bin { path: "/b" }));
        let kinds: Vec<(&str, &str)> =
            paths("obj /a b 1\njunk\nsym /c -> d 2\ndir /e f\n").collect();
        assert_eq!(kinds, vec![("obj", "/a"), ("sym", "/c"), ("dir", "/e f")]);
    }
}
