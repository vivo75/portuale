// Profile-chain + make.conf resolution for real USE/ACCEPT_KEYWORDS (see
// PORTING/PROMPT.md's depgraph/config-resolution follow-up work, and
// PORTING/README.md for the full scope writeup). Replaces the base
// `emerge --pretend` slice's hardcoded `ACCEPT_KEYWORDS="amd64"`/`USE=""`
// with the real mechanism: a profile inheritance chain (`make.profile` ->
// `parent` files) plus `/etc/portage/make.conf`, each level's
// `make.defaults` contributing incremental USE/ACCEPT_KEYWORDS tokens.
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
//     expansion, package.use/.mask/.accept_keywords, use.mask/.force, the
//     `packages` file, and ARCH-based KEYWORDS-format validation are all
//     out of scope.
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
/// stripping it, which this mirrors).
fn apply_incremental(tokens: &str, set: &mut HashSet<String>) {
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

/// Computes the real USE/ACCEPT_KEYWORDS `Config` for `config_root`: the
/// profile chain rooted at `<config_root>/etc/portage/make.profile` (if
/// it exists -- a missing profile is not an error, it just contributes
/// nothing), then `<config_root>/etc/portage/make.conf` (if it exists) as
/// the final, highest-priority layer.
pub fn resolve_config(config_root: &Path) -> Result<Config, String> {
    let mut config = Config::default();
    let mut scalars: HashMap<String, String> = HashMap::new();

    let make_profile = config_root.join("etc/portage/make.profile");
    if make_profile.exists() {
        for level in resolve_profile_chain(&make_profile)? {
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
        let config = resolve_config(&fixtures_root()).expect("fixture config must resolve");
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
        let config =
            resolve_config(&empty_root).expect("missing profile/make.conf is not an error");
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

        let err = resolve_config(&root).expect_err("cross-repo parent must be rejected");
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

        let config = resolve_config(&root).expect("diamond inheritance must resolve");
        assert_eq!(
            config.use_flags,
            HashSet::from(["sharedflag".to_string(), "topflag".to_string()])
        );
    }
}
