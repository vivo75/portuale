// Repo/metadata/vdb access for the single-atom `emerge --pretend` pilot
// slice (see PORTING/PROMPT.md's depgraph/config-resolution follow-up
// work, and PORTING/README.md for the full scope writeup).
//
// Overlays: every `[reponame]` section in `repos.conf` with a `location`
// (not just `[DEFAULT] main-repo`) is now a candidate source -- see
// `find_repos`. Candidates for a given category/package are gathered from
// ALL configured repos (mirroring real `portdbapi.cp_list`, which does
// the same: an overlay isn't "consulted only if the main repo has
// nothing," every repo's ebuilds are real candidates), sorted ascending
// by `(priority, name)` exactly like real portage's own `prepos_order`,
// so a tie between two repos providing the identical version is broken
// in favor of the higher-priority one (see `resolve_pretend`'s final
// `max_by`). A repo's `priority` is its explicit `repos.conf` value if
// present, else `-1000` for the main repo (real portage's own default --
// see `lib/portage/repository/config.py`) or `0` for anything else.
//
// KNOWN, DOCUMENTED SCOPE CUTS for v1 (all confirmed with the user before
// implementing):
//   - No per-repo `package.mask`/`.unmask`/`profiles/`, no `masters`
//     (eclass inheritance across repos), no `::repo`-constrained atoms
//     (out of `portage-dep`'s v1 grammar already) -- overlays only widen
//     *which ebuilds are candidates*, nothing about how they're
//     evaluated once found.
//   - Ebuild metadata comes from `metadata/md5-cache/<cat>/<pf>` (plain
//     `KEY=value` text -- confirmed against a real vendored tree), never
//     from executing the ebuild in bash. This is deliberate: it lets
//     `--pretend` work without the bash dependency that real phase
//     execution will eventually require (see PROMPT.md's "Deferred:
//     ebuild phase execution").
//   - No virtuals, no backtracking.
//
// USE/ACCEPT_KEYWORDS/package.mask/.unmask/.accept_keywords/.use are no
// longer hardcoded: they're computed by `portage_profile::resolve_config`
// (real profile chain + make.conf + package.*, with its own documented
// scope cuts -- see that crate's doc comment) and threaded through
// `resolve_pretend`/`resolve_pretend_graph` as a `&portage_profile::Config`.
// `package.use` in particular is applied per package, not globally: see
// `effective_use_flags` and its use in `resolve_pretend_graph` below --
// this is the one place `Config`'s fields aren't just handed to
// `use_reduce_flat`/`is_visible` unchanged, since matching a `package.use`
// entry against a specific candidate (to support slotted/versioned
// entries, not just bare atoms) needs that candidate's resolved SLOT,
// which only exists at this repo-aware layer.
//
// Dependency recursion (see `resolve_pretend_graph` below) walks DEPEND
// and RDEPEND only, with its own documented scope cuts -- see that
// function's doc comment.
//
// Config/target roots are read from the real `PORTAGE_CONFIGROOT` and
// `ROOT` environment variables (portage's own mechanism for relocating
// `/etc/portage` and the install target -- see lib/portage/const.py),
// defaulting to `/` when unset. This is not a pilot-only convention: it's
// what lets tests point at a fixture tree without needing anything
// pilot-specific.

use portage_versions::vercmp;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

pub fn config_root_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_CONFIGROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn root_from_env() -> PathBuf {
    std::env::var_os("ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub location: PathBuf,
    pub priority: i32,
    /// Whether this is `repos.conf`'s `[DEFAULT] main-repo` -- needed by
    /// callers that read repo-level config from the main repo specifically
    /// (e.g. `portage_profile::resolve_config`'s repo-level
    /// `profiles/package.mask`), since an overlay's own repo-level config
    /// is deliberately out of scope (see `resolve_pretend_graph`'s doc
    /// comment on the overlays follow-up).
    pub is_main: bool,
}

fn parse_ini(text: &str, sections: &mut HashMap<String, HashMap<String, String>>) {
    let mut current: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let name = inner.trim().to_string();
            sections.entry(name.clone()).or_default();
            current = Some(name);
            continue;
        }
        if let Some(section_name) = &current {
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_string();
                let value = line[eq + 1..].trim().to_string();
                sections
                    .get_mut(section_name)
                    .expect("section was inserted when its header was parsed")
                    .insert(key, value);
            }
        }
    }
}

/// Parses `repos.conf` (a file, or -- as on this pilot's dev machine -- a
/// directory of `*.conf` files merged in sorted-filename order) and
/// returns every `[reponame]` section that has a `location` (the main
/// repo plus any overlays), sorted ascending by `(priority, name)` --
/// matching real portage's own `prepos_order` (see
/// `lib/portage/repository/config.py`), which is also the order
/// `list_candidates` below iterates them in, so a tie between two repos
/// providing the identical version is broken toward the higher-priority
/// one.
pub fn find_repos(config_root: &Path) -> Result<Vec<RepoConfig>, String> {
    let repos_conf_path = config_root.join("etc/portage/repos.conf");
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();

    if repos_conf_path.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&repos_conf_path)
            .map_err(|e| format!("reading {}: {e}", repos_conf_path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for path in entries {
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            parse_ini(&text, &mut sections);
        }
    } else if repos_conf_path.is_file() {
        let text = fs::read_to_string(&repos_conf_path)
            .map_err(|e| format!("reading {}: {e}", repos_conf_path.display()))?;
        parse_ini(&text, &mut sections);
    } else {
        return Err(format!(
            "no repos.conf found at {}",
            repos_conf_path.display()
        ));
    }

    let main_repo = sections
        .get("DEFAULT")
        .and_then(|d| d.get("main-repo"))
        .ok_or("no [DEFAULT] main-repo in repos.conf")?
        .clone();

    let mut repos: Vec<RepoConfig> = Vec::new();
    for (name, kv) in &sections {
        if name == "DEFAULT" {
            continue;
        }
        let Some(location) = kv.get("location") else {
            continue;
        };
        let location = PathBuf::from(location);
        // Real repos.conf always uses absolute locations; relative ones
        // are a pilot/testing convenience so the fixture tree under
        // PORTING/fixtures can be committed without a machine-specific
        // absolute path baked in.
        let location = if location.is_absolute() {
            location
        } else {
            config_root.join(location)
        };
        // An explicit "priority" wins; otherwise the main repo defaults
        // to -1000 (real portage's own default -- see
        // lib/portage/repository/config.py) and every other repo to 0.
        let priority = kv
            .get("priority")
            .and_then(|p| p.parse::<i32>().ok())
            .unwrap_or(if *name == main_repo { -1000 } else { 0 });
        repos.push(RepoConfig {
            name: name.clone(),
            location,
            priority,
            is_main: *name == main_repo,
        });
    }

    if !repos.iter().any(|r| r.name == main_repo) {
        return Err(format!("no location for repo {main_repo:?} in repos.conf"));
    }

    repos.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(repos)
}

/// Reads `metadata/md5-cache/<category>/<pf>` (`pf` = "package-version",
/// e.g. "foo-1.2.3-r1") as a plain `KEY=value` map.
pub fn read_md5_cache(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> Result<HashMap<String, String>, String> {
    let path = repo_location
        .join("metadata")
        .join("md5-cache")
        .join(category)
        .join(pf);
    let text = fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut map = HashMap::new();
    for line in text.lines() {
        if let Some(eq) = line.find('=') {
            map.insert(line[..eq].to_string(), line[eq + 1..].to_string());
        }
    }
    Ok(map)
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub version: String,
    pub keywords: Vec<String>,
    pub slot: String,
    /// The sub-slot half of this candidate's own `SLOT` metadata (real
    /// `SLOT="main/sub"`), read via `split_slot` alongside `slot` --
    /// defaults to `slot` itself when the ebuild's own `SLOT` carries no
    /// `/` at all, matching real `_pkg_str`'s own slot-parsing fallback.
    /// Folded into every candidate string this crate builds for
    /// `portage_dep::match_from_list` as a `slot/sub_slot` suffix (see
    /// `portage_dep::Candidate`'s own regex, which already parses this
    /// shape) -- closes a real, previously-silent gap: any dependency
    /// atom restricted on sub-slot (`dev-libs/foo:0/2`, PMS 8.3.3) could
    /// never actually match anything here before, since every candidate
    /// string omitted the sub-slot half entirely, no matter what a
    /// candidate's real `SLOT` metadata said.
    pub sub_slot: String,
    /// Which repo this candidate's ebuild/metadata actually lives in --
    /// needed once there's more than one (see `list_candidates`), both to
    /// re-read this exact package's own DEPEND/RDEPEND later
    /// (`resolve_pretend_graph`) and to break a same-version tie between
    /// two repos toward the higher-priority one (`resolve_pretend`).
    pub repo_location: PathBuf,
    pub repo_priority: i32,
    /// The repo's own `repos.conf` section name (`RepoConfig::name`),
    /// e.g. "gentoo" -- appended as a `::name` suffix to every candidate
    /// string this crate builds for `portage_dep::match_from_list`
    /// (except the two paths noted in the module doc comment's
    /// `::reponame` bullet), so a `::repo`-constrained atom actually
    /// filters. Real portage cross-checks this against each repo's own
    /// `profiles/repo_name` file too; this pilot reuses the already-read
    /// `repos.conf` name as-is rather than reading a second file.
    pub repo_name: String,
    /// The raw `LICENSE` metadata string (PMS 7.3.2), unreduced -- read
    /// alongside `keywords`/`slot` at zero extra I/O cost (the same
    /// md5-cache metadata dict `list_candidates` already reads for
    /// them). Empty string for no `LICENSE` at all, matching real
    /// `use_reduce("")`'s own empty result -- see `is_visible`'s own
    /// license-masking check.
    pub license: String,
    /// The raw `IUSE` metadata string, same "already have it, zero
    /// extra I/O" reasoning as `license` -- only ever consulted when
    /// `license` actually contains a `?` USE-conditional, to resolve
    /// this specific candidate's own effective USE (real `use_reduce`'s
    /// own "if '?' in license_str" optimization, ported as-is: most
    /// packages' LICENSE has no conditional at all, so most candidates
    /// never need this field for anything).
    pub iuse: String,
    /// The raw `PROPERTIES` metadata string, same "already have it,
    /// zero extra I/O" reasoning as `license`/`iuse` -- see
    /// `properties_accepted`'s own doc comment.
    pub properties: String,
    /// The raw `RESTRICT` metadata string. See `properties`'s own doc
    /// comment.
    pub restrict: String,
}

/// A directory entry's name is only accepted as `<package>-<version>` if
/// what follows the "<package>-" prefix looks like a version (starts with
/// a digit) -- otherwise a package like "foo" would wrongly absorb a
/// sibling package's directory like "foo-bar-2.0" as if it were "foo"
/// version "bar-2.0". Same disambiguation principle as the PMS package-name
/// grammar ported in portage-dep's atom parser.
fn strip_version_prefix<'a>(dir_name: &'a str, package: &str) -> Option<&'a str> {
    let rest = dir_name.strip_prefix(package)?.strip_prefix('-')?;
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

/// Lists every version of `category/package` that has an ebuild in ANY of
/// `repos`, with metadata (KEYWORDS, SLOT) from each repo's own md5-cache
/// -- mirroring real `portdbapi.cp_list`, which gathers candidates from
/// every configured repo the same way, not just the first one that has
/// the package. `repos` is iterated in the order given (see
/// `find_repos`'s ascending `(priority, name)` sort), and each resulting
/// `Candidate` remembers which repo it came from. A candidate whose cache
/// entry is missing or unreadable is silently skipped (v1 doesn't
/// distinguish "stale cache" from "doesn't exist" -- both just mean "not
/// visible"); a repo with no directory at all for this category/package
/// simply contributes nothing, same as before.
pub fn list_candidates(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
) -> Result<Vec<Candidate>, String> {
    let mut candidates = Vec::new();
    for repo in repos {
        let pkg_dir = repo.location.join(category).join(package);
        if !pkg_dir.is_dir() {
            continue;
        }
        let entries =
            fs::read_dir(&pkg_dir).map_err(|e| format!("reading {}: {e}", pkg_dir.display()))?;

        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let Some(stem) = file_name.strip_suffix(".ebuild") else {
                continue;
            };
            let Some(version) = strip_version_prefix(stem, package) else {
                continue;
            };
            let Ok(metadata) = read_md5_cache(&repo.location, category, stem) else {
                continue;
            };
            let keywords = metadata
                .get("KEYWORDS")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
            let (slot, sub_slot) =
                split_slot(metadata.get("SLOT").map(String::as_str).unwrap_or(""));
            candidates.push(Candidate {
                version: version.to_string(),
                keywords,
                slot,
                sub_slot,
                repo_location: repo.location.clone(),
                repo_priority: repo.priority,
                repo_name: repo.name.clone(),
                license: metadata.get("LICENSE").cloned().unwrap_or_default(),
                iuse: metadata.get("IUSE").cloned().unwrap_or_default(),
                properties: metadata.get("PROPERTIES").cloned().unwrap_or_default(),
                restrict: metadata.get("RESTRICT").cloned().unwrap_or_default(),
            });
        }
    }
    Ok(candidates)
}

/// Whether `entry` (a `package.mask`/`.unmask`/`.accept_keywords` line)
/// matches this specific candidate. Two-tier: try the real, already-
/// verified `portage_dep::match_from_list` first (covers the vast
/// majority of real entries: versioned, slotted, bare atoms), and only
/// fall back to portage-dep's separate, bounded wildcard-atom matcher
/// (`*/*`, `category/*`, `*/package`) if that fails to parse `entry` at
/// all.
fn matches_config_entry(entry: &str, candidate_str: &str, category: &str, package: &str) -> bool {
    if let Some(matches) = portage_dep::match_from_list(entry, &[candidate_str]) {
        return !matches.is_empty();
    }
    match portage_dep::parse_wildcard_atom(entry) {
        Some(w) => portage_dep::wildcard_atom_matches(&w, category, package),
        None => false,
    }
}

/// The USE flags in effect for one specific package: `iuse`'s own
/// `+flag`/`-flag` default markers (real `pkginternal`, see below) seeded
/// first, then `use_tokens` (`portage_profile::Config::use_tokens`, the
/// *ordered raw* `USE=` value strings from every profile level's own
/// `make.defaults` plus `make.conf`, replayed via `apply_incremental`
/// directly -- not a pre-flattened set unioned on top, see the
/// `iuse`'s own defaults paragraph below for why that distinction
/// matters) with every matching `package.use` entry's tokens layered on
/// top after that, in file order, via the same incremental
/// `-flag`/`flag`/`+flag` semantics `USE` itself uses (see
/// `portage_profile::apply_incremental`). Unlike `is_visible`'s
/// mask/keywords checks (which only ever add to an accepted set), this
/// can both add and remove flags, and does so per package -- a
/// `package.use` entry never affects any other package's own
/// resolution.
///
/// **`iuse`'s own defaults**: found and grounded by comparing this
/// pilot's own output against the real, installed system `emerge` on a
/// real package (`media-video/ffmpeg`) -- REQUIRED_USE reported violated
/// for a USE combination that's actually fully satisfied once IUSE's own
/// `+`/`-` markers are honored (`ffmpeg`'s own real IUSE declares
/// `+gpl`/`+dav1d`/`+drm`/etc., none of which this pilot's prior
/// `effective_use_flags` ever enabled, silently defaulting every one of
/// them to disabled instead). Real `config.py`'s own `_setup_pkg_iuse`
/// (`lib/portage/package/ebuild/config.py`, ~line 1878) builds exactly
/// this from a package's raw `IUSE` string -- `+flag` contributes a bare
/// `flag` (enable) token, `-flag` contributes itself unchanged (disable),
/// a markerless `flag` contributes nothing at all -- and stores it under
/// `self.configdict["pkginternal"]["USE"]`, a real, named `USE_ORDER`
/// component (`lib/_emerge/actions.py`'s own default,
/// `"env:pkg:conf:defaults:pkginternal:features:repo:env.d"`) --
/// confirmed by reading `config.py`'s own `self.uvlist` construction
/// (`for x in self["USE_ORDER"].split(":"): ...; self.uvlist.reverse()`):
/// incremental application walks `uvlist` in *reversed* `USE_ORDER`, so
/// `pkginternal` (position 5 of 8) is applied well *before* `defaults`
/// (profile), `conf` (`make.conf`), and `pkg` (`package.use`) -- real
/// portage's own actual precedence has every one of those three able to
/// override an IUSE default; only `env`/`env.d` (real per-invocation/
/// stacked-profile-env overrides, positions 8 and 1) sit even lower/
/// higher than this pilot models at all. Ported here as the seed
/// `use_flags` starts from, with `use_tokens` (`defaults`/`conf`)
/// replayed directly on top via `apply_incremental` -- **not** a plain
/// set union of the already-flattened `use_flags`. An earlier version of
/// this pilot *did* union a flattened `base` here, which meant `base`
/// could only ever *add* a flag, never explicitly cancel an IUSE
/// `+default` the way real `defaults`/`conf` genuinely can (real
/// `regenerate()` runs one continuous incremental walk across the whole
/// reversed `uvlist` -- `pkginternal` then `defaults` then `conf` then
/// `pkg` -- so a `-flag` token in `defaults`/`conf` really does cancel
/// an earlier `pkginternal` `+flag`, exactly like any other incremental
/// variable). Replaying the *ordered raw tokens* instead of the
/// flattened set closes that gap: `portage_profile::resolve_config`
/// exposes both `use_flags` (the flattened result, still used elsewhere
/// for e.g. `--newuse` comparisons) and `use_tokens` (the ordered raw
/// values that produced it) -- see `Config::use_tokens`'s own doc
/// comment for the full grounding. The dominant real-world case -- an
/// ebuild author sets a sensible IUSE default, and nothing else ever
/// mentions the flag at all -- was already correct either way; this
/// closes the narrower case where a profile or `make.conf` genuinely
/// does mention it.
///
/// `use_stable_force`/`use_stable_mask`/`package_use_stable_force`/
/// `package_use_stable_mask` (`keywords`/`candidate_str` decide, via
/// `is_stable`, whether this candidate even counts as "stable") are the
/// `.stable.` variants of the global/per-package force/mask sources
/// already applied above -- ported from real `getUseMask`/`getUseForce`'s
/// own per-package (`pkg is not None`) branch, which appends the stable
/// variant right alongside the ordinary one at each accumulation step,
/// but *only* when `stable` -- see this module's own `is_stable` doc
/// comment for the real "would masking every keyword make this
/// invisible" definition. Grouped here as force-then-mask, ordinary
/// then stable, within each tier (not real portage's own
/// per-profile-level interleaving, which this pilot's own flat global
/// `use_force`/`use_mask` accumulation already doesn't replicate either
/// -- see `portage-profile`'s own `package.use.mask`/`.force` doc
/// comment for that established, confirmed simplification, extended
/// here rather than re-litigated).
#[allow(clippy::too_many_arguments)]
fn effective_use_flags(
    iuse: &str,
    use_tokens: &[String],
    package_use: &[(String, Vec<String>)],
    package_use_force: &[(String, Vec<String>)],
    package_use_mask: &[(String, Vec<String>)],
    use_force: &HashSet<String>,
    use_mask: &HashSet<String>,
    use_stable_force: &HashSet<String>,
    use_stable_mask: &HashSet<String>,
    package_use_stable_force: &[(String, Vec<String>)],
    package_use_stable_mask: &[(String, Vec<String>)],
    keywords: &[String],
    accept_keywords: &HashSet<String>,
    package_accept_keywords: &[(String, Vec<String>)],
    candidate_str: &str,
    category: &str,
    package: &str,
) -> HashSet<String> {
    // real pkginternal: only a token with an explicit "+"/"-" marker
    // contributes anything at all -- a markerless IUSE token (no
    // declared default) is a real, deliberate no-op here, matching real
    // config.py's own `if x.startswith("+"): ... elif x.startswith("-"):
    // ...` (no `else` branch at all).
    let iuse_defaults: String = iuse
        .split_whitespace()
        .filter(|tok| tok.starts_with('+') || tok.starts_with('-'))
        .collect::<Vec<_>>()
        .join(" ");
    let mut use_flags: HashSet<String> = HashSet::new();
    portage_profile::apply_incremental(&iuse_defaults, &mut use_flags);
    for token in use_tokens {
        portage_profile::apply_incremental(token, &mut use_flags);
    }
    for (entry, tokens) in package_use {
        if matches_config_entry(entry, candidate_str, category, package) {
            portage_profile::apply_incremental(&tokens.join(" "), &mut use_flags);
        }
    }

    let stable = is_stable(
        keywords,
        candidate_str,
        category,
        package,
        accept_keywords,
        package_accept_keywords,
    );

    // use.mask/use.force (global) and package.use.mask/.force (atom-
    // scoped), layered on top of package.use, force winning first then
    // mask -- see specificity_ordered_flags's own doc comment for how a
    // conflict between multiple matching package.use.mask/.force entries
    // is resolved, and the module doc comment's own `package.use.mask`/
    // `.force` bullet for the full scope writeup. use.stable.force/
    // package.use.stable.force (when stable) join the force tier;
    // use.stable.mask/package.use.stable.mask (when stable) join the
    // mask tier -- see this function's own doc comment. `use_force`/
    // `use_mask` (global) are applied at this exact position -- not
    // folded into `base` early the way an earlier version of this pilot
    // did -- matching real `regenerate()`'s own `self.useforce`/
    // `self.usemask` (which `setcpv()` sets to the *per-package*
    // `getUseForce(pkg)`/`getUseMask(pkg)`, i.e. global force/mask
    // combined with the atom-scoped variant) applied as the literal last
    // step of its incremental USE walk, strictly after `package.use` --
    // see `portage_profile::Config::use_force`'s own doc comment for the
    // full grounding.
    for flag in use_force {
        use_flags.insert(flag.clone());
    }
    for flag in specificity_ordered_flags(
        package_use_force,
        candidate_str,
        category,
        package,
        HashSet::new(),
    ) {
        use_flags.insert(flag);
    }
    if stable {
        for flag in use_stable_force {
            use_flags.insert(flag.clone());
        }
        for flag in specificity_ordered_flags(
            package_use_stable_force,
            candidate_str,
            category,
            package,
            HashSet::new(),
        ) {
            use_flags.insert(flag);
        }
    }
    for flag in use_mask {
        use_flags.remove(flag);
    }
    for flag in specificity_ordered_flags(
        package_use_mask,
        candidate_str,
        category,
        package,
        HashSet::new(),
    ) {
        use_flags.remove(&flag);
    }
    if stable {
        for flag in use_stable_mask {
            use_flags.remove(flag);
        }
        for flag in specificity_ordered_flags(
            package_use_stable_mask,
            candidate_str,
            category,
            package,
            HashSet::new(),
        ) {
            use_flags.remove(&flag);
        }
    }
    use_flags
}

/// Computes the final per-candidate flag set from `entries` (raw
/// `package.use.mask`/`.force`/`package.accept_keywords` `(atom,
/// tokens)` pairs): filters to entries whose atom actually matches
/// `candidate_str`, orders the matches from least to most specific (see
/// `atom_specificity`, a simplified port of real `best_match_to_list`'s
/// own ranking table, used by `ordered_by_atom_specificity`), then
/// applies each one's own tokens via the same incremental
/// `-flag`/`flag`/`+flag` semantics `package.use` itself uses
/// (`apply_incremental`), onto `seed` -- so a more-specific atom's own
/// `-flag` can cancel a less-specific atom's own mask/force (or, for
/// `keywords_accepted`'s own use below, even a keyword `seed` itself
/// already contains), exactly mirroring real portage's own
/// `stack_lists(incremental=True)` applied to the specificity-ordered
/// entry list. `seed` is empty for every `package.use.mask`/`.force`
/// caller (real `MaskManager`'s own equivalent stack has no comparable
/// "start from something already accepted" step) -- `keywords_accepted`
/// is the one caller that seeds it with something real, mirroring real
/// `KeywordsManager.getMissingKeywords`'s own `pgroups = global_accept_
/// keywords.split(); pgroups.extend(unmaskgroups)` (seed first, then
/// fold in package-specific contributions) exactly.
fn specificity_ordered_flags(
    entries: &[(String, Vec<String>)],
    candidate_str: &str,
    category: &str,
    package: &str,
    mut seed: HashSet<String>,
) -> HashSet<String> {
    let mut matching: Vec<&(String, Vec<String>)> = entries
        .iter()
        .filter(|(entry, _)| matches_config_entry(entry, candidate_str, category, package))
        .collect();
    // Stable sort: ties (including every comparison-operator atom, which
    // this pilot deliberately doesn't further distinguish -- see the
    // module doc comment) keep their original file/stacking order.
    matching.sort_by_key(|(entry, _)| atom_specificity(entry));
    for (_, tokens) in matching {
        portage_profile::apply_incremental(&tokens.join(" "), &mut seed);
    }
    seed
}

/// Simplified port of real `best_match_to_list`'s own specificity
/// ranking table: versioned/slotted bare atoms and this pilot's own
/// bounded wildcard atoms only, matching `portage-dep`'s v1 grammar
/// scope. A bounded wildcard atom (`*/*`, `category/*`, `*/package`)
/// always ranks below every real atom, at `-2` -- real portage's own
/// code has three separate extended-syntax tiers (`0` for a wildcard
/// combined with `=*`, `-1` for one combined with a slot, `-2` for a
/// bare one with neither), and this pilot's own wildcard grammar has no
/// slot or glob concept at all, so every wildcard entry always falls
/// into that third, lowest tier.
fn atom_specificity(entry: &str) -> i32 {
    let Some(atom) = portage_dep::parse_atom(entry) else {
        return -2;
    };
    let op_val = match atom.operator {
        portage_dep::Operator::Eq => 6,
        portage_dep::Operator::Tilde => 5,
        portage_dep::Operator::EqGlob => 4,
        portage_dep::Operator::Gt
        | portage_dep::Operator::Ge
        | portage_dep::Operator::Lt
        | portage_dep::Operator::Le => 2,
        portage_dep::Operator::None => 1,
    };
    let slot_val = if atom.slot.is_some() { 3 } else { i32::MIN };
    op_val.max(slot_val)
}

/// LICENSE's own PMS 7.3.2 grammar structure -- plain license tokens,
/// `||` any-of groups, and (once a `flag?` USE-conditional has already
/// been resolved) the *bundle* a conditional or plain sub-group
/// contributes when it sits directly inside a `||` group's own
/// alternative list (`AllOf`) -- verified directly against real
/// `portage.dep.use_reduce(..., opconvert=True)`: a `||` group's own
/// members are flat (`['||', 'MIT', 'BSD']`, not double-nested), but a
/// *plain* sub-group (or a conditional's own resolved contents) sitting
/// directly inside that same `||`'s member list stays a genuine nested
/// unit (`['||', ['GPL-2', 'MIT'], 'BSD']` for `|| ( ( GPL-2 MIT ) BSD
/// )` -- "GPL-2 AND MIT" is one whole alternative, not two independent
/// ones) -- while the identical sub-group anywhere *else* (top level, or
/// inside another plain/conditional group) flattens directly into its
/// parent instead (opconvert's own "AND of AND is just AND" collapse).
/// This distinction is why `use_reduce_flat` (which discards *all* group
/// boundaries, a deliberate, already-documented simplification real
/// DEPEND/RDEPEND recursion in this pilot relies on) can't be reused for
/// LICENSE at all -- the same reasoning that already made
/// `portage_required_use` its own separate algorithm rather than a mode
/// of this same function.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LicenseNode {
    License(String),
    AnyOf(Vec<LicenseNode>),
    AllOf(Vec<LicenseNode>),
}

/// What opened the bracket currently being collected -- mirrors
/// `use_reduce_flat`'s own `need_bracket`/conditional-pop dance, just
/// carried explicitly here since this parser's own stack holds
/// `LicenseNode`s (which can't carry a pending "flag?" or "||" marker
/// the way a flat `Vec<String>` stack can).
enum PendingBracket {
    None,
    Conditional(String),
    AnyOf,
}

/// Parses `tokens` (a `LICENSE` string, pre-split on whitespace) into
/// its own real `||`/USE-conditional tree structure -- see
/// `LicenseNode`'s own doc comment for the exact real-structure ground
/// truth this mirrors, and why. Bracket/`need_bracket` handling is
/// otherwise a direct structural port of `use_reduce_flat`'s own
/// (`portage-use-reduce`), just building a tree instead of flattening.
fn parse_license_tree(
    tokens: &[String],
    use_flags: &HashSet<String>,
) -> Result<Vec<LicenseNode>, String> {
    let mut stack: Vec<Vec<LicenseNode>> = vec![Vec::new()];
    let mut bracket_stack: Vec<PendingBracket> = Vec::new();
    let mut pending = PendingBracket::None;
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(format!(
                        "expected: dependency string, got: ')', token {}",
                        pos + 2
                    ));
                }
                need_bracket = false;
                stack.push(Vec::new());
                bracket_stack.push(std::mem::replace(&mut pending, PendingBracket::None));
            }
            ")" => {
                if need_bracket {
                    return Err(format!("expected: '(', got: ')', token {}", pos + 1));
                }
                if stack.len() <= 1 {
                    return Err(format!("no matching '(' for ')', token {}", pos + 1));
                }
                let collected = stack.pop().unwrap();
                let opened_by = bracket_stack.pop().unwrap();
                // Whether the group we just closed sits directly inside
                // a `||`'s own alternative list -- decides flatten
                // (`extend`) vs. nest (`AllOf`) for a plain/conditional
                // group; see `LicenseNode`'s own doc comment.
                let parent_is_any_of = matches!(bracket_stack.last(), Some(PendingBracket::AnyOf));
                match opened_by {
                    PendingBracket::AnyOf => {
                        stack
                            .last_mut()
                            .unwrap()
                            .push(LicenseNode::AnyOf(collected));
                    }
                    PendingBracket::Conditional(cond) => {
                        if portage_use_reduce::is_active(
                            &cond,
                            use_flags,
                            portage_use_reduce::MatchMode::Normal,
                        )? {
                            if parent_is_any_of {
                                stack
                                    .last_mut()
                                    .unwrap()
                                    .push(LicenseNode::AllOf(collected));
                            } else {
                                stack.last_mut().unwrap().extend(collected);
                            }
                        }
                    }
                    PendingBracket::None => {
                        if parent_is_any_of {
                            stack
                                .last_mut()
                                .unwrap()
                                .push(LicenseNode::AllOf(collected));
                        } else {
                            stack.last_mut().unwrap().extend(collected);
                        }
                    }
                }
            }
            "||" => {
                if need_bracket {
                    return Err(format!("expected: '(', got: '||', token {}", pos + 1));
                }
                need_bracket = true;
                pending = PendingBracket::AnyOf;
            }
            _ => {
                if need_bracket {
                    return Err(format!("expected: '(', got: '{token}', token {}", pos + 1));
                }
                if token.ends_with('?') {
                    need_bracket = true;
                    pending = PendingBracket::Conditional(token.clone());
                } else {
                    stack
                        .last_mut()
                        .unwrap()
                        .push(LicenseNode::License(token.clone()));
                }
            }
        }
    }

    if stack.len() != 1 {
        return Err("Missing ')' at end of string".to_string());
    }
    if need_bracket {
        return Err("Missing '(' at end of string".to_string());
    }
    Ok(stack.pop().unwrap())
}

/// Whether `nodes` (implicit AND -- the top level, or an `AllOf` bundle)
/// has at least one license that isn't in `acceptable`. Mirrors real
/// `LicenseManager._getMaskedLicenses`'s own non-`||` branch, as a bool
/// rather than the full "list every masked license" diagnostic (this
/// pilot has no mask-reason display to feed it -- same simplification
/// `check_required_use` already makes for REQUIRED_USE).
fn tree_has_masked_license(nodes: &[LicenseNode], acceptable: &HashSet<String>) -> bool {
    nodes.iter().any(|n| node_has_masked_license(n, acceptable))
}

fn node_has_masked_license(node: &LicenseNode, acceptable: &HashSet<String>) -> bool {
    match node {
        LicenseNode::License(name) => !acceptable.contains(name),
        LicenseNode::AllOf(members) => tree_has_masked_license(members, acceptable),
        // Satisfied (not masked) once at least one alternative is fully
        // unmasked -- mirrors real _getMaskedLicenses's own "||" branch:
        // "if not tmp: return []" the moment one alternative comes back
        // clean.
        LicenseNode::AnyOf(members) => !members
            .iter()
            .any(|m| !node_has_masked_license(m, acceptable)),
    }
}

/// Whether `license_str` (a candidate's own real `LICENSE` metadata,
/// PMS 7.3.2) has at least one required-but-unaccepted license, given
/// `use_flags` (this candidate's own resolved USE, only ever consulted
/// if `license_str` actually contains a `?`) and `acceptable` (the
/// fully-resolved, concrete set of accepted license names for this
/// specific candidate -- see `license_accepted`). Mirrors real
/// `LicenseManager.getMissingLicenses` (via `_getMaskedLicenses`), ported
/// as a bool. An empty `license_str` is never masked, matching real
/// `use_reduce("")`'s own empty result.
fn has_masked_license(
    license_str: &str,
    use_flags: &HashSet<String>,
    acceptable: &HashSet<String>,
) -> Result<bool, String> {
    if license_str.trim().is_empty() {
        return Ok(false);
    }
    let tokens: Vec<String> = license_str.split_whitespace().map(String::from).collect();
    let tree = parse_license_tree(&tokens, use_flags)?;
    Ok(tree_has_masked_license(&tree, acceptable))
}

/// Whether `candidate`'s own declared LICENSE is fully accepted -- real
/// `Package.py`'s own `settings._getMissingLicenses` check (via
/// `LicenseManager.getMissingLicenses`/`_getPkgAcceptLicense`).
///
/// `config.accept_license` (global, already `@group`-expanded but still
/// symbolic -- see that field's own doc comment, portage-profile) is
/// layered with every matching `package.license` entry's own tokens, in
/// atom-specificity order -- real `_getPkgAcceptLicense`'s own
/// `accept_license.extend(x)` loop over `ordered_by_atom_specificity`
/// matches, ported the same way `package.use.mask`/`.force` already
/// order multiple matches in this pilot (see `effective_use_flags`).
/// The resulting symbolic token list is resolved into a *concrete*
/// per-candidate acceptable-license set via the same `*`/`-*`/
/// `-license`/`license` algorithm real `getMissingLicenses`/
/// `get_pruned_accept_license` both use -- `*` needs "every license
/// LICENSE could possibly mention" (real `matchall=1`: every USE-
/// conditional forced active, ported here by reusing
/// `portage_use_reduce::use_reduce_flat` with `MatchMode::All`, since
/// group boundaries don't matter for this flat "what license names
/// exist at all" question the way they do for the real masking check
/// below).
///
/// A `LICENSE` string this pilot's own bespoke parser can't make sense
/// of is treated as masked (not visible) rather than accepted --
/// matching the "can't tell, so exclude" precedent this pilot's own
/// `reinstall_flags_for_use_change`/`candidate_iuse_and_use` already
/// establish for an unreadable candidate, rather than real portage's own
/// (differently-plumbed) `InvalidDependString` handling, which routes a
/// malformed `LICENSE` to a wholly separate "invalid metadata" masking
/// reason this pilot has no equivalent pathway for.
/// This candidate's own effective USE, only actually resolved if
/// `value_str` (a `LICENSE`/`PROPERTIES`/`RESTRICT` string) contains a
/// `?` at all -- real `use_reduce`'s own "if '?' in license_str"
/// optimization, shared by every metadata key that needs this same
/// "resolve USE, but only when it could possibly matter" treatment.
#[allow(clippy::too_many_arguments)]
fn use_flags_if_conditional(
    value_str: &str,
    candidate: &Candidate,
    category: &str,
    package: &str,
    candidate_str: &str,
    config: &portage_profile::Config,
) -> HashSet<String> {
    if !value_str.contains('?') {
        return HashSet::new();
    }
    effective_use_flags(
        &candidate.iuse,
        &config.use_tokens,
        &config.package_use,
        &config.package_use_force,
        &config.package_use_mask,
        &config.use_force,
        &config.use_mask,
        &config.use_stable_force,
        &config.use_stable_mask,
        &config.package_use_stable_force,
        &config.package_use_stable_mask,
        &candidate.keywords,
        &config.accept_keywords,
        &config.package_accept_keywords,
        candidate_str,
        category,
        package,
    )
}

/// This candidate's own effective `ACCEPT_LICENSE`/`ACCEPT_PROPERTIES`/
/// `ACCEPT_RESTRICT`-style symbolic token list: `global_accept`, with
/// every matching `package_accept` entry's own tokens layered on top,
/// in atom-specificity order -- real `_getPkgAcceptLicense`'s own
/// `accept_license.extend(x)` loop over `ordered_by_atom_specificity`
/// matches (and its `_getMissingProperties`/`_getMissingRestrict`
/// siblings, which do the identical thing for their own accept lists),
/// ported the same way `package.use.mask`/`.force` already order
/// multiple matches in this pilot (see `effective_use_flags`).
fn resolve_accept_tokens(
    global_accept: &[String],
    package_accept: &[(String, Vec<String>)],
    candidate_str: &str,
    category: &str,
    package: &str,
) -> Vec<String> {
    let mut matching: Vec<&(String, Vec<String>)> = package_accept
        .iter()
        .filter(|(atom, _)| matches_config_entry(atom, candidate_str, category, package))
        .collect();
    matching.sort_by_key(|(atom, _)| atom_specificity(atom));

    let mut accept_tokens = global_accept.to_vec();
    for (_, tokens) in matching {
        accept_tokens.extend(tokens.iter().cloned());
    }
    accept_tokens
}

/// Every token `value_str` (a `LICENSE`/`PROPERTIES`/`RESTRICT` string)
/// could possibly mention, USE-conditionals included -- real
/// `matchall=1` semantics (every conditional forced active), needed to
/// resolve a `"*"` token in an accept-list into something concrete.
/// Reuses `portage_use_reduce::use_reduce_flat` with `MatchMode::All`
/// directly: group boundaries (`||`) don't matter for this flat "what
/// token names exist at all" question, unlike the real masking check
/// itself for `LICENSE` (see `has_masked_license`'s own doc comment).
fn all_mentioned_tokens(value_str: &str) -> Result<HashSet<String>, String> {
    let tokens: Vec<String> = value_str.split_whitespace().map(String::from).collect();
    let flat = portage_use_reduce::use_reduce_flat(
        &tokens,
        &HashSet::new(),
        portage_use_reduce::MatchMode::All,
    )?;
    Ok(flat.into_iter().filter(|t| t != "||").collect())
}

/// Resolves `accept_tokens` (symbolic -- `"*"`/`"-*"`/`"-token"`/
/// `"token"`) into a concrete acceptable-token set, given
/// `all_mentioned` (see `all_mentioned_tokens`). Shared by
/// `license_accepted`/`metadata_key_accepted` -- real
/// `getMissingLicenses`/`_getMissingProperties`/`_getMissingRestrict`
/// all use this identical algorithm, just for a different metadata key.
fn resolve_acceptable_tokens(
    accept_tokens: &[String],
    all_mentioned: &HashSet<String>,
) -> HashSet<String> {
    let mut acceptable: HashSet<String> = HashSet::new();
    for token in accept_tokens {
        if token == "*" {
            acceptable.extend(all_mentioned.iter().cloned());
        } else if token == "-*" {
            acceptable.clear();
        } else if let Some(name) = token.strip_prefix('-') {
            acceptable.remove(name);
        } else {
            acceptable.insert(token.clone());
        }
    }
    acceptable
}

fn license_accepted(
    candidate: &Candidate,
    category: &str,
    package: &str,
    candidate_str: &str,
    config: &portage_profile::Config,
) -> bool {
    if candidate.license.trim().is_empty() {
        return true;
    }
    let use_flags = use_flags_if_conditional(
        &candidate.license,
        candidate,
        category,
        package,
        candidate_str,
        config,
    );
    let accept_tokens = resolve_accept_tokens(
        &config.accept_license,
        &config.package_license,
        candidate_str,
        category,
        package,
    );
    let Ok(all_mentioned) = all_mentioned_tokens(&candidate.license) else {
        return false;
    };
    let acceptable = resolve_acceptable_tokens(&accept_tokens, &all_mentioned);

    match has_masked_license(&candidate.license, &use_flags, &acceptable) {
        Ok(masked) => !masked,
        Err(_) => false,
    }
}

/// Whether every token in `value_str` (a candidate's own real
/// `PROPERTIES`/`RESTRICT` metadata) is accepted -- real
/// `_getMissingProperties`/`_getMissingRestrict`, ported as a bool.
/// Unlike `LICENSE` (which needs `||`-group *structure* -- see
/// `has_masked_license`'s own doc comment), `PROPERTIES`/`RESTRICT` have
/// no any-of semantics at all: real config.py's own comment says it
/// plainly, "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without
/// groups" -- every flattened token individually needs to be accepted,
/// so this reuses `use_reduce_flat` directly instead of the bespoke
/// `LicenseNode` tree (no `||`-structure to lose in the first place).
#[allow(clippy::too_many_arguments)]
fn metadata_key_accepted(
    value_str: &str,
    candidate: &Candidate,
    category: &str,
    package: &str,
    candidate_str: &str,
    config: &portage_profile::Config,
    global_accept: &[String],
    package_accept: &[(String, Vec<String>)],
) -> bool {
    if value_str.trim().is_empty() {
        return true;
    }
    let use_flags = use_flags_if_conditional(
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
    );
    let accept_tokens = resolve_accept_tokens(
        global_accept,
        package_accept,
        candidate_str,
        category,
        package,
    );
    let Ok(all_mentioned) = all_mentioned_tokens(value_str) else {
        return false;
    };
    let acceptable = resolve_acceptable_tokens(&accept_tokens, &all_mentioned);

    let tokens: Vec<String> = value_str.split_whitespace().map(String::from).collect();
    match portage_use_reduce::use_reduce_flat(
        &tokens,
        &use_flags,
        portage_use_reduce::MatchMode::Normal,
    ) {
        Ok(flat) => flat.iter().all(|t| acceptable.contains(t)),
        Err(_) => false,
    }
}

/// A candidate is visible if it isn't masked (matches a `package.mask`
/// entry and no `package.unmask` entry), its KEYWORDS intersect the
/// accepted set -- the global `config.accept_keywords`, plus any extra
/// keywords contributed by a matching `package.accept_keywords` entry,
/// with a `"**"` token in such an entry meaning "accept unconditionally"
/// for matching candidates (even ones with empty/no KEYWORDS) -- and its
/// own declared `LICENSE`/`PROPERTIES`/`RESTRICT` are all fully accepted
/// (see `license_accepted`/`metadata_key_accepted`) -- real `Package.py`'s
/// own `_masks` dict collects `package.mask`, `LICENSE`, `PROPERTIES`,
/// and `RESTRICT` as four independent masking reasons the same way.
pub fn is_visible(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );

    let masked = config
        .package_mask
        .iter()
        .any(|m| matches_config_entry(m, &candidate_str, category, package))
        && !config
            .package_unmask
            .iter()
            .any(|u| matches_config_entry(u, &candidate_str, category, package));
    if masked {
        return false;
    }

    if !license_accepted(candidate, category, package, &candidate_str, config) {
        return false;
    }

    if !metadata_key_accepted(
        &candidate.properties,
        candidate,
        category,
        package,
        &candidate_str,
        config,
        &config.accept_properties,
        &config.package_properties,
    ) {
        return false;
    }

    if !metadata_key_accepted(
        &candidate.restrict,
        candidate,
        category,
        package,
        &candidate_str,
        config,
        &config.accept_restrict,
        &config.package_accept_restrict,
    ) {
        return false;
    }

    keywords_accepted(
        &candidate.keywords,
        &candidate_str,
        category,
        package,
        &config.accept_keywords,
        &config.package_accept_keywords,
    )
}

/// `--autounmask`'s own keyword-suggestion sub-feature (real
/// `--autounmask-keep-keywords=n`, see `resolve_pretend_graph`'s doc
/// comment for the full on/off default-resolution logic this pilot
/// ported): true iff `candidate` would be `is_visible` except for its
/// own KEYWORDS -- every other check `is_visible` makes (package.mask,
/// license, properties, restrict) passes, only `keywords_accepted`
/// fails. Duplicates `is_visible`'s own body rather than refactoring it
/// to return a reason enum -- real portage's own `_get_masking_status`
/// is considerably more elaborate (distinguishing package.mask/license/
/// keyword/REQUIRED_USE/etc. reasons, each with its own "unmask hint"),
/// and this pilot only needs the single "keywords, and only keywords"
/// question for its own deliberately narrow v1 (see
/// `resolve_pretend_graph`'s own doc comment for the full scope
/// writeup, including what's deliberately still out: package.mask/
/// license/USE suggestions, real portage's own exact suggested-atom
/// syntax and dependency-chain-comment formatting).
fn keyword_masked_only(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );

    let masked = config
        .package_mask
        .iter()
        .any(|m| matches_config_entry(m, &candidate_str, category, package))
        && !config
            .package_unmask
            .iter()
            .any(|u| matches_config_entry(u, &candidate_str, category, package));
    if masked {
        return false;
    }

    if !license_accepted(candidate, category, package, &candidate_str, config) {
        return false;
    }

    if !metadata_key_accepted(
        &candidate.properties,
        candidate,
        category,
        package,
        &candidate_str,
        config,
        &config.accept_properties,
        &config.package_properties,
    ) {
        return false;
    }

    if !metadata_key_accepted(
        &candidate.restrict,
        candidate,
        category,
        package,
        &candidate_str,
        config,
        &config.accept_restrict,
        &config.package_accept_restrict,
    ) {
        return false;
    }

    !keywords_accepted(
        &candidate.keywords,
        &candidate_str,
        category,
        package,
        &config.accept_keywords,
        &config.package_accept_keywords,
    )
}

/// The keyword this pilot's own `--autounmask` v1 would suggest adding
/// to `package.accept_keywords` for `candidate` -- the first of its own
/// (non-`-`-prefixed; a `-`-prefixed KEYWORDS token means "explicitly
/// unsupported here," never a valid suggestion) `KEYWORDS` tokens.
/// Deliberately simpler than real portage's own `_get_masking_status`
/// (which picks specifically between the unstable (`~arch`) form, a
/// different arch entirely, or `**`, based on exactly what's already
/// accepted) -- see `keyword_masked_only`'s own doc comment for the
/// full scope writeup this shares. `None` only if `KEYWORDS` is empty or
/// every token is `-`-prefixed (unusual, but not impossible).
fn suggested_keyword(candidate: &Candidate) -> Option<&str> {
    candidate
        .keywords
        .iter()
        .find(|k| !k.starts_with('-'))
        .map(String::as_str)
}

/// The keyword-matching half of `is_visible` (everything except the
/// `package.mask`/`.unmask` check), factored out so `is_stable` below
/// can reuse it against an artificially-unstabilized keyword list
/// instead of `candidate.keywords` itself -- real `KeywordsManager.
/// isStable`/`getMissingKeywords` share this exact same matching logic
/// with real visibility checking too, just against a different input
/// keyword set, not a separate algorithm. Takes the two config pieces
/// it actually needs directly (rather than a whole `Config`) so
/// `effective_use_flags` -- which, like the rest of this file's
/// established style, takes individual pre-extracted fields rather
/// than a `Config` reference -- can call `is_stable` without needing
/// one either.
///
/// Grounded against real `KeywordsManager.getMissingKeywords`/
/// `_getEgroups` (`lib/portage/package/ebuild/_config/
/// KeywordsManager.py`): a `package.accept_keywords` entry doesn't just
/// *add* keywords on top of the global `ACCEPT_KEYWORDS` set -- real
/// `_getEgroups` folds `-token`/`-*` removals too, over the *combined*
/// list (global keywords first, then each matching entry's own tokens,
/// in atom-specificity order), so a more-specific `package.accept_
/// keywords` line can revoke a keyword the global set already granted,
/// not just add new ones. Ported here via `specificity_ordered_flags`
/// (already established for `package.use.mask`/`.force`'s own identical
/// "specificity-ordered incremental fold" shape) seeded with
/// `accept_keywords` itself, rather than the previous, incorrect
/// "union everything a matching entry ever mentions, ignore any `-`
/// prefix" accumulation. `"**"` is folded in exactly like any other
/// token now (removable by a later `-*`/`-**`), rather than a separate
/// unconditional-accept pre-scan that ignored fold order entirely --
/// once folded, its presence in the final accepted set still means
/// "accept any KEYWORDS state, even empty," the same real `"**" in
/// pgroups` unconditional-match rule this pilot already documented. A
/// bare `package.accept_keywords` atom with no keyword list at all no
/// longer reaches this function empty: `resolve_config`
/// (`portage-profile`) already substitutes real `accept_keywords_
/// defaults`'s own implicit meaning -- the `~`-prefixed unstable form of
/// every currently-accepted keyword -- before this function ever sees
/// it, so it folds in through `specificity_ordered_flags` exactly like
/// any other entry's own explicit tokens would (see
/// `parse_package_accept_keywords_lines`'s own doc comment,
/// portage-profile).
///
/// A second real mechanism, previously unhandled: a literal `"*"`/`"~*"`
/// token in the accepted set means "accept any stable keyword"/"accept
/// any testing keyword" respectively -- distinct from `"**"` (accept
/// even an *empty* `KEYWORDS`) and from a plain keyword name, which
/// `apply_incremental` would otherwise insert as an inert string that
/// can never equal a real `KEYWORDS` entry. Ported from real
/// `_getMissingKeywords`'s own per-candidate-keyword loop
/// (`lib/portage/package/ebuild/_config/KeywordsManager.py`, lines
/// ~273-300): each of the candidate's own `keywords` is checked for a
/// direct match first (short-circuiting immediately, same as real
/// `match = True; break`); a `-`-prefixed one (explicit "not supported
/// here", distinct from simply absent) never matches and is excluded
/// from classification entirely, matching real portage's own elif
/// chain; anything else is classified stable or testing (`~`-prefixed)
/// for the final fallback -- `"*"` grants acceptance if *any* declared
/// keyword was stable-classified, `"~*"` if any was testing-classified,
/// matching real `(hastesting and "~*" in pgroups) or (hasstable and
/// "*" in pgroups)` exactly (the third real disjunct, `"**" in pgroups`,
/// is the unconditional check already handled above).
fn keywords_accepted(
    keywords: &[String],
    candidate_str: &str,
    category: &str,
    package: &str,
    accept_keywords: &HashSet<String>,
    package_accept_keywords: &[(String, Vec<String>)],
) -> bool {
    let accepted = specificity_ordered_flags(
        package_accept_keywords,
        candidate_str,
        category,
        package,
        accept_keywords.clone(),
    );
    if accepted.contains("**") {
        return true;
    }

    // Real _getMissingKeywords's own per-keyword loop: a "-"-prefixed
    // KEYWORDS token (explicit "not supported here", distinct from
    // simply absent) never itself matches and never counts toward
    // has_stable/has_testing either -- it's excluded from every branch
    // of real portage's own elif chain the same way. A direct match
    // (the accepted set literally contains this exact keyword) wins
    // immediately; otherwise this keyword only contributes to the
    // has_stable/has_testing tally used by the "*"/"~*" fallback below.
    let mut has_stable = false;
    let mut has_testing = false;
    for k in keywords {
        if k.starts_with('-') {
            continue;
        }
        if accepted.contains(k) {
            return true;
        }
        if k.starts_with('~') {
            has_testing = true;
        } else {
            has_stable = true;
        }
    }
    (has_testing && accepted.contains("~*")) || (has_stable && accepted.contains("*"))
}

/// Whether `keywords` (a candidate's own KEYWORDS) count as "stable"
/// for the purposes of `use.stable.mask`/`.force`/`package.use.stable.
/// mask`/`.force` -- ported from real `KeywordsManager.isStable`
/// (`lib/portage/package/ebuild/_config/KeywordsManager.py`): NOT a raw
/// "no `~` prefix" check. A candidate counts as stable if replacing
/// *every* one of its own keywords with its `~`-prefixed unstable form
/// would make it invisible under the current `ACCEPT_KEYWORDS`/
/// `package.accept_keywords` -- real portage's own comment explains why:
/// "this guarantees that the effective use.force/mask settings for a
/// particular ebuild do not change when that ebuild is stabilized."
/// Reuses `keywords_accepted` (the same matching logic `is_visible`
/// itself uses for its own KEYWORDS half) against that artificially-
/// unstabilized list rather than reimplementing keyword matching a
/// second time.
fn is_stable(
    keywords: &[String],
    candidate_str: &str,
    category: &str,
    package: &str,
    accept_keywords: &HashSet<String>,
    package_accept_keywords: &[(String, Vec<String>)],
) -> bool {
    let unstable: Vec<String> = keywords
        .iter()
        .map(|k| {
            if k.starts_with('~') {
                k.clone()
            } else {
                format!("~{k}")
            }
        })
        .collect();
    !keywords_accepted(
        &unstable,
        candidate_str,
        category,
        package,
        accept_keywords,
        package_accept_keywords,
    )
}

fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}

/// Lists every installed `(version, slot, sub_slot)` triple for
/// `category/package` found in the vdb under `root`
/// (`<root>/var/db/pkg/<category>/<package>-<version>/`), reading each
/// entry's `SLOT` file via `split_slot` (defaulting to `("0", "0")` if
/// missing, same fallback as `list_candidates`). Used for blocker
/// matching, which needs slots (sub-slot included, same "closes a real
/// gap" reasoning as `Candidate::sub_slot`'s own doc comment) to support
/// slotted blocker atoms -- `installed_versions` below doesn't need this
/// and stays a plain version list for its existing callers. `pub`
/// (unlike most of this crate's own internal helpers) so
/// `multicall/src/pretend.rs`'s own `--deselect`/`-W` implementation can
/// reuse it directly -- real `action_deselect` (`lib/_emerge/actions.py`)
/// only ever consults `vardb`/the world file, never repos/config at
/// all, so this is the one real feature in this pilot with no need for
/// `portage-repo`'s own repo/config-resolution machinery. `run_deselect`
/// itself only ever uses `version`/`slot` from this (real
/// `Atom(f"{pkg.cp}:{pkg.slot}")` never includes sub-slot either), so
/// adding `sub_slot` here doesn't change its behavior at all.
pub fn installed_candidates(
    root: &Path,
    category: &str,
    package: &str,
) -> Vec<(String, String, String)> {
    let cat_dir = root.join("var/db/pkg").join(category);
    let Ok(entries) = fs::read_dir(&cat_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let version = strip_version_prefix(&name, package)?.to_string();
            let raw_slot = fs::read_to_string(e.path().join("SLOT")).unwrap_or_default();
            let (slot, sub_slot) = split_slot(raw_slot.trim());
            Some((version, slot, sub_slot))
        })
        .collect()
}

/// Lists every installed version of `category/package` found in the vdb
/// under `root` (`<root>/var/db/pkg/<category>/<package>-<version>/`).
pub fn installed_versions(root: &Path, category: &str, package: &str) -> Vec<String> {
    installed_candidates(root, category, package)
        .into_iter()
        .map(|(version, _slot, _sub_slot)| version)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PretendOutcome {
    NoVisibleCandidate,
    New {
        version: String,
    },
    Upgrade {
        from: String,
        to: String,
    },
    AlreadyInstalled {
        version: String,
    },
    /// `--newuse`/`--changed-use` and/or `--changed-deps` and/or
    /// `--changed-slot`: already installed at this exact version, but
    /// this package's currently-effective USE differs from what the vdb
    /// recorded at merge time (see `reinstall_flags_for_use_change`),
    /// and/or its own vdb-recorded dependency strings differ from the
    /// repo's current ebuild (see `deps_changed`), and/or its own
    /// vdb-recorded `SLOT` differs from the repo's current ebuild (see
    /// `slot_changed`) -- real portage treats these as independent,
    /// freely-combinable reinstall reasons, not mutually exclusive ones.
    /// `changed_flags` is the sorted set of flag names that triggered
    /// the USE-based reason (real depgraph's own `_reinstall_for_flags`
    /// return value, kept here purely for display, matching `Upgrade`'s
    /// own `from`/`to` pattern) -- empty when it didn't trigger this
    /// outcome. At least one of `changed_flags`/`deps_changed`/
    /// `slot_changed` is always non-empty/`true`; a `Reinstall` with
    /// none of the three is never constructed.
    Reinstall {
        version: String,
        changed_flags: Vec<String>,
        deps_changed: bool,
        slot_changed: bool,
    },
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/<filename>`
/// (a vdb aux file, e.g. `USE` or `IUSE` -- same directory `SLOT`/
/// `CATEGORY` already come from) as a set of flag names, one per
/// whitespace-separated token, with any `+`/`-` IUSE default-marker
/// prefix stripped (irrelevant here: a reinstall check only cares which
/// flags exist/are enabled, not their declared defaults). A missing file
/// (e.g. an older vdb entry from before this pilot's fixtures modeled
/// USE/IUSE at all) is an empty set, not an error -- same "absence is a
/// real, valid state" precedent `read_world_atoms` (pretend.rs) already
/// established.
fn read_vdb_flag_set(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    filename: &str,
) -> HashSet<String> {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(format!("{package}-{version}"))
        .join(filename);
    fs::read_to_string(path)
        .unwrap_or_default()
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect()
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/<filename>`
/// as a raw string (e.g. `DEPEND`/`RDEPEND`), unlike `read_vdb_flag_set`
/// which splits into a flag-name set -- a dependency string needs to
/// stay intact for `portage_use_reduce::use_reduce_flat` to parse
/// (`||`/USE-conditional groups, not just bare tokens). A missing file
/// is an empty string, not an error -- same "absence is a real, valid
/// state" precedent `read_vdb_flag_set` already established (a vdb
/// entry with nothing recorded for this key, e.g. `DEPEND=""` at merge
/// time, is indistinguishable from -- and handled the same as -- one
/// that's simply missing the file).
fn read_vdb_string(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    filename: &str,
) -> String {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(format!("{package}-{version}"))
        .join(filename);
    fs::read_to_string(path).unwrap_or_default()
}

/// Flattens `depstr` (one or more concatenated dependency-string keys)
/// against `use_flags`, into a `HashSet` of dependency-atom tokens
/// (`||` markers dropped) suitable for order-independent equality
/// comparison. `None` if `depstr` doesn't parse at all.
fn flat_dep_atoms(depstr: &str, use_flags: &HashSet<String>) -> Option<HashSet<String>> {
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let flat = portage_use_reduce::use_reduce_flat(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    )
    .ok()?;
    Some(flat.into_iter().filter(|t| t != "||").collect())
}

/// Real `find_libc_deps(vardb, realized=False)` (`portage.dep.libc`): the
/// `(category, package)` identity of every atom `virtual/libc`'s own
/// installed (vdb) `RDEPEND` names, once flattened against its own
/// installed `USE` -- empty if `virtual/libc` isn't installed at all,
/// same as real `vardb.match("virtual/libc")` finding nothing. A
/// simplified, one-level port of real `expand_new_virt`: real Gentoo's
/// own `virtual/libc` `RDEPEND` is always a flat `|| ( sys-libs/glibc
/// sys-libs/musl ... )` of real (non-virtual) packages, so this doesn't
/// replicate `expand_new_virt`'s own further case of recursing into a
/// *second* virtual reached this way, which real `virtual/libc` never
/// actually needs. Used by `deps_changed` to strip libc atoms out of
/// both sides of its own comparison before comparing -- real
/// `strip_libc_deps`'s whole purpose: practically every ebuild silently
/// gains/loses an implicit libc dependency across revisions, and that's
/// noise, not a real dependency change worth reporting.
fn libc_provider_cps(root: &Path) -> HashSet<(String, String)> {
    let mut result = HashSet::new();
    for version in installed_versions(root, "virtual", "libc") {
        let use_flags = read_vdb_flag_set(root, "virtual", "libc", &version, "USE");
        let rdepend = read_vdb_string(root, "virtual", "libc", &version, "RDEPEND");
        let Some(atoms) = flat_dep_atoms(&rdepend, &use_flags) else {
            continue;
        };
        for atom_str in atoms {
            if let Some(atom) = portage_dep::parse_atom(&atom_str) {
                result.insert((atom.category, atom.package));
            }
        }
    }
    result
}

/// Removes any atom in `atoms` whose own `(category, package)` is in
/// `libc_cps` -- see `libc_provider_cps`'s own doc comment.
fn strip_libc_atoms(
    atoms: HashSet<String>,
    libc_cps: &HashSet<(String, String)>,
) -> HashSet<String> {
    if libc_cps.is_empty() {
        return atoms;
    }
    atoms
        .into_iter()
        .filter(|a| {
            portage_dep::parse_atom(a)
                .map(|atom| !libc_cps.contains(&(atom.category, atom.package)))
                .unwrap_or(true)
        })
        .collect()
}

/// `--changed-deps`: whether `version`'s own vdb-recorded dependency
/// strings differ from the repo's own *current* ebuild for that exact
/// version, once both are flattened against the *same* input -- the
/// installed package's own recorded `USE` (real `depgraph.py`'s own
/// `_changed_deps`: `uselist=pkg.use.enabled`, used for *both* sides of
/// the comparison), so a difference driven purely by a USE change is
/// never what this detects -- that's `--newuse`/`--changed-use`'s own
/// job, and can fire independently of (or alongside) this one. Which
/// keys are compared respects `with_bdeps` exactly like
/// `enqueue_dependencies`'s own dep-key list does (real `depgraph.py`'s
/// own `if self._dynamic_config.myparams.get("bdeps") in ("y", "auto"):
/// depvars = Package._dep_keys ... else: depvars = Package._runtime_keys`).
///
/// KNOWN, DOCUMENTED SCOPE CUT: real `_changed_deps` compares real
/// *structured* `use_reduce` output (`||`-group boundaries preserved,
/// via `token_class=Atom`) key by key, so a dependency moved between
/// two of the five keys with the same net atom set, or a pure
/// `||`-group restructuring with the same underlying atoms, would count
/// as "changed" there but not here. This pilot has no structured
/// (non-flat) `use_reduce` at all -- `portage_use_reduce::use_reduce_flat`
/// is a deliberate, already-established simplification used by *every*
/// dependency-recursion path in this pilot (see this module's own doc
/// comment on `resolve_pretend_graph`), so this reuses that same flat
/// comparison rather than building bespoke structured-tree machinery
/// just for this one feature -- consistent with, not a new exception to,
/// the rest of this pilot's own dependency handling.
///
/// A vdb-side dependency string that fails to parse counts as
/// "changed" unconditionally, matching real portage's own `except
/// InvalidDependString: changed = True`; a repo-side one that fails to
/// parse instead reports "unchanged" (`false`), the same tolerant
/// "can't tell, don't crash" fallback `enqueue_dependencies` already
/// uses for its own unreadable-metadata cases, since real portage has
/// no equivalent fallback to mirror there (the repo side is assumed
/// always well-formed).
///
/// Both atom sets are filtered through `libc_provider_cps` first (see
/// its own doc comment) -- real `strip_libc_deps`, closing the gap this
/// function's own doc comment used to name explicitly as unaddressed.
fn deps_changed(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
    with_bdeps: bool,
) -> bool {
    let dep_keys: &[&str] = if with_bdeps {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    };

    let installed_use = read_vdb_flag_set(root, category, package, version, "USE");

    let mut vdb_depstr = String::new();
    for key in dep_keys {
        let s = read_vdb_string(root, category, package, version, key);
        if !s.is_empty() {
            vdb_depstr.push_str(&s);
            vdb_depstr.push(' ');
        }
    }

    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = read_md5_cache(&resolved.repo_location, category, &pf) else {
        return false;
    };
    let mut repo_depstr = String::new();
    for key in dep_keys {
        if let Some(d) = metadata.get(*key) {
            repo_depstr.push_str(d);
            repo_depstr.push(' ');
        }
    }

    let Some(repo_atoms) = flat_dep_atoms(&repo_depstr, &installed_use) else {
        return false;
    };
    let libc_cps = libc_provider_cps(root);
    let repo_atoms = strip_libc_atoms(repo_atoms, &libc_cps);
    match flat_dep_atoms(&vdb_depstr, &installed_use) {
        Some(vdb_atoms) => strip_libc_atoms(vdb_atoms, &libc_cps) != repo_atoms,
        None => true,
    }
}

/// Splits a raw `SLOT` string into `(slot, sub_slot)` -- real portage:
/// `SLOT="main/sub"`, `sub_slot` defaulting to the slot itself when no
/// `/` is present (real `portage.versions._pkg_str`'s own slot-parsing
/// branch). An empty string (missing `SLOT` file/key) defaults to
/// `("0", "0")`, matching the same `"0"` fallback `list_candidates`/
/// `installed_candidates` already use for a missing `SLOT`.
fn split_slot(raw: &str) -> (String, String) {
    if raw.is_empty() {
        return ("0".to_string(), "0".to_string());
    }
    match raw.split_once('/') {
        Some((slot, sub_slot)) => (slot.to_string(), sub_slot.to_string()),
        None => (raw.to_string(), raw.to_string()),
    }
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/SLOT` and
/// splits it via `split_slot` -- real `vardbapi`'s own `SLOT` file is
/// written verbatim from the same `SLOT` variable a repo's own ebuild
/// declares, so this is the identical format `list_candidates`'s own
/// (main-slot-only) parsing already reads from the repo side, just with
/// the sub-slot component kept too instead of discarded.
fn read_vdb_slot(root: &Path, category: &str, package: &str, version: &str) -> (String, String) {
    split_slot(read_vdb_string(root, category, package, version, "SLOT").trim())
}

/// `--changed-slot`: whether `version`'s own vdb-recorded `SLOT`
/// (main+sub) differs from the repo's own *current* ebuild for that
/// exact version. Real `depgraph.py`'s own `_changed_slot`: `ebuild =
/// self._equiv_ebuild(pkg); return ebuild is not None and (ebuild.slot,
/// ebuild.sub_slot) != (pkg.slot, pkg.sub_slot)`.
///
/// KNOWN, DOCUMENTED SCOPE CUT: real portage's own consumers of
/// `_changed_slot` live deep inside binary-package/slot-operator-rebuild
/// scheduling this pilot has none of
/// (`_slot_operator_replace_installed`, `built`/`useoldpkg` branches in
/// `_privileged_size_fetch`/candidate selection) -- rejecting a matched
/// installed candidate and, depending on context, either aborting the
/// search or continuing to look for a binary package with the right
/// `SLOT`. Ported here as simply another independent
/// `PretendOutcome::Reinstall` trigger instead, the same "report a
/// reinstall" simplification `--changed-deps` already established --
/// captures the dominant real-world effect (a package whose `SLOT`
/// metadata changed upstream, e.g. an ABI-bump `SLOT="0"` ->
/// `SLOT="0/2"`, gets flagged for reinstall) without replicating real
/// portage's own considerably messier, binpkg-entangled control flow.
/// Deliberately does *not* reuse `Candidate::slot` (which
/// `list_candidates` already truncates to the main component only, see
/// its own doc comment) -- re-reads the repo's own raw `SLOT` value
/// directly instead, the same "re-read metadata this pilot's general
/// `Candidate` model doesn't carry" approach `deps_changed` already uses
/// for `DEPEND`/`RDEPEND`. A repo-side lookup that fails (version no
/// longer in the tree, unreadable metadata) reports "unchanged" (`false`),
/// the same tolerant fallback `deps_changed` already uses, matching real
/// `_equiv_ebuild(pkg) is None` -> `False` exactly.
fn slot_changed(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    let vdb_slot = read_vdb_slot(root, category, package, version);

    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = read_md5_cache(&resolved.repo_location, category, &pf) else {
        return false;
    };
    let repo_slot = split_slot(
        metadata
            .get("SLOT")
            .map(String::as_str)
            .unwrap_or("")
            .trim(),
    );

    vdb_slot != repo_slot
}

/// `candidate`'s own current IUSE (read fresh from its own md5-cache
/// entry -- the current tree's metadata, not the vdb) and its own
/// effective (computed) USE set, via `effective_use_flags`. Shared by
/// `reinstall_flags_for_use_change`'s own "cur_iuse"/"cur_use" (the
/// current-tree side of a `--newuse`/`--changed-use` comparison) and
/// `resolve_pretend`'s own USE-dep filtering (`use_deps_satisfied`,
/// portage-dep) -- both need exactly this same pair for a candidate
/// that's about to be installed or is already installed, computed the
/// same way regardless of which. Returns `None` if this candidate's own
/// metadata can't be read at all (e.g. `IUSE` missing).
fn candidate_iuse_and_use(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Option<(HashSet<String>, HashSet<String>)> {
    let pf = format!("{package}-{}", candidate.version);
    let metadata = read_md5_cache(&candidate.repo_location, category, &pf).ok()?;
    // A missing IUSE key is a real, valid "declares no USE flags at all"
    // state (same "absence is real, not an error" precedent
    // read_vdb_flag_set already sets for a missing vdb IUSE/USE file),
    // not a reason to treat this whole candidate as unreadable -- unlike
    // a missing md5-cache entry entirely (the `?` just above), which
    // really does mean "can't tell anything about this candidate".
    let iuse: HashSet<String> = metadata
        .get("IUSE")
        .map(|s| s.as_str())
        .unwrap_or_default()
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let use_flags = effective_use_flags(
        metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
        &config.use_tokens,
        &config.package_use,
        &config.package_use_force,
        &config.package_use_mask,
        &config.use_force,
        &config.use_mask,
        &config.use_stable_force,
        &config.use_stable_mask,
        &config.package_use_stable_force,
        &config.package_use_stable_mask,
        &candidate.keywords,
        &config.accept_keywords,
        &config.package_accept_keywords,
        &candidate_str,
        category,
        package,
    );
    Some((iuse, use_flags))
}

/// `--newuse`/`--changed-use`: ports both the `newuse` and `elif
/// changed_use` branches of real `depgraph.py`'s `_reinstall_for_flags`
/// -- whether `candidate` (a version already installed) needs
/// reinstalling because its currently-effective USE differs from what
/// the vdb recorded at merge time. Returns the sorted list of flags
/// that triggered it, or `None` if nothing did. Only ever called when
/// at least one of `newuse`/`changed_use` is set; if both are (real
/// emerge accepts giving both at once), `newuse` wins -- matching real
/// portage's own `if newuse or (...): ... elif changed_use or (...):
/// ...`, which checks `newuse` first.
///
/// Both branches share one term: `(orig_iuse∩orig_use) ^
/// (cur_iuse∩cur_use)` (which flags were enabled, among those actually
/// declared on each side) -- this alone is the *entire* `--changed-use`
/// formula, deliberately narrower than `--newuse`: it only ever reacts
/// to an *enablement* change of a flag that exists in IUSE on both
/// sides, never to IUSE gaining or losing a flag entirely. `--newuse`
/// adds a second term on top, `(orig_iuse ^ cur_iuse) - forced_flags`
/// (`config.use_force ∪ config.use_mask`, subtracted here and only
/// here -- real portage's own `flags -= forced_flags` line sits between
/// the `^=` and the final `|=`, so a flag forced on/off by the profile
/// never *by itself* triggers a `--newuse` reinstall, but can still
/// contribute via the shared enablement term above, exactly like real
/// portage): whether a flag exists in IUSE changed at all, regardless of
/// whether it's even enabled.
fn reinstall_flags_for_use_change(
    root: &Path,
    category: &str,
    package: &str,
    candidate: &Candidate,
    config: &portage_profile::Config,
    newuse: bool,
) -> Option<Vec<String>> {
    let version = &candidate.version;
    let orig_use = read_vdb_flag_set(root, category, package, version, "USE");
    let orig_iuse = read_vdb_flag_set(root, category, package, version, "IUSE");

    let (cur_iuse, cur_use) = candidate_iuse_and_use(candidate, category, package, config)?;

    let orig_enabled: HashSet<String> = orig_iuse.intersection(&orig_use).cloned().collect();
    let cur_enabled: HashSet<String> = cur_iuse.intersection(&cur_use).cloned().collect();
    let mut flags: HashSet<String> = orig_enabled
        .symmetric_difference(&cur_enabled)
        .cloned()
        .collect();

    if newuse {
        let mut presence_diff: HashSet<String> =
            orig_iuse.symmetric_difference(&cur_iuse).cloned().collect();
        for forced in config.use_force.union(&config.use_mask) {
            presence_diff.remove(forced);
        }
        flags.extend(presence_diff);
    }

    if flags.is_empty() {
        return None;
    }
    let mut sorted: Vec<String> = flags.into_iter().collect();
    sorted.sort();
    Some(sorted)
}

/// The single-atom v1 `emerge --pretend` decision: find the best visible
/// candidate matching `atom_str` across all of `repos` (the main repo and
/// any overlays -- see `find_repos`), compare it against what's
/// installed. `atom_str` may be a full atom (operator, slot -- anything
/// portage-dep's v1 grammar supports), not just a bare category/package:
/// this is what lets dependency atoms extracted from DEPEND/RDEPEND (see
/// `resolve_pretend_graph`) reuse the exact same resolution logic as the
/// top-level CLI atom. `newuse`/`changed_use` each enable their own
/// reinstall check (see `reinstall_flags_for_use_change`) for an
/// already-installed match, `newuse` winning if both are set; `false`
/// for both reproduces this function's pre-`--newuse`/`--changed-use`
/// behavior exactly.
///
/// `update` (`--update`/`-u`) gates real `_wrapped_select_pkg_highest_available_imp`'s
/// own `avoid_update`/`dont_miss_updates` behavior (`lib/_emerge/depgraph.py`,
/// lines 7814 and 8448): `"--update" not in myopts` is real portage's
/// *default*, and when it holds, an already-installed version that
/// itself still satisfies the atom is returned immediately, without ever
/// searching for a newer one -- real emerge does NOT offer an upgrade
/// just because `emerge cat/pkg` was run with no other flags; that's
/// what `--update`/`-u` is for. Ported below as an early return, checked
/// before the "always resolve to the single best visible candidate"
/// logic that already existed: if `!update` and some installed version
/// both matches `atom_str` and still has a visible candidate in `visible`
/// (mask/keyword-filtered above), the highest such version is used
/// as-is, exactly like the pre-existing "installed version equals best"
/// branch below, `newuse`/`changed_use` included. Requiring a *visible*
/// candidate (not just checking the vdb directly) is deliberate, not an
/// oversight: it's what lets an installed version that's since become
/// masked fall through to the ordinary best-visible-candidate path
/// below unchanged, matching real portage's own "enable upgrade or
/// downgrade to a version with visible KEYWORDS when the installed
/// version is masked" comment right above its own `avoid_update` check.
/// When `update` is true, or no installed version qualifies this way,
/// behavior is exactly as before this parameter existed.
///
/// `selective`/`is_top_level`: a real, previously-undiscovered gap this
/// pilot's own `update` handling above didn't capture, found by
/// comparing this pilot's own output against the real, installed system
/// `emerge` on a real package (`sys-apps/portage`) and tracing real
/// portage's own decision live. Real portage's `avoid_update` shortcut
/// above (`!update`, ported as `update` here) is NOT sufficient on its
/// own for a **directly-requested (top-level) atom**: real
/// `_wrapped_select_pkg_highest_available_imp`'s own per-candidate loop
/// (`lib/_emerge/depgraph.py`) computes `want_reinstall = reinstall or
/// empty or (found_available_arg and not selective)`, and `if
/// want_reinstall and matched_packages: continue` -- for a "found via an
/// atom on the command line" (`found_available_arg`, real
/// `_iter_atoms_for_pkg`) candidate, this SKIPS ever re-adding the
/// already-installed `Package` object as a further candidate at all
/// whenever `selective` is absent, so the later `if avoid_update: ...
/// return pkg` shortcut (`lib/_emerge/depgraph.py` line ~8447) finds
/// nothing installed to return and falls through to picking the best
/// *available* (ebuild) candidate instead -- even when its version is
/// identical to what's already installed. The net real effect: a bare
/// `emerge <atom>` with no other flags, on an atom named directly (not
/// reached via a dependency string), always resolves against the best
/// *available* version (searching for a newer one exactly as `--update`
/// would), and even when nothing newer exists, still reports a bare
/// reinstall (real `[ebuild R] cat/pkg-ver`, no parenthetical reason at
/// all) rather than treating the identical installed version as
/// satisfying -- confirmed live: `--noreplace`/`--selective` (both of
/// which set real `myparams["selective"]`) restore the "nothing to do"
/// result. `selective` here mirrors real `create_depgraph_params.py`'s
/// own `myparams["selective"] = True` condition, computed from
/// whichever of its own real trigger flags this pilot actually
/// implements: `update`, `newuse`, `changed_use` (real portage's own
/// `--changed-use`/`-U` rewrites to `--reinstall=changed-use` before
/// `create_depgraph_params` ever runs, `lib/_emerge/main.py`, and
/// `--reinstall` is itself constrained to that one literal choice in
/// real portage -- so `changed_use` alone covers this pilot's whole
/// share of that real condition, no separate `--reinstall` flag
/// needed), `changed_deps` (any non-`"n"` value), `changed_slot`, plus
/// the two flags whose *entire* real effect is exactly this (see
/// `pretend.rs`'s own CLI parsing): `--noreplace`/`-n` and
/// `--selective[=y|n]` (`n` explicitly cancels `selective` even if one
/// of the other conditions set it, matching real `create_depgraph_
/// params.py`'s own `if myopts.get("--selective") == "n"`: pop
/// unconditionally). Real `--newrepo` (forces reinstall specifically on
/// an installed-vs-current repo mismatch, and separately contributes to
/// `selective`) is a documented, narrower scope cut, deliberately not
/// modeled: this pilot has no vdb `REPOSITORY` reader (confirmed absent
/// during this same investigation -- the real vdb file is even
/// lowercase `repository`, unlike every other metadata key), the same
/// "no vdb-metadata reader" simplification already documented elsewhere
/// in this crate (e.g. `enqueue_dependencies`'s own doc comment).
///
/// `is_top_level` is this pilot's own existing "argument" equivalent --
/// `resolve_pretend_graph`'s own `depth == 0`, the identical
/// equivalence already established for `--with-test-deps`'s own
/// `pkg.depth == 0 and self._is_argument(pkg)` gating -- since every
/// depth-0 atom here already came from `atoms` itself or a `@world`/
/// `@system` expansion of it, both of which real portage's own
/// `_iter_atoms_for_pkg` also counts as an "argument" for this exact
/// purpose. A dependency atom (`is_top_level = false`) is NEVER affected
/// by `selective` at all -- real `found_available_arg` is only ever set
/// for an argument-derived candidate in the first place, so a
/// dependency atom's own already-installed, still-satisfying version
/// keeps exactly its pre-existing `AlreadyInstalled` treatment,
/// unconditionally, matching real `_want_installed_pkg`'s own `return
/// not arg` fallback (empty `arg` for a non-argument package).
///
/// Applied at both places this function can otherwise decide an
/// installed version satisfies the atom "as is": the `!update`
/// shortcut immediately below (skipped entirely -- not just its
/// outcome adjusted -- whenever `is_top_level && !selective`, so
/// version selection also falls through to the ordinary "best across
/// everything visible" comparison below, exactly reproducing real
/// portage's own "searches for a newer version even without `--update`"
/// effect for this case) and the final "best visible candidate happens
/// to already be installed" comparison further down (where, instead,
/// the outcome is forced to `Reinstall` -- with whatever
/// `changed_flags`/`deps_changed`/`slot_changed` were independently
/// computed, all three possibly still empty/false, exactly matching
/// real portage's own bare, reasonless `[ebuild R]`).
///
/// `excluded` (`--exclude`/`-X`) is a list of raw atom/wildcard-atom
/// strings (real `WildcardPackageSet`, ported here as the same "try
/// `match_from_list`, fall back to `parse_wildcard_atom`" two-tier
/// matcher `matches_config_entry` already uses for `package.mask`/
/// `.unmask` -- both are real portage atom-set matchers with the
/// identical "plain atom or `*`-wildcard" grammar). Checked in two
/// places, mirroring real depgraph.py's own scattered
/// `excluded_pkgs.findAtomForPackage` call sites: (1) if an installed
/// version matches both `atom_str` and an exclude atom, it's returned
/// as `AlreadyInstalled` immediately, before `update`/`newuse`/
/// `changed_use` ever get a say -- ported from `_want_update_pkg`'s and
/// `_replace_installed_atom`'s own excluded-check-first pattern (real
/// portage: "the user has not explicitly requested for this package to
/// be replaced", so excluding it means never touching it, full stop);
/// (2) an excluded candidate is never eligible to be selected as the
/// New/Upgrade "best visible candidate" either, mirroring the
/// `excluded_pkgs`-gated candidate-selection loops elsewhere in
/// depgraph.py (e.g. around its own lines 2331 and 5544) -- if every
/// remaining candidate for this atom is excluded and none is already
/// installed, this resolves to `NoVisibleCandidate`, the same outcome
/// this pilot already gives an atom with no eligible candidate for any
/// other reason. Deliberately NOT replicated: real depgraph.py's own
/// ~18 `excluded_pkgs` call sites cover many more specific interaction
/// points (autounmask, binpkg selection, `--complete-graph`, ...) this
/// pilot doesn't implement at all -- the two checks above cover the
/// dominant real-world use ("pin an installed package so `--update`/
/// `--deep` never touch it") and the New/Upgrade selection case, not
/// every real edge case.
/// Whether every atom in `atoms` currently has a satisfying candidate --
/// the probe `use_reduce_flat_disjunctive` (portage-use-reduce) needs to
/// pick a `"||"` group's own first currently-resolvable alternative
/// (see `resolve_pretend_graph`'s own doc comment for the full
/// grounding). A blocker atom (`!foo/bar`/`!!foo/bar`) is always
/// satisfiable here, vacuously -- it isn't a dependency to *resolve* at
/// all, just a conflict to report (`enqueue_flat_deps` handles that
/// separately, unaffected by which `"||"` alternative was chosen), so
/// it never disqualifies an otherwise-fine alternative.
///
/// Deliberately the *early* half of `resolve_pretend`'s own logic only
/// (`list_candidates` -> filter `is_visible` -> `match_from_list` ->
/// USE-dep post-filter) -- not a call to `resolve_pretend` itself,
/// which also applies `--update`/`--newuse`/`--exclude`/reinstall
/// refinements that only matter once an alternative has already been
/// chosen and is actually being resolved, not for this "is it even
/// possible" probe. A minor, deliberate duplication of that logic
/// rather than a shared refactor -- the same "duplicate a small,
/// stable chunk rather than force two different call sites through one
/// function" precedent `keyword_masked_only` already set against
/// `is_visible` itself.
fn atom_currently_satisfiable(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return true;
    }
    let Ok(candidates) = list_candidates(repos, &atom.category, &atom.package) else {
        return false;
    };
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if visible.is_empty() {
        return false;
    }

    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &candidate_str_refs) else {
        return false;
    };

    let Some(use_deps) = atom.use_deps.as_ref().filter(|d| !d.is_empty()) else {
        return !matched.is_empty();
    };
    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }
    matched.into_iter().any(|m| {
        let Some(candidate) = by_str.get(m) else {
            return false;
        };
        let Some((iuse, use_flags)) =
            candidate_iuse_and_use(candidate, &atom.category, &atom.package, config)
        else {
            return false;
        };
        portage_dep::use_deps_satisfied(use_deps, &iuse, &use_flags)
    })
}

// 11 args trips clippy::too_many_arguments; a bundled options struct
// would touch every one of this function's own call sites (production
// and test) for a single-slice-sized addition of one more CLI flag
// alongside six already threaded the same way -- not worth it, same
// reasoning as resolve_pretend_graph's own identical allow below.
#[allow(clippy::too_many_arguments)]
pub fn resolve_pretend(
    repos: &[RepoConfig],
    root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
    newuse: bool,
    changed_use: bool,
    update: bool,
    excluded: &[String],
    changed_deps: bool,
    with_bdeps: bool,
    changed_slot: bool,
    selective: bool,
    is_top_level: bool,
) -> Result<PretendOutcome, String> {
    let atom =
        portage_dep::parse_atom(atom_str).ok_or_else(|| format!("invalid atom {atom_str:?}"))?;

    let candidates = list_candidates(repos, &atom.category, &atom.package)?;
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if visible.is_empty() {
        return Ok(PretendOutcome::NoVisibleCandidate);
    }

    // Reuses portage-dep's already-verified match_from_list rather than
    // re-deriving version/slot matching rules here; candidate strings are
    // round-tripped back to their Candidate via `by_str` below.
    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(atom_str, &candidate_str_refs)
        .ok_or_else(|| format!("invalid atom {atom_str:?}"))?;

    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }

    // USE deps (`dev-libs/foo[bar]`/`[-bar]`, `(+)`/`(-)` defaults --
    // PMS 8.3.4): a post-filter on top of match_from_list's own version/
    // slot/repo matching, exactly where real portage's own
    // `match_from_list` applies its equivalent USE-dep post-pass too --
    // see `use_deps_satisfied`'s own doc comment (portage-dep) for the
    // ported algorithm and why match_from_list itself doesn't do this.
    // Each surviving candidate's own current-tree IUSE/effective-USE
    // (`candidate_iuse_and_use`) decides it -- a candidate whose own
    // metadata can't even be read is dropped, same "can't tell, so
    // exclude" precedent `reinstall_flags_for_use_change` already sets.
    let matched: Vec<&str> = match &atom.use_deps {
        Some(use_deps) if !use_deps.is_empty() => matched
            .into_iter()
            .filter(|m| {
                let Some(candidate) = by_str.get(m) else {
                    return false;
                };
                let Some((iuse, use_flags)) =
                    candidate_iuse_and_use(candidate, &atom.category, &atom.package, config)
                else {
                    return false;
                };
                portage_dep::use_deps_satisfied(use_deps, &iuse, &use_flags)
            })
            .collect(),
        _ => matched,
    };

    let installed = installed_versions(root, &atom.category, &atom.package);

    // --exclude/-X: an installed version matching an exclude atom is
    // left exactly as-is, unconditionally, before --update/--newuse/
    // --changed-use ever get a say -- see this function's own doc
    // comment.
    if !excluded.is_empty() {
        if let Some(installed_best) = matched
            .iter()
            .filter_map(|m| by_str.get(m).copied())
            .filter(|c| installed.iter().any(|v| v == &c.version))
            .max_by(|a, b| {
                vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
            })
        {
            let installed_str = format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category,
                atom.package,
                installed_best.version,
                installed_best.slot,
                installed_best.sub_slot,
                installed_best.repo_name
            );
            if excluded
                .iter()
                .any(|ex| matches_config_entry(ex, &installed_str, &atom.category, &atom.package))
            {
                return Ok(PretendOutcome::AlreadyInstalled {
                    version: installed_best.version.clone(),
                });
            }
        }
    }

    // --update/-u: see this function's own doc comment. Skipped
    // entirely (not just its outcome adjusted) for a top-level atom
    // without `selective` -- see the doc comment's own `selective`/
    // `is_top_level` paragraph -- so version selection falls through to
    // the ordinary best-visible-candidate comparison below too.
    if !update && (!is_top_level || selective) {
        if let Some(installed_best) = matched
            .iter()
            .filter_map(|m| by_str.get(m).copied())
            .filter(|c| installed.iter().any(|v| v == &c.version))
            .max_by(|a, b| {
                vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
            })
        {
            let changed_flags = if newuse || changed_use {
                reinstall_flags_for_use_change(
                    root,
                    &atom.category,
                    &atom.package,
                    installed_best,
                    config,
                    newuse,
                )
                .unwrap_or_default()
            } else {
                Vec::new()
            };
            let deps_changed_flag = changed_deps
                && deps_changed(
                    root,
                    repos,
                    &atom.category,
                    &atom.package,
                    &installed_best.version,
                    with_bdeps,
                );
            let slot_changed_flag = changed_slot
                && slot_changed(
                    root,
                    repos,
                    &atom.category,
                    &atom.package,
                    &installed_best.version,
                );
            if !changed_flags.is_empty() || deps_changed_flag || slot_changed_flag {
                return Ok(PretendOutcome::Reinstall {
                    version: installed_best.version.clone(),
                    changed_flags,
                    deps_changed: deps_changed_flag,
                    slot_changed: slot_changed_flag,
                });
            }
            return Ok(PretendOutcome::AlreadyInstalled {
                version: installed_best.version.clone(),
            });
        }
    }

    // Ties on identical version (possible once more than one repo can
    // provide it) are broken toward the higher-priority repo, matching
    // real portage's own `(pkg.version, repo.priority)` sort in
    // `portdbapi.cp_list`.
    // --exclude/-X: an excluded candidate is never eligible to become
    // the New/Upgrade "best visible candidate" either -- see this
    // function's own doc comment. Any already-installed match was
    // already handled (and returned) above, so nothing here can
    // silently drop an installed-and-excluded version -- only a
    // not-yet-installed one can end up filtered out entirely.
    let Some(best) = matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .filter(|c| {
            let candidate_str = format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            );
            !excluded
                .iter()
                .any(|ex| matches_config_entry(ex, &candidate_str, &atom.category, &atom.package))
        })
        .max_by(|a, b| {
            vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
        })
    else {
        return Ok(PretendOutcome::NoVisibleCandidate);
    };

    if installed.iter().any(|v| v == &best.version) {
        let changed_flags = if newuse || changed_use {
            reinstall_flags_for_use_change(
                root,
                &atom.category,
                &atom.package,
                best,
                config,
                newuse,
            )
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        let deps_changed_flag = changed_deps
            && deps_changed(
                root,
                repos,
                &atom.category,
                &atom.package,
                &best.version,
                with_bdeps,
            );
        let slot_changed_flag =
            changed_slot && slot_changed(root, repos, &atom.category, &atom.package, &best.version);
        // `is_top_level && !selective`: real portage's own bare,
        // reasonless `[ebuild R]` -- see this function's own doc
        // comment's `selective`/`is_top_level` paragraph. `changed_flags`/
        // `deps_changed_flag`/`slot_changed_flag` may all still be
        // empty/false here; that's the whole point of this case.
        if !changed_flags.is_empty()
            || deps_changed_flag
            || slot_changed_flag
            || (is_top_level && !selective)
        {
            return Ok(PretendOutcome::Reinstall {
                version: best.version.clone(),
                changed_flags,
                deps_changed: deps_changed_flag,
                slot_changed: slot_changed_flag,
            });
        }
        return Ok(PretendOutcome::AlreadyInstalled {
            version: best.version.clone(),
        });
    }

    match installed.iter().max_by(|a, b| vercmp_ordering(a, b)) {
        Some(current) => Ok(PretendOutcome::Upgrade {
            from: current.clone(),
            to: best.version.clone(),
        }),
        None => Ok(PretendOutcome::New {
            version: best.version.clone(),
        }),
    }
}

/// A blocker atom (from a package's own dependency strings) that matches
/// either a currently-installed package or another package this same
/// `resolve_pretend_graph` run would also newly merge/upgrade. Purely
/// informational (see `resolve_pretend_graph`'s doc comment): v1 makes no
/// attempt to resolve or refuse anything on account of a blocker, the
/// same "report, don't enforce" spirit as `PretendOutcome::NoVisibleCandidate`
/// for an unresolvable dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerConflict {
    /// The raw blocker atom text, e.g. `"!!dev-libs/foo"`.
    pub atom_str: String,
    /// `true` for a strong (`!!`) blocker, `false` for a weak (`!`) one.
    pub strong: bool,
    pub matched_category: String,
    pub matched_package: String,
    pub matched_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEntry {
    pub category: String,
    pub package: String,
    pub outcome: PretendOutcome,
    /// Only ever non-empty for `New`/`Upgrade` entries -- an entry's own
    /// DEPEND/RDEPEND (and therefore its blockers) are only read when it
    /// would newly merge or upgrade, same as dependency recursion itself.
    pub blockers: Vec<BlockerConflict>,
    /// `Some(slot)` only for `New`/`Upgrade` entries (the ones a slot can
    /// meaningfully be tracked for -- see `resolve_pretend_graph`'s doc
    /// comment on multi-slot support), `None` for `AlreadyInstalled`/
    /// `NoVisibleCandidate`.
    pub slot: Option<String>,
    /// This package's own IUSE-declared flags (default markers stripped),
    /// each paired with whether `effective_use_flags` resolved it enabled
    /// -- alphabetically sorted, matching real `--pretend -v`'s own
    /// ordering. Only ever non-empty for `New`/`Upgrade` entries whose
    /// md5-cache metadata was readable, same as `blockers`. Always
    /// computed regardless of `--verbose` (cheap; the CLI layer decides
    /// whether to print it) -- see pretend.rs.
    pub use_flags_display: Vec<(String, bool)>,
    /// Every `(category, package)` that reached this entry via its own
    /// DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND (sorted, deduplicated) --
    /// empty for a directly-requested top-level atom with no other
    /// owner. A package required by more than one parent (a diamond
    /// dependency) lists every one of them, not just whichever reached
    /// it first -- tracked separately from the BFS's own dedup/
    /// recursion decisions (`visited_atoms`/`resolved_slots`/
    /// `other_outcomes`), which only ever decide whether to *resolve*
    /// an atom again, never whether to *record* who asked for it. See
    /// `resolve_pretend_graph`'s own doc comment.
    pub required_by: Vec<(String, String)>,
}

/// A blocker atom found while flattening one package's own dependency strings,
/// not yet matched against anything -- collected during the BFS in
/// `resolve_pretend_graph` and resolved in a single post-pass (see
/// `resolve_blockers`) once the whole graph's New/Upgrade set is known, so
/// a match doesn't depend on BFS discovery order (two packages can block
/// each other regardless of which one the queue reaches first).
struct PendingBlocker {
    atom_str: String,
    strong: bool,
    target_category: String,
    target_package: String,
    owner_key: (String, String),
    owner_version: String,
}

/// Matches each `pending` blocker's target `category/package` against
/// both currently-installed candidates (`installed_candidates`, sub-slot
/// included -- see `Candidate::sub_slot`'s own doc comment) and this
/// graph's own resolved New/Upgrade set (`entries`, which may now hold
/// more than one slot for the same category/package -- every one of them
/// is a real candidate, not just the first), reusing
/// `portage_dep::match_from_list` exactly as every other atom-vs-candidate
/// check in this crate does (it ignores an atom's `blocker` field
/// entirely, so a `!`/`!!`-prefixed atom string matches candidates by
/// category/package/version/slot exactly like a normal one). A match
/// against the owner package's own resolved version is dropped
/// defensively (a package blocking itself is nonsensical, but cheap to
/// guard against). Returns `(owner_key, conflict)` pairs rather than
/// mutating `entries` directly, since `entries` is still needed
/// immutably to build the graph-resolved candidate list here.
///
/// `entries`' own contribution has no real sub-slot data at all --
/// `GraphEntry::slot` deliberately stays main-slot-only for now (a
/// documented, narrower scope cut than `Candidate::sub_slot`'s own repo/
/// vdb-backed fix), so it defaults sub-slot to the main slot itself,
/// the same fallback `split_slot` already uses for a plain (no `/`)
/// `SLOT` value -- "unknown" and "not yet split from an unslashed SLOT"
/// look identical here, and both mean "assume it matches the slot".
fn resolve_blockers(
    root: &Path,
    pending: &[PendingBlocker],
    entries: &[GraphEntry],
) -> Vec<((String, String), BlockerConflict)> {
    let mut conflicts = Vec::new();
    for pb in pending {
        let target_key = (pb.target_category.clone(), pb.target_package.clone());
        let mut candidates = installed_candidates(root, &pb.target_category, &pb.target_package);
        for entry in entries {
            if entry.category != pb.target_category || entry.package != pb.target_package {
                continue;
            }
            let version = match &entry.outcome {
                PretendOutcome::New { version } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                _ => None,
            };
            let (Some(version), Some(slot)) = (version, entry.slot.clone()) else {
                continue;
            };
            if !candidates
                .iter()
                .any(|(v, s, _ss)| *v == version && *s == slot)
            {
                candidates.push((version, slot.clone(), slot));
            }
        }
        let candidate_strs: Vec<String> = candidates
            .iter()
            .map(|(v, s, ss)| format!("{}/{}-{v}:{s}/{ss}", pb.target_category, pb.target_package))
            .collect();
        let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
        let Some(matched) = portage_dep::match_from_list(&pb.atom_str, &refs) else {
            continue;
        };
        let by_str: HashMap<&str, &(String, String, String)> = candidate_strs
            .iter()
            .map(String::as_str)
            .zip(candidates.iter())
            .collect();
        for m in matched {
            let Some((version, _slot, _sub_slot)) = by_str.get(m).copied() else {
                continue;
            };
            if target_key == pb.owner_key && *version == pb.owner_version {
                continue;
            }
            conflicts.push((
                pb.owner_key.clone(),
                BlockerConflict {
                    atom_str: pb.atom_str.clone(),
                    strong: pb.strong,
                    matched_category: pb.target_category.clone(),
                    matched_package: pb.target_package.clone(),
                    matched_version: version.clone(),
                },
            ));
        }
    }
    conflicts
}

/// A genuine slot conflict: two different dependency atoms, both landing
/// on the same category/package/slot, whose independently-resolved
/// "best" versions are incompatible -- the atom that reached this slot
/// *second* doesn't actually accept the version the *first* one already
/// caused to be resolved (and recursed into). This is distinct from two
/// atoms simply requesting *different* slots of the same package (e.g.
/// `dev-lang/python:3.11` and `dev-lang/python:3.12`), which real portage
/// -- and this pilot, see `resolve_pretend_graph`'s doc comment -- treats
/// as normal, valid coexistence, not a conflict at all.
///
/// Purely informational, the same "report, don't enforce" spirit as
/// `BlockerConflict`: real portage's own depgraph treats an unresolved
/// slot conflict as fatal and refuses to proceed; this pilot instead
/// reports it and keeps going (using whichever version was resolved
/// first), consistent with the rest of this follow-up series --
/// `--pretend` itself never touches anything real, so nothing here is
/// truly "fatal" to calculate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflict {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The version already resolved (and recursed into) for this slot,
    /// by whichever atom reached it first.
    pub resolved_version: String,
    /// The atom text that does NOT accept `resolved_version`.
    pub conflicting_atom: String,
}

/// `--changed-deps-report`: an installed package, still in the graph at
/// `version`, whose vdb-recorded dependency strings differ from the
/// repo's current ebuild for that exact version (`deps_changed`) -- but
/// reported, not reinstalled (see `resolve_pretend_graph`'s own doc
/// comment). `repo_name` is the repo that currently provides `version`,
/// standing in for real `pkg.repo` (this pilot has no vdb `REPOSITORY`
/// reader to know what the *installed* copy's own repo was) -- real
/// `_changed_deps_report`'s own `if pkg.repo != ebuild.repo: continue`
/// filter requires the two to already be equal before a package is even
/// collected, so using the ebuild's repo here loses no real cases,
/// consistent with every other "no vdb-metadata reader" simplification
/// already documented in this crate (e.g. `enqueue_dependencies`'s own
/// doc comment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedDepsReportEntry {
    pub category: String,
    pub package: String,
    pub version: String,
    pub repo_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphResult {
    pub entries: Vec<GraphEntry>,
    pub slot_conflicts: Vec<SlotConflict>,
    pub changed_deps_report: Vec<ChangedDepsReportEntry>,
}

/// `--deep`/`-D` (real `lib/_emerge/main.py`'s own `"--deep": valid_integers`
/// declaration, `create_depgraph_params.py`'s `myparams["deep"]`, and
/// `depgraph.py`'s own `_too_deep`/`_add_pkg` combination): how far past
/// an already-installed, already-satisfied package to keep walking
/// dependencies. Real portage's own default (`deep` absent from
/// `myparams` entirely, since `create_depgraph_params.py` only sets it
/// when `--deep`'s own value is present and non-zero) means an
/// AlreadyInstalled package's own further dependencies are *never*
/// walked, at any depth -- exactly this pilot's own pre-existing,
/// hardcoded behavior (see `resolve_pretend_graph`'s own doc comment,
/// "A package's dependencies are only walked if..."), which is why
/// `NotRequested` needed no new code of its own to stay correct. A bare
/// `--deep` stores real Python `True` (unlimited depth); `--deep=N`
/// stores the integer `N`, rejected by real `parser.error()` if negative
/// -- `--deep=0` parses fine but is indistinguishable from not passing
/// `--deep` at all, since `create_depgraph_params.py`'s own `!= 0` check
/// excludes it from `myparams` either way (`Bounded(0)` is never
/// constructed here for the same reason -- see the CLI parsing in
/// `multicall/src/pretend.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Deep {
    #[default]
    NotRequested,
    Unlimited,
    Bounded(u32),
}

impl Deep {
    /// Whether an already-installed, already-satisfied package sitting at
    /// `depth` (0 for a directly-requested top-level atom) should have
    /// its own further dependencies walked. Mirrors real depgraph.py's
    /// `recurse = deep is True or not self._too_deep(self._depth_increment(depth, n=1))`:
    /// `NotRequested` (`deep=0`) is never satisfied, regardless of depth;
    /// `Unlimited` (`deep is True`) always is; `Bounded(n)` is satisfied
    /// while `depth < n`, so a dependency discovered this way lands at
    /// `depth + 1 <= n`.
    fn recurses_at(self, depth: u32) -> bool {
        match self {
            Deep::NotRequested => false,
            Deep::Unlimited => true,
            Deep::Bounded(n) => depth < n,
        }
    }
}

/// One BFS-queued dependency-walk item: the atom text, its own depth
/// (see `Deep`), and the `(category, package)` that pushed it, if any
/// (`None` for a directly-requested top-level atom) -- the latter only
/// consulted by `resolve_pretend_graph`'s own `required_by_map`, for
/// `GraphEntry::required_by`.
type QueueItem = (String, u32, Option<(String, String)>);

/// Queues every atom in `flat_deps` (a `use_reduce_flat`/
/// `use_reduce_flat_subset` result) onto `queue` at `depth + 1`, owned by
/// `key`/`version`, splitting off a blocker atom into `pending_blockers`
/// instead -- shared by `resolve_pretend_graph`'s own normal-deps queueing
/// and its `--with-test-deps` follow-up below, so the two can't drift
/// apart on blocker handling or `depth`/`owner` bookkeeping.
fn enqueue_flat_deps(
    flat_deps: Vec<String>,
    key: &(String, String),
    version: &str,
    depth: u32,
    queue: &mut VecDeque<QueueItem>,
    pending_blockers: &mut Vec<PendingBlocker>,
) {
    for tok in flat_deps {
        if tok == "||" {
            continue;
        }
        if let Some(dep_atom) = portage_dep::parse_atom(&tok) {
            if dep_atom.blocker != portage_dep::Blocker::None {
                pending_blockers.push(PendingBlocker {
                    atom_str: tok,
                    strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                    target_category: dep_atom.category,
                    target_package: dep_atom.package,
                    owner_key: key.clone(),
                    owner_version: version.to_string(),
                });
                continue;
            }
        }
        queue.push_back((tok, depth + 1, Some(key.clone())));
    }
}

/// Recursively resolves every atom in `atoms` and -- for packages that
/// would newly merge or upgrade -- its DEPEND+RDEPEND+BDEPEND+PDEPEND+
/// IDEPEND atoms, breadth-first. Returns one `GraphEntry` per distinct
/// category/package/slot combination visited, in discovery order (not
/// topologically sorted): unlike a package name alone, two *different*
/// slots of the same package are both real, independent entries (each
/// gets its own recursion into its own dependency strings) -- mirroring
/// how real portage genuinely allows
/// multiple slots of the same package to coexist in one merge list (the
/// entire point of `SLOT`, e.g. `dev-lang/python:3.11` and
/// `dev-lang/python:3.12` side by side). A *conflict* only exists when
/// two atoms need the identical slot at incompatible versions -- see
/// `SlotConflict`.
///
/// `atoms` seeds the BFS queue together, in the order given, before any
/// dependency is ever pushed -- so all of them are dequeued and resolved
/// first (level-order guarantee), and the existing visited-atom/
/// resolved-slot/blocker bookkeeping below, already keyed by atom text or
/// `(category, package, slot)` rather than by "the one root", handles
/// sharing between them for free: a dependency common to two requested
/// atoms dedupes exactly like a diamond dependency does, and a slot
/// conflict between two top-level atoms (not just between two deps) is
/// now detected too. A top-level atom with no visible candidate is fatal
/// to the whole call (see the `NoVisibleCandidate` check below, matching
/// real portage's own `depgraph.py` "there are no ebuilds to satisfy"
/// behavior) rather than reported-and-continued the way a *dependency's*
/// `NoVisibleCandidate` is -- confirmed with the user before implementing
/// -- and since top-level atoms are always dequeued in argv order before
/// any dependency, the first bad one aborts before any later atom
/// (top-level or not) is even attempted.
///
/// Each package's own `package.use` overrides (see `effective_use_flags`)
/// only affect how *that* package's own dependency strings are flattened --
/// they never leak into a sibling or dependency's resolution, matching
/// real portage's per-package USE. The same already-computed
/// `effective_use_flags` result, filtered down to just this package's own
/// IUSE-declared flags, is also attached to every New/Upgrade entry as
/// `GraphEntry::use_flags_display`, for `--pretend -v`'s USE="..." display
/// -- see pretend.rs.
///
/// KNOWN, DOCUMENTED SCOPE CUTS (all confirmed with the user before
/// implementing):
///   - All five real dependency-string keys are walked (DEPEND, RDEPEND,
///     BDEPEND, PDEPEND, IDEPEND), concatenated and flattened together
///     with no distinction between them: real portage's own merge
///     ordering treats these differently (BDEPEND must be satisfied on
///     the build host before compiling; PDEPEND only after this package
///     itself merges; IDEPEND only at install time) -- meaningless
///     distinctions for a `--pretend`-only pilot with no real merge
///     ordering or phase execution to begin with (see PROMPT.md's
///     "Deferred: ebuild phase execution"), so v1 treats all five as "a
///     dependency this package needs, resolve and report it" uniformly,
///     the same "report, don't enforce" simplification already applied to
///     blockers and slot conflicts below. Atoms whose *exact text*
///     repeats (e.g. a shared dependency, or a cycle) are deduped via a
///     visited-atom-text set purely to guarantee termination -- see below
///     for how repeat visits to the same resolved category/package/slot
///     are actually handled. `with_bdeps` (`--with-bdeps`, see below) is
///     the one exception to "no distinction between them": DEPEND/BDEPEND
///     specifically (never RDEPEND/PDEPEND/IDEPEND) are skipped when it's
///     `false`, but only for an AlreadyInstalled package's own dependency
///     walk under `--deep` -- see `enqueue_dependencies`'s own doc
///     comment, which is the only place that distinction is ever made.
///   - `config` (USE, ACCEPT_KEYWORDS, package.mask/.unmask/.accept_keywords)
///     is supplied by the caller (computed via `portage_profile::resolve_config`
///     -- see that crate's doc comment for what real profile/make.conf/
///     package.* mechanics are and aren't implemented) rather than being
///     read here; this crate stays decoupled from profile-parsing logic
///     even though it now depends on portage-profile for the `Config` type.
///   - `||` (any-of) groups: NOW resolved with real semantics --
///     `use_reduce_flat_disjunctive` (portage-use-reduce) picks the
///     first alternative every one of whose own atoms
///     `atom_currently_satisfiable` (below) accepts, instead of the
///     earlier v1's own "resolve every atom in the group" (an
///     over-inclusive but never-silently-wrong stopgap, back when
///     `use_reduce(flat=True)`'s own group-boundary-discarding output
///     left no reliable way to identify "the first alternative" without
///     structured, non-flat parsing -- see `portage-use-reduce`'s own
///     doc comment for the `DepNode`/`build_dep_tree` machinery this
///     reuses, originally built for the `--with-test-deps` `subset`
///     follow-up). Falls back to the old "resolve every alternative"
///     behavior when *none* is currently satisfiable, so the same
///     "never silently wrong about whether a dependency exists"
///     invariant still holds -- nothing regresses for a dependency this
///     pilot genuinely can't resolve either way. Real portage's own
///     considerably richer preference order (installed packages first,
///     backtracking on a later constraint failure) isn't ported -- this
///     pilot has no backtracking architecture at all -- just the single
///     "first currently-resolvable alternative wins" rule.
///   - A dependency atom with no visible candidate does not fail the
///     whole graph: it still gets a `GraphEntry` with
///     `PretendOutcome::NoVisibleCandidate` (so it's visible in the
///     output, not silently dropped), it's just not recursed into
///     further, matching the "best effort" spirit of the rest of this
///     pilot slice. Repeat atoms resolving to `NoVisibleCandidate` (or
///     `AlreadyInstalled`) for the same category/package are still
///     deduped to a single entry -- these two outcomes carry no slot to
///     usefully distinguish repeats by, unlike New/Upgrade.
///   - Blockers (`!foo/bar`/`!!foo/bar` tokens) are recognized and matched
///     against installed packages and other New/Upgrade entries in this
///     same graph (see `BlockerConflict`), but purely for reporting: v1
///     makes no attempt to resolve a conflict (no merge reordering, no
///     refusing to proceed) or to enforce anything -- a strong (`!!`)
///     match doesn't change the graph's outcome or exit code any
///     differently than a weak (`!`) one. This matches real `--pretend`
///     itself (which only *calculates and shows* what would happen,
///     without touching anything), and stays consistent with the
///     "unresolvable dependency doesn't fail the graph" rule above, and
///     with `SlotConflict` being reported rather than enforced too.
///   - A package's dependencies are only walked if resolving it produced
///     New, Upgrade, or (with `newuse`/`changed_use`) Reinstall; an
///     already-installed package that stays AlreadyInstalled has its
///     own dependencies presumed already satisfied, same as before
///     `--newuse` existed -- unless `deep` (`--deep`/`-D`, see `Deep`)
///     says otherwise for this particular package's own depth, in which
///     case its dependencies are walked too (via the same repo-metadata
///     lookup as any other resolved candidate, since this pilot has no
///     vdb-metadata reader of its own -- a deliberate simplification vs
///     real portage, which reads the installed copy's own recorded
///     metadata instead; see `enqueue_dependencies`'s own doc comment).
///     This also means blockers -- and slot conflicts -- are only ever
///     detected from New/Upgrade/Reinstall packages' own dependency
///     strings, and from an AlreadyInstalled package's own dependency
///     strings only when `deep` says to walk them; blockers are never
///     inspected for a `NotRequested`-depth AlreadyInstalled package,
///     same as before `--deep` existed. See
///     `reinstall_flags_for_use_change`'s own doc comment for how
///     `newuse`/`changed_use` combine.
///   - `with_test_deps` (`--with-test-deps`, real `depgraph.py`'s own
///     `_add_pkg`) additionally pulls in `test?`-gated deps for a
///     top-level atom (`depth == 0` -- this pilot's own equivalent of
///     real `pkg.depth == 0 and self._is_argument(pkg)`, since every
///     depth-0 atom here already came from `atoms` itself or a `@world`/
///     `@system` expansion of it, both of which real portage also
///     treats as "arguments" for this exact purpose) whose own IUSE
///     declares a `"test"` flag not already enabled and not use-masked
///     (global `use_mask` or a matching `package_use_mask` entry --
///     mirrors real `"test" not in pkg.use.mask"` exactly, reusing the
///     same `specificity_ordered_flags` fold `effective_use_flags`
///     itself already applies for masking). Extracted via
///     `use_reduce_flat_subset(dep_string, use_flags ∪ {"test"}, ...,
///     subset={"test"})` -- see that function's own doc comment
///     (`portage-use-reduce`) for why this needs a real nested-structure
///     `subset` filter, not just another flat pass -- and queued exactly
///     like any other dependency (same blocker extraction, same `depth +
///     1`), additive on top of the package's own normal (non-test) deps,
///     never replacing them.
///   - `nodeps` (`--nodeps`/`-O`) disables the dependency walk entirely,
///     for every entry, not just top-level atoms: only `atoms` themselves
///     are ever resolved, ported from real `create_depgraph_params.py`
///     popping `"recurse"` out of `myparams` (which depgraph.py's own
///     dependency-walk checks for and returns early without). Each
///     resolved entry's own USE display is still computed (real
///     portage's `-v` output shows a package's own USE regardless of
///     whether its dependencies get walked), but no DEPEND/RDEPEND/etc
///     is ever read, so no dependency atom is ever queued and no blocker
///     is ever collected -- blockers only ever come from a dependency
///     string in this pilot, so this falls out for free rather than
///     needing its own special case.
///   - `update` (`--update`/`-u`) is threaded uniformly to every atom this
///     BFS resolves, top-level and dependency alike, via `resolve_pretend`
///     -- see that function's own doc comment for the real
///     `avoid_update`/`dont_miss_updates` behavior it ports, same
///     whole-graph-uniform application `newuse`/`changed_use` already get
///     above. `avoid_update`/`dont_miss_updates` are themselves plain
///     `myopts` checks inside real `_wrapped_select_pkg_highest_available_imp`,
///     the one package-selection function every atom resolution (args and
///     dependencies alike) already funnels through in real portage too,
///     so this isn't a new pilot-specific simplification beyond the one
///     `newuse`/`changed_use` already made.
///   - `deep` (`--deep`/`-D`, see `Deep`'s own doc comment): gates only
///     whether an AlreadyInstalled package's own further dependencies get
///     walked. It has no effect at all on New/Upgrade/Reinstall packages
///     (already always walked, `deep` or not) and is itself ignored
///     outright when `nodeps` disables the dependency walk entirely.
///   - `with_bdeps` (`--with-bdeps`, real `create_depgraph_params.py`'s
///     own `bdeps` param): grounded against real `depgraph.py`'s own
///     `_add_pkg_dep_string` (`if pkg.built and not removal_action: ...
///     else: ignore_build_time_deps = True`) -- real portage only ever
///     drops DEPEND/BDEPEND for a package that's *already built*
///     (installed), never for one being freshly resolved from an ebuild,
///     so this has no effect on New/Upgrade/Reinstall packages either,
///     same shape as `deep` immediately above; the two combine naturally
///     since an AlreadyInstalled package's own dependencies are only ever
///     walked at all once `deep` says to. `false` is real
///     `--with-bdeps=n`; `true` covers both real `y` and the real default
///     `auto` (`create_depgraph_params.py`'s own `myparams["bdeps"] =
///     "auto"` whenever `--usepkg` isn't given, which this pilot's own
///     `--usepkg`-less CLI always satisfies) -- `depgraph.py` itself only
///     ever tests `in ("y", "auto")`, never distinguishing the two, so
///     collapsing them into one caller-facing bool loses no real
///     behavior. `--with-bdeps-auto` (the only other real lever on this
///     same `myparams["bdeps"]` value) is a documented, out-of-scope cut
///     -- see `pretend.rs`'s own module doc comment.
///   - `excluded` (`--exclude`/`-X`, see `resolve_pretend`'s own doc
///     comment) is threaded uniformly to every atom this BFS resolves,
///     top-level and dependency alike, same whole-graph-uniform
///     application every other flag above already gets -- including an
///     AlreadyInstalled package reached only via `--deep`'s own walk,
///     since that dependency atom re-enters this same BFS loop and
///     `resolve_pretend` call like any other, with no special case
///     needed.
///   - Each `GraphEntry`'s own `required_by` (which package(s), if any,
///     pulled it in via a dependency string -- pilot-specific, no real
///     portage equivalent asked for by this pilot's own `--json` output;
///     see `GraphEntry`'s own doc comment) is tracked in a separate
///     `required_by_map` accumulated throughout the BFS and merged into
///     `entries` in one pass at the end, the same "accumulate now, merge
///     once the whole graph is known" shape `pending_blockers`/
///     `resolve_blockers` already use -- deliberately independent of
///     `visited_atoms`/`resolved_slots`/`other_outcomes`'s own dedup
///     decisions, so a diamond dependency's second (deduped) owner is
///     still recorded even though it never triggers a new resolution.
///   - `selective` (see `resolve_pretend`'s own doc comment for the full
///     real `selective`/`is_top_level` grounding) is threaded uniformly
///     to every atom this BFS resolves, but its own effect only ever
///     reaches `resolve_pretend` for a top-level one: `is_top_level`
///     (`resolve_pretend`'s own new parameter) is this BFS's own
///     pre-existing `depth == 0`, passed at the one call site below --
///     the same equivalence `--with-test-deps` already established
///     between real "argument" and this pilot's own `depth == 0`.
///   - `--autounmask` (`autounmask_suggest_keywords`): a deliberately
///     narrow v1 of a considerably bigger real feature (real portage's
///     own version tracks *why* each candidate was rejected via
///     `_get_masking_status`, builds dependency-chain comments via
///     `_get_dep_chain_as_comment`, and picks specific suggested-atom
///     syntax based on whether the suggested version is the latest --
///     none of that is ported here). This pilot's own v1 only detects
///     the single "masked by KEYWORDS, and only KEYWORDS" case (see
///     `keyword_masked_only`'s own doc comment) for a top-level atom's
///     own fatal `NoVisibleCandidate`, and appends a pilot-specific
///     summary line naming the best such candidate and its own
///     suggested keyword (see `suggested_keyword`'s own doc comment) --
///     not real portage's own exact suggested-atom syntax or comment-
///     chain formatting, the same "pilot-specific summary, not a port
///     of real formatting" precedent already established for
///     REQUIRED_USE's own error message. Deliberately still out of
///     scope: package.mask/license/USE suggestions (real
///     `--autounmask-keep-masks`/`-license`/`-use`), suggestions for a
///     *dependency's* own `NoVisibleCandidate` (matching the same
///     "only a top-level atom's own `NoVisibleCandidate` is fatal, so
///     only it gets a suggestion attached" scope this function's own
///     top-level-fatal check already draws), and any actual mutation
///     (`--autounmask-write`) -- report only, matching this pilot's own
///     "report, don't enforce" spirit throughout.
///
///     `autounmask_suggest_keywords` itself is the caller's own
///     already-resolved on/off decision (computed in `pretend.rs`),
///     grounded against real `create_depgraph_params.py`'s own
///     `autounmask`/`autounmask_keep_keywords` default-resolution logic,
///     simplified since this pilot's v1 never reads
///     `--autounmask-use`/`-license` at all (confirmed: with those two
///     always unset, real `create_depgraph_params.py`'s own "is
///     autounmask itself enabled" branch always takes its "yes" arm
///     regardless, so this is a faithful simplification for exactly the
///     scope this pilot supports, not a shortcut around it):
///     `autounmask` itself defaults to enabled, off only via explicit
///     `--autounmask=n`; `autounmask_keep_keywords` (real: "suppress
///     keyword suggestions") defaults to suppressed (kept) when
///     `--autounmask` itself was never explicitly given at all, but
///     defaults to *not* suppressed once `--autounmask` itself WAS
///     explicitly given (any value) -- real portage's own "explicitly
///     asking for autounmask implies wanting its keyword suggestions
///     too, but the ambient always-on default doesn't" asymmetry.
///     Either way, an explicit `--autounmask-keep-keywords=y`/`=n`
///     always wins outright. Confirmed live: `--autounmask=n` and no
///     flags at all both suppress; bare `--autounmask` and
///     `--autounmask-keep-keywords=n` alone both suggest;
///     `--autounmask --autounmask-keep-keywords=y` suppresses again.
// 14 args trips clippy::too_many_arguments; a bundled options struct
// would touch every one of this function's own call sites (production
// and test) for a single-slice-sized addition of one more CLI flag
// alongside eight already threaded the same way -- not worth it.
#[allow(clippy::too_many_arguments)]
pub fn resolve_pretend_graph(
    config_root: &Path,
    root: &Path,
    atoms: &[String],
    config: &portage_profile::Config,
    newuse: bool,
    changed_use: bool,
    nodeps: bool,
    update: bool,
    deep: Deep,
    excluded: &[String],
    with_bdeps: bool,
    changed_deps: bool,
    changed_slot: bool,
    with_test_deps: bool,
    changed_deps_report: bool,
    selective: bool,
    autounmask_suggest_keywords: bool,
) -> Result<GraphResult, String> {
    let repos = find_repos(config_root)?;

    // Only used to tell a top-level atom's own NoVisibleCandidate (fatal
    // to the whole call) apart from a dependency's (reported, not fatal)
    // -- see the doc comment above.
    let top_level: HashSet<&str> = atoms.iter().map(|s| s.as_str()).collect();

    // Guards against infinite requeuing (e.g. a dependency cycle): the
    // exact same atom *text* is only ever resolved once. This is
    // deliberately coarser than the (category, package, slot) dedup
    // below -- it exists purely for termination, not to decide whether a
    // given slot has already been fully resolved (two different atom
    // texts, e.g. a bare "dev-libs/foo" and a slotted "dev-libs/foo:1",
    // can both need to be resolved even though they'd share a visited-atom
    // check keyed any coarser than exact text).
    let mut visited_atoms: HashSet<String> = HashSet::new();
    // (category, package, slot) -> index into `entries`, for New/Upgrade
    // outcomes only. The first atom to resolve a given slot "wins" (its
    // version is what gets recursed into); every later atom landing on
    // the same slot is checked against that already-resolved version
    // (see `SlotConflict`) instead of triggering a second, independent
    // resolution.
    let mut resolved_slots: HashMap<(String, String, String), usize> = HashMap::new();
    // (category, package) -> already added an AlreadyInstalled/
    // NoVisibleCandidate entry for it. Separate from `resolved_slots`
    // since neither outcome carries a slot to usefully key repeats by.
    let mut other_outcomes: HashSet<(String, String)> = HashSet::new();

    let mut entries: Vec<GraphEntry> = Vec::new();
    // REQUIRED_USE (see the check further below, in the main BFS loop):
    // real depgraph.py's own `_add_pkg` sets
    // `_dynamic_config._required_use_unsatisfied = True` and returns 0
    // on a violation -- it does NOT abort the whole graph walk, unlike a
    // top-level atom's own `NoVisibleCandidate`. Every violation
    // encountered anywhere in the walk is collected here and the BFS
    // keeps going; the whole call only fails at the very end, once every
    // reachable candidate has had a chance to resolve (or fail) on its
    // own terms -- matching real portage's own `_unsatisfied_deps_for_
    // display` list (checked once, at the very end of the real resolve)
    // rather than this pilot's own previous "abort on the first hit"
    // shortcut.
    let mut required_use_violations: Vec<String> = Vec::new();
    let mut slot_conflicts: Vec<SlotConflict> = Vec::new();
    // `--changed-deps-report`: real `_changed_deps_pkgs` is a dict keyed
    // by the installed `Package` object, so a repeat visit to the same
    // installed category/package/version (e.g. via both a bare
    // "dev-libs/foo" and an explicit "dev-libs/foo:0" atom text, or a
    // diamond dependency) naturally collapses to one entry -- mirrored
    // here with an explicit dedup set, keyed the same way, preserving
    // first-encountered order (real dict iteration order) rather than
    // sorting.
    let mut changed_deps_report_seen: HashSet<(String, String, String)> = HashSet::new();
    let mut changed_deps_report_entries: Vec<ChangedDepsReportEntry> = Vec::new();
    // Each queued atom carries its own depth (0 for a directly-requested
    // top-level atom, parent's depth + 1 for anything reached only via a
    // dependency string) -- only consulted by `deep.recurses_at` below,
    // for deciding whether an AlreadyInstalled package's own further
    // dependencies get walked; every other outcome ignores it entirely
    // (see `Deep`'s own doc comment) -- and the `(category, package)`
    // that pushed it, if any (`None` for a directly-requested top-level
    // atom), only consulted by `required_by_map` below, for `GraphEntry`'s
    // own `required_by` field.
    let mut queue: VecDeque<QueueItem> = VecDeque::new();
    for a in atoms {
        queue.push_back((a.clone(), 0, None));
    }

    let mut pending_blockers: Vec<PendingBlocker> = Vec::new();
    // (category, package) -> every distinct owner that reached it via a
    // dependency string, accumulated separately from the BFS's own
    // dedup/recursion decisions below (`visited_atoms`/`resolved_slots`/
    // `other_outcomes`) so a diamond dependency's *second* (deduped)
    // owner still gets recorded even though it never triggers a new
    // resolution -- merged into `entries` in a single post-pass at the
    // end, mirroring `pending_blockers`/`resolve_blockers`'s own
    // "accumulate now, merge once the whole graph is known" shape.
    let mut required_by_map: HashMap<(String, String), HashSet<(String, String)>> = HashMap::new();

    while let Some((current_atom, depth, owner)) = queue.pop_front() {
        let Some(atom) = portage_dep::parse_atom(&current_atom) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        let key = (atom.category.clone(), atom.package.clone());
        if let Some(owner) = owner {
            required_by_map
                .entry(key.clone())
                .or_default()
                .insert(owner);
        }
        if !visited_atoms.insert(current_atom.clone()) {
            continue;
        }

        let outcome = resolve_pretend(
            &repos,
            root,
            &current_atom,
            config,
            newuse,
            changed_use,
            update,
            excluded,
            changed_deps,
            with_bdeps,
            changed_slot,
            selective,
            depth == 0,
        )?;

        // `--changed-deps-report`: real portage stays "completely
        // silent" whenever `--changed-deps` itself is also given (its
        // own collected `_changed_deps_pkgs` dict is discarded unread by
        // `_changed_deps_report`'s own early return in that case) -- so,
        // rather than collecting anything now and discarding it at print
        // time, this simply never bothers computing `deps_changed` at
        // all when `changed_deps` is true, an equivalent, simpler
        // no-op-preserving shortcut. Only AlreadyInstalled/Reinstall
        // outcomes name a version that's genuinely installed right now
        // (the only case `deps_changed` -- a vdb-vs-current-ebuild
        // comparison for one specific version -- is meaningful for); a
        // Reinstall here can only be for `newuse`/`changed_use`/
        // `changed_slot` (never for `changed_deps` itself, since that's
        // false in this branch), so this still fires independently of
        // those other reasons, matching real portage's own
        // freely-combinable reinstall triggers.
        if changed_deps_report && !changed_deps {
            let installed_version = match &outcome {
                PretendOutcome::AlreadyInstalled { version }
                | PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                _ => None,
            };
            if let Some(version) = installed_version {
                let dedup_key = (key.0.clone(), key.1.clone(), version.clone());
                if changed_deps_report_seen.insert(dedup_key)
                    && deps_changed(root, &repos, &key.0, &key.1, &version, with_bdeps)
                {
                    if let Ok(repo_candidates) = list_candidates(&repos, &key.0, &key.1) {
                        if let Some(c) = repo_candidates.iter().find(|c| c.version == version) {
                            changed_deps_report_entries.push(ChangedDepsReportEntry {
                                category: key.0.clone(),
                                package: key.1.clone(),
                                version,
                                repo_name: c.repo_name.clone(),
                            });
                        }
                    }
                }
            }
        }

        // A top-level atom (as opposed to a dependency reached while
        // recursing) with no visible candidate aborts the whole call --
        // matching real portage's own depgraph.py behavior for an
        // unsatisfiable target, not the "report and keep going" treatment
        // a dependency's own NoVisibleCandidate gets a few lines down.
        // Top-level atoms are always dequeued (and so reach this point)
        // in argv order, before any dependency, so this also guarantees
        // the *first* bad top-level atom is the one that aborts.
        if top_level.contains(current_atom.as_str())
            && matches!(outcome, PretendOutcome::NoVisibleCandidate)
        {
            let mut message = format!("there are no ebuilds to satisfy {current_atom:?}.");
            // --autounmask's own keyword-suggestion sub-feature (see
            // this function's own doc comment for the full on/off
            // default-resolution logic): only even attempted when
            // enabled, and only ever finds something to suggest when a
            // real candidate exists that's masked by KEYWORDS alone
            // (see `keyword_masked_only`'s own doc comment) -- a
            // candidate masked by package.mask/license/etc. too gets no
            // suggestion here, matching real portage's own "only
            // suggest a change that would actually fix it" spirit,
            // even though this pilot doesn't yet combine multiple
            // simultaneous suggestion kinds the way real portage can.
            if autounmask_suggest_keywords {
                if let Ok(candidates) = list_candidates(&repos, &atom.category, &atom.package) {
                    if let Some((candidate, keyword)) = candidates
                        .iter()
                        .filter(|c| keyword_masked_only(c, &atom.category, &atom.package, config))
                        .filter_map(|c| suggested_keyword(c).map(|k| (c, k)))
                        .max_by(|(a, _), (b, _)| {
                            vercmp_ordering(&a.version, &b.version)
                                .then(a.repo_priority.cmp(&b.repo_priority))
                        })
                    {
                        message.push_str(&format!(
                            "\nnote: {}/{}-{} exists but is masked by KEYWORDS; \
                             --autounmask-keep-keywords=n suggests adding \"{}/{} {}\" \
                             to package.accept_keywords",
                            atom.category,
                            atom.package,
                            candidate.version,
                            atom.category,
                            atom.package,
                            keyword
                        ));
                    }
                }
            }
            return Err(message);
        }

        let resolved_version = match &outcome {
            PretendOutcome::New { version } => Some(version.clone()),
            PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
            PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
            _ => None,
        };

        let Some(version) = resolved_version else {
            // AlreadyInstalled / NoVisibleCandidate: no slot to key a
            // repeat by, so dedup on category/package alone, same as v1
            // always did before slot-aware resolution existed.
            if !other_outcomes.insert(key.clone()) {
                continue;
            }
            // `--deep`: an AlreadyInstalled package's own further
            // dependencies are walked too, once `deep` allows recursion
            // at this package's own depth (see `Deep::recurses_at`'s own
            // doc comment) -- never for NoVisibleCandidate (no version to
            // look anything up by), and never when `nodeps` disables the
            // dependency walk entirely, matching every other outcome's
            // own `nodeps` handling further below.
            if let PretendOutcome::AlreadyInstalled { version } = &outcome {
                if !nodeps && deep.recurses_at(depth) {
                    enqueue_dependencies(
                        &repos,
                        &key.0,
                        &key.1,
                        version,
                        config,
                        depth + 1,
                        &mut queue,
                        &mut pending_blockers,
                        key.clone(),
                        version.clone(),
                        with_bdeps,
                    );
                }
            }
            entries.push(GraphEntry {
                category: key.0,
                package: key.1,
                outcome,
                blockers: Vec::new(),
                slot: None,
                use_flags_display: Vec::new(),
                required_by: Vec::new(),
            });
            continue;
        };

        // The resolved version may have come from any of `repos` (not
        // necessarily the main one), so re-derive which repo it actually
        // lives in -- reusing `list_candidates` rather than threading a
        // repo location back out of `PretendOutcome`, since more than one
        // repo could in principle carry the identical version, tie-broken
        // the same way `resolve_pretend` itself does.
        let Ok(repo_candidates) = list_candidates(&repos, &key.0, &key.1) else {
            continue;
        };
        let Some(resolved) = repo_candidates
            .iter()
            .filter(|c| c.version == version)
            .max_by_key(|c| c.repo_priority)
        else {
            continue;
        };
        let slot = resolved.slot.clone();
        let sub_slot = resolved.sub_slot.clone();
        let repo_location = resolved.repo_location.clone();
        let repo_name = resolved.repo_name.clone();
        let keywords = resolved.keywords.clone();

        let slot_key = (key.0.clone(), key.1.clone(), slot.clone());
        if let Some(&existing_idx) = resolved_slots.get(&slot_key) {
            // This exact category/package/slot was already resolved by
            // an earlier atom. If the current atom's own constraint
            // doesn't accept that already-resolved version, it's a real
            // slot conflict -- report it and move on, without a second,
            // independent resolution or any attempt to reconcile the two.
            let existing_version = match &entries[existing_idx].outcome {
                PretendOutcome::New { version } => version.clone(),
                PretendOutcome::Upgrade { to, .. } => to.clone(),
                PretendOutcome::Reinstall { version, .. } => version.clone(),
                _ => unreachable!("resolved_slots only ever indexes New/Upgrade/Reinstall entries"),
            };
            let existing_str = format!("{}/{}-{existing_version}:{slot}", key.0, key.1);
            let satisfied = portage_dep::match_from_list(&current_atom, &[existing_str.as_str()])
                .is_some_and(|m| !m.is_empty());
            if !satisfied {
                slot_conflicts.push(SlotConflict {
                    category: key.0,
                    package: key.1,
                    slot,
                    resolved_version: existing_version,
                    conflicting_atom: current_atom,
                });
            }
            continue;
        }
        let entry_idx = entries.len();
        resolved_slots.insert(slot_key, entry_idx);
        entries.push(GraphEntry {
            category: key.0.clone(),
            package: key.1.clone(),
            outcome,
            blockers: Vec::new(),
            slot: Some(slot.clone()),
            use_flags_display: Vec::new(),
            required_by: Vec::new(),
        });

        let pf = format!("{}-{version}", key.1);
        let Ok(metadata) = read_md5_cache(&repo_location, &key.0, &pf) else {
            continue;
        };
        let candidate_str = format!(
            "{}/{}-{version}:{slot}/{sub_slot}::{repo_name}",
            key.0, key.1
        );
        let use_flags = effective_use_flags(
            metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
            &config.use_tokens,
            &config.package_use,
            &config.package_use_force,
            &config.package_use_mask,
            &config.use_force,
            &config.use_mask,
            &config.use_stable_force,
            &config.use_stable_mask,
            &config.package_use_stable_force,
            &config.package_use_stable_mask,
            &keywords,
            &config.accept_keywords,
            &config.package_accept_keywords,
            &candidate_str,
            &key.0,
            &key.1,
        );

        // IUSE's own "+flag"/"-flag" default markers only matter for
        // resolving a flag's default when nothing else decides it --
        // already handled upstream, wherever `use_flags` itself came
        // from -- so display only needs the bare flag name, paired with
        // whatever `use_flags` (the real resolved set) says. Computed
        // (and shown by `--pretend -v`) regardless of `nodeps` below --
        // real portage's own USE display is about the package's own
        // metadata, unrelated to whether its dependencies get walked.
        // REQUIRED_USE (PMS 7.3.4/8.2): checked once, here, right after a
        // candidate is newly resolved -- real depgraph.py's own "NOTE:
        // REQUIRED_USE checks are delayed until after package selection"
        // (it's a genuine *post*-selection check, no part of matching/
        // visibility at all, unlike package.use/package.mask). A
        // violation eventually fails the whole run regardless of whether
        // this candidate was reached as a top-level atom or a dependency
        // deep in the graph -- but NOT immediately: real depgraph.py's
        // own `_add_pkg` (~line 3600) sets
        // `_dynamic_config._required_use_unsatisfied = True` and returns
        // 0 on a violation, which does NOT stop the rest of the graph
        // walk (unlike a top-level atom's own `NoVisibleCandidate`,
        // which genuinely does abort immediately). Every violation
        // anywhere in the walk is collected into
        // `required_use_violations` and the BFS keeps going -- see that
        // variable's own doc comment, near the top of this function, for
        // the full grounding and where the collected violations actually
        // get turned into this call's own `Err`. An genuinely *invalid*
        // REQUIRED_USE (the `Err(e)` branch below, e.g. referencing a
        // flag that isn't even valid IUSE) is different: real
        // `check_required_use` itself raises for that case, outside the
        // explicit `if not required_use_is_sat:` branch that the delayed
        // collection above lives in -- so this pilot keeps that one
        // immediately fatal, same as before.
        if let Some(required_use) = metadata.get("REQUIRED_USE") {
            if !required_use.trim().is_empty() {
                // Real `check_required_use` validates a referenced flag
                // against `pkg.iuse.is_valid_flag`, not a package's own
                // literal IUSE alone -- real config.py's own
                // `_get_implicit_iuse()` folds `PORTAGE_ARCHLIST`
                // (profiles/arch.list), `use.mask ∪ use.force`, and
                // literal `build`/`bootstrap` into every package's
                // effective IUSE regardless of what that package's own
                // IUSE declares. Without this, a REQUIRED_USE referencing
                // an implicit flag never mentioned in a package's own
                // IUSE (e.g. real media-libs/mesa's own REQUIRED_USE
                // referencing "x86", a valid arch.list entry that isn't
                // the profile's own active arch) spuriously fails with
                // "USE flag ... is not in IUSE" -- confirmed live against
                // the real, installed system. See `portage_profile::
                // Config::archlist`'s own doc comment for the full
                // grounding and the deliberate USE_EXPAND_HIDDEN
                // (elibc_*/kernel_*/userland_*) simplification.
                let mut iuse_set: HashSet<String> = metadata
                    .get("IUSE")
                    .map(String::as_str)
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
                    .collect();
                iuse_set.extend(config.archlist.iter().cloned());
                iuse_set.extend(config.use_mask.iter().cloned());
                iuse_set.extend(config.use_force.iter().cloned());
                iuse_set.insert("build".to_string());
                iuse_set.insert("bootstrap".to_string());
                match portage_required_use::check_required_use(required_use, &use_flags, &iuse_set)
                {
                    Ok(true) => {}
                    Ok(false) => {
                        let normalized = required_use
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        required_use_violations.push(format!(
                            "REQUIRED_USE not satisfied for {}/{}-{version}: \"{normalized}\"",
                            key.0, key.1
                        ));
                        continue;
                    }
                    Err(e) => {
                        return Err(format!(
                            "REQUIRED_USE for {}/{}-{version} is invalid: {e}",
                            key.0, key.1
                        ));
                    }
                }
            }
        }

        if let Some(iuse) = metadata.get("IUSE") {
            let mut display: Vec<(String, bool)> = iuse
                .split_whitespace()
                .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
                .map(|flag| {
                    let enabled = use_flags.contains(&flag);
                    (flag, enabled)
                })
                .collect();
            display.sort_by(|a, b| a.0.cmp(&b.0));
            entries[entry_idx].use_flags_display = display;
        }

        // `--nodeps`: real create_depgraph_params.py pops "recurse" from
        // myparams, and depgraph.py's own dependency-walk returns early
        // when "recurse" isn't in myparams -- ported here as skipping
        // this package's own DEPEND/RDEPEND/etc entirely, so nothing of
        // its is ever parsed, flattened, or queued. This also means no
        // blockers are ever collected from a `--nodeps` run (blockers
        // only ever come from a dependency string, which is never read),
        // matching real portage exactly.
        if nodeps {
            continue;
        }

        let mut depstr = String::new();
        for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
            if let Some(d) = metadata.get(dep_key) {
                depstr.push_str(d);
                depstr.push(' ');
            }
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
            &tokens,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
            &mut |atoms: &[String]| {
                atoms
                    .iter()
                    .all(|a| atom_currently_satisfiable(&repos, a, config))
            },
        ) else {
            continue;
        };
        enqueue_flat_deps(
            flat_deps,
            &key,
            &version,
            depth,
            &mut queue,
            &mut pending_blockers,
        );

        // --with-test-deps: additive on top of the normal deps just
        // queued above, never a replacement for them -- see
        // resolve_pretend_graph's own doc comment for the full gating
        // (depth == 0, "test" a valid, not-already-enabled, not-masked
        // IUSE flag) and why this needs use_reduce_flat_subset rather
        // than another plain use_reduce_flat pass.
        if with_test_deps && depth == 0 && !use_flags.contains("test") {
            let iuse_flags: HashSet<String> = metadata
                .get("IUSE")
                .map(String::as_str)
                .unwrap_or_default()
                .split_whitespace()
                .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
                .collect();
            let test_masked = config.use_mask.contains("test")
                || specificity_ordered_flags(
                    &config.package_use_mask,
                    &candidate_str,
                    &key.0,
                    &key.1,
                    HashSet::new(),
                )
                .contains("test");
            if iuse_flags.contains("test") && !test_masked {
                let mut test_uselist = use_flags.clone();
                test_uselist.insert("test".to_string());
                let subset: HashSet<String> = ["test".to_string()].into_iter().collect();
                if let Ok(test_deps) = portage_use_reduce::use_reduce_flat_subset(
                    &tokens,
                    &test_uselist,
                    portage_use_reduce::MatchMode::Normal,
                    &subset,
                ) {
                    enqueue_flat_deps(
                        test_deps,
                        &key,
                        &version,
                        depth,
                        &mut queue,
                        &mut pending_blockers,
                    );
                }
            }
        }
    }

    for entry in &mut entries {
        if let Some(owners) =
            required_by_map.remove(&(entry.category.clone(), entry.package.clone()))
        {
            let mut owners: Vec<(String, String)> = owners.into_iter().collect();
            owners.sort();
            entry.required_by = owners;
        }
    }

    resolve_blockers(root, &pending_blockers, &entries)
        .into_iter()
        .for_each(|(owner_key, conflict)| {
            if let Some(entry) = entries
                .iter_mut()
                .find(|e| (e.category.clone(), e.package.clone()) == owner_key)
            {
                entry.blockers.push(conflict);
            }
        });

    if !required_use_violations.is_empty() {
        return Err(required_use_violations.join("\n"));
    }

    Ok(GraphResult {
        entries,
        slot_conflicts,
        changed_deps_report: changed_deps_report_entries,
    })
}

/// Reads `category/package-version`'s own DEPEND+RDEPEND+BDEPEND+PDEPEND+
/// IDEPEND metadata (from whichever repo actually carries this exact
/// version) and enqueues each flattened dependency token -- into
/// `pending_blockers` if it's a blocker atom, `queue` (at `child_depth`)
/// otherwise -- exactly the same lookup-and-flatten steps
/// `resolve_pretend_graph`'s own main loop already takes for a freshly
/// resolved New/Upgrade/Reinstall candidate, factored out here so
/// `--deep`'s AlreadyInstalled walk (see `Deep`) can reuse it without
/// duplicating that logic. Silently does nothing if `version` can't be
/// found in any repo, or its md5-cache entry can't be read -- matching
/// the same tolerance the main loop already has for those cases.
///
/// Deliberate simplification: real portage reads an AlreadyInstalled
/// package's metadata from the vdb's own installed-time snapshot, not
/// the repo's *current* ebuild -- this pilot has no vdb-metadata reader
/// (`installed_versions` only checks presence, never reads DEPEND/USE/
/// etc), so this reuses the repo's current metadata for that version
/// instead, same as every other candidate lookup in this pilot already
/// does. This can only observably differ from real portage if the repo's
/// own copy of that exact version's ebuild changed since it was
/// installed (rare, and already a pre-existing gap for e.g. `--newuse`'s
/// own IUSE-diffing, not a new one introduced here).
///
/// `with_bdeps` (real `--with-bdeps`, see `resolve_pretend_graph`'s own
/// doc comment for the full grounding): when `false`, DEPEND and BDEPEND
/// are left out of the dep-key list entirely, so their own tokens are
/// never parsed, flattened, or queued -- RDEPEND/PDEPEND/IDEPEND are
/// unaffected. This is the one place that distinction is ever made:
/// `resolve_pretend_graph`'s own main loop (the New/Upgrade/Reinstall
/// path, which this function's own doc comment above says it mirrors)
/// deliberately does *not* take a `with_bdeps` parameter at all, since
/// real portage only ever drops build-time deps for an *already-built*
/// package -- see `resolve_pretend_graph`'s own doc comment.
#[allow(clippy::too_many_arguments)]
fn enqueue_dependencies(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
    config: &portage_profile::Config,
    child_depth: u32,
    queue: &mut VecDeque<QueueItem>,
    pending_blockers: &mut Vec<PendingBlocker>,
    owner_key: (String, String),
    owner_version: String,
    with_bdeps: bool,
) {
    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return;
    };
    let slot = resolved.slot.clone();
    let sub_slot = resolved.sub_slot.clone();
    let repo_location = resolved.repo_location.clone();
    let repo_name = resolved.repo_name.clone();
    let keywords = resolved.keywords.clone();

    let pf = format!("{package}-{version}");
    let Ok(metadata) = read_md5_cache(&repo_location, category, &pf) else {
        return;
    };
    let candidate_str = format!("{category}/{package}-{version}:{slot}/{sub_slot}::{repo_name}");
    let use_flags = effective_use_flags(
        metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
        &config.use_tokens,
        &config.package_use,
        &config.package_use_force,
        &config.package_use_mask,
        &config.use_force,
        &config.use_mask,
        &config.use_stable_force,
        &config.use_stable_mask,
        &config.package_use_stable_force,
        &config.package_use_stable_mask,
        &keywords,
        &config.accept_keywords,
        &config.package_accept_keywords,
        &candidate_str,
        category,
        package,
    );

    let dep_keys: &[&str] = if with_bdeps {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    };
    let mut depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = metadata.get(*dep_key) {
            depstr.push_str(d);
            depstr.push(' ');
        }
    }
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
        &tokens,
        &use_flags,
        portage_use_reduce::MatchMode::Normal,
        &mut |atoms: &[String]| {
            atoms
                .iter()
                .all(|a| atom_currently_satisfiable(repos, a, config))
        },
    ) else {
        return;
    };
    for tok in flat_deps {
        if tok == "||" {
            continue;
        }
        if let Some(dep_atom) = portage_dep::parse_atom(&tok) {
            if dep_atom.blocker != portage_dep::Blocker::None {
                pending_blockers.push(PendingBlocker {
                    atom_str: tok,
                    strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                    target_category: dep_atom.category,
                    target_package: dep_atom.package,
                    owner_key: owner_key.clone(),
                    owner_version: owner_version.clone(),
                });
                continue;
            }
        }
        queue.push_back((tok, child_depth, Some(owner_key.clone())));
    }
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

    /// These unit tests exercise portage-repo's own resolution logic in
    /// isolation, independent of real profile/make.conf/package.* parsing
    /// (that's portage-profile's job, tested separately) -- so visibility
    /// is pinned to a fixed "amd64"-only, no-overrides config here,
    /// matching what the fixture's real profile chain also happens to
    /// resolve to (see PORTING/fixtures/repo/profiles and
    /// test_emerge_pretend_contract.py for the end-to-end version of
    /// this). Constructed directly (not via portage_profile::resolve_config)
    /// so these tests don't depend on real file parsing at all.
    fn test_config() -> portage_profile::Config {
        portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        }
    }

    fn resolve(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &test_config(),
            false,
            false,
            false,
            &[],
            false,
            true,
            false,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    /// Like `resolve`, but with `--update` enabled -- for the `upgrade`
    /// test below, and anywhere else `--update`'s own "search for a
    /// better version even when the installed one already satisfies the
    /// atom" behavior needs to be exercised directly.
    fn resolve_update(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &test_config(),
            false,
            false,
            true,
            &[],
            false,
            true,
            false,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    /// Like `resolve`, but with `selective=false` -- real portage's own
    /// default for a bare top-level atom with no other flags given (see
    /// `resolve_pretend`'s own `selective`/`is_top_level` doc comment
    /// paragraph). `is_top_level=true` throughout, matching a
    /// directly-requested atom.
    fn resolve_not_selective(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &test_config(),
            false,
            false,
            false,
            &[],
            false,
            true,
            false,
            false,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    #[test]
    fn not_selective_top_level_atom_reinstalls_with_no_reason_when_nothing_else_changed() {
        assert_eq!(
            resolve_not_selective("dev-libs", "samepkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
            }
        );
    }

    #[test]
    fn not_selective_top_level_atom_still_offers_a_newer_visible_version() {
        // dev-libs/upgradepkg is installed at 1.0, 2.0 is visible --
        // without `update`, real avoid_update's own shortcut never gets
        // a chance to fire for a not-selective top-level atom (the
        // installed version is never even a matched candidate to begin
        // with), so the ordinary "best across everything visible"
        // search proceeds and finds 2.0, exactly as if `update` were
        // true.
        assert_eq!(
            resolve_not_selective("dev-libs", "upgradepkg"),
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            }
        );
    }

    #[test]
    fn selective_true_preserves_the_pre_existing_already_installed_outcome() {
        // Same fixture as the reinstall test above, but selective=true
        // (via the pre-existing `resolve` helper) -- must still be
        // AlreadyInstalled, matching every test written before this
        // slice.
        assert_eq!(
            resolve("dev-libs", "samepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    /// Like `resolve_update`, but with `excluded` too -- for exercising
    /// `--exclude`'s own "leave an installed package alone regardless of
    /// --update" precedence directly.
    fn resolve_update_excluded(
        category: &str,
        package: &str,
        excluded: &[String],
    ) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &test_config(),
            false,
            false,
            true,
            excluded,
            false,
            true,
            false,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    #[test]
    fn exclude_leaves_an_already_installed_package_alone_even_with_update() {
        // dev-libs/upgradepkg is installed at 1.0, a newer 2.0 is
        // visible -- without --exclude, --update offers the upgrade
        // (see the `upgrade` test below); --exclude matching it
        // overrides --update entirely, same as real
        // _want_update_pkg's/_replace_installed_atom's own
        // excluded-checked-first precedent.
        assert_eq!(
            resolve_update_excluded(
                "dev-libs",
                "upgradepkg",
                &["dev-libs/upgradepkg".to_string()]
            ),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn exclude_matches_via_a_wildcard_atom_too() {
        // Real WildcardPackageSet accepts wildcard atoms, not just plain
        // ones -- ported here as the same two-tier matches_config_entry
        // package.mask/.unmask already uses.
        assert_eq!(
            resolve_update_excluded("dev-libs", "upgradepkg", &["dev-libs/*".to_string()]),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn exclude_does_not_affect_a_non_matching_package() {
        // A --exclude atom for an unrelated package has no effect at
        // all -- --update still offers the upgrade normally.
        assert_eq!(
            resolve_update_excluded(
                "dev-libs",
                "upgradepkg",
                &["dev-libs/does-not-exist".to_string()]
            ),
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string()
            }
        );
    }

    #[test]
    fn exclude_prevents_a_not_yet_installed_package_from_being_offered() {
        // dev-libs/newpkg has no installed version at all -- excluding
        // it means there's no eligible candidate left, same
        // NoVisibleCandidate outcome as any other unsatisfiable atom.
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        assert_eq!(
            resolve_pretend(
                &repos,
                &root,
                "dev-libs/newpkg",
                &test_config(),
                false,
                false,
                false,
                &["dev-libs/newpkg".to_string()],
                false,
                true,
                false,
                true,
                true,
            )
            .expect("resolve_pretend must succeed"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn new_install() {
        assert_eq!(
            resolve("dev-libs", "newpkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn already_installed() {
        assert_eq!(
            resolve("dev-libs", "samepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn without_update_an_installed_version_that_still_satisfies_the_atom_is_kept() {
        // dev-libs/upgradepkg is installed at 1.0; a newer 2.0 is visible
        // in the tree too. Real depgraph.py's own `avoid_update`
        // (lines 7814/8448) means plain `emerge dev-libs/upgradepkg`,
        // with no --update, never even looks for a better version --
        // see resolve_pretend's own doc comment. This was, before this
        // slice, this pilot's own (inaccurate) default behavior for
        // `upgrade` below.
        assert_eq!(
            resolve("dev-libs", "upgradepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn upgrade() {
        // Same fixture as above, but with --update: now a real Upgrade,
        // matching real depgraph.py's own `dont_miss_updates` branch.
        assert_eq!(
            resolve_update("dev-libs", "upgradepkg"),
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            }
        );
    }

    #[test]
    fn masked_by_keywords_is_not_visible() {
        assert_eq!(
            resolve("dev-libs", "maskedpkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn nonexistent_package_is_no_visible_candidate() {
        assert_eq!(
            resolve("dev-libs", "does-not-exist"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    /// Unlike `resolve()`, uses the fixture's *real* resolved config
    /// (package.mask/.unmask/.accept_keywords included), not the
    /// synthetic `test_config()`.
    fn resolve_real(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &config,
            false,
            false,
            false,
            &[],
            false,
            true,
            false,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    /// Like `resolve_real`, but with `--newuse` enabled -- for the
    /// `PretendOutcome::Reinstall` tests below.
    fn resolve_real_newuse(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &config,
            true,
            false,
            false,
            &[],
            false,
            true,
            false,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    /// Like `resolve_real`, but with `--changed-use` enabled.
    fn resolve_real_changed_use(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &config,
            false,
            true,
            false,
            &[],
            false,
            true,
            false,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    #[test]
    fn newuse_reinstalls_when_vdb_use_differs_from_current_use() {
        // dev-libs/reinstallpkg is installed at 1.0 with IUSE="foo" but an
        // empty vdb USE file (foo was off at merge time); the fixture
        // profile chain enables "foo" globally now, so --newuse must
        // report a Reinstall for the changed "foo" flag.
        assert_eq!(
            resolve_real_newuse("dev-libs", "reinstallpkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: vec!["foo".to_string()],
                deps_changed: false,
                slot_changed: false,
            }
        );
    }

    #[test]
    fn without_newuse_the_same_package_stays_already_installed() {
        // Same fixture as above, but without --newuse: the USE mismatch
        // is real but the flag that would detect it is off, so this must
        // stay the pre-existing AlreadyInstalled outcome.
        assert_eq!(
            resolve_real("dev-libs", "reinstallpkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn newuse_does_not_reinstall_when_use_has_not_changed() {
        // dev-libs/samepkg has no IUSE at all (declared or in the vdb),
        // so there's nothing for --newuse to detect a change in -- must
        // stay AlreadyInstalled even with --newuse enabled.
        assert_eq!(
            resolve_real_newuse("dev-libs", "samepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn newuse_forced_flags_suppresses_a_spurious_reinstall() {
        // dev-libs/usemaskreinstallpkg is installed with an empty vdb
        // IUSE, but its own ebuild now declares
        // IUSE="masked_newly_added_flag" -- a flag
        // PORTING/fixtures/repo/profiles/base/use.mask masks off, so
        // it's never enabled either before or after. Without
        // reinstall_flags_for_use_change's own "flags -= forced_flags"
        // step (real depgraph.py's own line, ported exactly), this
        // would incorrectly report a Reinstall just because the flag
        // now exists in IUSE at all.
        assert_eq!(
            resolve_real_newuse("dev-libs", "usemaskreinstallpkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    fn resolve_real_changed_deps(
        category: &str,
        package: &str,
        with_bdeps: bool,
    ) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &config,
            false,
            false,
            false,
            &[],
            true,
            with_bdeps,
            false,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    #[test]
    fn changed_deps_reinstalls_when_vdb_rdepend_differs_from_the_current_ebuild() {
        // dev-libs/changeddepspkg is installed with a vdb-recorded
        // RDEPEND="dev-libs/samepkg", but its current ebuild's own
        // RDEPEND is "dev-libs/newpkg" instead.
        assert_eq!(
            resolve_real_changed_deps("dev-libs", "changeddepspkg", true),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: true,
                slot_changed: false,
            }
        );
    }

    #[test]
    fn without_changed_deps_the_same_package_stays_already_installed() {
        assert_eq!(
            resolve_real("dev-libs", "changeddepspkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn changed_deps_still_fires_when_with_bdeps_is_false() {
        // The changed dependency here is RDEPEND, which --with-bdeps=n
        // never excludes (only DEPEND/BDEPEND are ever dropped) -- so
        // this must still detect the change.
        assert_eq!(
            resolve_real_changed_deps("dev-libs", "changeddepspkg", false),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: true,
                slot_changed: false,
            }
        );
    }

    fn graph_changed_deps_report(
        atom_str: &str,
        changed_deps: bool,
        changed_deps_report: bool,
    ) -> GraphResult {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            changed_deps,
            false,
            false,
            changed_deps_report,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
    }

    #[test]
    fn changed_deps_report_reports_without_reinstalling() {
        let result = graph_changed_deps_report("dev-libs/changeddepspkg", false, true);
        assert_eq!(
            result.changed_deps_report,
            vec![ChangedDepsReportEntry {
                category: "dev-libs".to_string(),
                package: "changeddepspkg".to_string(),
                version: "1.0".to_string(),
                repo_name: "testrepo".to_string(),
            }]
        );
        // Report-only: still AlreadyInstalled, never reinstalled.
        assert_eq!(
            result.entries[0].outcome,
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn changed_deps_report_is_silent_when_changed_deps_is_also_given() {
        // Real portage: "This is completely silent... if --changed-deps
        // ... is enabled" -- the Vec must stay empty even though the
        // underlying dependency change is real (changed_deps=true here
        // actually reinstalls it, proven by the second assertion).
        let result = graph_changed_deps_report("dev-libs/changeddepspkg", true, true);
        assert!(result.changed_deps_report.is_empty());
        assert_eq!(
            result.entries[0].outcome,
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: true,
                slot_changed: false,
            }
        );
    }

    #[test]
    fn changed_deps_report_is_empty_without_the_flag() {
        let result = graph_changed_deps_report("dev-libs/changeddepspkg", false, false);
        assert!(result.changed_deps_report.is_empty());
    }

    #[test]
    fn changed_deps_ignores_a_libc_only_dependency_change() {
        // dev-libs/libcnoisepkg's own vdb RDEPEND names sys-libs/glibc,
        // its current ebuild names sys-libs/musl instead -- both are
        // real virtual/libc providers (the fixture vdb's own
        // virtual/libc entry RDEPENDs on "|| ( sys-libs/glibc
        // sys-libs/musl )"), so real strip_libc_deps must strip both
        // before comparing, leaving only the identical
        // "dev-libs/samepkg" on each side -- no reinstall.
        assert_eq!(
            resolve_real_changed_deps("dev-libs", "libcnoisepkg", true),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn changed_deps_does_not_fire_for_a_package_with_no_recorded_difference() {
        // dev-libs/samepkg's own vdb has no RDEPEND file at all, and its
        // current ebuild declares none either -- both sides flatten to
        // an empty set, so no reinstall is reported even with
        // --changed-deps enabled.
        assert_eq!(
            resolve_real_changed_deps("dev-libs", "samepkg", true),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    fn resolve_real_changed_slot(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &config,
            false,
            false,
            false,
            &[],
            false,
            true,
            true,
            true,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    #[test]
    fn changed_slot_reinstalls_when_vdb_slot_differs_from_the_current_ebuild() {
        // dev-libs/changedslotpkg is installed with a vdb-recorded
        // SLOT="0", but its current ebuild's own SLOT is "0/2" instead
        // (an ABI-bump sub-slot change).
        assert_eq!(
            resolve_real_changed_slot("dev-libs", "changedslotpkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: true,
            }
        );
    }

    #[test]
    fn without_changed_slot_the_same_package_stays_already_installed() {
        assert_eq!(
            resolve_real("dev-libs", "changedslotpkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn changed_slot_does_not_fire_for_a_package_with_no_recorded_difference() {
        // dev-libs/samepkg's own vdb SLOT ("0") matches its current
        // ebuild's own SLOT exactly -- no reinstall even with
        // --changed-slot enabled.
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        assert_eq!(
            resolve_pretend(
                &repos,
                &root,
                "dev-libs/samepkg",
                &config,
                false,
                false,
                false,
                &[],
                false,
                true,
                true,
                true,
                true,
            )
            .expect("resolve_pretend must succeed"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn split_slot_defaults_sub_slot_to_the_slot_itself_when_no_slash_is_present() {
        assert_eq!(split_slot("0"), ("0".to_string(), "0".to_string()));
        assert_eq!(split_slot("0/2"), ("0".to_string(), "2".to_string()));
        assert_eq!(split_slot(""), ("0".to_string(), "0".to_string()));
    }

    #[test]
    fn list_candidates_reads_the_real_sub_slot_not_just_the_main_slot() {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let candidates =
            list_candidates(&repos, "dev-libs", "subslotpkg").expect("list_candidates");
        let c = candidates
            .iter()
            .find(|c| c.version == "1.0")
            .expect("subslotpkg-1.0 must be listed");
        assert_eq!(c.slot, "0");
        assert_eq!(c.sub_slot, "2");
    }

    #[test]
    fn resolve_pretend_matches_a_sub_slot_restricted_atom_against_the_real_sub_slot() {
        // dev-libs/subslotpkg's own SLOT is "0/2" -- an exact sub-slot
        // match, unlike a slot-operator atom (":="/"pkg:slot="), which
        // `matches_slot`'s own doc comment already established needs no
        // candidate-side sub-slot data at all to match correctly. This
        // is the plain PMS 8.3.3 "cat/pkg:slot/sub-slot" restriction
        // form instead, which DOES need it -- see `Candidate::sub_slot`'s
        // own doc comment for the bug this closes.
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let outcome = resolve_pretend(
            &repos,
            &root,
            "dev-libs/subslotpkg:0/2",
            &test_config(),
            false,
            false,
            false,
            &[],
            false,
            true,
            false,
            true,
            true,
        )
        .expect("resolve_pretend must succeed");
        assert_eq!(
            outcome,
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn resolve_pretend_rejects_a_sub_slot_restricted_atom_that_does_not_match() {
        // Same candidate (SLOT "0/2"), but the atom itself asks for
        // sub-slot "3" -- a genuine mismatch, proving this is real
        // matching and not just "always accept regardless of sub-slot".
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let outcome = resolve_pretend(
            &repos,
            &root,
            "dev-libs/subslotpkg:0/3",
            &test_config(),
            false,
            false,
            false,
            &[],
            false,
            true,
            false,
            true,
            true,
        )
        .expect("resolve_pretend must succeed");
        assert_eq!(outcome, PretendOutcome::NoVisibleCandidate);
    }

    #[test]
    fn changed_deps_and_changed_slot_combine_in_one_reinstall_outcome() {
        // dev-libs/changedslotpkg's own vdb has BOTH a stale RDEPEND
        // (samepkg, vs. the current ebuild's newpkg) and a stale SLOT
        // ("0" vs. the current ebuild's "0/2") -- real portage treats
        // --changed-deps and --changed-slot as independent, freely
        // combinable triggers, so giving both must set both fields on
        // the same Reinstall outcome, not just whichever fires first.
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        assert_eq!(
            resolve_pretend(
                &repos,
                &root,
                "dev-libs/changedslotpkg",
                &config,
                false,
                false,
                false,
                &[],
                true,
                true,
                true,
                true,
                true,
            )
            .expect("resolve_pretend must succeed"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: true,
                slot_changed: true,
            }
        );
    }

    #[test]
    fn newuse_vs_changed_use_diverge_on_a_newly_added_iuse_flag() {
        // dev-libs/changedusepkg is installed with an empty vdb IUSE,
        // and its current ebuild now declares IUSE="brandnewflag" -- a
        // real, unmasked, not-globally-enabled flag. --newuse's own
        // presence-diff term reacts to the flag simply existing in IUSE
        // now, regardless of enablement; --changed-use's own, narrower
        // formula never looks at IUSE presence at all, only at
        // enablement of flags declared on both sides -- proving the two
        // are genuinely different checks.
        assert_eq!(
            resolve_real_newuse("dev-libs", "changedusepkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: vec!["brandnewflag".to_string()],
                deps_changed: false,
                slot_changed: false,
            }
        );
        assert_eq!(
            resolve_real_changed_use("dev-libs", "changedusepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn changed_use_still_catches_an_enablement_change_on_a_shared_flag() {
        // dev-libs/reinstallpkg's own "foo" flag exists in IUSE on both
        // sides -- only its enablement changed, exactly the shared term
        // both --newuse and --changed-use compute the same way.
        assert_eq!(
            resolve_real_changed_use("dev-libs", "reinstallpkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: vec!["foo".to_string()],
                deps_changed: false,
                slot_changed: false,
            }
        );
    }

    #[test]
    fn changed_use_reinstall_still_recurses_into_its_own_dependencies() {
        // Same proof as newuse_reinstall_still_recurses_into_its_own_dependencies
        // below, but via --changed-use instead of --newuse.
        assert_eq!(
            graph_real_changed_use("dev-libs/reinstallpkg"),
            vec![
                (
                    "dev-libs/reinstallpkg".to_string(),
                    PretendOutcome::Reinstall {
                        version: "1.0".to_string(),
                        changed_flags: vec!["foo".to_string()],
                        deps_changed: false,
                        slot_changed: false,
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn fixture_overlay_only_package_is_found() {
        // dev-libs/overlayonlypkg exists only in the fixture's overlay
        // repo (see PORTING/fixtures/etc/portage/repos.conf), not the
        // main repo -- proving the overlay is actually searched, not
        // just present in repos.conf.
        assert_eq!(
            resolve_real("dev-libs", "overlayonlypkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_repo_constrained_atom_finds_the_named_repo_only() {
        // dev-libs/overlayonlypkg exists only in the fixture's overlay
        // repo (named "overlay" in repos.conf) -- "::overlay" must
        // resolve it, "::testrepo" (the main repo's own name) must not,
        // proving portage-repo's own candidate strings really do carry
        // "::reponame" end to end, not just that portage-dep can parse
        // and match it in isolation.
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        assert_eq!(
            resolve_pretend(
                &repos,
                &root,
                "dev-libs/overlayonlypkg::overlay",
                &config,
                false,
                false,
                false,
                &[],
                false,
                true,
                false,
                true,
                true,
            )
            .expect("resolve_pretend must succeed"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
        assert_eq!(
            resolve_pretend(
                &repos,
                &root,
                "dev-libs/overlayonlypkg::testrepo",
                &config,
                false,
                false,
                false,
                &[],
                false,
                true,
                false,
                true,
                true,
            )
            .expect("resolve_pretend must succeed"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_best_version_wins_regardless_of_which_repo_has_it() {
        // dev-libs/overlaynewerpkg-1.0 is in the main repo, -2.0 is in
        // the overlay -- the higher version wins even though it isn't
        // in the main (lower-priority) repo.
        assert_eq!(
            resolve_real("dev-libs", "overlaynewerpkg"),
            PretendOutcome::New {
                version: "2.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_same_version_tie_across_repos_is_broken_toward_higher_priority() {
        // dev-libs/overlaytiepkg-1.0 exists identically-versioned in both
        // the main repo (priority -1000, no deps) and the overlay
        // (priority 10, RDEPENDs on dev-libs/newpkg): resolving it must
        // pull in newpkg, proving the overlay's own copy -- not the main
        // repo's -- is the one whose metadata got read.
        let full_names: Vec<String> = graph_entries_real("dev-libs/overlaytiepkg")
            .into_iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec!["dev-libs/overlaytiepkg", "dev-libs/newpkg"]
        );
    }

    #[test]
    fn fixture_slot_conflict_is_reported_between_two_incompatible_version_constraints() {
        // dev-libs/slotconflictparent pulls in slotconflictnewconsumer
        // (bare RDEPEND on slotconflicttarget, resolves the best version,
        // 2.0, first) and slotconflictoldconsumer (RDEPEND
        // "<dev-libs/slotconflicttarget-2.0", which 2.0 itself does NOT
        // satisfy) -- both want slot 0 of the same package, at versions
        // that can't both be right, so this must surface as a
        // SlotConflict, not a second, silently-overwriting entry.
        let result = graph_result_real("dev-libs/slotconflictparent");
        let full_names: Vec<String> = result
            .entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/slotconflictparent",
                "dev-libs/slotconflictnewconsumer",
                "dev-libs/slotconflictoldconsumer",
                "dev-libs/slotconflicttarget",
            ]
        );
        assert_eq!(
            result.slot_conflicts,
            vec![SlotConflict {
                category: "dev-libs".to_string(),
                package: "slotconflicttarget".to_string(),
                slot: "0".to_string(),
                resolved_version: "2.0".to_string(),
                conflicting_atom: "<dev-libs/slotconflicttarget-2.0".to_string(),
            }]
        );
    }

    #[test]
    fn fixture_different_slots_of_the_same_package_coexist_without_conflict() {
        // dev-libs/multislotparent RDEPENDs on both
        // dev-libs/multislotpkg:0 and dev-libs/multislotpkg:1 -- real,
        // different slots of the same package are normal coexistence
        // (like dev-lang/python:3.11 and :3.12), not a conflict: both
        // must appear as independent entries, and neither is silently
        // dropped by the visited-set the way v1 always did before slot
        // tracking existed.
        let result = graph_result_real("dev-libs/multislotparent");
        let full_names: Vec<String> = result
            .entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/multislotparent",
                "dev-libs/multislotpkg",
                "dev-libs/multislotpkg",
            ]
        );
        let slots: Vec<Option<String>> =
            result.entries[1..].iter().map(|e| e.slot.clone()).collect();
        assert_eq!(slots, vec![Some("0".to_string()), Some("1".to_string())]);
        assert!(result.slot_conflicts.is_empty());
    }

    #[test]
    fn fixture_virtual_resolves_through_ordinary_category_and_any_of_machinery() {
        // virtual/texteditor is shaped exactly like a real virtual (e.g.
        // virtual/pager in the real Gentoo tree, confirmed by
        // inspection): an ordinary ebuild whose RDEPEND is a
        // "|| ( dev-libs/newpkg dev-libs/samepkg )" any-of group of real
        // providers -- no PROVIDE mechanism, no dedicated virtuals
        // resolution code anywhere in this pilot. Real "||" semantics
        // (see use_reduce_flat_disjunctive, portage-use-reduce): the
        // first alternative with a currently-satisfiable candidate wins
        // -- dev-libs/newpkg (listed first, and visible) -- so
        // dev-libs/samepkg (second, and already installed -- also
        // satisfiable, but never even reached) is correctly never
        // enqueued at all, unlike this pilot's own earlier "resolve
        // every alternative" v1.
        let entries = graph_entries_real("virtual/texteditor");
        let full_names: Vec<String> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(full_names, vec!["virtual/texteditor", "dev-libs/newpkg"]);
        assert_eq!(
            entries[1].outcome,
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_mask_hides_with_no_unmask() {
        assert_eq!(
            resolve_real("dev-libs", "hardmaskedpkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_package_unmask_cancels_the_matching_mask() {
        assert_eq!(
            resolve_real("dev-libs", "maskedandunmaskedpkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_mask_minus_atom_removal_is_unaffected() {
        // dev-libs/samepkg is masked then immediately un-masked via
        // "-dev-libs/samepkg" within package.mask itself -- it must
        // resolve completely normally (already installed), not as if it
        // were ever masked.
        assert_eq!(
            resolve_real("dev-libs", "samepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_accept_keywords_wildcard_extends_visibility() {
        assert_eq!(
            resolve_real("dev-libs", "wildcardkeywordpkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_accept_keywords_double_star_accepts_no_keywords_package() {
        assert_eq!(
            resolve_real("dev-libs", "livekeywordpkg"),
            PretendOutcome::New {
                version: "9999".to_string()
            }
        );
    }

    #[test]
    fn fixture_unrelated_masked_by_keywords_package_is_still_hidden() {
        // Regression guard: the "*/wildcardkeywordpkg" package.accept_keywords
        // entry is scoped to that package name only (not "dev-libs/*"),
        // specifically so it can't accidentally make dev-libs/maskedpkg
        // visible too.
        assert_eq!(
            resolve_real("dev-libs", "maskedpkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_eula_style_license_is_masked_by_the_real_default_accept_license() {
        // Neither the fixture profile chain nor make.conf sets
        // ACCEPT_LICENSE at all -- real portage's own "* -@EULA"
        // default applies, and profiles/base/license_groups defines
        // EULA="SomeEula", so dev-libs/eulapkg's own LICENSE="SomeEula"
        // is masked.
        assert_eq!(
            resolve_real("dev-libs", "eulapkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_any_of_license_group_is_visible_via_the_accepted_alternative() {
        assert_eq!(
            resolve_real("dev-libs", "anyoflicensepkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_license_unmasks_an_otherwise_eula_masked_package() {
        // fixtures/etc/portage/package.license accepts SomeEula for this
        // one package specifically, despite the same global "* -@EULA"
        // default that masks dev-libs/eulapkg above.
        assert_eq!(
            resolve_real("dev-libs", "packagelicensepkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_use_conditional_license_is_visible_when_the_flag_is_off() {
        assert_eq!(
            resolve_real("dev-libs", "uselicensepkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_use_conditional_license_is_masked_once_package_use_forces_the_flag_on() {
        // fixtures/etc/portage/package.use forces "nonfreeflag" on for
        // this specific package, activating its own LICENSE's
        // "nonfreeflag? ( SomeEula )" conditional -- masked by the same
        // real "* -@EULA" default as dev-libs/eulapkg.
        assert_eq!(
            resolve_real("dev-libs", "uselicensepkgforced"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_properties_default_star_accepts_a_declared_property() {
        assert_eq!(
            resolve_real("dev-libs", "propertiespkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_properties_narrows_acceptance_for_one_package() {
        // fixtures/etc/portage/package.properties revokes "interactive"
        // for dev-libs/interactivepkg specifically, despite the real
        // default ACCEPT_PROPERTIES=* that leaves dev-libs/propertiespkg
        // (above) visible.
        assert_eq!(
            resolve_real("dev-libs", "interactivepkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_package_accept_restrict_narrows_acceptance_for_one_package() {
        // fixtures/etc/portage/package.accept_restrict revokes "bindist"
        // for dev-libs/restrictedpkg specifically, same "-token narrows
        // despite a permissive global default" mechanism as
        // package.properties above.
        assert_eq!(
            resolve_real("dev-libs", "restrictedpkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    /// Regression test: a sibling package sharing a name prefix
    /// ("foo-bar" installed) must not be misread as an installed version
    /// of "foo" when scanning the vdb category directory. Without the
    /// digit-start guard in strip_version_prefix, this would wrongly
    /// report an Upgrade from a bogus "bar-2.0" pseudo-version instead of
    /// a clean New.
    #[test]
    fn sibling_package_prefix_does_not_contaminate_vdb_scan() {
        assert_eq!(
            resolve("dev-libs", "foo"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
        assert_eq!(
            resolve("dev-libs", "foo-bar"),
            PretendOutcome::AlreadyInstalled {
                version: "2.0".to_string()
            }
        );
    }

    fn graph(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph`, but with `--nodeps` enabled.
    fn graph_nodeps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            true,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph`, but with `--update` enabled -- proves `update` threads
    /// through the whole BFS, not just a top-level atom (see
    /// `resolve_pretend_graph`'s own doc comment).
    fn graph_update(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            true,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn exclude_threads_through_dependency_recursion_not_just_top_level() {
        // dev-libs/upgradepkg is reached only as a dependency of
        // dev-libs/withdeps here, never a top-level atom -- --exclude
        // must still leave it alone despite --update, proving the flag
        // threads uniformly through the whole BFS (see
        // resolve_pretend_graph's own doc comment), not just a
        // top-level atom.
        let root = fixtures_root();
        let entries: Vec<(String, PretendOutcome)> = resolve_pretend_graph(
            &root,
            &root,
            &["dev-libs/withdeps".to_string()],
            &test_config(),
            false,
            false,
            false,
            true,
            Deep::NotRequested,
            &["dev-libs/upgradepkg".to_string()],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect();
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/withdeps".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/upgradepkg".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    /// Like `graph`, but with a specific `Deep` value.
    fn graph_deep(atom_str: &str, deep: Deep) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            false,
            deep,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn deep_not_requested_never_walks_an_already_installed_packages_own_dependencies() {
        // dev-libs/deeppkg is already installed and stays AlreadyInstalled
        // (no --update); its own RDEPEND chain (-> deeppkg2 -> newpkg) is
        // never read at all without --deep, matching real portage's own
        // default (deep=0, permanently "too deep" at every depth -- see
        // `Deep::NotRequested`'s own doc comment).
        assert_eq!(
            graph_deep("dev-libs/deeppkg", Deep::NotRequested),
            vec![(
                "dev-libs/deeppkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            )]
        );
    }

    #[test]
    fn deep_unlimited_walks_the_whole_already_installed_chain() {
        // Bare --deep: unlimited depth, so both already-installed steps
        // (deeppkg -> deeppkg2) get walked, reaching deeppkg2's own
        // RDEPEND on newpkg (New).
        assert_eq!(
            graph_deep("dev-libs/deeppkg", Deep::Unlimited),
            vec![
                (
                    "dev-libs/deeppkg".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/deeppkg2".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn deep_bounded_one_walks_exactly_one_level_of_already_installed_packages() {
        // --deep=1: deeppkg (depth 0) recurses since 0 < 1, discovering
        // deeppkg2 at depth 1 -- but deeppkg2 itself does NOT recurse
        // (1 < 1 is false), so newpkg is never reached.
        assert_eq!(
            graph_deep("dev-libs/deeppkg", Deep::Bounded(1)),
            vec![
                (
                    "dev-libs/deeppkg".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/deeppkg2".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    fn graph_deep_with_bdeps(atom_str: &str, with_bdeps: bool) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            false,
            Deep::Unlimited,
            &[],
            with_bdeps,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn with_bdeps_default_true_walks_depend_and_bdepend_of_an_already_installed_package() {
        // withbdepspkg is already installed, RDEPENDs on newpkg, DEPENDs
        // on builddeponlypkg, BDEPENDs on hostdeponlypkg -- with_bdeps
        // defaulting to true (real --with-bdeps=auto/y, this pilot's own
        // --usepkg-less default) walks all three under --deep, same as
        // before --with-bdeps existed.
        // Dep-key iteration order (DEPEND, RDEPEND, BDEPEND, PDEPEND,
        // IDEPEND) determines queue order here, same as every other
        // dependency-recursion test in this file: DEPEND's own
        // builddeponlypkg is queued (and therefore resolved) before
        // RDEPEND's newpkg, which is queued before BDEPEND's
        // hostdeponlypkg.
        assert_eq!(
            graph_deep_with_bdeps("dev-libs/withbdepspkg", true),
            vec![
                (
                    "dev-libs/withbdepspkg".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/builddeponlypkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/hostdeponlypkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn with_bdeps_false_skips_depend_and_bdepend_but_not_rdepend() {
        // Real depgraph.py's own "if pkg.built and not removal_action":
        // --with-bdeps=n only ever drops DEPEND/BDEPEND for an
        // already-built (here: AlreadyInstalled) package -- RDEPEND is
        // never affected, so newpkg (RDEPEND) still shows up while
        // builddeponlypkg (DEPEND) and hostdeponlypkg (BDEPEND) don't.
        assert_eq!(
            graph_deep_with_bdeps("dev-libs/withbdepspkg", false),
            vec![
                (
                    "dev-libs/withbdepspkg".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn deep_is_ignored_when_nodeps_disables_the_dependency_walk_entirely() {
        // --nodeps trumps --deep -- real create_depgraph_params.py pops
        // "recurse" out of myparams outright, which the dependency walk
        // itself checks for before `deep` is ever consulted.
        let root = fixtures_root();
        let entries: Vec<(String, PretendOutcome)> = resolve_pretend_graph(
            &root,
            &root,
            &["dev-libs/deeppkg".to_string()],
            &test_config(),
            false,
            false,
            true,
            false,
            Deep::Unlimited,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .expect("resolve_pretend_graph must succeed")
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect();
        assert_eq!(
            entries,
            vec![(
                "dev-libs/deeppkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            )]
        );
    }

    #[test]
    fn recursion_basic_chain() {
        // dev-libs/upgradepkg is installed at 1.0 with a newer 2.0
        // visible in the tree too -- without --update, it stays
        // AlreadyInstalled, same as resolve_pretend's own
        // without_update_an_installed_version_that_still_satisfies_the_atom_is_kept
        // test above, just reached here as a dependency instead of a
        // top-level atom.
        let entries = graph("dev-libs/withdeps");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/withdeps".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/upgradepkg".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn recursion_basic_chain_with_update_upgrades_the_dependency() {
        // Same fixture as recursion_basic_chain above, but with --update:
        // dev-libs/upgradepkg -- reached only as a *dependency* of
        // withdeps, never a top-level atom -- still upgrades, proving
        // `update` threads uniformly through the whole BFS (see
        // resolve_pretend_graph's own doc comment), not just top-level
        // atoms.
        let entries = graph_update("dev-libs/withdeps");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/withdeps".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/upgradepkg".to_string(),
                    PretendOutcome::Upgrade {
                        from: "1.0".to_string(),
                        to: "2.0".to_string(),
                    }
                ),
            ]
        );
    }

    #[test]
    fn nodeps_resolves_the_top_level_atom_but_never_recurses() {
        // Same dev-libs/withdeps fixture as recursion_basic_chain above --
        // with --nodeps, only the top-level atom itself is ever resolved;
        // its own DEPEND/RDEPEND (which would otherwise pull in newpkg
        // and upgradepkg) is never even read.
        let entries = graph_nodeps("dev-libs/withdeps");
        assert_eq!(
            entries,
            vec![(
                "dev-libs/withdeps".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )]
        );
    }

    #[test]
    fn nodeps_still_computes_use_flags_display_for_the_top_level_atom() {
        // Real portage's own -v USE display is about a package's own
        // metadata, unrelated to whether its dependencies get walked --
        // --nodeps must not blank it out. dev-libs/useflagpkg's own
        // foo?-gated RDEPEND on dev-libs/newpkg proves the *dependency*
        // walk really is skipped (no second entry), while its own
        // IUSE="foo missingflag" still shows up correctly.
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let entries = resolve_pretend_graph(
            &root,
            &root,
            &["dev-libs/useflagpkg".to_string()],
            &config,
            false,
            false,
            true,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .expect("resolve_pretend_graph must succeed")
        .entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].use_flags_display,
            vec![
                ("foo".to_string(), true),
                ("missingflag".to_string(), false),
            ]
        );
    }

    #[test]
    fn recursion_walks_bdepend_pdepend_idepend_same_as_depend_rdepend() {
        for pkg in ["bdependpkg", "pdependpkg", "idependpkg"] {
            let entries = graph(&format!("dev-libs/{pkg}"));
            assert_eq!(
                entries,
                vec![
                    (
                        format!("dev-libs/{pkg}"),
                        PretendOutcome::New {
                            version: "1.0".to_string()
                        }
                    ),
                    (
                        "dev-libs/newpkg".to_string(),
                        PretendOutcome::New {
                            version: "1.0".to_string()
                        }
                    ),
                ],
                "{pkg}"
            );
        }
    }

    #[test]
    fn recursion_dedupes_diamond_dependency() {
        let entries = graph("dev-libs/diamond");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "dev-libs/diamond",
                "dev-libs/shared-a",
                "dev-libs/shared-b",
                "dev-libs/common",
            ]
        );
        // "common" must appear exactly once despite being reachable via
        // both shared-a and shared-b.
        assert_eq!(names.iter().filter(|n| **n == "dev-libs/common").count(), 1);
    }

    fn full_graph(atom_str: &str) -> Vec<GraphEntry> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
    }

    #[test]
    fn required_by_is_empty_for_a_directly_requested_top_level_atom() {
        let entries = full_graph("dev-libs/newpkg");
        assert_eq!(entries[0].category, "dev-libs");
        assert_eq!(entries[0].package, "newpkg");
        assert_eq!(entries[0].required_by, Vec::new());
    }

    #[test]
    fn required_by_names_the_single_owner_of_a_plain_dependency() {
        // dev-libs/withdeps RDEPENDs on newpkg and upgradepkg -- both
        // should list withdeps as their only owner.
        let entries = full_graph("dev-libs/withdeps");
        for entry in &entries {
            if entry.category == "dev-libs"
                && (entry.package == "newpkg" || entry.package == "upgradepkg")
            {
                assert_eq!(
                    entry.required_by,
                    vec![("dev-libs".to_string(), "withdeps".to_string())],
                    "{}/{}",
                    entry.category,
                    entry.package
                );
            }
        }
    }

    #[test]
    fn required_by_lists_every_owner_of_a_diamond_dependency() {
        // dev-libs/common is reached via both shared-a and shared-b --
        // both owners must be recorded (sorted), not just whichever one
        // the BFS happened to resolve it through first.
        let entries = full_graph("dev-libs/diamond");
        let common = entries
            .iter()
            .find(|e| e.category == "dev-libs" && e.package == "common")
            .expect("dev-libs/common must be in the graph");
        assert_eq!(
            common.required_by,
            vec![
                ("dev-libs".to_string(), "shared-a".to_string()),
                ("dev-libs".to_string(), "shared-b".to_string()),
            ]
        );
    }

    #[test]
    fn recursion_terminates_on_a_dependency_cycle() {
        let entries = graph("dev-libs/cycle-a");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["dev-libs/cycle-a", "dev-libs/cycle-b"]);
    }

    #[test]
    fn recursion_resolves_only_the_first_satisfiable_any_of_alternative() {
        // dev-libs/anyof's own RDEPEND is
        // "|| ( dev-libs/newpkg dev-libs/samepkg )" -- dev-libs/newpkg
        // (listed first) has a visible candidate, so it wins outright;
        // dev-libs/samepkg (second, already installed -- also
        // satisfiable) is never even reached. See
        // use_reduce_flat_disjunctive's own doc comment
        // (portage-use-reduce) for the full "first satisfiable
        // alternative wins" grounding this replaced this pilot's
        // earlier "resolve every alternative" v1 with.
        let entries = graph("dev-libs/anyof");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["dev-libs/anyof", "dev-libs/newpkg"]);
    }

    #[test]
    fn recursion_survives_an_unresolvable_dependency() {
        // The top-level package still resolves, and the unresolvable
        // dependency still shows up in the graph (as NoVisibleCandidate,
        // not silently dropped) -- it just isn't recursed into further.
        let entries = graph("dev-libs/missingdep");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/missingdep".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/doesnotexist-anywhere".to_string(),
                    PretendOutcome::NoVisibleCandidate
                ),
            ]
        );
    }

    #[test]
    fn recursion_dedupes_across_depend_and_rdepend() {
        let entries = graph("dev-libs/dualdep");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["dev-libs/dualdep", "dev-libs/newpkg"]);
    }

    /// End-to-end wiring check: uses the *real* profile-resolved config
    /// (via portage-profile, a dev-dependency only) instead of the fixed
    /// test_accept_keywords()/empty-USE sets every other test in this file
    /// uses, proving real USE flags -- not just a hardcoded empty set --
    /// actually reach use_reduce and change dependency resolution.
    /// PORTING/fixtures/repo/profiles resolves "foo" enabled and
    /// "missingflag" disabled (see portage-profile's own fixture test),
    /// so useflagpkg's `foo? ( dev-libs/newpkg )` must be pulled in and
    /// its `missingflag? ( dev-libs/hiddendep )` must not be.
    #[test]
    fn real_resolved_use_flags_drive_dependency_recursion() {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let entries = resolve_pretend_graph(
            &root,
            &root,
            &["dev-libs/useflagpkg".to_string()],
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .expect("resolve_pretend_graph must succeed")
        .entries;
        let full_names: Vec<String> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(full_names, vec!["dev-libs/useflagpkg", "dev-libs/newpkg"]);
    }

    #[test]
    fn use_expand_flag_drives_dependency_recursion() {
        // profiles/base/make.defaults declares USE_EXPAND="VIDEO_CARDS"
        // and VIDEO_CARDS="nvidia" -- expands into "video_cards_nvidia",
        // which dev-libs/useexpandpkg's own RDEPEND is gated on.
        let full_names: Vec<String> = graph_real("dev-libs/useexpandpkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(full_names, vec!["dev-libs/useexpandpkg", "dev-libs/newpkg"]);
    }

    #[test]
    fn package_use_expand_shorthand_drives_dependency_recursion() {
        // fixtures/etc/portage/package.use has "dev-libs/
        // packageuseexpandpkg PYTHON_TARGETS: python3_12" -- expands into
        // "python_targets_python3_12", which
        // dev-libs/packageuseexpandpkg's own RDEPEND is gated on.
        let full_names: Vec<String> = graph_real("dev-libs/packageuseexpandpkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            full_names,
            vec!["dev-libs/packageuseexpandpkg", "dev-libs/newpkg"]
        );
    }

    #[test]
    fn fixture_stable_use_force_and_package_use_stable_mask_apply_when_stable() {
        // dev-libs/stableusepkg's own KEYWORDS="amd64" (no "~") is
        // genuinely stable -- use.stable.force (profiles/base) pulls in
        // its own RDEPEND.
        let full_names: Vec<String> = graph_real("dev-libs/stableusepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(full_names, vec!["dev-libs/stableusepkg", "dev-libs/newpkg"]);
    }

    #[test]
    fn fixture_stable_use_force_skips_an_unstable_candidate() {
        // dev-libs/unstableusepkg's own KEYWORDS="~amd64" is genuinely
        // NOT stable -- the same use.stable.force never applies, so its
        // own stableforceflag?-gated RDEPEND is never pulled in.
        let full_names: Vec<String> = graph_real("dev-libs/unstableusepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(full_names, vec!["dev-libs/unstableusepkg"]);
    }

    fn graph_real(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph_real`, but with `--with-test-deps` enabled.
    fn graph_real_with_test_deps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            true,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn with_test_deps_pulls_in_a_top_level_atoms_own_test_gated_dependency() {
        let full_names: Vec<String> = graph_real_with_test_deps("dev-libs/withtestdeppkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/withtestdeppkg",
                "dev-libs/newpkg",
                "dev-libs/testonlydep",
            ]
        );
    }

    #[test]
    fn without_with_test_deps_the_test_gated_dependency_is_absent() {
        let full_names: Vec<String> = graph_real("dev-libs/withtestdeppkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            full_names,
            vec!["dev-libs/withtestdeppkg", "dev-libs/newpkg"]
        );
    }

    #[test]
    fn with_test_deps_does_not_apply_beyond_a_top_level_atom() {
        // dev-libs/withtestdepconsumer RDEPENDs on dev-libs/withtestdeppkg,
        // reached at depth 1, not depth 0 -- testonlydep must stay absent
        // even though --with-test-deps is given.
        let full_names: Vec<String> = graph_real_with_test_deps("dev-libs/withtestdepconsumer")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/withtestdepconsumer",
                "dev-libs/withtestdeppkg",
                "dev-libs/newpkg",
            ]
        );
    }

    #[test]
    fn with_test_deps_is_a_no_op_for_a_package_with_no_test_iuse_flag_at_all() {
        // dev-libs/newpkg has no "test" IUSE flag declared at all -- real
        // portage's own "pkg.iuse.is_valid_flag(\"test\")" guard means
        // --with-test-deps changes nothing for it.
        assert_eq!(
            graph_real_with_test_deps("dev-libs/newpkg"),
            graph_real("dev-libs/newpkg")
        );
    }

    /// Like `graph_real`, but for a call expected to fail outright --
    /// returns the error message instead of panicking on one.
    fn graph_real_err(atom_str: &str) -> String {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .expect_err(&format!(
            "resolve_pretend_graph({atom_str}) should have failed"
        ))
    }

    /// Like `graph_real`, but with `--newuse` enabled.
    fn graph_real_newuse(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            true,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph_real`, but with `--changed-use` enabled.
    fn graph_real_changed_use(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            true,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph_real`, but with `--nodeps` enabled.
    fn graph_real_nodeps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            true,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn newuse_reinstall_still_recurses_into_its_own_dependencies() {
        // dev-libs/reinstallpkg RDEPENDs on dev-libs/newpkg -- proving a
        // Reinstall entry is walked for dependencies exactly like New/
        // Upgrade, not treated like the AlreadyInstalled dead-end it used
        // to be before --newuse existed.
        assert_eq!(
            graph_real_newuse("dev-libs/reinstallpkg"),
            vec![
                (
                    "dev-libs/reinstallpkg".to_string(),
                    PretendOutcome::Reinstall {
                        version: "1.0".to_string(),
                        changed_flags: vec!["foo".to_string()],
                        deps_changed: false,
                        slot_changed: false,
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn fixture_package_use_enables_a_flag_not_on_globally() {
        // Neither the profile nor make.conf enables "pkguseflag", but
        // fixtures/etc/portage/package.use has a "*/packageuseenablepkg
        // pkguseflag" wildcard entry, so newpkg's foo?-unrelated,
        // pkguseflag?-gated dependency must be pulled in.
        let full_names: Vec<String> = graph_real("dev-libs/packageuseenablepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            full_names,
            vec!["dev-libs/packageuseenablepkg", "dev-libs/newpkg"]
        );
    }

    #[test]
    fn nodeps_skips_recursion_even_when_package_use_would_otherwise_pull_something_in() {
        // Same fixture as fixture_package_use_enables_a_flag_not_on_globally
        // above, but with --nodeps: the real profile-resolved config still
        // decides packageuseenablepkg's own visibility/USE normally, it's
        // only the *recursion* into its now-enabled pkguseflag?-gated
        // RDEPEND that --nodeps skips -- dev-libs/newpkg must NOT appear.
        let full_names: Vec<String> = graph_real_nodeps("dev-libs/packageuseenablepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(full_names, vec!["dev-libs/packageuseenablepkg"]);
    }

    #[test]
    fn fixture_package_use_disables_a_flag_that_is_on_globally() {
        // The fixture profile chain enables "foo" globally (see
        // resolves_fixture_profile_chain_and_make_conf in portage-profile,
        // and dev-libs/useflagpkg's own test above, which -- unlike this
        // package -- DOES pull in its foo?-gated dependency), but
        // fixtures/etc/portage/package.use has a "dev-libs/packageusedisablepkg
        // -foo" entry scoped to just this package, so its own foo?-gated
        // dependency must NOT be pulled in.
        let full_names: Vec<String> = graph_real("dev-libs/packageusedisablepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(full_names, vec!["dev-libs/packageusedisablepkg"]);
    }

    #[test]
    fn use_dep_enforcement_matches_a_declared_and_enabled_flag() {
        // dev-libs/useflagpkg's own IUSE="foo missingflag", "foo"
        // enabled globally by the fixture profile chain -- see
        // resolve_pretend's own doc comment on use_deps_satisfied.
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[foo]"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_a_negated_but_actually_enabled_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[-foo]"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_a_declared_but_disabled_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[missingflag]"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn use_dep_enforcement_matches_a_negated_and_actually_disabled_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[-missingflag]"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_an_undeclared_flag_with_no_default() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[nonexistentflag]"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn use_dep_enforcement_plus_default_rescues_an_undeclared_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[nonexistentflag(+)]"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_a_dependency_atom_but_does_not_fail_the_graph() {
        // dev-libs/usedeprejectedpkg's own RDEPEND is
        // "dev-libs/useflagpkg[-foo]", genuinely unsatisfiable -- the
        // parent still resolves, and the dependency gets its own
        // NoVisibleCandidate entry (reported, not silently dropped or
        // failing the whole graph -- same "report, don't fail"
        // precedent an unresolvable dependency atom already gets; see
        // resolve_pretend_graph's own doc comment).
        let entries = graph_real("dev-libs/usedeprejectedpkg");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/usedeprejectedpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/useflagpkg".to_string(),
                    PretendOutcome::NoVisibleCandidate
                ),
            ]
        );
    }

    #[test]
    fn required_use_satisfied_resolves_normally() {
        // dev-libs/requireduseokpkg's own REQUIRED_USE is "foo? ( bar )"
        // -- foo enabled globally, bar forced on by this package's own
        // package.use entry, so genuinely satisfied.
        assert_eq!(
            graph_real("dev-libs/requireduseokpkg"),
            vec![(
                "dev-libs/requireduseokpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )]
        );
    }

    #[test]
    fn required_use_violated_top_level_fails_the_whole_call() {
        // dev-libs/requiredusebadpkg's own REQUIRED_USE is "foo? ( bar )"
        // -- foo enabled globally, bar never forced on, genuinely
        // violated. Real depgraph.py's own REQUIRED_USE check happens
        // right after package selection and aborts the whole run on
        // failure -- a materially different severity than a merely
        // unresolvable dependency (report, don't fail).
        let err = graph_real_err("dev-libs/requiredusebadpkg");
        assert_eq!(
            err,
            "REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: \"foo? ( bar )\""
        );
    }

    #[test]
    fn required_use_violated_dependency_still_fails_the_whole_call() {
        // dev-libs/requiredusebadparentpkg RDEPENDs on
        // dev-libs/requiredusebadpkg -- proving the same fatal severity
        // applies regardless of whether the violating package was
        // reached as a top-level atom or a dependency.
        let err = graph_real_err("dev-libs/requiredusebadparentpkg");
        assert_eq!(
            err,
            "REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: \"foo? ( bar )\""
        );
    }

    #[test]
    fn required_use_violations_are_collected_across_the_whole_walk_not_just_the_first() {
        // Real depgraph.py's own _add_pkg sets
        // _dynamic_config._required_use_unsatisfied = True and returns 0
        // on a violation -- it does NOT abort the whole graph walk, so a
        // SECOND, independent top-level atom's own violation (here,
        // dev-libs/requiredusebadpkg2's own "baz? ( qux )", unrelated to
        // dev-libs/requiredusebadpkg's own "foo? ( bar )") still gets
        // reached, resolved, and reported too -- not silently skipped
        // because the first atom already failed. Confirmed live against
        // both this pilot's own Rust and Python implementations
        // (byte-identical joined output) before this test was written.
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let err = resolve_pretend_graph(
            &root,
            &root,
            &[
                "dev-libs/requiredusebadpkg".to_string(),
                "dev-libs/requiredusebadpkg2".to_string(),
            ],
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .expect_err("both atoms should fail their own REQUIRED_USE");
        assert_eq!(
            err,
            "REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: \"foo? ( bar )\"\n\
             REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg2-1.0: \"baz? ( qux )\""
        );
    }

    #[test]
    fn autounmask_suggests_a_keyword_only_when_enabled() {
        // dev-libs/autounmaskkeywordpkg's own KEYWORDS is "~amd64", not
        // accepted by the fixture profile's own ACCEPT_KEYWORDS, and it
        // has no package.accept_keywords entry of its own -- masked by
        // KEYWORDS alone (package.mask/license/properties/restrict all
        // pass), the exact "keyword_masked_only" shape --autounmask's
        // own v1 suggestion targets. With autounmask_suggest_keywords
        // off (the real, correct default -- see resolve_pretend_graph's
        // own doc comment for the full on/off default-resolution logic
        // this mirrors), no suggestion is appended, matching this
        // pilot's own pre-existing behavior exactly.
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        let atoms = vec!["dev-libs/autounmaskkeywordpkg".to_string()];
        let err_without_suggestion = resolve_pretend_graph(
            &root,
            &root,
            &atoms,
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .expect_err("no visible candidate at all");
        assert_eq!(
            err_without_suggestion,
            "there are no ebuilds to satisfy \"dev-libs/autounmaskkeywordpkg\"."
        );

        let err_with_suggestion = resolve_pretend_graph(
            &root,
            &root,
            &atoms,
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            true,
        )
        .expect_err("no visible candidate at all");
        assert_eq!(
            err_with_suggestion,
            "there are no ebuilds to satisfy \"dev-libs/autounmaskkeywordpkg\".\n\
             note: dev-libs/autounmaskkeywordpkg-1.0 exists but is masked by KEYWORDS; \
             --autounmask-keep-keywords=n suggests adding \"dev-libs/autounmaskkeywordpkg ~amd64\" \
             to package.accept_keywords"
        );
    }

    #[test]
    fn required_use_referencing_an_implicit_arch_list_flag_is_valid() {
        // dev-libs/archiuseimplicitpkg's own IUSE is empty and its own
        // REQUIRED_USE is "!x86" -- "x86" is never declared by this
        // package's own IUSE at all, but IS a real, valid flag via
        // PORTAGE_ARCHLIST (fixtures/repo/profiles/base/arch.list),
        // exactly like real media-libs/mesa's own REQUIRED_USE
        // referencing "x86" without declaring it in IUSE -- confirmed
        // live against the real, installed system. Before this fix,
        // this pilot's own iuse_set never consulted PORTAGE_ARCHLIST at
        // all, so this would fail with "USE flag 'x86' is not in IUSE"
        // instead of resolving (x86 isn't the active profile's own arch,
        // so it stays disabled, and "!x86" is satisfied).
        assert_eq!(
            graph_real("dev-libs/archiuseimplicitpkg"),
            vec![(
                "dev-libs/archiuseimplicitpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )]
        );
    }

    fn graph_result_real(atom_str: &str) -> GraphResult {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(
            &root,
            &root.join("repo"),
            &[("overlay".to_string(), root.join("overlay"))],
            "testrepo",
        )
        .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            false,
            false,
            Deep::NotRequested,
            &[],
            true,
            false,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
    }

    fn graph_entries_real(atom_str: &str) -> Vec<GraphEntry> {
        graph_result_real(atom_str).entries
    }

    #[test]
    fn fixture_strong_blocker_matches_an_installed_package() {
        // dev-libs/blockerpkg's RDEPEND is "!!dev-libs/samepkg", and
        // dev-libs/samepkg-1.0 is already installed per the fixture vdb.
        let entries = graph_entries_real("dev-libs/blockerpkg");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].blockers,
            vec![BlockerConflict {
                atom_str: "!!dev-libs/samepkg".to_string(),
                strong: true,
                matched_category: "dev-libs".to_string(),
                matched_package: "samepkg".to_string(),
                matched_version: "1.0".to_string(),
            }]
        );
    }

    #[test]
    fn fixture_weak_blocker_matches_another_new_package_in_the_same_graph() {
        // dev-libs/graphblockerparent pulls in both dev-libs/blockerpartnerpkg
        // and dev-libs/weakblockerpkg (whose RDEPEND is
        // "!dev-libs/blockerpartnerpkg") as New in the same run, so the
        // weak blocker must be reported against blockerpartnerpkg's
        // graph-resolved version, not just against the (empty) vdb.
        let entries = graph_entries_real("dev-libs/graphblockerparent");
        let full_names: Vec<String> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/graphblockerparent",
                "dev-libs/blockerpartnerpkg",
                "dev-libs/weakblockerpkg",
            ]
        );
        assert!(entries[0].blockers.is_empty());
        assert!(entries[1].blockers.is_empty());
        assert_eq!(
            entries[2].blockers,
            vec![BlockerConflict {
                atom_str: "!dev-libs/blockerpartnerpkg".to_string(),
                strong: false,
                matched_category: "dev-libs".to_string(),
                matched_package: "blockerpartnerpkg".to_string(),
                matched_version: "1.0".to_string(),
            }]
        );
    }

    #[test]
    fn fixture_unrelated_packages_report_no_blockers() {
        // Regression guard: the diamond fixture has no blockers at all, so
        // none of its entries should gain a spurious one.
        let entries = graph_entries_real("dev-libs/diamond");
        assert!(entries.iter().all(|e| e.blockers.is_empty()));
    }

    fn candidate(version: &str, keywords: &[&str]) -> Candidate {
        Candidate {
            version: version.to_string(),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
            slot: "0".to_string(),
            sub_slot: "0".to_string(),
            repo_location: PathBuf::new(),
            repo_priority: 0,
            repo_name: "test".to_string(),
            license: String::new(),
            iuse: String::new(),
            properties: String::new(),
            restrict: String::new(),
        }
    }

    fn license_tokens(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parse_license_tree_flattens_a_plain_group_at_top_level() {
        // Verified directly against real portage.dep.use_reduce(...,
        // opconvert=True): "GPL-2 MIT" -> ['GPL-2', 'MIT'] (a bare
        // top-level list already has AND semantics, so a plain "(...)"
        // there adds nothing structurally worth keeping).
        let tree = parse_license_tree(&license_tokens("GPL-2 ( MIT )"), &HashSet::new()).unwrap();
        assert_eq!(
            tree,
            vec![
                LicenseNode::License("GPL-2".to_string()),
                LicenseNode::License("MIT".to_string()),
            ]
        );
    }

    #[test]
    fn parse_license_tree_keeps_any_of_members_flat() {
        // Verified: "GPL-2 || ( MIT BSD )" -> ['GPL-2', ['||', 'MIT', 'BSD']]
        // -- the || group's own members sit directly in its own list,
        // not double-nested.
        let tree =
            parse_license_tree(&license_tokens("GPL-2 || ( MIT BSD )"), &HashSet::new()).unwrap();
        assert_eq!(
            tree,
            vec![
                LicenseNode::License("GPL-2".to_string()),
                LicenseNode::AnyOf(vec![
                    LicenseNode::License("MIT".to_string()),
                    LicenseNode::License("BSD".to_string()),
                ]),
            ]
        );
    }

    #[test]
    fn parse_license_tree_nests_a_plain_group_directly_inside_any_of() {
        // Verified: "|| ( ( GPL-2 MIT ) BSD )" ->
        // [['||', ['GPL-2', 'MIT'], 'BSD']] -- a plain sub-group sitting
        // directly inside a || group's own member list stays a genuine
        // nested "this whole bundle is one alternative" unit, unlike
        // the top-level case above.
        let tree = parse_license_tree(&license_tokens("|| ( ( GPL-2 MIT ) BSD )"), &HashSet::new())
            .unwrap();
        assert_eq!(
            tree,
            vec![LicenseNode::AnyOf(vec![
                LicenseNode::AllOf(vec![
                    LicenseNode::License("GPL-2".to_string()),
                    LicenseNode::License("MIT".to_string()),
                ]),
                LicenseNode::License("BSD".to_string()),
            ])]
        );
    }

    #[test]
    fn parse_license_tree_resolves_a_use_conditional() {
        // Verified: "|| ( foo? ( GPL-2 MIT ) BSD )" with foo enabled ->
        // [['||', ['GPL-2', 'MIT'], 'BSD']]; with foo disabled ->
        // ['BSD'] (the conditional contributes nothing at all, and the
        // now-single-alternative || itself collapses away too -- real
        // use_reduce's own behavior, verified directly).
        let enabled = HashSet::from(["foo".to_string()]);
        let tree =
            parse_license_tree(&license_tokens("|| ( foo? ( GPL-2 MIT ) BSD )"), &enabled).unwrap();
        assert_eq!(
            tree,
            vec![LicenseNode::AnyOf(vec![
                LicenseNode::AllOf(vec![
                    LicenseNode::License("GPL-2".to_string()),
                    LicenseNode::License("MIT".to_string()),
                ]),
                LicenseNode::License("BSD".to_string()),
            ])]
        );

        let disabled = HashSet::new();
        let tree_disabled =
            parse_license_tree(&license_tokens("|| ( foo? ( GPL-2 MIT ) BSD )"), &disabled)
                .unwrap();
        // This pilot's own parser keeps the || wrapper (unlike real
        // use_reduce's own further single-alternative collapse -- see
        // the module doc comment on why that specific collapse isn't
        // replicated); masking-wise this is equivalent either way,
        // since an AnyOf of one alternative and that alternative alone
        // make an identical accept/reject decision.
        assert_eq!(
            tree_disabled,
            vec![LicenseNode::AnyOf(vec![LicenseNode::License(
                "BSD".to_string()
            )])]
        );
    }

    #[test]
    fn parse_license_tree_rejects_unbalanced_parens() {
        assert!(parse_license_tree(&license_tokens("( GPL-2"), &HashSet::new()).is_err());
        assert!(parse_license_tree(&license_tokens("GPL-2 )"), &HashSet::new()).is_err());
    }

    #[test]
    fn parse_license_tree_rejects_a_dangling_double_pipe() {
        assert!(parse_license_tree(&license_tokens("|| GPL-2"), &HashSet::new()).is_err());
    }

    #[test]
    fn has_masked_license_empty_string_is_never_masked() {
        assert!(!has_masked_license("", &HashSet::new(), &HashSet::new()).unwrap());
    }

    #[test]
    fn has_masked_license_plain_and_semantics_needs_every_license_accepted() {
        let acceptable = HashSet::from(["GPL-2".to_string()]);
        assert!(has_masked_license("GPL-2 MIT", &HashSet::new(), &acceptable).unwrap());
        let acceptable_both = HashSet::from(["GPL-2".to_string(), "MIT".to_string()]);
        assert!(!has_masked_license("GPL-2 MIT", &HashSet::new(), &acceptable_both).unwrap());
    }

    #[test]
    fn has_masked_license_any_of_needs_only_one_alternative_accepted() {
        let acceptable = HashSet::from(["BSD".to_string()]);
        assert!(!has_masked_license("|| ( GPL-2 BSD )", &HashSet::new(), &acceptable).unwrap());
        let acceptable_neither = HashSet::from(["MIT".to_string()]);
        assert!(
            has_masked_license("|| ( GPL-2 BSD )", &HashSet::new(), &acceptable_neither).unwrap()
        );
    }

    #[test]
    fn has_masked_license_any_of_alternative_bundle_needs_the_whole_bundle_accepted() {
        // "|| ( ( GPL-2 MIT ) BSD )": accepting only MIT (not GPL-2)
        // must NOT satisfy the (GPL-2 AND MIT) bundle -- only accepting
        // BSD, or both GPL-2 and MIT together, satisfies this.
        let only_mit = HashSet::from(["MIT".to_string()]);
        assert!(
            has_masked_license("|| ( ( GPL-2 MIT ) BSD )", &HashSet::new(), &only_mit).unwrap()
        );
        let both = HashSet::from(["GPL-2".to_string(), "MIT".to_string()]);
        assert!(!has_masked_license("|| ( ( GPL-2 MIT ) BSD )", &HashSet::new(), &both).unwrap());
        let bsd_only = HashSet::from(["BSD".to_string()]);
        assert!(
            !has_masked_license("|| ( ( GPL-2 MIT ) BSD )", &HashSet::new(), &bsd_only).unwrap()
        );
    }

    #[test]
    fn license_default_accept_license_star_makes_any_declared_license_visible() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["*".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn license_not_in_the_acceptable_set_is_masked() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn license_eula_style_negation_masks_a_matching_license() {
        // accept_license here is the already-@group-expanded form
        // portage-profile's own resolve_config would have produced from
        // real "* -@EULA" with license_groups EULA="SomeEula" --
        // portage-repo itself never expands groups, only consumes the
        // already-expanded tokens (see accept_license's own doc
        // comment, portage-profile).
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["*".to_string(), "-SomeEula".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            license: "SomeEula".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn license_package_license_override_unmasks_for_one_matching_package_only() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["*".to_string(), "-SomeEula".to_string()],
            package_license: vec![("dev-libs/foo".to_string(), vec!["SomeEula".to_string()])],
            ..Default::default()
        };
        let c = Candidate {
            license: "SomeEula".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(is_visible(&c, "dev-libs", "foo", &config));
        assert!(!is_visible(&c, "dev-libs", "bar", &config));
    }

    #[test]
    fn license_any_of_group_is_satisfied_by_one_accepted_alternative() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["BSD".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            license: "|| ( GPL-2 BSD )".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn license_use_conditional_only_masks_once_the_flag_is_actually_enabled() {
        let c = Candidate {
            license: "GPL-2 foo? ( MIT )".to_string(),
            iuse: "foo".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        let config_foo_off = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["GPL-2".to_string()],
            ..Default::default()
        };
        assert!(is_visible(&c, "dev-libs", "foo-pkg", &config_foo_off));

        let config_foo_on = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["GPL-2".to_string()],
            use_flags: HashSet::from(["foo".to_string()]),
            use_tokens: vec!["foo".to_string()],
            ..Default::default()
        };
        assert!(!is_visible(&c, "dev-libs", "foo-pkg", &config_foo_on));
    }

    #[test]
    fn properties_default_star_accepts_any_declared_property() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_properties: vec!["*".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            properties: "live".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn properties_not_in_the_acceptable_set_is_masked() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_properties: vec!["interactive".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            properties: "live".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn package_properties_override_unmasks_for_one_matching_package_only() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_properties: vec![],
            package_properties: vec![("dev-libs/foo".to_string(), vec!["live".to_string()])],
            ..Default::default()
        };
        let c = Candidate {
            properties: "live".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(is_visible(&c, "dev-libs", "foo", &config));
        assert!(!is_visible(&c, "dev-libs", "bar", &config));
    }

    #[test]
    fn restrict_default_star_accepts_any_declared_token() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_restrict: vec!["*".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            restrict: "test bindist".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn restrict_not_in_the_acceptable_set_is_masked() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_restrict: vec!["test".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            restrict: "bindist".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn package_accept_restrict_override_unmasks_for_one_matching_package_only() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_restrict: vec![],
            package_accept_restrict: vec![(
                "dev-libs/foo".to_string(),
                vec!["bindist".to_string()],
            )],
            ..Default::default()
        };
        let c = Candidate {
            restrict: "bindist".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(is_visible(&c, "dev-libs", "foo", &config));
        assert!(!is_visible(&c, "dev-libs", "bar", &config));
    }

    #[test]
    fn restrict_multiple_tokens_all_need_accepting() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_restrict: vec!["test".to_string()],
            ..Default::default()
        };
        let c = Candidate {
            restrict: "test bindist".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!is_visible(&c, "dev-libs", "foo", &config));
    }

    #[test]
    fn package_mask_hides_a_matching_candidate() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/foo".to_string()],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "foo",
            &config
        ));
    }

    #[test]
    fn package_unmask_cancels_a_matching_mask() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/foo".to_string()],
            package_unmask: vec!["dev-libs/foo".to_string()],
            ..Default::default()
        };
        assert!(is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "foo",
            &config
        ));
    }

    #[test]
    fn package_mask_wildcard_matches_whole_category() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/*".to_string()],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "anything",
            &config
        ));
        assert!(is_visible(
            &candidate("1.0", &["amd64"]),
            "app-misc",
            "anything",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_wildcard_extends_visibility() {
        // Globally only "amd64" is accepted, but a package.accept_keywords
        // wildcard entry additionally accepts "~amd64" for dev-qt/*.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_accept_keywords: vec![("dev-qt/*".to_string(), vec!["~amd64".to_string()])],
            ..Default::default()
        };
        assert!(is_visible(
            &candidate("1.0", &["~amd64"]),
            "dev-qt",
            "qtcore",
            &config
        ));
        // A package outside the wildcard doesn't get the extra keyword.
        assert!(!is_visible(
            &candidate("1.0", &["~amd64"]),
            "dev-libs",
            "foo",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_double_star_accepts_unconditionally() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_accept_keywords: vec![("dev-libs/live".to_string(), vec!["**".to_string()])],
            ..Default::default()
        };
        // No KEYWORDS at all (e.g. a live/9999 ebuild) is still visible.
        assert!(is_visible(
            &candidate("9999", &[]),
            "dev-libs",
            "live",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_negation_revokes_a_globally_accepted_keyword() {
        // Globally "amd64" is accepted, but a package.accept_keywords
        // "-amd64" entry revokes it for this one package specifically --
        // real KeywordsManager._getEgroups folds "-token" removals over
        // the combined global+package list, not just unions everything
        // a matching entry ever mentions.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_accept_keywords: vec![(
                "dev-libs/norevoke".to_string(),
                vec!["-amd64".to_string()],
            )],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "norevoke",
            &config
        ));
        // An unrelated package keeps the global "amd64" acceptance.
        assert!(is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "unrelated",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_dash_star_clears_every_globally_accepted_keyword() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string(), "x86".to_string()]),
            package_accept_keywords: vec![(
                "dev-libs/norevoke".to_string(),
                vec!["-*".to_string()],
            )],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "norevoke",
            &config
        ));
        assert!(!is_visible(
            &candidate("1.0", &["x86"]),
            "dev-libs",
            "norevoke",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_more_specific_entry_re_grants_after_a_less_specific_revoke() {
        // A bare-package "-amd64" revokes it, but a more specific
        // exact-version entry re-adds "amd64" -- specificity ordering
        // applies to package.accept_keywords the same way it already
        // does for package.use.mask/.force (specificity_ordered_flags).
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_accept_keywords: vec![
                (
                    "=dev-libs/regrant-2.0".to_string(),
                    vec!["amd64".to_string()],
                ),
                ("dev-libs/regrant".to_string(), vec!["-amd64".to_string()]),
            ],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "regrant",
            &config
        ));
        assert!(is_visible(
            &candidate("2.0", &["amd64"]),
            "dev-libs",
            "regrant",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_dash_star_can_revoke_an_earlier_double_star() {
        // A more specific "-*" revokes even an unconditional "**" grant
        // from a less specific entry -- proving "**" is folded in fold
        // order now, not checked via a separate order-blind pre-scan.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_accept_keywords: vec![
                ("dev-libs/live".to_string(), vec!["**".to_string()]),
                ("=dev-libs/live-9999".to_string(), vec!["-*".to_string()]),
            ],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("9999", &[]),
            "dev-libs",
            "live",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_star_accepts_any_stable_keyword() {
        // Global ACCEPT_KEYWORDS "*" accepts any stable-classified
        // keyword the candidate declares, even one never otherwise
        // mentioned anywhere.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["*".to_string()]),
            ..Default::default()
        };
        assert!(is_visible(
            &candidate("1.0", &["arm64"]),
            "dev-libs",
            "starpkg",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_star_does_not_accept_a_testing_only_candidate() {
        // "*" only ever covers stable-classified keywords -- a
        // testing-only ("~"-prefixed) candidate still needs "~*"
        // specifically.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["*".to_string()]),
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["~arm64"]),
            "dev-libs",
            "starpkg",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_tilde_star_accepts_any_testing_keyword() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["~*".to_string()]),
            ..Default::default()
        };
        assert!(is_visible(
            &candidate("1.0", &["~arm64"]),
            "dev-libs",
            "starpkg",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_tilde_star_does_not_accept_a_stable_only_candidate() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["~*".to_string()]),
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["arm64"]),
            "dev-libs",
            "starpkg",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_negative_keyword_never_counts_toward_star() {
        // A "-arm64" KEYWORDS token (explicit "not supported here") is
        // excluded from classification entirely -- with no other real
        // keyword declared, "*" has nothing stable-classified to grant
        // acceptance for.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["*".to_string()]),
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["-arm64"]),
            "dev-libs",
            "starpkg",
            &config
        ));
    }

    #[test]
    fn effective_use_flags_applies_a_plus_iuse_default_when_nothing_else_says_otherwise() {
        let use_flags = effective_use_flags(
            "+foo -bar baz",
            &[],
            &[],
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/pkg-1.0:0",
            "dev-libs",
            "pkg",
        );
        // "+foo" defaults on; "-bar" stays off (same as omission, but a
        // real, explicit marker); "baz" (no marker at all) is genuinely
        // undecided by IUSE itself and stays off too.
        assert_eq!(use_flags, HashSet::from(["foo".to_string()]));
    }

    #[test]
    fn effective_use_flags_lets_use_tokens_add_alongside_a_plus_iuse_default() {
        // `use_tokens` mentioning an entirely different flag doesn't
        // suppress "+foo"'s own default -- the additive half.
        let base = ["other".to_string()].to_vec();
        let use_flags = effective_use_flags(
            "+foo",
            &base,
            &[],
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/pkg-1.0:0",
            "dev-libs",
            "pkg",
        );
        assert_eq!(
            use_flags,
            HashSet::from(["foo".to_string(), "other".to_string()])
        );
    }

    #[test]
    fn effective_use_flags_lets_use_tokens_explicitly_cancel_a_plus_iuse_default() {
        // The gap this pilot's own IUSE-defaults slice originally left
        // open, now closed: real regenerate() runs ONE continuous
        // incremental walk (pkginternal -> defaults -> conf -> pkg), so
        // a genuine "-foo" in profile/make.conf really does cancel an
        // earlier "+foo" IUSE default -- not just fail to add on top of
        // it. `use_tokens` here is two separate raw USE= values
        // ("foo" then "-foo bar"), exactly what `resolve_config` would
        // produce from two profile levels -- replayed via
        // apply_incremental, not unioned as a pre-flattened set, so the
        // "-foo" genuinely reaches back and cancels IUSE's own "+foo".
        let use_tokens = vec!["foo".to_string(), "-foo bar".to_string()];
        let use_flags = effective_use_flags(
            "+foo",
            &use_tokens,
            &[],
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/pkg-1.0:0",
            "dev-libs",
            "pkg",
        );
        assert_eq!(use_flags, HashSet::from(["bar".to_string()]));
    }

    #[test]
    fn effective_use_flags_lets_package_use_override_a_plus_iuse_default() {
        let package_use = vec![("dev-libs/pkg".to_string(), vec!["-foo".to_string()])];
        let use_flags = effective_use_flags(
            "+foo",
            &[],
            &package_use,
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/pkg-1.0:0",
            "dev-libs",
            "pkg",
        );
        assert!(use_flags.is_empty());
    }

    #[test]
    fn effective_use_flags_layers_a_matching_package_use_entry_on_top_of_base() {
        let base = ["foo".to_string()].to_vec();
        let package_use = vec![(
            "dev-libs/bar".to_string(),
            vec!["baz".to_string(), "-foo".to_string()],
        )];
        let use_flags = effective_use_flags(
            "",
            &base,
            &package_use,
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert_eq!(use_flags, HashSet::from(["baz".to_string()]));
    }

    #[test]
    fn effective_use_flags_does_not_affect_a_non_matching_package() {
        let base = ["foo".to_string()].to_vec();
        let package_use = vec![("dev-libs/bar".to_string(), vec!["baz".to_string()])];
        let use_flags = effective_use_flags(
            "",
            &base,
            &package_use,
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/unrelated-1.0:0",
            "dev-libs",
            "unrelated",
        );
        assert_eq!(use_flags, HashSet::from(["foo".to_string()]));
    }

    #[test]
    fn effective_use_flags_matches_a_wildcard_package_use_entry() {
        let base: Vec<String> = Vec::new();
        let package_use = vec![("*/bar".to_string(), vec!["baz".to_string()])];
        let use_flags = effective_use_flags(
            "",
            &base,
            &package_use,
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert_eq!(use_flags, HashSet::from(["baz".to_string()]));
    }

    #[test]
    fn effective_use_flags_applies_package_use_force_and_mask() {
        let base: Vec<String> = Vec::new();
        let package_use_force = vec![("dev-libs/bar".to_string(), vec!["forceflag".to_string()])];
        let package_use_mask = vec![("dev-libs/bar".to_string(), vec!["maskflag".to_string()])];
        let use_flags = effective_use_flags(
            "",
            &base,
            &[],
            &package_use_force,
            &package_use_mask,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert_eq!(use_flags, HashSet::from(["forceflag".to_string()]));
    }

    #[test]
    fn effective_use_flags_package_use_mask_wins_over_force_on_conflict() {
        // Same flag both forced and masked by two entries at the SAME
        // specificity tier -- mask must win, matching real portage's own
        // force-then-mask application order (see effective_use_flags's
        // own doc comment).
        let base: Vec<String> = Vec::new();
        let package_use_force = vec![("dev-libs/bar".to_string(), vec!["bothflag".to_string()])];
        let package_use_mask = vec![("dev-libs/bar".to_string(), vec!["bothflag".to_string()])];
        let use_flags = effective_use_flags(
            "",
            &base,
            &[],
            &package_use_force,
            &package_use_mask,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert!(!use_flags.contains("bothflag"));
    }

    #[test]
    fn effective_use_flags_global_use_force_wins_over_a_package_use_disable() {
        // Real regenerate() applies self.useforce (global use.force
        // combined with per-package getUseForce(pkg)) as the literal
        // last step of its own incremental USE walk, strictly after the
        // "pkg" (package.use) tier -- so a package.use "-flag" entry can
        // NEVER turn off a globally use.force'd flag, unlike an earlier
        // version of this pilot which folded use_force into `base` too
        // early, letting package.use incorrectly win.
        let use_force = HashSet::from(["forceflag".to_string()]);
        let package_use = vec![("dev-libs/bar".to_string(), vec!["-forceflag".to_string()])];
        let use_flags = effective_use_flags(
            "",
            &[],
            &package_use,
            &[],
            &[],
            &use_force,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert!(use_flags.contains("forceflag"));
    }

    #[test]
    fn effective_use_flags_global_use_mask_wins_over_a_package_use_enable() {
        // Mirror of the force case above: global use.mask (folded into
        // real self.usemask, applied via difference_update as the
        // literal last operation of all) beats a package.use "+flag"
        // entry trying to turn it on.
        let use_mask = HashSet::from(["maskflag".to_string()]);
        let package_use = vec![("dev-libs/bar".to_string(), vec!["maskflag".to_string()])];
        let use_flags = effective_use_flags(
            "",
            &[],
            &package_use,
            &[],
            &[],
            &HashSet::new(),
            &use_mask,
            &HashSet::new(),
            &HashSet::new(),
            &[],
            &[],
            &[],
            &HashSet::new(),
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert!(!use_flags.contains("maskflag"));
    }

    #[test]
    fn effective_use_flags_applies_stable_force_and_mask_only_when_stable() {
        // "amd64" (no "~") is stable in this synthetic config: converting
        // it to "~amd64" would fall outside accept_keywords={"amd64"},
        // so is_stable's own "would masking every keyword make this
        // invisible" check is true. use_stable_force/package_use_stable_force
        // and use_stable_mask/package_use_stable_mask should all apply.
        let base: Vec<String> = Vec::new();
        let use_stable_force = HashSet::from(["globalstableforce".to_string()]);
        let use_stable_mask = HashSet::from(["globalstablemask".to_string()]);
        let package_use_stable_force = vec![(
            "dev-libs/bar".to_string(),
            vec!["pkgstableforce".to_string()],
        )];
        let package_use_stable_mask = vec![(
            "dev-libs/bar".to_string(),
            vec!["pkgstablemask".to_string()],
        )];
        let accept_keywords = HashSet::from(["amd64".to_string()]);
        let use_flags = effective_use_flags(
            "",
            &base,
            &[],
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &use_stable_force,
            &use_stable_mask,
            &package_use_stable_force,
            &package_use_stable_mask,
            &["amd64".to_string()],
            &accept_keywords,
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert_eq!(
            use_flags,
            HashSet::from([
                "globalstableforce".to_string(),
                "pkgstableforce".to_string()
            ])
        );
    }

    #[test]
    fn effective_use_flags_skips_stable_force_and_mask_when_not_stable() {
        // "~amd64" (already unstable) is NOT stable: replacing it with
        // its own already-"~"-prefixed form changes nothing, so it stays
        // visible either way -- is_stable's own check is false. None of
        // the stable-only sources should apply at all.
        let base: Vec<String> = Vec::new();
        let use_stable_force = HashSet::from(["globalstableforce".to_string()]);
        let accept_keywords = HashSet::from(["~amd64".to_string()]);
        let use_flags = effective_use_flags(
            "",
            &base,
            &[],
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &use_stable_force,
            &HashSet::new(),
            &[],
            &[],
            &["~amd64".to_string()],
            &accept_keywords,
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert!(use_flags.is_empty());
    }

    #[test]
    fn specificity_ordered_flags_lets_a_more_specific_entry_override_a_less_specific_one() {
        // A bare atom masks "flag", a more specific exact-version atom
        // un-masks it again ("-flag") -- the more specific entry must
        // win regardless of which order the two entries appear in the
        // input list, proving this is genuine specificity-based
        // reordering, not just "last entry wins".
        let entries = vec![
            ("=dev-libs/bar-1.0".to_string(), vec!["-flag".to_string()]),
            ("dev-libs/bar".to_string(), vec!["flag".to_string()]),
        ];
        let flags = specificity_ordered_flags(
            &entries,
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
            HashSet::new(),
        );
        assert!(!flags.contains("flag"));
    }

    #[test]
    fn atom_specificity_ranks_exact_version_above_bare_above_wildcard() {
        assert!(atom_specificity("=dev-libs/bar-1.0") > atom_specificity("dev-libs/bar:0"));
        assert!(atom_specificity("dev-libs/bar:0") > atom_specificity("dev-libs/bar"));
        assert!(atom_specificity("dev-libs/bar") > atom_specificity("*/*"));
    }

    fn graph_entry(category: &str, package: &str, version: &str) -> GraphEntry {
        GraphEntry {
            category: category.to_string(),
            package: package.to_string(),
            outcome: PretendOutcome::New {
                version: version.to_string(),
            },
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            use_flags_display: Vec::new(),
            required_by: Vec::new(),
        }
    }

    #[test]
    fn resolve_blockers_matches_a_graph_resolved_package_with_no_installed_candidates() {
        let entries = vec![
            graph_entry("dev-libs", "owner", "1.0"),
            graph_entry("dev-libs", "target", "2.0"),
        ];
        let pending = vec![PendingBlocker {
            atom_str: "!!dev-libs/target".to_string(),
            strong: true,
            target_category: "dev-libs".to_string(),
            target_package: "target".to_string(),
            owner_key: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
        }];
        let conflicts = resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &pending,
            &entries,
        );
        assert_eq!(
            conflicts,
            vec![(
                ("dev-libs".to_string(), "owner".to_string()),
                BlockerConflict {
                    atom_str: "!!dev-libs/target".to_string(),
                    strong: true,
                    matched_category: "dev-libs".to_string(),
                    matched_package: "target".to_string(),
                    matched_version: "2.0".to_string(),
                }
            )]
        );
    }

    #[test]
    fn resolve_blockers_skips_a_blocker_matching_its_own_owner() {
        let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
        let pending = vec![PendingBlocker {
            atom_str: "!dev-libs/owner".to_string(),
            strong: false,
            target_category: "dev-libs".to_string(),
            target_package: "owner".to_string(),
            owner_key: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
        }];
        let conflicts = resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &pending,
            &entries,
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn resolve_blockers_returns_nothing_when_the_target_matches_nothing() {
        let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
        let pending = vec![PendingBlocker {
            atom_str: "!dev-libs/nonexistent".to_string(),
            strong: false,
            target_category: "dev-libs".to_string(),
            target_package: "nonexistent".to_string(),
            owner_key: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
        }];
        let conflicts = resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &pending,
            &entries,
        );
        assert!(conflicts.is_empty());
    }
}
