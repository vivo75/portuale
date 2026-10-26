//! Profile-file atom validation: a port of Portage's `grabfile_package`
//! and `grabdict_package` with the per-node EAPI and per-repo formats
//! those readers take. Invalid lines are dropped with a `---` warning
//! (collected in [`ProfileWarning`], printed once by the CLI); valid
//! lines keep their original text for the existing downstream folds.

use crate::*;

use portage_dep::eapi::{eapi_attrs, eapi_is_supported};
use portage_dep::{AtomEapi, AtomOptions};

/// Warning ranks. Portage reads profile files manager by manager, so every
/// `packages` warning precedes every `package.use` one no matter in which
/// order this crate folds the files; `resolve_config` stable-sorts by rank.
/// User-config-only files (`package.license`, `package.properties`,
/// `package.accept_restrict`, `package.env`) warn between `package.use` and
/// `package.accept_keywords`, where Portage's LicenseManager and the
/// config.py inline readers run (config.py:855-902, oracle Q2e).
// Portage: config.py (`packages`), UseManager, LicenseManager, KeywordsManager, MaskManager
pub(crate) const WARN_RANK_REPO: u8 = 0;
pub(crate) const WARN_RANK_PACKAGES: u8 = 1;
pub(crate) const WARN_RANK_USE: u8 = 2;
pub(crate) const WARN_RANK_LICENSE: u8 = 3;
pub(crate) const WARN_RANK_KEYWORDS: u8 = 4;
pub(crate) const WARN_RANK_MASK: u8 = 5;
/// `virtuals` files read through the profile chain: Portage's
/// `VirtualsManager` warns after the `package.mask` reads of the same run.
pub(crate) const WARN_RANK_VIRTUALS_PROFILE: u8 = 6;

/// One collected config-load warning plus its file-kind rank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileWarning {
    pub rank: u8,
    pub text: String,
}

/// Per-profile-node atom rules: the node's `eapi` plus the `profile_formats`
/// / `allow_build_id` of the repo containing it.
// Portage: LocationsManager.py:204-343, repository/config.py:1476-1485
#[derive(Debug, Clone)]
pub(crate) struct NodeAtomRules {
    pub eapi: Option<String>,
    pub allow_repo: bool,
    pub allow_build_id: bool,
    pub allow_wildcard: bool,
}

/// One repo's atom rules: its `profile-formats`, the computed `repo.eapi`,
/// and the file-adjacent `profiles/eapi` value repo-level readers parse with.
// Portage: repository/config.py:524-547, util/__init__.py:441-475
#[derive(Debug, Clone)]
pub(crate) struct RepoRules {
    pub name: String,
    pub formats: Vec<String>,
    pub eapi: String,
    pub file_eapi: String,
}

/// Portage `allow_profile_repo_deps`: `eapi_has_repo_deps(eapi)` (false for
/// every supported EAPI, true for `None`/unsupported) or `profile-repo-deps`
/// in the formats.
// Portage: repository/config.py:1476-1485, eapi.py:111-112
pub(crate) fn allow_profile_repo_deps(eapi: Option<&str>, formats: &[String]) -> bool {
    eapi_attrs(eapi).map(|a| a.repo_deps).unwrap_or(true)
        || formats.iter().any(|f| f == "profile-repo-deps")
}

/// Whether `eapi` carries the `profile_eapi_default` attr (EAPI >= 9):
/// a repo's top-level `profiles/eapi` only becomes `repo.eapi` then.
// Portage: eapi.py:159-160, repository/config.py:524-547
fn eapi_has_profile_eapi_default(eapi: &str) -> bool {
    eapi_attrs(Some(eapi))
        .map(|a| a.profile_eapi_default)
        .unwrap_or(false)
}

/// This repo's `metadata/layout.conf`: the `profile-formats` tokens plus the
/// raw `profile_eapi_when_unspecified` value. The underscore spelling is
/// normative (S0 matrix §3; repository/config.py:529,547,1689).
// Portage: repository/config.py, parse_layout_conf
pub(crate) fn repo_layout_conf(location: &Path) -> (Vec<String>, Option<String>) {
    let mut formats = Vec::new();
    let mut key = None;
    if let Ok(text) = fs::read_to_string(location.join("metadata/layout.conf")) {
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            if k.trim() == "profile-formats" {
                formats = v.split_whitespace().map(String::from).collect();
            } else if k.trim() == "profile_eapi_when_unspecified" {
                key = Some(v.trim().to_string());
            }
        }
    }
    (formats, key)
}

/// This repo's computed `repo.eapi` plus the file-adjacent `profiles/eapi`
/// value its repo-level files parse with. Ranks `WARN_RANK_REPO`: the layout
/// `SyntaxWarning` (if any) precedes every `---` line, as in Portage.
// Portage: repository/config.py:524-547 (repo.eapi), :1688-1727 (the key
// gate), util/__init__.py:441-475 (the file-adjacent reader)
pub(crate) fn repo_rules_for(
    repos: &[(String, PathBuf)],
    warn: &mut Vec<ProfileWarning>,
) -> Vec<RepoRules> {
    repos
        .iter()
        .map(|(name, location)| {
            let (formats, key) = repo_layout_conf(location);
            let layout_file = location
                .join("metadata/layout.conf")
                .display()
                .to_string();
            // The key counts only with `profile-default-eapi` in the
            // formats and only for a supported EAPI; otherwise it is
            // dropped with a SyntaxWarning and the node default is "0".
            let gated = match key {
                Some(k)
                    if formats.iter().any(|f| f == "profile-default-eapi")
                        && eapi_is_supported(&k) =>
                {
                    Some(k)
                }
                Some(k) => {
                    let text = if !formats.iter().any(|f| f == "profile-default-eapi") {
                        format!(
                            "SyntaxWarning: Repository named '{name}' has profile_eapi_when_unspecified setting in '{layout_file}', but 'profile-default-eapi' is not listed in the profile-formats field. Please report this issue to the repository maintainer."
                        )
                    } else {
                        format!(
                            "SyntaxWarning: Repository named '{name}' has unsupported EAPI '{k}' setting in '{layout_file}'; please upgrade portage."
                        )
                    };
                    warn.push(ProfileWarning {
                        rank: WARN_RANK_REPO,
                        text,
                    });
                    None
                }
                None => None,
            };
            // The file-adjacent reader: exactly one line (rstrip `\n`
            // only), else warn once and fall back; missing stays silent.
            // Computed once per repo, like Portage's `_eapi_cache`.
            let top = read_adjacent_eapi(&location.join("profiles"), warn);
            // A `profiles/eapi` with `profile_eapi_default` (EAPI >= 9)
            // wins over the layout key; anything else falls back to it.
            // (Portage also warns when a present key conflicts with such a
            // top-level default; no fixture reaches that: not ported.)
            let eapi = match &top {
                Some(t) if eapi_has_profile_eapi_default(t) => t.clone(),
                _ => gated.unwrap_or_else(|| "0".to_string()),
            };
            let file_eapi = top.unwrap_or_else(|| eapi.clone());
            RepoRules {
                name: name.clone(),
                formats,
                eapi,
                file_eapi,
            }
        })
        .collect()
}

/// Portage `read_corresponding_eapi_file` for the directory `dir`: its
/// `eapi` file must hold exactly one line (only `\n` stripped), else warn
/// `--- Invalid 'eapi' file …` and return `None`; a missing file is `None`
/// with no warning.
// Portage: util/__init__.py:441-475
pub(crate) fn read_adjacent_eapi(dir: &Path, warn: &mut Vec<ProfileWarning>) -> Option<String> {
    let path = dir.join("eapi");
    let text = fs::read_to_string(&path).ok()?;
    // `readlines()` line count, not `str::lines()`: both agree on the
    // fixture shapes (LF, optional single trailing newline); empty means
    // zero lines, like Python.
    let count = if text.is_empty() {
        0
    } else {
        text.split('\n').count() - usize::from(text.ends_with('\n'))
    };
    if count != 1 {
        warn.push(ProfileWarning {
            rank: WARN_RANK_REPO,
            text: format!(
                "--- Invalid 'eapi' file (doesn't contain exactly one line): {}",
                path.display()
            ),
        });
        return None;
    }
    Some(text.split('\n').next().unwrap_or("").to_string())
}

/// This profile node's rules: its `eapi` file (`readline().strip()`), else
/// the containing repo's `repo.eapi` (or `"0"` outside any known repo).
/// An unsupported value raises Portage's `ParseError`, which empties the
/// whole profile list (the caller aborts).
// Portage: LocationsManager.py:216-242
pub(crate) fn node_atom_rules(
    node: &Path,
    formats: &[String],
    repo_default_eapi: &str,
) -> Result<NodeAtomRules, Error> {
    let eapi_file = node.join("eapi");
    let eapi = match fs::read_to_string(&eapi_file) {
        Err(_) => repo_default_eapi.to_string(),
        Ok(text) => {
            let first = text.split('\n').next().unwrap_or("").trim();
            if !eapi_is_supported(first) {
                return Err(Error::UnsupportedProfileEapi {
                    eapi: first.to_string(),
                    path: eapi_file.display().to_string(),
                });
            }
            first.to_string()
        }
    };
    Ok(NodeAtomRules {
        allow_repo: allow_profile_repo_deps(Some(&eapi), formats),
        allow_build_id: formats.iter().any(|f| f == "build-id"),
        allow_wildcard: false,
        eapi: Some(eapi),
    })
}

/// This profile node's rules via the longest-prefix repo match (unknown
/// nodes behave like Portage's empty-`intersecting_repos` case). Callers
/// build `repo_rules` with [`repo_rules_for`] (cheap; its warnings dedupe
/// at the end of `resolve_config`).
// Portage: LocationsManager.py:204-233 (_addProfile's repo lookup)
pub(crate) fn rules_for_node(
    node: &Path,
    all_repos: &[(String, PathBuf)],
    repo_rules: &[RepoRules],
) -> Result<NodeAtomRules, Error> {
    let hit = repo_containing(node, all_repos).and_then(|(name, _)| {
        repo_rules
            .iter()
            .find(|r| r.name == name)
            .map(|r| (r.formats.clone(), r.eapi.clone()))
    });
    let (formats, default) = hit.unwrap_or_default();
    let default = if default.is_empty() {
        "0".to_string()
    } else {
        default
    };
    node_atom_rules(node, &formats, &default)
}

/// The `profile-formats` of the repo containing `node` (empty outside any
/// known repo).
// Portage: LocationsManager.py:204-233
pub(crate) fn node_profile_formats(
    node: &Path,
    all_repos: &[(String, PathBuf)],
    repo_rules: &[RepoRules],
) -> Vec<String> {
    repo_containing(node, all_repos)
        .and_then(|(name, _)| repo_rules.iter().find(|r| r.name == name))
        .map(|r| r.formats.clone())
        .unwrap_or_default()
}

/// User-config (`/etc/portage/package.*`) and user-profile
/// (`/etc/portage/profile/*`) rules: Portage reads both with `eapi=None`
/// (`verify_eapi=False`) and `allow_repo=True`; wildcards and build-ids
/// are per file (the caller picks them from the table below).
// Portage: MaskManager.py:214-232, KeywordsManager.py:73-104,
// UseManager.py:277-288,337-348, LicenseManager.py:37-42,
// config.py:655-666,855-902 (oracle Q2e)
pub(crate) fn user_atom_rules(allow_wildcard: bool, allow_build_id: bool) -> NodeAtomRules {
    NodeAtomRules {
        eapi: None,
        allow_repo: true,
        allow_build_id,
        allow_wildcard,
    }
}

/// The user profile's `profile-formats`: Portage hard-codes
/// `("profile-bashrcs", "profile-set")` for the appended node, so the
/// node qualifies for `@profile` (`profile-set`) but never for the
/// format-gated readers (`build-id`, `profile-license`, ...).
// Portage: LocationsManager.py:169-189
pub(crate) const USER_PROFILE_FORMATS: [&str; 2] = ["profile-bashrcs", "profile-set"];

/// The user profile (`/etc/portage/profile`) as Portage's last profile
/// node: present only when a `make.profile` resolved (the chain is
/// non-empty) and the directory exists. The node's own `parent` file is
/// never walked (the node is appended, not visited).
// Portage: LocationsManager.py:169-189
// (`self._user_config and self.profiles` plus `os.path.exists`)
pub(crate) struct UserProfileNode {
    pub path: PathBuf,
    /// The node's `eapi` file content, or `None` when the file is absent
    /// or malformed (never the repo default, never an abort — unlike
    /// chain nodes, whose bad `eapi` empties the profile list).
    pub eapi: Option<String>,
}

/// [`UserProfileNode`] for this resolve, or `None` when Portage would
/// not append the node. The `eapi` read warns exactly like the
/// repo-level reader on a malformed file (deduped to once downstream).
// Portage: read_corresponding_eapi_file(custom_prof + os.sep, default=None)
pub(crate) fn resolve_user_profile_node(
    config_root: &Path,
    chain: &[PathBuf],
    warnings: &mut Vec<ProfileWarning>,
) -> Option<UserProfileNode> {
    if chain.is_empty() {
        return None;
    }
    let path = config_root.join("etc/portage/profile");
    if !path.exists() {
        return None;
    }
    let eapi = read_adjacent_eapi(&path, warnings);
    Some(UserProfileNode { path, eapi })
}

/// Which user-profile file's rules to build: Portage validates every
/// file with the node's EAPI but varies wildcards/build-ids per file.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UserNodeFile {
    /// `package.mask`, `package.unmask`, `packages`: no wildcards.
    Mask,
    /// `package.use*`: wildcards only when the node has no EAPI.
    PackageUse,
    /// `package.accept_keywords`: no wildcards, no build-ids.
    AcceptKeywords,
    /// Profile-node `package.keywords`: no wildcards, no build-ids on
    /// the user node (its formats never carry `build-id`); chain levels
    /// keep their own node rules instead (build-ids per formats).
    PackageKeywords,
}

/// Rules for one user-profile file: the node's EAPI drives the atom
/// grammar (`None` parses permissively, like the user files); wildcards
/// only for `package.use*` with no EAPI (`extended_syntax = eapi is None
/// and user_config`); `allow_repo` from the user formats; build-ids
/// everywhere but `package.accept_keywords`/`package.keywords`.
// Portage: MaskManager.py:171-193, KeywordsManager.py:25-60,
// UseManager.py:277-288, config.py:655-666
pub(crate) fn user_node_rules(eapi: Option<&str>, file: UserNodeFile) -> NodeAtomRules {
    let formats: Vec<String> = USER_PROFILE_FORMATS.iter().map(|s| s.to_string()).collect();
    NodeAtomRules {
        allow_repo: allow_profile_repo_deps(eapi, &formats),
        allow_build_id: !matches!(
            file,
            UserNodeFile::AcceptKeywords | UserNodeFile::PackageKeywords
        ),
        allow_wildcard: matches!(file, UserNodeFile::PackageUse) && eapi.is_none(),
        eapi: eapi.map(str::to_string),
    }
}

/// A repo's own repo-level file rules: parsed with the file-adjacent
/// `profiles/eapi` value, but `allow_repo` follows the computed `repo.eapi`
/// (Portage passes the whole `RepoConfig` to `allow_profile_repo_deps`).
// Portage: MaskManager.py (grab_pmask), KeywordsManager, UseManager
pub(crate) fn repo_file_rules(repo_rules: &[RepoRules], name: &str) -> NodeAtomRules {
    let r = repo_rules
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("repo_file_rules: unknown repo {name:?}"));
    NodeAtomRules {
        allow_repo: allow_profile_repo_deps(Some(&r.eapi), &r.formats),
        allow_build_id: r.formats.iter().any(|f| f == "build-id"),
        allow_wildcard: false,
        eapi: Some(r.file_eapi.clone()),
    }
}

/// Which line grammar `filter_atom_lines` validates.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AtomLineKind {
    /// `package.mask`/`package.unmask`/`packages`: whole-line atoms; a
    /// leading `-` is removal (kept for the stacker), and `packages`
    /// additionally strips a leading `*` before parsing (kept as well).
    File { packages_file: bool },
    /// `<atom> <flags...>` dict files; with `allow_use = false` a
    /// use-carrying atom is dropped with Portage's other message.
    Dict { allow_use: bool },
}

/// `grabfile_package` / `grabdict_package`: keep the valid lines (original
/// text), warn `--- Invalid atom in <file>: <msg>` (or the use-deps variant)
/// and drop the rest. `rank` orders the warnings Portage's way; `warn`
/// collects them for one print at config load.
// Portage: util/__init__.py:481-607
pub(crate) fn filter_atom_lines(
    sources: &[(PathBuf, String)],
    rules: &NodeAtomRules,
    kind: AtomLineKind,
    rank: u8,
    warn: &mut Vec<ProfileWarning>,
) -> Vec<String> {
    filter_atom_lines_with_sources(sources, rules, kind, rank, warn)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}

/// [`filter_atom_lines`] keeping each kept line's source file: the
/// `package.mask` fold needs the origin fragment to answer Portage's
/// `getmaskingreason` (`# <file>:` + comment lines in the unmask block,
/// backlog #462). Same validation, warnings and order; only the shape differs.
// Portage: util/__init__.py:481-607
pub(crate) fn filter_atom_lines_with_sources(
    sources: &[(PathBuf, String)],
    rules: &NodeAtomRules,
    kind: AtomLineKind,
    rank: u8,
    warn: &mut Vec<ProfileWarning>,
) -> Vec<(String, PathBuf)> {
    // A supported-but-unparseable EAPI (`3_pre1`, padded) makes Portage
    // raise; here it falls back to the permissive set (see S1's cut).
    let eapi = AtomEapi::parse(rules.eapi.as_deref()).unwrap_or(AtomEapi::Unsupported);
    let eapi_str = rules.eapi.as_deref().unwrap_or("");
    let opts = AtomOptions {
        eapi,
        allow_wildcard: rules.allow_wildcard,
        allow_repo: Some(rules.allow_repo),
        allow_build_id: Some(rules.allow_build_id),
    };
    let mut kept = Vec::new();
    for (source, line) in sources {
        let file = source.display().to_string();
        match kind {
            AtomLineKind::File { packages_file } => {
                let mut text = line.as_str();
                if text.starts_with('-') {
                    if packages_file && text == "-*" {
                        kept.push((line.clone(), source.clone()));
                        continue;
                    }
                    text = &text[1..];
                }
                if packages_file && text.starts_with('*') {
                    text = &text[1..];
                }
                match portage_dep::parse_atom_with(text, &opts, eapi_str) {
                    Ok(_) => kept.push((line.clone(), source.clone())),
                    Err(e) => warn.push(ProfileWarning {
                        rank,
                        text: format!("--- Invalid atom in {file}: {}", e.message),
                    }),
                }
            }
            AtomLineKind::Dict { allow_use } => {
                let atom_text = line.split_whitespace().next().unwrap_or("");
                match portage_dep::parse_atom_with(atom_text, &opts, eapi_str) {
                    Err(e) => warn.push(ProfileWarning {
                        rank,
                        text: format!("--- Invalid atom in {file}: {}", e.message),
                    }),
                    Ok(atom) if !allow_use && atom.use_deps.is_some() => {
                        warn.push(ProfileWarning {
                            rank,
                            text: format!(
                                "--- Atom is not allowed to have USE flag(s) in {file}: {atom_text}"
                            ),
                        });
                    }
                    Ok(_) => kept.push((line.clone(), source.clone())),
                }
            }
        }
    }
    kept
}
