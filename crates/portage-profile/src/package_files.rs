//! package.mask, package.accept_keywords, package.license and package.use parsers.

use crate::*;

/// Reads every non-comment, non-blank, trimmed line from `path`, which
/// may be a single file or a directory of fragments. A directory is read
/// with real `_recursive_file_list` semantics
/// (`lib/portage/util/__init__.py`, via `portage_util::recursive_config_files`):
/// arbitrary depth, one ascending sort per directory over files and
/// subdirectories together with each subdirectory expanded in place,
/// skipping `VCS_DIRS` directories, dot-prefixed names and `~`-suffixed
/// backups at every level including the root path itself, following
/// symlinks. A missing path yields an empty list, not an error.
pub(crate) fn read_config_lines(path: &Path) -> Result<Vec<String>, Error> {
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

    let mut lines = Vec::new();
    // Missing paths stay empty (not an error); unreadable ones report.
    if path.symlink_metadata().is_err() {
        return Ok(lines);
    }
    let files = portage_util::recursive_config_files(path).map_err(|e| Error::ReadFile {
        path: path.display().to_string(),
        source: e,
    })?;
    for entry in files {
        lines.extend(read_file_lines(&entry)?);
    }
    Ok(lines)
}

/// Scopes each of `lines` (raw `package.mask`/`.unmask` entries, a
/// leading `-` meaning removal) to `repo_name` by appending a `::name`
/// suffix to the atom portion -- real `append_repo`'s own "atoms
/// without an explicit repo part get one; atoms that already have one
/// are left alone" rule (`lib/portage/util/__init__.py`), applied here
/// to an overlay's own repo-level entries so they can never silently
/// mask/unmask a same-named package in a *different* repo. A leading
/// `-` (removal) is preserved ahead of the atom, not swallowed into it
/// -- `-cat/pkg` scopes to `-cat/pkg::name`, matching real portage's own
/// behavior of scoping a removal atom exactly like an addition one, so
/// it can only ever cancel a same-repo-scoped entry.
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

/// Same real "add `::repo` to every atom without one" rule as
/// `scope_repo_mask_lines`, applied to the `package.use`/`.mask`/`.force`/
/// `.stable.mask`/`.stable.force` line shape instead
/// (`<atom> <flag> <flag> ...`, see `parse_package_use_lines`): only the
/// leading atom token gets scoped, the flag tokens after it are passed
/// through untouched. Unlike `package.mask`/`.unmask`, none of these
/// files has `-atom` whole-entry removal syntax (confirmed by
/// `parse_package_use_lines`'s own doc comment: real portage only ever
/// `.extend()`s these, and a leading `-` inside the flag list masks/
/// forces a single flag off, not an atom) -- so there's no leading-`-`
/// case to preserve here, unlike `scope_repo_mask_lines`.
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

/// Stacks ordered `package.mask`/`.unmask` lines from multiple sources
/// (earlier sources first) with real portage's own `-atom` removal
/// semantics -- see `MaskManager.py`'s `stack_lists(incremental=1)`: a
/// `-atom` line removes the exact matching atom text added by ANY
/// earlier source in this same stack, not just within its own source
/// (e.g. a user-level `-atom` in `package.mask` can remove an atom the
/// repo or a profile level added). Shared between `package.mask` and
/// `package.unmask`, which real portage stacks identically -- unlike
/// portuale's previous, user-level-only `package.unmask` handling,
/// which treated a leading `-` there as meaningless; it's meaningful
/// once more than one source can contribute an unmask entry.
///
/// Ports real `stack_lists`'s own `ignore_repo=True` behavior (the flag
/// real `MaskManager.__init__` always passes for its own final
/// `[repo_pkgmasklines, profile_pkgmasklines, user_pkgmasklines]`
/// combination -- confirmed by reading it directly): "let `-cat/pkg`
/// remove `cat/pkg::repo`" -- an unscoped removal token (no `::` of its
/// own, which is all a profile-level or user-level `-atom` can ever be,
/// since only repo-level entries ever get `::repo`-scoped at all, see
/// `scope_repo_mask_lines`) strips any `::repo` suffix off every
/// existing entry before comparing, so it cancels a repo-scoped atom
/// from *any* repo, not just an identically-unscoped one -- without
/// this, a profile's `-dev-libs/foo` could never again cancel the main
/// repo's own (now `::reponame`-scoped) `dev-libs/foo` mask entry, a
/// real regression the main-repo-scoping follow-up above would
/// otherwise have introduced. A removal token that's already `::repo`-
/// scoped itself (rare -- only possible if a user writes one by hand)
/// keeps exact-match semantics instead, matching real `stack_lists`'s
/// own `"::" not in token` guard exactly.
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

/// `package.accept_keywords`: each line is `<atom-or-wildcard>
/// <keyword...>`. A line with no keyword tokens after the atom is kept
/// here with an *empty* token list, not dropped -- real portage gives a
/// bare atom an implicit `~arch` meaning at *both* levels, confirmed by
/// reading `KeywordsManager.__init__` (the user-level source: a bare
/// `pkgdict` entry's value is replaced with `accept_keywords_defaults`
/// right there, before it's ever stored in `self.pkeywordsdict`) and
/// `getPKeywords` (the profile-level source: the identical substitution
/// happens at read time instead, per matching entry) -- the same
/// `accept_keywords_defaults` formula either way: `"~" + keyword` for
/// each *plain* (non-`~`/`-`-prefixed) token in the *current* global
/// `ACCEPT_KEYWORDS`. `resolve_config` fills in the actual defaults
/// once `config.accept_keywords` is final (see its own call site) --
/// this function only preserves the bare atom itself so that
/// substitution has something to act on; every other caller of this
/// shape (`parse_package_license_lines` below) filters bare atoms back
/// out immediately after, since no other file gets this treatment.
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
/// each line is `<atom-or-wildcard> <token...>`. Same shape as
/// `package.accept_keywords`, reused directly for all three real files
/// (unlike `parse_package_use_lines`'s own deliberate separateness from
/// `parse_package_accept_keywords_lines`, which exists only because of
/// `package.use`'s own `USE_EXPAND`-shorthand parameter -- these three
/// have no such per-file divergence to justify three near-identical
/// wrapper functions) -- except for a bare atom's own meaning: none of
/// these three files gets `package.accept_keywords`'s own implicit
/// `~arch`-default treatment in real portage, so a bare atom is filtered
/// back out here as a genuine no-op, unlike the shared parser's own
/// bare-atom-preserving behavior.
pub(crate) fn parse_package_license_lines(lines: &[String]) -> Vec<(String, Vec<String>)> {
    parse_package_accept_keywords_lines(lines)
        .into_iter()
        .filter(|(_, tokens)| !tokens.is_empty())
        .collect()
}

/// `license_groups` (real `LicenseManager._read_license_groups`): each
/// non-comment, non-blank line is `<group-name> <license or @group
/// ...>` (`grabdict` format -- no `-atom`/removal semantics at all,
/// unlike `package.mask`'s own file format). Later sources *extend*
/// (never replace or stack-remove) whatever the same group name already
/// has, matching real `self._license_groups.setdefault(k, []).extend(v)`
/// exactly.
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

/// Expands a single `ACCEPT_LICENSE`/`package.license` token against
/// `groups`: a plain license name (or `*`/`-*`, real portage's own
/// symbolic wildcard tokens) passes through unchanged; an `@group-name`
/// token (optionally `-`-negated) expands to every one of that group's
/// own members, each recursively expanded the same way (a member that's
/// itself a nested `@group` reference, real portage's own
/// `_expandLicenseToken` "elif license_group:" recursion) -- negation
/// applies to every expanded member, not just the group reference
/// itself (`-@EULA` with `EULA = "Eula1 Eula2"` expands to `-Eula1
/// -Eula2`, matching real portage's own tail `if negate: rValue = ["-" +
/// token for token in rValue]`). `traversed` guards against a circular
/// group reference (matching real portage's own cycle guard): a group
/// already being expanded higher up this same call stack is left as its
/// own literal `@group-name` text instead of recursing infinitely, same
/// for a genuinely undefined group name (never declared in any
/// `license_groups` file at all) -- both deliberately silent here (no
/// stderr warning), unlike real portage's own `writemsg`, since this
/// portuale has no precedent for emitting warnings to stderr from deep
/// inside config resolution (every other real portage warning-only path
/// in portuale is silently skipped the same way).
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
                // Real portage: "Skipping invalid element %s in license
                // group '%s'" -- a group's own member list is never
                // itself allowed to contain a "-"-negated entry.
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

/// Expands every token in `tokens` against `groups`, in order -- see
/// `expand_license_token`'s own doc comment. Mirrors real
/// `LicenseManager.expandLicenseTokens` exactly.
pub(crate) fn expand_license_tokens(
    tokens: &[String],
    groups: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    tokens
        .iter()
        .flat_map(|t| expand_license_token(t, groups))
        .collect()
}

/// `package.use`: each line is `<atom-or-wildcard> <use-token...>`. A line
/// with no tokens after the atom is a documented no-op, matching
/// `parse_package_accept_keywords_lines`. Purely additive across sources,
/// like `package.accept_keywords` and unlike `package.mask`/`.unmask`:
/// real portage's own `package.use` consumption (`config.py`'s
/// `regenerate` -- see the module doc comment on `USE_ORDER`) only ever
/// `.extend()`s a growing token list per source, never removes a
/// previous entry, so there's no `-atom` semantics to port here at all.
///
/// `use_expand_shorthand`, when true, ports real
/// `UseManager._parse_user_files_to_extatomdict`'s own `VIDEO_CARDS:
/// nvidia intel` syntax: a token ending in `:` sets a
/// `lowercase(name) + "_"` prefix applied to every following token on
/// *that same line* (a leading `-` stays outside the new prefix, e.g.
/// `-flag` becomes `-video_cards_flag`, not `video_cards_-flag`) --
/// reset back to none at the start of every line, confirmed by reading
/// real `grabdict_package`'s own `newlines=1` marker handling (a fresh
/// `"\n"` token is inserted between every physical line for the same
/// atom, and the real code's own loop resets its prefix on each one).
/// Callers pass `false` for repo-level/profile-level lines: confirmed by
/// reading `UseManager.__init__`, only the *user*-level source
/// (`_parse_user_files_to_extatomdict`) ever applies this shorthand at
/// all -- `_parse_repository_files_to_dict_of_dicts`/
/// `_parse_profile_files_to_tuple_of_dicts` both go through
/// `_parse_file_to_dict` instead, which never passes `newlines=1` and
/// has no such expansion step, so a `VIDEO_CARDS:` token in a
/// repo-level or profile-level `package.use` file is (in real portage)
/// just a literal, almost-certainly-invalid USE token, not shorthand at
/// all. This is genuine real behavior, not a portuale-invented
/// restriction -- see `resolve_config`'s own call sites.
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
