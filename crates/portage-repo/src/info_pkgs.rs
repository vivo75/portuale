//! Package rows for `emerge --info`.

use crate::*;

/// One row of `emerge --info`'s `info_pkgs` version table (real
/// `action_info`'s second `myvars` loop): a resolved `cat/pkg` and its
/// installed versions, each already rendered `<ver>::<repo>` with the
/// ` (virtual/foo)` suffix appended when the row was reached by
/// expanding a `virtual/*` atom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoPkgRow {
    pub cp: String,
    pub versions: Vec<String>,
}

/// One level of real `expand_new_virt` (`portage.dbapi._expand_new_virt`):
/// a non-`virtual/*` `(cat, pkg)` yields itself (with no provide-suffix);
/// a `virtual/*` yields the `(cat, pkg)` of every atom in its highest
/// installed version's vdb `RDEPEND`, flattened against that version's
/// own installed `USE`, that is itself installed -- each tagged with the
/// originating `virtual/foo` string. A `virtual/*` that isn't installed,
/// or whose `RDEPEND` names nothing installed, yields itself unexpanded.
/// Real recurses into a second virtual reached this way; portuale does
/// not (real's standard `info` list only ever hits `virtual/os-headers`,
/// which resolves in one hop).
pub(crate) fn expand_new_virt(
    root: &Path,
    category: &str,
    package: &str,
) -> Vec<(String, String, Option<String>)> {
    if category != "virtual" {
        return vec![(category.to_string(), package.to_string(), None)];
    }
    let orig = format!("{category}/{package}");
    let Some(version) = installed_candidates(root, category, package)
        .into_iter()
        .map(|(v, _, _)| v)
        .max_by(|a, b| vercmp_ordering(a, b))
    else {
        return vec![(category.to_string(), package.to_string(), None)];
    };
    let use_flags = read_vdb_flag_set(root, category, package, &version, "USE");
    let rdepend = read_vdb_string(root, category, package, &version, "RDEPEND");
    let mut out: Vec<(String, String, Option<String>)> = Vec::new();
    if let Some(atoms) = flat_dep_atoms(&rdepend, &use_flags) {
        let mut sorted: Vec<String> = atoms.into_iter().collect();
        sorted.sort();
        for atom_str in sorted {
            if let Some(atom) = portage_dep::parse_atom(&atom_str)
                && atom.category != "virtual"
                && !installed_candidates(root, &atom.category, &atom.package).is_empty()
            {
                out.push((atom.category, atom.package, Some(orig.clone())));
            }
        }
    }
    if out.is_empty() {
        vec![(category.to_string(), package.to_string(), None)]
    } else {
        out
    }
}

/// The `info_pkgs` version table `emerge --info` prints (real
/// `action_info`, `actions.py:2109-2165`): the six hardcoded atoms plus
/// `<main_repo>/profiles/info_pkgs`, each `expand_new_virt`'d, then one
/// row per resolved `cat/pkg` that has an installed version -- versions
/// `vercmp`-sorted, `<ver>::<repo>` (real `_unknown_repo` ->
/// `<unknown repository>`), with ` (virtual/foo)` on a virtual-provided
/// row. Rows are returned `cat/pkg`-sorted; the caller pads `cp` to the
/// widest one.
pub fn info_pkgs_table(root: &Path, main_repo_location: &Path) -> Vec<InfoPkgRow> {
    let mut atoms: Vec<String> = [
        "dev-build/autoconf",
        "dev-build/automake",
        "virtual/os-headers",
        "sys-devel/binutils",
        "dev-build/libtool",
        "dev-lang/python",
    ]
    .map(String::from)
    .to_vec();
    let extra =
        fs::read_to_string(main_repo_location.join("profiles/info_pkgs")).unwrap_or_default();
    for line in extra.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with('#') {
            atoms.push(l.to_string());
        }
    }

    // (orig cp string, resolved cat, resolved pkg, provide_suffix)
    let mut resolved: Vec<(String, String, String, Option<String>)> = Vec::new();
    for a in &atoms {
        // Strip an operator / version / slot / use-dep down to `cat/pkg`
        // (real builds an `Atom`, then only uses `atom.cp` + `vardb.match`).
        let Some(atom) = portage_dep::parse_atom(a) else {
            continue;
        };
        let orig_cp = format!("{}/{}", atom.category, atom.package);
        for (cat, pkg, provide) in expand_new_virt(root, &atom.category, &atom.package) {
            resolved.push((orig_cp.clone(), cat, pkg, provide));
        }
    }
    resolved.sort();
    resolved.dedup();

    use std::collections::BTreeMap;
    // matched_cp -> ver -> "ver::repo (suffix)"
    let mut cp_map: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (orig_cp, cat, pkg, _provide) in &resolved {
        let matched_cp = format!("{cat}/{pkg}");
        for r in installed_refs(root, cat, pkg) {
            let repo = if r.repo == "__unknown__" {
                "<unknown repository>".to_string()
            } else {
                r.repo.clone()
            };
            let provide_suffix = if &matched_cp != orig_cp {
                format!(" ({orig_cp})")
            } else {
                String::new()
            };
            let entry = cp_map.entry(matched_cp.clone()).or_default();
            // Prefer a duplicate match that carries provider info (real
            // `if prev_match.provide_suffix: continue`).
            if let Some(existing) = entry.get(&r.version)
                && existing.contains(" (")
            {
                continue;
            }
            entry.insert(
                r.version.clone(),
                format!("{}::{}{}", r.version, repo, provide_suffix),
            );
        }
    }

    cp_map
        .into_iter()
        .map(|(cp, ver_map)| {
            let mut versions: Vec<(String, String)> = ver_map.into_iter().collect();
            versions.sort_by(|a, b| vercmp_ordering(&a.0, &b.0));
            InfoPkgRow {
                cp,
                versions: versions.into_iter().map(|(_, s)| s).collect(),
            }
        })
        .collect()
}

/// The best visible candidate for `atom_str` plus its `-pv`-style USE
/// display -- what `emerge --info <atom>` shows in real `action_info`'s
/// per-package "`<cpv>::<repo> would be built with the following:`"
/// block (`actions.py:2321` + `pkg_use_display`). `None` when the atom
/// resolves to no visible ebuild candidate (real `action_info` skips it
/// with a `- <atom>` "no matches" note -- rendered by the caller).
///
/// Deliberately narrow vs real `action_info`: **ebuild candidates only**
/// (no installed-package `was built with` + `CHOST`/`CFLAGS` vdb
/// `_aux_env_search` diff, no `(non-installed binary)` line), and no
/// `pkg_info()` phase execution. The USE display is built exactly the
/// way a `New` `GraphEntry`'s `use_expand_display` is (all flags, real
/// `pkg_use_display` for a non-installed package), so it matches the
/// `emerge -pv` `USE="…"` column verbatim.
pub fn resolve_info_candidate(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
) -> Result<Option<InfoCandidate>, Error> {
    let atom = portage_dep::parse_atom(atom_str).ok_or_else(|| Error::InvalidAtom {
        atom: atom_str.to_string(),
    })?;
    let candidates = list_candidates(repos, &atom.category, &atom.package)?;
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if visible.is_empty() {
        return Ok(None);
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
    let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &refs) else {
        return Ok(None);
    };
    let by_str: HashMap<&str, &Candidate> =
        refs.iter().copied().zip(visible.iter().copied()).collect();
    // USE-dep post-filter (`dev-libs/foo[bar]`), same as `resolve_pretend`.
    let matched: Vec<&&Candidate> = matched
        .iter()
        .filter_map(|m| by_str.get(m))
        .filter(|c| match &atom.use_deps {
            Some(use_deps) if !use_deps.is_empty() => candidate_iuse_and_use(
                c,
                &atom.category,
                &atom.package,
                config,
            )
            .is_some_and(|(iuse, use_flags)| {
                portage_dep::use_deps_satisfied(use_deps, &valid_iuse(&iuse, config), &use_flags)
            }),
            _ => true,
        })
        .collect();
    let Some(best) = matched.into_iter().copied().max_by(|a, b| {
        vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
    }) else {
        return Ok(None);
    };

    let Some((_iuse, use_flags)) =
        candidate_iuse_and_use(best, &atom.category, &atom.package, config)
    else {
        return Ok(None);
    };
    // Dedup a flag `IUSE` names twice (eclass + ebuild) -- real renders
    // `sorted(pkg.iuse.all)`, a frozenset. See `refresh_entry_use_display`.
    let mut disp_seen: HashSet<String> = HashSet::new();
    let mut disp: Vec<(String, bool)> = best
        .iuse
        .split_whitespace()
        .map(|t| t.trim_start_matches(['+', '-']).to_string())
        .filter(|f| disp_seen.insert(f.clone()))
        .map(|f| {
            let on = use_flags.contains(&f);
            (f, on)
        })
        .collect();
    disp.sort_by_key(|p| alnum_sort_key(&p.0));
    let cpv = format!("{}/{}-{}", atom.category, atom.package, best.version);
    let candidate_str = format!(
        "{}/{}-{}:{}/{}::{}",
        atom.category, atom.package, best.version, best.slot, best.sub_slot, best.repo_name
    );
    let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
        iuse: &best.iuse,
        keywords: &best.keywords,
        candidate_str: &candidate_str,
        category: &atom.category,
        package: &atom.package,
        config,
    });
    // `all_flags = true` (real `pkg_use_display` for a non-installed
    // package renders every flag), no installed diff, no reinstall flags.
    let use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &disp,
        config,
        installed: None,
        forced: &forced,
        all_flags: true,
        reinst_flags: &HashSet::new(),
    });
    // Real `action_info`'s `mypkgs` gate (`actions.py:1896-1900`): a
    // candidate is only listed in the per-package block when its EAPI is
    // 4+ and its `DEFINED_PHASES` names `info` (so `pkg_info()` can run).
    let pf = format!("{}-{}", atom.package, best.version);
    let defines_pkg_info = repo_aux_metadata(&best.repo_location, &atom.category, &pf)
        .ok()
        .and_then(|md| md.get("DEFINED_PHASES").cloned())
        .is_some_and(|dp| dp.split_whitespace().any(|p| p == "info"));
    // Real `action_info`'s own `portdb.findname(pkg.cpv, myrepo=pkg.repo)`
    // for the `pkg_info()` run -- `<repo>/<cat>/<pkg>/<pf>.ebuild`.
    let ebuild_path = best
        .repo_location
        .join(&atom.category)
        .join(&atom.package)
        .join(format!("{pf}.ebuild"));
    Ok(Some(InfoCandidate {
        cpv,
        repo_name: best.repo_name.clone(),
        use_expand_display,
        defines_pkg_info,
        ebuild_path,
    }))
}

/// One `emerge --info <atom>` per-package result -- see
/// `resolve_info_candidate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoCandidate {
    /// `category/package-version` of the best visible ebuild candidate.
    pub cpv: String,
    pub repo_name: String,
    /// Real `_display_use`'s `USE="…"` + one `VAR="…"` per non-hidden
    /// `USE_EXPAND` group -- each value already enabled-first/disabled-
    /// last ordered, exactly the pairs `pretend.rs::use_suffix` renders
    /// for a `New` entry under `-pv`.
    pub use_expand_display: Vec<(String, String)>,
    /// Whether this candidate's ebuild defines `pkg_info()` (EAPI 4+ +
    /// `info` in `DEFINED_PHASES`) -- real `action_info` only prints the
    /// `<cpv> would be built with the following:` block for such
    /// packages (`actions.py:1896`).
    pub defines_pkg_info: bool,
    /// `<repo>/<cat>/<pkg>/<pf>.ebuild` -- real `action_info`'s own
    /// `portdb.findname(pkg.cpv, myrepo=pkg.repo)` for the `pkg_info()`
    /// phase run (`actions.py:2360`).
    pub ebuild_path: PathBuf,
}

/// One `emerge --usepkg --info <atom>` result for a non-installed binary
/// package -- see `resolve_info_binary_candidate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoBinaryCandidate {
    /// `category/package-version` of the chosen binary build.
    pub cpv: String,
    pub repo_name: String,
    /// Real `pkg_use_display` for a `built=True` package -- the binary's
    /// own recorded `USE`/`IUSE`, `( )`-wrapped for the current profile's
    /// forced/masked flags (same as `InstalledInfo`).
    pub use_expand_display: Vec<(String, String)>,
    /// `$PKGDIR/<PATH>` -- the archive itself, for extracting its
    /// embedded `<pf>.ebuild` to run `pkg_info()` (real `action_info`'s
    /// `bindb.bintree.getname(cpv)` + `xpak.tbz2(...).getfile` / `gpkg
    /// (...).get_metadata`, `actions.py:2362-2384`).
    pub archive_path: PathBuf,
    /// `category`, `package`, `pf` of the chosen build -- the caller lays
    /// the extracted ebuild out as `<scratch>/<category>/<package>/
    /// <pf>.ebuild` so the phase driver can derive `CATEGORY`/`PN` from
    /// the path the way real `doebuild(tree="bintree")` derives them from
    /// the binpkg metadata.
    pub category: String,
    pub package: String,
    pub pf: String,
}

/// Whether `atom_str`'s `cat/pkg` has *any* local binary build -- real
/// `action_info`'s own `bindb.match(x.cp)` `cp_exists` probe
/// (`actions.py:1883`), which (unlike `resolve_info_binary_candidate`)
/// does not gate on `pkg_info()`: a binary-only package still isn't a
/// "no ebuilds to satisfy" error even if it can't be `pkg_info()`'d.
/// Takes the run's already-built index (backlog #123) instead of
/// re-deriving it from `&Config`.
pub fn has_local_binary_candidate(
    local_binpkg: &std::sync::Arc<BinaryIndex>,
    atom_str: &str,
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    !list_binary_candidates(local_binpkg, &atom.category, &atom.package).is_empty()
}

/// Real `action_info`'s `(portdb, "ebuild"), (bindb, "binary")` search
/// loop for the **binary** half (`actions.py:1876-1900`), reached only
/// when the atom has no installed vdb match and no visible ebuild
/// candidate, and only when `--usepkg`/`--usepkgonly` is on. A binary
/// build is chosen (highest version) *only* if its EAPI is 4+ **and**
/// its `DEFINED_PHASES` names `info` -- real never lists a binary
/// candidate it couldn't run `pkg_info()` for, so unlike the ebuild
/// path there is no separate "list but don't run" state. `None` when no
/// such build exists. Takes the run's already-built index (backlog #123)
/// instead of re-deriving it from `&Config`.
pub fn resolve_info_binary_candidate(
    config: &portage_profile::Config,
    atom_str: &str,
    local_binpkg: &std::sync::Arc<BinaryIndex>,
) -> Result<Option<InfoBinaryCandidate>, Error> {
    let atom = portage_dep::parse_atom(atom_str).ok_or_else(|| Error::InvalidAtom {
        atom: atom_str.to_string(),
    })?;
    let index = local_binpkg;
    let candidates = list_binary_candidates(index, &atom.category, &atom.package);
    if candidates.is_empty() {
        return Ok(None);
    }
    let candidate_strs: Vec<String> = candidates
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &refs) else {
        return Ok(None);
    };
    let by_str: HashMap<&str, &Candidate> = refs.iter().copied().zip(candidates.iter()).collect();
    // Highest version first, then take the first that defines pkg_info()
    // -- real `matches.reverse()` + `break` on the first that passes.
    let mut ranked: Vec<&Candidate> = matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .collect();
    ranked.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));
    for best in ranked {
        let Some(metadata) = read_binary_metadata_instance(
            index,
            &atom.category,
            &atom.package,
            &best.version,
            best.build_id.as_deref(),
        ) else {
            continue;
        };
        let eapi = metadata.get("EAPI").map(String::as_str).unwrap_or("0");
        let defines_info = metadata
            .get("DEFINED_PHASES")
            .is_some_and(|dp| dp.split_whitespace().any(|p| p == "info"));
        if matches!(eapi, "0" | "1" | "2" | "3") || !defines_info {
            continue;
        }
        let Some(rel_path) = metadata.get("PATH").filter(|p| !p.is_empty()) else {
            continue;
        };
        let Some((_iuse, use_flags)) =
            candidate_iuse_and_use(best, &atom.category, &atom.package, config)
        else {
            continue;
        };
        let mut disp_seen: HashSet<String> = HashSet::new();
        let mut disp: Vec<(String, bool)> = best
            .iuse
            .split_whitespace()
            .map(|t| t.trim_start_matches(['+', '-']).to_string())
            .filter(|f| disp_seen.insert(f.clone()))
            .map(|f| {
                let on = use_flags.contains(&f);
                (f, on)
            })
            .collect();
        disp.sort_by_key(|p| alnum_sort_key(&p.0));
        let cpv = format!("{}/{}-{}", atom.category, atom.package, best.version);
        let candidate_str = format!(
            "{}/{}-{}:{}/{}::{}",
            atom.category, atom.package, best.version, best.slot, best.sub_slot, best.repo_name
        );
        let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse: &best.iuse,
            keywords: &best.keywords,
            candidate_str: &candidate_str,
            category: &atom.category,
            package: &atom.package,
            config,
        });
        let use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &disp,
            config,
            installed: None,
            forced: &forced,
            all_flags: true,
            reinst_flags: &HashSet::new(),
        });
        return Ok(Some(InfoBinaryCandidate {
            cpv,
            repo_name: best.repo_name.clone(),
            use_expand_display,
            archive_path: Path::new(&config.pkgdir).join(rel_path),
            category: atom.category.clone(),
            package: atom.package.clone(),
            pf: format!("{}-{}", atom.package, best.version),
        }));
    }
    Ok(None)
}

/// Real `mydesiredvars` -- the `action_info` per-package block for an
/// **installed** package prints each of these whose vdb-recorded value
/// differs from the current config, and an `Unset: …` line for the ones
/// with no recorded value (`actions.py:2333-2344`).
pub(crate) const INFO_INSTALLED_VARS: &[&str] =
    &["CHOST", "CFLAGS", "CXXFLAGS", "FEATURES", "LDFLAGS"];

/// Real `vartree._aux_env_search` (`dbapi/vartree.py:1059-1126`): the
/// saved build-time environment of an installed package, read from its
/// vdb `environment.bz2` -- a bzip2-compressed bash `set`-style env
/// dump. Real decompresses by spawning `bzip2 -d -c` (or
/// `PORTAGE_BUNZIP2_COMMAND -c`); this crate is subprocess-free, so it
/// decompresses in-process with the pure-Rust `bzip2` backend instead
/// (see this crate's `Cargo.toml` -- zero C linkage, musl story
/// untouched). Returns the present `wanted` vars only; a missing or
/// unreadable `environment.bz2` yields an empty map (real's own
/// `if not os.path.isfile(env_file): return {}` -- and a corrupt
/// archive breaks real's pipe the same effective way), which the caller
/// reports as all-`Unset:`, exactly like real.
pub fn read_vdb_env_vars(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    wanted: &[&str],
) -> HashMap<String, String> {
    use std::io::Read as _;
    let key = resolve_vdb_entry(root, category, package, version);
    let Ok(Some(compressed)) = portage_vdb::for_root(root).read_file(&key, "environment.bz2")
    else {
        return HashMap::new();
    };
    let mut decoder = bzip2::read::BzDecoder::new(&compressed[..]);
    let mut text = String::new();
    if decoder.read_to_string(&mut text).is_err() {
        return HashMap::new();
    }
    parse_env_assignments(&text, wanted)
}

/// Real `_aux_env_search`'s own assignment scanner
/// (`vartree.py:1082-1123`, "borrowed from
/// `filter-bash-environment.py`"): `var_assign_re =
/// (^|^declare\s+-\S+\s+|^declare\s+|^export\s+)([^=\s]+)=("|\')?(.*)$`
/// with `close_quote_re = (\\"|"|\')\s*$`. A quoted value whose closer
/// is not on the same line consumes following lines verbatim until the
/// closer (or EOF); the value is then right-stripped with the closing
/// quote dropped -- even at EOF without a closer, real still drops the
/// last char. Later assignments overwrite earlier ones. Only `wanted`
/// keys are returned, but continuation lines are consumed regardless
/// (real builds the value before its `if key in variables` check, so a
/// skipped multi-line value must not leak its body lines as fresh
/// assignments).
pub(crate) fn parse_env_assignments(text: &str, wanted: &[&str]) -> HashMap<String, String> {
    fn strip_prefix(line: &str) -> &str {
        // Real alternation order, longest first (regex backtracking
        // reaches the same): `declare -FLAGS<ws>`, `declare<ws>`,
        // `export<ws>`, plain. `<ws>` is any whitespace run (real
        // `\s+`), and a failed dash-form backtracks to the plain
        // `declare<ws>` form.
        for word in ["declare", "export"] {
            if let Some(rest) = line.strip_prefix(word)
                && rest.starts_with(|c: char| c.is_whitespace())
            {
                let after_ws = rest.trim_start();
                if word == "declare"
                    && let Some(flags) = after_ws.strip_prefix('-')
                {
                    // `-\S+\s+`: dash, non-empty non-whitespace token,
                    // then whitespace.
                    let token_len = flags
                        .bytes()
                        .take_while(|b| !b.is_ascii_whitespace())
                        .count();
                    if token_len > 0 && flags[token_len..].starts_with(|c: char| c.is_whitespace())
                    {
                        return after_ws[1 + token_len..].trim_start();
                    }
                }
                return after_ws;
            }
        }
        line
    }
    fn closer_at_end(s: &str, quote: char) -> bool {
        // Real `have_end_quote`: one leftmost `close_quote_re` search;
        // an escaped `\"` (backslash + quote) at the very end does NOT
        // close, a bare trailing quote does.
        let t = s.trim_end();
        let mut chars = t.chars().rev();
        if chars.next() != Some(quote) {
            return false;
        }
        chars.next() != Some('\\')
    }
    let mut out = HashMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        // `([^=\s]+)=`: non-empty key of non-`=`/non-whitespace,
        // immediately followed by `=`.
        let body = strip_prefix(line);
        let eq = body
            .bytes()
            .take_while(|b| *b != b'=' && !b.is_ascii_whitespace())
            .count();
        if eq == 0 || body.as_bytes().get(eq) != Some(&b'=') {
            continue;
        }
        let (key, after_eq) = (&body[..eq], &body[eq + 1..]);
        let (quote, rest) = match after_eq.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), &after_eq[q.len_utf8()..]),
            _ => (None, after_eq),
        };
        let value = match quote {
            None => after_eq.trim_end().to_string(),
            Some(q) => {
                if closer_at_end(rest, q) {
                    let t = rest.trim_end();
                    t[..t.len() - 1].to_string()
                } else {
                    // Real appends each raw stdout line *with* its
                    // `\n` (only the first fragment, real's regex
                    // group, has none) -- `lines()` stripped them, so
                    // the non-closing ones are restored.
                    let mut joined = rest.to_string();
                    for next in lines.by_ref() {
                        joined.push_str(next);
                        if closer_at_end(next, q) {
                            break;
                        }
                        joined.push('\n');
                    }
                    let t = joined.trim_end();
                    t[..t.len().saturating_sub(1)].to_string()
                }
            }
        };
        if wanted.contains(&key) {
            out.insert(key.to_string(), value);
        }
    }
    out
}

/// Every installed vdb entry `atom_str` matches, with the data real
/// `action_info` prints for an installed package: `<cpv>::<repo> was
/// built with the following:` + its `USE="…"` line (from the vdb `USE`/
/// `IUSE` files) + the `mydesiredvars` (`INFO_INSTALLED_VARS`) whose
/// stored value differs from the current config, plus the ones with no
/// stored value. Real `action_info` checks the vdb **first**: any
/// installed match short-circuits the ebuild/binary lookup
/// (`actions.py:1869-1875`).
///
/// The `mydesiredvars` values come from the package's vdb
/// `environment.bz2` via [`read_vdb_env_vars`] -- real
/// `_aux_env_search`, and *only* from there (real never consults the
/// individual `build-info` files for these five). A var absent from the
/// saved env -- including a vdb entry with no `environment.bz2` at
/// all -- is `Unset:`, even if a same-named individual file exists; a
/// present-but-empty value that matches the (empty) current config
/// prints nowhere at all (real's own `None` vs `split()`-equal
/// branches, `actions.py:2336-2342`). The `USE` line's `( )`
/// force/mask wrap (real `pkg_use_display`) is shipped, via the same
/// `forced_or_masked_flags` the ebuild-candidate side
/// (`resolve_info_candidate`) already uses.
pub fn resolve_installed_info(
    root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
) -> Vec<InstalledInfo> {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return Vec::new();
    };
    let installed = installed_candidates(root, &atom.category, &atom.package);
    let cpv_strs: Vec<String> = installed
        .iter()
        .map(|(v, s, ss)| {
            format!(
                "{}/{}-{v}:{s}/{ss}::{}",
                atom.category,
                atom.package,
                installed_pkg_repo(root, &atom.category, &atom.package, v)
            )
        })
        .collect();
    let refs: Vec<&str> = cpv_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &refs) else {
        return Vec::new();
    };
    let by_str: HashMap<&str, &(String, String, String)> =
        refs.iter().copied().zip(installed.iter()).collect();

    let mut out = Vec::new();
    for m in matched {
        let Some((version, _slot, _sub)) = by_str.get(m).copied() else {
            continue;
        };
        let repo_name = installed_pkg_repo(root, &atom.category, &atom.package, version);
        let vdb_use = read_vdb_flag_set(root, &atom.category, &atom.package, version, "USE");
        let vdb_iuse_raw = read_vdb_string(root, &atom.category, &atom.package, version, "IUSE");
        let mut disp: Vec<(String, bool)> = vdb_iuse_raw
            .split_whitespace()
            .map(|t| t.trim_start_matches(['+', '-']).to_string())
            .map(|f| {
                let on = vdb_use.contains(&f);
                (f, on)
            })
            .collect();
        disp.sort_by_key(|p| alnum_sort_key(&p.0));
        disp.dedup();
        // Real `pkg_use_display`'s `( )` force/mask wrap: same
        // `forced_or_masked_flags` the ebuild-candidate side already
        // uses (`resolve_info_candidate`), fed from the vdb's own
        // recorded `IUSE`/`KEYWORDS` instead of a tree candidate's.
        let vdb_keywords: Vec<String> =
            read_vdb_string(root, &atom.category, &atom.package, version, "KEYWORDS")
                .split_whitespace()
                .map(String::from)
                .collect();
        let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse: &vdb_iuse_raw,
            keywords: &vdb_keywords,
            candidate_str: m,
            category: &atom.category,
            package: &atom.package,
            config,
        });
        let use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &disp,
            config,
            installed: None,
            forced: &forced,
            all_flags: true,
            reinst_flags: &HashSet::new(),
        });

        // Real `mydesiredvars` sourcing (`actions.py:2335-2342` over
        // `_aux_env_search`): the saved env, or nothing -- a var the
        // env lacks is `Unset:`, and a present value that matches the
        // current config (including empty-equals-empty) prints nowhere.
        // The printed value is real's raw `myval`, untrimmed.
        let env_vars = read_vdb_env_vars(
            root,
            &atom.category,
            &atom.package,
            version,
            INFO_INSTALLED_VARS,
        );
        let mut differing_vars = Vec::new();
        let mut unset_vars = Vec::new();
        for &var in INFO_INSTALLED_VARS {
            let Some(stored) = env_vars.get(var) else {
                unset_vars.push(var.to_string());
                continue;
            };
            let current = config.other_vars.get(var).map(String::as_str).unwrap_or("");
            if stored.split_whitespace().ne(current.split_whitespace()) {
                differing_vars.push((var.to_string(), stored.clone()));
            }
        }

        // Real `action_info`'s `if metadata["DEFINED_PHASES"]: if "info"
        // not in ...: continue` -- names `info`, OR is entirely empty
        // (falsy-check quirk).
        let vdb_defined_phases = read_vdb_string(
            root,
            &atom.category,
            &atom.package,
            version,
            "DEFINED_PHASES",
        );
        let vdb_defined_phases = vdb_defined_phases.trim();
        let defines_pkg_info = vdb_defined_phases.is_empty()
            || vdb_defined_phases.split_whitespace().any(|p| p == "info");

        out.push(InstalledInfo {
            cpv: format!("{}/{}-{version}", atom.category, atom.package),
            repo_name,
            use_expand_display,
            differing_vars,
            unset_vars,
            defines_pkg_info,
            category: atom.category.clone(),
            package: atom.package.clone(),
            version: version.clone(),
        });
    }
    out
}

/// One `emerge --info <atom>` result for an installed package -- see
/// `resolve_installed_info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledInfo {
    pub cpv: String,
    pub repo_name: String,
    /// The vdb-recorded `USE="…"` display, same shape as
    /// `InfoCandidate::use_expand_display`.
    pub use_expand_display: Vec<(String, String)>,
    /// `INFO_INSTALLED_VARS` whose vdb value differs from the current
    /// config, as `(name, stored value)` -- rendered `NAME="value"`.
    pub differing_vars: Vec<(String, String)>,
    /// `INFO_INSTALLED_VARS` with no vdb value -- the `Unset: …` line.
    pub unset_vars: Vec<String>,
    /// Whether `action_info` attempts `>>> Attempting to run pkg_info()`
    /// for this installed match (`actions.py:2350-2356`). Unlike the
    /// ebuild-candidate path, an installed package is listed in `mypkgs`
    /// regardless; the phase step is gated on real's own
    /// `if metadata["DEFINED_PHASES"]: if "info" not in ...split(): continue`
    /// -- so it fires when `DEFINED_PHASES` *names* `info`, **and also**
    /// when `DEFINED_PHASES` is entirely empty (real's falsy-check quirk
    /// -- an old / hand-written vdb entry with no `DEFINED_PHASES` file at
    /// all). A `DEFINED_PHASES` of `"-"` (the modern "no phases" marker)
    /// is truthy and does not name `info`, so it does *not* fire.
    pub defines_pkg_info: bool,
    /// `category`, `package`, `version` of this vdb entry -- real
    /// `vardb.findname(pkg.cpv)` locates `<pf>.ebuild`; portuale runs the
    /// `pkg_info()` phase from the vdb-saved `environment.bz2` +
    /// `<pf>.ebuild` (`ebuild_merge::run_vdb_saved_env_phase`), the same
    /// path `--config` uses.
    pub category: String,
    pub package: String,
    pub version: String,
}
