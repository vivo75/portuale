//! package.mask, package.accept_keywords, package.license and package.use parsers.

use crate::*;

/// Reads every non-comment, non-blank, trimmed line from `path`, which
/// may be a single file or a directory of fragments. A directory is read
/// in `portage_util::recursive_config_files` order; a missing path yields
/// an empty list, not an error.
// Portage: lib/portage/util/__init__.py, _recursive_file_list
pub(crate) fn read_config_lines(path: &Path) -> Result<Vec<String>, Error> {
    Ok(read_config_lines_with_sources(path)?
        .into_iter()
        .map(|(_, line)| line)
        .collect())
}

/// [`read_config_lines`] keeping each line's source file: profile atom
/// warnings cite the fragment, as Portage's `remember_source_file` does.
// Portage: lib/portage/util/__init__.py, grabfile_package
pub(crate) fn read_config_lines_with_sources(path: &Path) -> Result<Vec<(PathBuf, String)>, Error> {
    fn read_file_lines(path: &Path) -> Result<Vec<String>, Error> {
        let text = fs::read_to_string(path).map_err(|e| Error::ReadFile {
            path: path.display().to_string(),
            source: e,
        })?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect())
    }

    let mut lines: Vec<(PathBuf, String)> = Vec::new();
    // Missing paths stay empty (not an error); unreadable ones report.
    if path.symlink_metadata().is_err() {
        return Ok(lines);
    }
    let files = portage_util::recursive_config_files(path).map_err(|e| Error::ReadFile {
        path: path.display().to_string(),
        source: e,
    })?;
    for entry in files {
        for line in read_file_lines(&entry)? {
            lines.push((entry.clone(), line));
        }
    }
    Ok(lines)
}

/// Scopes each of `lines` (raw `package.mask`/`.unmask` entries, a
/// leading `-` meaning removal) to `repo_name` by appending `::name` to
/// atoms without an explicit repo part. A leading `-` is preserved ahead
/// of the atom, so a removal can only ever cancel a same-repo entry.
// Portage: lib/portage/util/__init__.py, append_repo
pub(crate) fn scope_repo_mask_lines(lines: &[String], repo_name: &str) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let (prefix, atom) = match line.strip_prefix('-') {
                Some(rest) => ("-", rest),
                None => ("", line.as_str()),
            };
            if atom.contains("::") {
                line.clone()
            } else {
                format!("{prefix}{atom}::{repo_name}")
            }
        })
        .collect()
}

/// Same `::repo`-scoping rule as [`scope_repo_mask_lines`], applied to
/// the `<atom> <flag> ...` line shape: only the leading atom token gets
/// scoped. These files have no `-atom` whole-entry removal syntax.
// Portage: lib/portage/util/__init__.py, append_repo
pub(crate) fn scope_repo_package_use_lines(lines: &[String], repo_name: &str) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let (atom, rest) = match line.split_once(char::is_whitespace) {
                Some((a, r)) => (a, r.trim_start()),
                None => (line.as_str(), ""),
            };
            let scoped_atom = if atom.contains("::") {
                atom.to_string()
            } else {
                format!("{atom}::{repo_name}")
            };
            if rest.is_empty() {
                scoped_atom
            } else {
                format!("{scoped_atom} {rest}")
            }
        })
        .collect()
}

/// Stacks `package.mask`/`.unmask` lines (earlier sources first) with
/// `-atom` removal across the whole stack. Unscoped removals ignore any
/// `::repo` suffix; scoped ones match exactly.
// Portage: MaskManager.py, stack_lists(incremental=1) with ignore_repo=True
pub(crate) fn stack_mask_lines(sources: &[Vec<String>]) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for lines in sources {
        for line in lines {
            match line.strip_prefix('-') {
                Some(removed) if removed.contains("::") => {
                    list.retain(|x| x != removed);
                }
                Some(removed) => {
                    list.retain(|x| {
                        x.split_once("::").map_or(x.as_str(), |(atom, _)| atom) != removed
                    });
                }
                None => list.push(line.clone()),
            }
        }
    }
    list
}

/// `package.accept_keywords`: `<atom-or-wildcard> <keyword...>` per
/// line. Bare atoms keep an empty list; `resolve_config` fills in the
/// implicit `~arch` defaults once `accept_keywords` is final.
pub(crate) fn parse_package_accept_keywords_lines(lines: &[String]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(atom) = parts.next() else {
            continue;
        };
        let keywords: Vec<String> = parts.map(String::from).collect();
        result.push((atom.to_string(), keywords));
    }
    result
}

/// `package.license`/`package.properties`/`package.accept_restrict`:
/// each line is `<atom-or-wildcard> <token...>`; a bare atom is a no-op
/// and filtered out (unlike `package.accept_keywords`, no other file gets
/// the implicit-default treatment).
pub(crate) fn parse_package_license_lines(lines: &[String]) -> Vec<(String, Vec<String>)> {
    parse_package_accept_keywords_lines(lines)
        .into_iter()
        .filter(|(_, tokens)| !tokens.is_empty())
        .collect()
}

/// `license_groups`: each line is `<group-name> <license or @group ...>`.
/// Later sources extend (never replace or remove) the same group name.
// Portage: LicenseManager._read_license_groups
pub(crate) fn parse_license_groups_lines(lines: &[String]) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        groups
            .entry(name.to_string())
            .or_default()
            .extend(parts.map(String::from));
    }
    groups
}

/// Expands one license token against `groups`: plain names pass
/// through; `@group` (optionally negated) expands recursively, silently
/// literal on cycles or unknown groups.
// Portage: LicenseManager._expandLicenseToken
pub(crate) fn expand_license_token(
    token: &str,
    groups: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    fn expand(
        token: &str,
        groups: &HashMap<String, Vec<String>>,
        traversed: &mut HashSet<String>,
    ) -> Vec<String> {
        let (negate, license_name) = match token.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, token),
        };
        let Some(group_name) = license_name.strip_prefix('@') else {
            return vec![token.to_string()];
        };
        let mut result: Vec<String> = if traversed.contains(group_name) {
            vec![format!("@{group_name}")]
        } else if let Some(members) = groups.get(group_name) {
            traversed.insert(group_name.to_string());
            let mut out = Vec::new();
            for member in members {
                // A group's member list never contains a "-"-negated entry.
                if !member.starts_with('-') {
                    out.extend(expand(member, groups, traversed));
                }
            }
            traversed.remove(group_name);
            out
        } else {
            vec![format!("@{group_name}")]
        };
        if negate {
            result = result.iter().map(|t| format!("-{t}")).collect();
        }
        result
    }
    expand(token, groups, &mut HashSet::new())
}

/// Expands every token in `tokens` against `groups`, in order.
// Portage: LicenseManager.expandLicenseTokens
pub(crate) fn expand_license_tokens(
    tokens: &[String],
    groups: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    tokens
        .iter()
        .flat_map(|t| expand_license_token(t, groups))
        .collect()
}

/// `package.use`: `<atom-or-wildcard> <use-token...>` per line, purely
/// additive (no `-atom` removal). `use_expand_shorthand` ports the
/// user-level-only `VIDEO_CARDS: nvidia` prefix syntax; callers pass
/// `false` for repo-level/profile-level lines.
// Portage: UseManager._parse_user_files_to_extatomdict
pub(crate) fn parse_package_use_lines(
    lines: &[String],
    use_expand_shorthand: bool,
) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(atom) = parts.next() else {
            continue;
        };
        let mut prefix = String::new();
        let mut tokens: Vec<String> = Vec::new();
        for tok in parts {
            if use_expand_shorthand && let Some(name) = tok.strip_suffix(':') {
                prefix = format!("{}_", name.to_lowercase());
                continue;
            }
            if prefix.is_empty() {
                tokens.push(tok.to_string());
            } else if let Some(rest) = tok.strip_prefix('-') {
                tokens.push(format!("-{prefix}{rest}"));
            } else {
                tokens.push(format!("{prefix}{tok}"));
            }
        }
        if tokens.is_empty() {
            continue;
        }
        result.push((atom.to_string(), tokens));
    }
    result
}
