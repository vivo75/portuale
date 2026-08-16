// Profile-chain + make.conf + package.mask/.unmask/.accept_keywords
// resolution for real USE/ACCEPT_KEYWORDS/visibility (see
// PORTING/PROMPT.md's depgraph/config-resolution follow-up work, and
// PORTING/README.md for the full scope writeup). Replaces the base
// `emerge --pretend` slice's hardcoded `ACCEPT_KEYWORDS="amd64"`/`USE=""`
// with the real mechanism: a profile inheritance chain (`make.profile` ->
// `parent` files) plus `/etc/portage/make.conf`, each level's
// `make.defaults` contributing incremental USE/ACCEPT_KEYWORDS tokens,
// plus per-package overrides from `/etc/portage/package.mask`,
// `package.unmask`, and `package.accept_keywords`.
//
// KNOWN, DOCUMENTED SCOPE CUTS (confirmed with the user before
// implementing):
//   - Cross-repo profile parent references (`reponame:path` syntax) are
//     rejected with a clear error rather than resolved -- doing so would
//     need full multi-repo resolution (this pilot only ever looks at the
//     main repo). This means a profile using a cross-repo parent (as real
//     Gentoo's desktop/plasma profiles often do) will NOT fully resolve
//     under v1; testing this mechanism needs a same-repo synthetic
//     fixture chain instead (see PORTING/fixtures/repo/profiles).
//   - USE_EXPAND (LINGUAS, VIDEO_CARDS, ...), wildcard `_*` IUSE-aware
//     expansion, and ARCH-based KEYWORDS-format validation are all out
//     of scope. `package.use`'s
//     USE_EXPAND-prefix shorthand (`VIDEO_CARDS: nvidia` lines applying a
//     `video_cards_` prefix to subsequent flags until a blank line resets
//     it -- see real `UseManager._parse_user_files_to_extatomdict`) is
//     also out of scope; only plain `-flag`/`flag`/`+flag` tokens are
//     read.
//   - Only the `defaults` (profile) and `conf` (make.conf) layers of real
//     config.py's `USE_ORDER` are implemented -- no `env`, `pkginternal`,
//     `features`, `repo`, `env.d`, or per-package (`pkg`) layers.
//   - No line continuation / multi-line quoted values, and no trailing
//     `# comment` after a real assignment (a `#` is only recognized as a
//     comment when it starts the (trimmed) line). Real make.defaults/
//     make.conf files in practice don't rely on either.
//   - Any variable other than USE/ACCEPT_KEYWORDS is tracked only as a
//     plain scalar for `${VAR}` substitution purposes (last-value-wins,
//     no incremental merge), matching how it's actually used in real
//     profiles (e.g. ARCH feeding `ACCEPT_KEYWORDS="${ARCH}"`).
//   - `package.mask`/`.unmask` are stacked from all three real sources --
//     the main repo's own repo-level `profiles/package.mask`/`.unmask`,
//     every profile level's own pair (in chain order), and the
//     user-level `/etc/portage` files -- with `-atom` removal applying
//     across the whole combined stream, exactly matching real
//     `MaskManager.py`'s `stack_lists(incremental=1)` (see
//     `stack_mask_lines`). Still out of scope: an *overlay* repo's own
//     repo-level `package.mask`/`.unmask` (only the main repo's is read;
//     matches the overlays follow-up's own already-confirmed "per-repo
//     package.mask/.unmask/profiles/ out of scope" cut), and `masters`
//     (eclass/mask inheritance across repos via a repo's own `masters`
//     setting).
//   - `package.accept_keywords` is stacked from profile-chain (in chain
//     order) + user-level sources, mirroring real `KeywordsManager.
//     getPKeywords` exactly -- confirmed by reading it, there's no
//     repo-level source for this file in real portage at all (unlike
//     `package.mask`'s repo-level `profiles/package.mask`). Purely
//     additive, like the pilot's own pre-existing user-level-only
//     handling always was: no `-atom` removal exists for this file in
//     real portage either, so every matching source's keyword tokens are
//     just unioned together (see `is_visible`). A bare atom with no
//     keyword tokens is a no-op at *both* levels here, which is only a
//     simplification for the profile-level source -- real portage gives
//     a bare *profile*-level entry an implicit derived `~arch` meaning
//     (`accept_keywords_defaults` in `getPKeywords`) that a bare
//     *user*-level entry never gets, so kept simple and symmetric
//     between the two rather than adding a profile-only special case
//     (see `parse_package_accept_keywords_lines`'s own doc comment).
//   - `package.use` is stacked from all three real sources -- repo-level
//     (`<main_repo_location>/profiles/package.use`), every profile
//     level's own `package.use` (in chain order), and user-level -- the
//     same file-location convention `package.mask` and
//     `package.accept_keywords` both already use (confirmed by reading
//     `UseManager.__init__`), concatenated and parsed once, purely
//     additive like `package.accept_keywords` (no `-atom` removal
//     exists for this file at all -- see `parse_package_use_lines`).
//     This is a deliberate, confirmed-with-the-user simplification, not
//     a full port of real portage's own mechanism: real repo-level
//     `package.use` lands in a distinct `configdict["repo"]` USE_ORDER
//     layer and profile-level in `configdict["defaults"]` (merged
//     per-level with that level's own `make.defaults` USE), both part of
//     the full `USE_ORDER` precedence sequence this pilot only partially
//     implements (see the "Only the `defaults`... layers of real
//     config.py's `USE_ORDER`" bullet above) -- but since this pilot's
//     own per-package application (see below) already flattens
//     `package.use` into one incremental list regardless of source,
//     extending that flat model from one source to three doesn't add a
//     *new* simplification, it just applies the pre-existing one more
//     widely, the same reasoning that applied to `package.mask` and
//     `package.accept_keywords` before it. `package.use` entries are
//     applied per package (not globally): a matching entry's tokens are
//     layered on top of the base `use_flags` set with the same
//     incremental semantics as `USE` itself (see `apply_incremental`),
//     scoped to only the one package being resolved/recursed into -- see
//     `portage-repo`'s `resolve_pretend_graph` for where that
//     per-package application happens (it needs the candidate's SLOT to
//     match slotted `package.use` entries, which only exists at that
//     later, repo-aware layer).
//   - `packages` (`@system`'s real source -- `PackagesSystemSet` in
//     `lib/portage/_sets/profiles.py`) IS now read: every profile level's
//     own `<level>/packages` file, in chain order, stacked with the
//     identical `stack_lists(incremental=1)` semantics `package.mask`
//     already ports (see `stack_mask_lines`) -- confirmed by reading
//     `PackagesSystemSet.load`, which calls the exact same real
//     `stack_lists` function `MaskManager` does, on the *raw* lines
//     (`*foo` and plain `foo` alike -- `-foo` only ever removes an
//     earlier exact-text `foo`, never a `*foo`, same plain string
//     equality `stack_mask_lines` already uses). Only *after* stacking
//     does real portage keep the subset starting with `*` (stripping the
//     `*`) as the actual `@system` atom list -- every other stacked line
//     is a "this package is known to the profile but not part of the
//     base system" hint with no `@system`-set meaning of its own, so
//     `system_packages` applies that same post-stack filter. No
//     repo-level or user-level source exists for this file in real
//     portage at all -- confirmed by reading (only
//     `PackagesSystemSet.__init__`'s `profiles` list, never
//     `config_root`), unlike `package.mask`'s repo-level
//     `profiles/package.mask`.
//   - `use.mask`/`use.force` (global USE forcing) ARE now read: every
//     profile level's own `<level>/use.mask`/`use.force` file, in chain
//     order, stacked with the identical `stack_lists(incremental=True)`
//     semantics `package.mask`/`packages` already port (see
//     `stack_mask_lines`) -- confirmed by reading `UseManager.
//     getUseMask`/`getUseForce`'s own `pkg=None` case, the one real
//     `config.py`'s `regenerate()` actually calls to build the *global*
//     `USE` value this pilot's flat model corresponds to: it returns
//     `stack_lists(self._usemask_list/self._useforce_list,
//     incremental=True)` directly, never touching a repo-level or
//     per-package source at all -- those only exist on the *per-package*
//     path (`pkg` not `None`), out of scope here the same way
//     `package.use`'s own repo/profile/user-only sourcing already
//     doesn't reach a per-package USE_ORDER layer either. Applied last,
//     after every other real accumulation source (profile chain,
//     make.conf): every `use.force` flag is force-added, THEN every
//     `use.mask` flag is force-removed, exactly matching real
//     `regenerate()`'s own `myflags.update(useforce)` followed by
//     `myflags.difference_update(usemask)` -- so a flag listed in both
//     ends up masked, not forced. `use_force`/`use_mask` are also
//     exposed on `Config` directly (not just folded into `use_flags`),
//     since real portage's own `forced_flags` (consumed by
//     `--newuse`'s `reinstall_flags_for_newuse` in `portage-repo`,
//     previously always empty -- see that function's own doc comment)
//     is `use.force ∪ use.mask`, not either alone.
//
// One real, deliberately-preserved quirk from lib/portage/package/ebuild/
// config.py (see the comment above its `expand_map.pop("USE", None)`):
// `${VAR}` substitution persists across profile levels for every variable
// *except* USE, which is reset before each level's make.defaults is
// parsed. This stops a parent profile's accumulated USE from leaking into
// a child's own `USE="${USE} flag"` self-append. In the flat, single-set
// consumption model this pilot uses (no package.use interaction, unlike
// the real bug this quirk guards against), that particular scenario
// usually doesn't change the final *set* of enabled flags -- it's ported
// anyway for fidelity with the real algorithm, since it's cheap to do and
// it's what real portage actually does.

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub use_flags: HashSet<String>,
    pub accept_keywords: HashSet<String>,
    /// Raw atom or bounded-wildcard-atom strings (see
    /// `portage_dep::parse_wildcard_atom`) from `package.mask`, with
    /// `-atom` removal already applied within this source.
    pub package_mask: Vec<String>,
    /// Raw atom or bounded-wildcard-atom strings from `package.unmask`.
    pub package_unmask: Vec<String>,
    /// (atom-or-wildcard string, extra accepted keyword tokens) pairs
    /// from `package.accept_keywords`. A `"**"` keyword token means
    /// "accept any keyword" for matching packages.
    pub package_accept_keywords: Vec<(String, Vec<String>)>,
    /// (atom-or-wildcard string, raw USE tokens) pairs from `package.use`.
    /// Tokens use the same `-flag`/`flag`/`+flag` incremental syntax as
    /// `USE` itself -- see `apply_incremental`.
    pub package_use: Vec<(String, Vec<String>)>,
    /// `@system`'s real atom source: every profile level's own `packages`
    /// file, stacked in chain order and filtered to `*`-prefixed lines
    /// (the `*` stripped) -- see the module doc comment's `packages`
    /// bullet and `PackagesSystemSet.load`.
    pub system_packages: Vec<String>,
    /// Flags forced on by every profile level's own `use.force` file,
    /// already folded into `use_flags` -- exposed separately too since
    /// real portage's own `forced_flags` (e.g. `--newuse`'s
    /// `reinstall_flags_for_newuse`) is `use.force ∪ use.mask`, not
    /// either one alone. See the module doc comment's `use.mask`/
    /// `use.force` bullet.
    pub use_force: HashSet<String>,
    /// Flags forced off by every profile level's own `use.mask` file,
    /// already folded out of `use_flags`. See `use_force`'s own doc
    /// comment.
    pub use_mask: HashSet<String>,
}

fn var_ref_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap())
}

/// Substitutes `${VARNAME}` references against `scalars`, matching bash's
/// default (unset-as-empty) behavior for unknown variables.
fn substitute(value: &str, scalars: &HashMap<String, String>) -> String {
    var_ref_re()
        .replace_all(value, |caps: &regex::Captures| {
            scalars.get(&caps[1]).cloned().unwrap_or_default()
        })
        .into_owned()
}

/// Parses one `KEY="value"` / `KEY='value'` / `KEY=value` line. Returns
/// `None` for comments, blank lines, or anything that isn't a simple
/// assignment (conditionals, function defs, etc. -- out of scope; real
/// make.defaults/make.conf files don't use them).
fn parse_kv_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    if key.is_empty()
        || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || key.chars().next().unwrap().is_ascii_digit()
    {
        return None;
    }
    let mut value = line[eq + 1..].trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            value = &value[1..value.len() - 1];
        }
    }
    Some((key, value))
}

/// Applies real incremental-variable token semantics: `-*` clears
/// everything accumulated so far, `-flag` removes, `flag`/`+flag` adds
/// (a leading `+` is invalid per PMS but real config.py tolerates it by
/// stripping it, which this mirrors). Public so `portage-repo` can reuse
/// it to apply `package.use` tokens on top of a per-package clone of the
/// base USE set -- see the module doc comment.
pub fn apply_incremental(tokens: &str, set: &mut HashSet<String>) {
    for tok in tokens.split_whitespace() {
        if tok == "-*" {
            set.clear();
        } else if let Some(rest) = tok.strip_prefix('-') {
            set.remove(rest);
        } else if let Some(rest) = tok.strip_prefix('+') {
            if !rest.is_empty() {
                set.insert(rest.to_string());
            }
        } else {
            set.insert(tok.to_string());
        }
    }
}

/// Processes one file's lines against the shared scalar/USE/ACCEPT_KEYWORDS
/// state, without any `source` support (used for make.defaults; make.conf
/// wraps this with `source` handling -- see `process_make_conf_file`).
fn process_lines(text: &str, scalars: &mut HashMap<String, String>, config: &mut Config) {
    for line in text.lines() {
        let Some((key, raw_value)) = parse_kv_line(line) else {
            continue;
        };
        let value = substitute(raw_value, scalars);
        match key {
            "USE" => apply_incremental(&value, &mut config.use_flags),
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            _ => {}
        }
        scalars.insert(key.to_string(), value);
    }
}

fn read_parent_lines(profile_dir: &Path) -> Result<Vec<String>, String> {
    let parent_path = profile_dir.join("parent");
    if !parent_path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&parent_path)
        .map_err(|e| format!("reading {}: {e}", parent_path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}

/// Recursively resolves the profile inheritance chain starting at `leaf`,
/// ancestors before descendants (parents listed in a level's `parent`
/// file are visited in the order given), cycle/diamond-safe via a visited
/// set keyed on the canonicalized directory.
fn resolve_profile_chain(leaf: &Path) -> Result<Vec<PathBuf>, String> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut chain: Vec<PathBuf> = Vec::new();
    visit_profile(leaf, &mut visited, &mut chain)?;
    Ok(chain)
}

fn visit_profile(
    dir: &Path,
    visited: &mut HashSet<PathBuf>,
    chain: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let canon = dir
        .canonicalize()
        .map_err(|e| format!("resolving profile {}: {e}", dir.display()))?;
    if !visited.insert(canon.clone()) {
        return Ok(());
    }
    for parent in read_parent_lines(&canon)? {
        if parent.contains(':') {
            return Err(format!(
                "cross-repo profile parent {parent:?} (referenced from {}) is out of v1 scope",
                canon.display()
            ));
        }
        visit_profile(&canon.join(&parent), visited, chain)?;
    }
    chain.push(canon);
    Ok(())
}

/// Resolves `source <path>` directives against `config_root` as if it
/// were `/` (chroot-style), matching PORTAGE_CONFIGROOT/ROOT semantics
/// elsewhere in this pilot -- an absolute `source /etc/make.local` reads
/// `<config_root>/etc/make.local`, not the real host path. A missing
/// sourced file is silently skipped (lenient default; real bash would
/// error, but no fixture or real usage in this pilot relies on that).
fn process_make_conf_file(
    path: &Path,
    config_root: &Path,
    scalars: &mut HashMap<String, String>,
    config: &mut Config,
    visited_sources: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let canon = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => return Ok(()), // missing file: lenient no-op, see doc comment
    };
    if !visited_sources.insert(canon.clone()) {
        return Ok(());
    }
    let text =
        fs::read_to_string(&canon).map_err(|e| format!("reading {}: {e}", canon.display()))?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("source ") {
            let sourced = rest.trim();
            let sourced_path = Path::new(sourced);
            let resolved = if sourced_path.is_absolute() {
                config_root.join(sourced_path.strip_prefix("/").unwrap_or(sourced_path))
            } else {
                canon
                    .parent()
                    .map(|p| p.join(sourced_path))
                    .unwrap_or_else(|| sourced_path.to_path_buf())
            };
            process_make_conf_file(&resolved, config_root, scalars, config, visited_sources)?;
            continue;
        }
        let Some((key, raw_value)) = parse_kv_line(trimmed) else {
            continue;
        };
        let value = substitute(raw_value, scalars);
        match key {
            "USE" => apply_incremental(&value, &mut config.use_flags),
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            _ => {}
        }
        scalars.insert(key.to_string(), value);
    }
    Ok(())
}

/// Reads every non-comment, non-blank, trimmed line from `path`, which
/// may be a single file or (like `repos.conf` elsewhere in this pilot) a
/// directory of files merged in sorted-filename order. A missing path
/// yields an empty list, not an error.
fn read_config_lines(path: &Path) -> Result<Vec<String>, String> {
    fn read_file_lines(path: &Path) -> Result<Vec<String>, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect())
    }

    let mut lines = Vec::new();
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for entry in entries {
            lines.extend(read_file_lines(&entry)?);
        }
    } else if path.is_file() {
        lines.extend(read_file_lines(path)?);
    }
    Ok(lines)
}

/// Stacks ordered `package.mask`/`.unmask` lines from multiple sources
/// (earlier sources first) with real portage's own `-atom` removal
/// semantics -- see `MaskManager.py`'s `stack_lists(incremental=1)`: a
/// `-atom` line removes the exact matching atom text added by ANY
/// earlier source in this same stack, not just within its own source
/// (e.g. a user-level `-atom` in `package.mask` can remove an atom the
/// repo or a profile level added). Shared between `package.mask` and
/// `package.unmask`, which real portage stacks identically -- unlike
/// this pilot's previous, user-level-only `package.unmask` handling,
/// which treated a leading `-` there as meaningless; it's meaningful
/// once more than one source can contribute an unmask entry.
fn stack_mask_lines(sources: &[Vec<String>]) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for lines in sources {
        for line in lines {
            match line.strip_prefix('-') {
                Some(removed) => list.retain(|x| x != removed),
                None => list.push(line.clone()),
            }
        }
    }
    list
}

/// `package.accept_keywords`: each line is `<atom-or-wildcard>
/// <keyword...>`. A line with no keyword tokens after the atom is a
/// documented no-op for v1 -- real portage gives a bare profile-level
/// entry EAPI/ARCH-dependent implicit meaning (see `KeywordsManager.
/// getPKeywords`'s `accept_keywords_defaults`, which derives an implicit
/// `~arch` set from the *current* global `ACCEPT_KEYWORDS`); a bare
/// user-level entry is already a no-op in real portage too (no
/// `accept_keywords_defaults` substitution happens for
/// `self.pkeywordsdict`), so this v1 simplification only actually
/// changes behavior for the profile-level source, kept deliberately
/// simple and consistent between the two rather than adding a
/// profile-only special case.
fn parse_package_accept_keywords_lines(lines: &[String]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(atom) = parts.next() else {
            continue;
        };
        let keywords: Vec<String> = parts.map(String::from).collect();
        if keywords.is_empty() {
            continue;
        }
        result.push((atom.to_string(), keywords));
    }
    result
}

/// `package.use`: each line is `<atom-or-wildcard> <use-token...>`. A line
/// with no tokens after the atom is a documented no-op, matching
/// `parse_package_accept_keywords_lines`. The USE_EXPAND-prefix shorthand
/// (`VIDEO_CARDS: nvidia`) is out of scope -- see the module doc comment.
/// Purely additive across sources, like `package.accept_keywords` and
/// unlike `package.mask`/`.unmask`: real portage's own `package.use`
/// consumption (`config.py`'s `regenerate` -- see the module doc comment
/// on `USE_ORDER`) only ever `.extend()`s a growing token list per
/// source, never removes a previous entry, so there's no `-atom`
/// semantics to port here at all.
fn parse_package_use_lines(lines: &[String]) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let Some(atom) = parts.next() else {
            continue;
        };
        let tokens: Vec<String> = parts.map(String::from).collect();
        if tokens.is_empty() {
            continue;
        }
        result.push((atom.to_string(), tokens));
    }
    result
}

/// Computes the real USE/ACCEPT_KEYWORDS/visibility `Config` for
/// `config_root`: the profile chain rooted at
/// `<config_root>/etc/portage/make.profile` (if it exists -- a missing
/// profile is not an error, it just contributes nothing), then
/// `<config_root>/etc/portage/make.conf` (if it exists) as the final,
/// highest-priority USE/ACCEPT_KEYWORDS layer, then
/// `package.mask`/`.unmask`/`.accept_keywords`.
///
/// `main_repo_location` (the main repo's own tree root, e.g. what
/// `portage_repo::find_repos` marks `is_main` -- see that crate) is
/// needed for `package.mask`/`.unmask`'s repo-level source,
/// `<main_repo_location>/profiles/package.mask` -- real portage's most
/// common real-world masking source (security/arch masks, etc.), stacked
/// together with every profile level's own `package.mask`/`.unmask` (in
/// chain order) and the user-level `/etc/portage` files, exactly
/// matching real `MaskManager.py`'s three-source stack (see
/// `stack_mask_lines`'s doc comment for the `-atom`-removal semantics).
/// An overlay repo's own repo-level `package.mask`/`.unmask` stays
/// deliberately out of scope, same as the rest of overlays' per-repo
/// config (see `resolve_pretend_graph`'s doc comment on that follow-up)
/// -- only the one main repo's is read here.
pub fn resolve_config(config_root: &Path, main_repo_location: &Path) -> Result<Config, String> {
    let mut config = Config::default();
    let mut scalars: HashMap<String, String> = HashMap::new();

    let make_profile = config_root.join("etc/portage/make.profile");
    let chain: Vec<PathBuf> = if make_profile.exists() {
        resolve_profile_chain(&make_profile)?
    } else {
        Vec::new()
    };
    for level in &chain {
        let make_defaults = level.join("make.defaults");
        if !make_defaults.is_file() {
            continue;
        }
        // Real config.py quirk: USE is excluded from cross-level
        // substitution -- see the module doc comment.
        scalars.remove("USE");
        let text = fs::read_to_string(&make_defaults)
            .map_err(|e| format!("reading {}: {e}", make_defaults.display()))?;
        process_lines(&text, &mut scalars, &mut config);
    }

    let make_conf = config_root.join("etc/portage/make.conf");
    if make_conf.is_file() {
        let mut visited_sources = HashSet::new();
        process_make_conf_file(
            &make_conf,
            config_root,
            &mut scalars,
            &mut config,
            &mut visited_sources,
        )?;
    }

    // use.mask/use.force: every profile level's own file (in chain
    // order), stacked with the same -atom removal semantics
    // package.mask uses (see stack_mask_lines) -- confirmed by reading
    // UseManager.getUseMask/getUseForce's own pkg=None (global) case,
    // which returns stack_lists(self._usemask_list/self._useforce_list,
    // incremental=True) directly, never consulting a repo-level or
    // per-package source at all (those only exist on the *per-package*
    // path, out of scope for this pilot's flat/global USE model, same
    // as package.use's own repo/profile/user-only sourcing already is).
    // Applied last, after every other real accumulation source above,
    // matching config.py's own regenerate(): force-add every useforce
    // flag, THEN force-remove every usemask flag -- so a flag in both
    // ends up masked, not forced, exactly like real portage.
    let mut usemask_sources: Vec<Vec<String>> = Vec::new();
    let mut useforce_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        usemask_sources.push(read_config_lines(&level.join("use.mask"))?);
        useforce_sources.push(read_config_lines(&level.join("use.force"))?);
    }
    config.use_force = stack_mask_lines(&useforce_sources).into_iter().collect();
    config.use_mask = stack_mask_lines(&usemask_sources).into_iter().collect();
    for flag in &config.use_force {
        config.use_flags.insert(flag.clone());
    }
    for flag in &config.use_mask {
        config.use_flags.remove(flag);
    }

    let mut mask_sources: Vec<Vec<String>> = vec![read_config_lines(
        &main_repo_location.join("profiles/package.mask"),
    )?];
    let mut unmask_sources: Vec<Vec<String>> = vec![read_config_lines(
        &main_repo_location.join("profiles/package.unmask"),
    )?];
    for level in &chain {
        mask_sources.push(read_config_lines(&level.join("package.mask"))?);
        unmask_sources.push(read_config_lines(&level.join("package.unmask"))?);
    }
    mask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.mask"),
    )?);
    unmask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.unmask"),
    )?);

    config.package_mask = stack_mask_lines(&mask_sources);
    config.package_unmask = stack_mask_lines(&unmask_sources);

    // package.accept_keywords: profile-chain (in chain order), then
    // user-level -- real KeywordsManager.getPKeywords iterates its own
    // per-profile-level dicts first, then the user-level one, extending
    // the same accumulating "extra accepted keywords" list each time (no
    // "-atom" removal exists for this file at all, unlike package.mask,
    // so a flat concatenation-then-parse is equivalent to parsing each
    // source separately and concatenating the results). No repo-level
    // source exists for this file in real portage at all (unlike
    // package.mask's repo-level profiles/package.mask) -- confirmed by
    // reading KeywordsManager.__init__, which never reads a
    // repo-location path for either package.accept_keywords or its
    // package.keywords alias.
    let mut accept_keywords_lines: Vec<String> = Vec::new();
    for level in &chain {
        accept_keywords_lines.extend(read_config_lines(&level.join("package.accept_keywords"))?);
    }
    accept_keywords_lines.extend(read_config_lines(
        &config_root.join("etc/portage/package.accept_keywords"),
    )?);
    config.package_accept_keywords = parse_package_accept_keywords_lines(&accept_keywords_lines);

    // package.use: repo-level (<main_repo_location>/profiles/package.use),
    // then every profile level's own package.use (in chain order), then
    // user-level -- same file-location convention package.mask and
    // package.accept_keywords both already use (confirmed by reading
    // UseManager.__init__'s _parse_repository_files_to_dict_of_dicts/
    // _parse_profile_files_to_tuple_of_dicts calls), and purely additive
    // like package.accept_keywords (see parse_package_use_lines). This is
    // a deliberate, confirmed-with-the-user simplification, not a full
    // port of real portage's own package.use handling: real repo-level
    // package.use lands in a distinct configdict["repo"] USE_ORDER layer
    // and profile-level in configdict["defaults"] (merged per-level with
    // that level's own make.defaults USE), while this pilot's existing
    // per-package application (see portage-repo's effective_use_flags)
    // already flattens everything into one incremental list regardless
    // of source -- extending that flat model to three sources instead of
    // one doesn't add a new simplification, it just applies the
    // pre-existing one more widely.
    let mut use_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use"))?;
    for level in &chain {
        use_lines.extend(read_config_lines(&level.join("package.use"))?);
    }
    use_lines.extend(read_config_lines(
        &config_root.join("etc/portage/package.use"),
    )?);
    config.package_use = parse_package_use_lines(&use_lines);

    // packages (@system): every profile level's own file, in chain
    // order, stacked with the same -atom removal semantics package.mask
    // uses (see stack_mask_lines) -- confirmed by reading
    // PackagesSystemSet.load, which calls the identical real
    // stack_lists(incremental=1) function. Only *after* stacking are the
    // "*"-prefixed lines kept (with the "*" stripped) as the real
    // @system atom list -- see the module doc comment's `packages`
    // bullet for why every other stacked line is read/stacked but never
    // contributes an atom of its own.
    let mut packages_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        packages_sources.push(read_config_lines(&level.join("packages"))?);
    }
    config.system_packages = stack_mask_lines(&packages_sources)
        .into_iter()
        .filter_map(|line| line.strip_prefix('*').map(String::from))
        .collect();

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .expect("PORTING/fixtures must exist")
    }

    /// End-to-end check against PORTING/fixtures/repo/profiles (base,
    /// arch/amd64 -> multi-parent -> default) + fixtures/etc/portage/make.conf
    /// (which sources fixtures/etc/make.local). Traced by hand:
    ///   base:            USE="foo"; USE="${USE} bar"      -> {foo, bar}
    ///   arch/amd64:      ARCH="amd64"; ACCEPT_KEYWORDS="${ARCH}" -> keywords {amd64}
    ///   default:         USE="-bar baz"                   -> {foo, baz}
    ///   make.local (sourced first from make.conf):
    ///                    USE="${USE} localflag"            -> {foo, baz, localflag}
    ///   make.conf:       USE="confflag"                    -> {foo, baz, localflag, confflag}
    #[test]
    fn resolves_fixture_profile_chain_and_make_conf() {
        let root = fixtures_root();
        let config =
            resolve_config(&root, &root.join("repo")).expect("fixture config must resolve");
        assert_eq!(
            config.use_flags,
            HashSet::from([
                "foo".to_string(),
                "baz".to_string(),
                "localflag".to_string(),
                "confflag".to_string(),
            ])
        );
        assert_eq!(config.accept_keywords, HashSet::from(["amd64".to_string()]));
    }

    #[test]
    fn missing_profile_and_make_conf_yield_empty_config() {
        let empty_root = std::env::temp_dir().join("portage-profile-test-empty-root");
        let _ = fs::create_dir_all(&empty_root);
        let config = resolve_config(&empty_root, &empty_root.join("repo"))
            .expect("missing profile/make.conf is not an error");
        assert_eq!(config.use_flags, HashSet::new());
        assert_eq!(config.accept_keywords, HashSet::new());
    }

    #[test]
    fn cross_repo_profile_parent_is_rejected_with_a_clear_error() {
        let root = std::env::temp_dir().join("portage-profile-test-cross-repo");
        let profile_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("parent"), "gentoo:default/linux/amd64\n").unwrap();
        let make_profile = profile_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let err = resolve_config(&root, &root.join("repo"))
            .expect_err("cross-repo parent must be rejected");
        assert!(err.contains("out of v1 scope"), "unexpected error: {err}");
    }

    #[test]
    fn diamond_profile_inheritance_does_not_double_apply_a_shared_ancestor() {
        // level "top" has two parents ("left", "right") that both inherit
        // from the same "shared" ancestor; "shared" must only contribute
        // its USE flag once, not twice (which -- since it's a set -- isn't
        // observable via *membership*, but the visited-set/cycle-safety
        // mechanism is exactly what also protects a genuine cycle, so this
        // proves that mechanism doesn't accidentally block a legitimate
        // multi-path DAG from resolving at all).
        let root = std::env::temp_dir().join("portage-profile-test-diamond");
        let profile_dir = root.join("etc/portage");
        fs::create_dir_all(&profile_dir).unwrap();
        for name in ["shared", "left", "right", "top"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::write(root.join("shared/make.defaults"), "USE=\"sharedflag\"\n").unwrap();
        fs::write(root.join("left/parent"), "../shared\n").unwrap();
        fs::write(root.join("right/parent"), "../shared\n").unwrap();
        fs::write(root.join("top/parent"), "../left\n../right\n").unwrap();
        fs::write(root.join("top/make.defaults"), "USE=\"${USE} topflag\"\n").unwrap();
        let make_profile = profile_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("top"), &make_profile).unwrap();

        let config =
            resolve_config(&root, &root.join("repo")).expect("diamond inheritance must resolve");
        assert_eq!(
            config.use_flags,
            HashSet::from(["sharedflag".to_string(), "topflag".to_string()])
        );
    }

    #[test]
    fn package_mask_unmask_accept_keywords_load_correctly() {
        let root = std::env::temp_dir().join("portage-profile-test-package-star");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();

        // package.mask: two atoms added, then one removed via "-atom" --
        // only the surviving one should remain.
        fs::write(
            portage_dir.join("package.mask"),
            "dev-libs/foo\ndev-libs/bar\n-dev-libs/bar\n",
        )
        .unwrap();
        // package.unmask: unrelated to the removal above -- a completely
        // separate list, checked per-candidate by the caller.
        fs::write(portage_dir.join("package.unmask"), "dev-libs/baz\n").unwrap();
        // package.accept_keywords: a normal entry, a "**" entry, and a
        // bare-atom (no keywords) line that must be a no-op.
        fs::write(
            portage_dir.join("package.accept_keywords"),
            "dev-qt/* ~amd64\nsci-misc/live-thing **\ndev-libs/bare-no-op\n",
        )
        .unwrap();

        let config = resolve_config(&root, &root.join("repo"))
            .expect("config with package.* files must resolve");
        assert_eq!(config.package_mask, vec!["dev-libs/foo".to_string()]);
        assert_eq!(config.package_unmask, vec!["dev-libs/baz".to_string()]);
        assert_eq!(
            config.package_accept_keywords,
            vec![
                ("dev-qt/*".to_string(), vec!["~amd64".to_string()]),
                ("sci-misc/live-thing".to_string(), vec!["**".to_string()]),
            ]
        );
    }

    #[test]
    fn package_mask_atom_removal_applies_across_repo_profile_and_user_sources() {
        // repo-level: masks a and b.
        // profile-level (the one chain level): "-a" removes the
        // repo-level entry, adds c.
        // user-level: "-c" removes the profile-level entry, adds d.
        // Final: b (repo, survives) + d (user) -- a and c were each
        // removed by a LATER source than the one that added them, which
        // only works if -atom removal spans all three sources, not just
        // within each file on its own.
        let root = std::env::temp_dir().join("portage-profile-test-cross-source-mask");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&repo_profiles).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(
            repo_profiles.join("package.mask"),
            "dev-libs/a\ndev-libs/b\n",
        )
        .unwrap();
        fs::write(leaf.join("package.mask"), "-dev-libs/a\ndev-libs/c\n").unwrap();
        fs::write(
            portage_dir.join("package.mask"),
            "-dev-libs/c\ndev-libs/d\n",
        )
        .unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo).expect("config must resolve");
        assert_eq!(
            config.package_mask,
            vec!["dev-libs/b".to_string(), "dev-libs/d".to_string()]
        );
    }

    #[test]
    fn system_packages_stack_across_profile_levels_with_atom_removal_and_star_filter() {
        // base: adds "*dev-libs/a" (a real @system atom) and a bare
        // "dev-libs/hint" line (no "*" -- a real "known but not system"
        // hint, must never contribute an atom on its own).
        // leaf (its own parent -> base): "-*dev-libs/a" removes base's
        // own system atom (proving -atom removal spans levels, not just
        // within one file), and adds "*dev-libs/b".
        // Final @system list: just "dev-libs/b" -- "a" was added then
        // removed, "hint" was never eligible in the first place.
        let root = std::env::temp_dir().join("portage-profile-test-system-packages");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(base.join("packages"), "*dev-libs/a\ndev-libs/hint\n").unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(leaf.join("packages"), "-*dev-libs/a\n*dev-libs/b\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo).expect("config must resolve");
        assert_eq!(config.system_packages, vec!["dev-libs/b".to_string()]);
    }

    #[test]
    fn use_mask_and_use_force_stack_across_levels_and_mask_wins_over_force() {
        // base: make.defaults enables "normalflag" and "maskflag"
        // normally; use.force forces on "forceflag" and "bothflag".
        // leaf (its own parent -> base): use.mask masks "maskflag" (an
        // otherwise-normal USE flag -- proving use.mask overrides plain
        // USE accumulation, not just use.force) and "bothflag" (proving
        // mask wins when a flag is both forced AND masked, matching real
        // regenerate()'s update-then-difference_update order).
        // Final use_flags: {normalflag, forceflag} -- maskflag and
        // bothflag are both gone despite being enabled/forced upstream.
        let root = std::env::temp_dir().join("portage-profile-test-use-mask-force");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let base = repo_profiles.join("base");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(base.join("make.defaults"), "USE=\"normalflag maskflag\"\n").unwrap();
        fs::write(base.join("use.force"), "forceflag\nbothflag\n").unwrap();
        fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
        fs::write(leaf.join("use.mask"), "maskflag\nbothflag\n").unwrap();

        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo).expect("config must resolve");
        assert_eq!(
            config.use_flags,
            HashSet::from(["normalflag".to_string(), "forceflag".to_string()])
        );
        assert_eq!(
            config.use_force,
            HashSet::from(["forceflag".to_string(), "bothflag".to_string()])
        );
        assert_eq!(
            config.use_mask,
            HashSet::from(["maskflag".to_string(), "bothflag".to_string()])
        );
    }

    #[test]
    fn package_accept_keywords_stacks_profile_chain_then_user_no_repo_source() {
        // Profile-level entry for "a", user-level entry for "b" -- both
        // must appear, in that order (profile-chain first, matching real
        // KeywordsManager.getPKeywords), proving there's no repo-level
        // source at all (only the pilot's existing repo/profiles/package.mask
        // convention would exist if there were one, and this test
        // deliberately never creates that file).
        let root = std::env::temp_dir().join("portage-profile-test-accept-keywords-stack");
        let repo = root.join("repo");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(leaf.join("package.accept_keywords"), "dev-libs/a ~amd64\n").unwrap();
        fs::write(
            portage_dir.join("package.accept_keywords"),
            "dev-libs/b ~amd64\ndev-libs/bare-no-op\n",
        )
        .unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo).expect("config must resolve");
        assert_eq!(
            config.package_accept_keywords,
            vec![
                ("dev-libs/a".to_string(), vec!["~amd64".to_string()]),
                ("dev-libs/b".to_string(), vec!["~amd64".to_string()]),
            ]
        );
    }

    #[test]
    fn package_use_stacks_repo_then_profile_chain_then_user() {
        // Repo-level entry for "a", profile-level entry for "b",
        // user-level entry for "c" -- all three must appear, in that
        // order, proving no repo/profile-level source is silently
        // dropped and no `-atom` removal happens anywhere (package.use
        // is purely additive, unlike package.mask).
        let root = std::env::temp_dir().join("portage-profile-test-package-use-stack");
        let repo = root.join("repo");
        let repo_profiles = repo.join("profiles");
        let portage_dir = root.join("etc/portage");
        let leaf = root.join("leaf-profile");
        fs::create_dir_all(&repo_profiles).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::create_dir_all(&leaf).unwrap();

        fs::write(repo_profiles.join("package.use"), "dev-libs/a flaga\n").unwrap();
        fs::write(leaf.join("package.use"), "dev-libs/b flagb\n").unwrap();
        fs::write(portage_dir.join("package.use"), "dev-libs/c flagc\n").unwrap();

        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

        let config = resolve_config(&root, &repo).expect("config must resolve");
        assert_eq!(
            config.package_use,
            vec![
                ("dev-libs/a".to_string(), vec!["flaga".to_string()]),
                ("dev-libs/b".to_string(), vec!["flagb".to_string()]),
                ("dev-libs/c".to_string(), vec!["flagc".to_string()]),
            ]
        );
    }

    #[test]
    fn package_use_loads_tokens_and_skips_bare_atom_lines() {
        let root = std::env::temp_dir().join("portage-profile-test-package-use");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&portage_dir).unwrap();
        fs::write(
            portage_dir.join("package.use"),
            "dev-libs/foo flag1 -flag2\n*/bar +flag3\ndev-libs/bare-no-op\n",
        )
        .unwrap();

        let config = resolve_config(&root, &root.join("repo"))
            .expect("config with package.use must resolve");
        assert_eq!(
            config.package_use,
            vec![
                (
                    "dev-libs/foo".to_string(),
                    vec!["flag1".to_string(), "-flag2".to_string()]
                ),
                ("*/bar".to_string(), vec!["+flag3".to_string()]),
            ]
        );
    }

    #[test]
    fn apply_incremental_is_reusable_for_per_package_use_overrides() {
        let mut set = HashSet::from(["foo".to_string()]);
        apply_incremental("flag1 -foo +flag2", &mut set);
        assert_eq!(
            set,
            HashSet::from(["flag1".to_string(), "flag2".to_string()])
        );
    }
}
