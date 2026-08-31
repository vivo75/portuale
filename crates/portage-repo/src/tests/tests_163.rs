/// Result/display-assembly unit tests: each test observes the
/// assembled structures directly, never a contract pin's text.
use crate::*;
use portage_util::TempDir;

/// Minimal merge-bound `GraphEntry` for the assembly legs (slot and
/// `new_slot` as the leg needs; everything display-side left at its
/// empty default). Mirrors `tests_162::entry_162`, which must stay
/// untouched.
fn entry_163(
    cat: &str,
    pkg: &str,
    outcome: PretendOutcome,
    required_by: &[(&str, &str)],
) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: cat.to_string(),
        package: pkg.to_string(),
        outcome,
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: required_by
            .iter()
            .map(|(c, p)| (c.to_string(), p.to_string()))
            .collect(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}

fn new_163(version: &str) -> PretendOutcome {
    PretendOutcome::New {
        version: version.to_string(),
    }
}

fn installed_163(version: &str) -> PretendOutcome {
    PretendOutcome::AlreadyInstalled {
        version: version.to_string(),
    }
}

/// Fresh unique scratch dir per test (vdb + repo live under it, so
/// the per-root memo caches never see a reused path). Mirrors
/// `tests_162::dir_162`, which must stay untouched.
fn dir_163(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-163-{tag}")).keep()
}

/// Fixed "amd64"-only, no-overrides config, mirroring `tests_162`'
/// `cfg_162` (a `KEYWORDS="amd64"` scratch ebuild is always
/// visible).
fn cfg_163() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}
/// Arguments of [`write_pkg_163`].
struct WritePkg163Args<'a> {
    repo: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    iuse: &'a str,
    rdepend: &'a str,
    keywords: &'a str,
}

/// One scratch ebuild plus its md5-cache entry, in the
/// shape: `SLOT`, `KEYWORDS`, `IUSE`, `RDEPEND` lines in both.
fn write_pkg_163(args: WritePkg163Args<'_>) {
    let WritePkg163Args {
        repo,
        cp,
        pv,
        slot,
        iuse,
        rdepend,
        keywords,
    } = args;
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = format!(
        "EAPI=8\nDESCRIPTION=\"163 result assembly\"\nSLOT=\"{slot}\"\nKEYWORDS=\"{keywords}\"\n"
    );
    if !iuse.is_empty() {
        writeln!(body, "IUSE=\"{iuse}\"").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=163 result assembly\nEAPI=8\n".to_string();
    if !iuse.is_empty() {
        writeln!(entry, "IUSE={iuse}").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(entry, "RDEPEND={rdepend}").unwrap();
    }
    writeln!(entry, "KEYWORDS={keywords}\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
}

/// Single scratch repo (name `testrepo`, priority 0, main) holding
/// `(cp, version, slot, iuse, rdepend, keywords)` ebuilds.
fn repo_pkgs_163(dir: &Path, pkgs: &[(&str, &str, &str, &str, &str, &str)]) -> Vec<RepoConfig> {
    let repo = dir.join("repo");
    for (cp, pv, slot, iuse, rdepend, keywords) in pkgs {
        write_pkg_163(WritePkg163Args {
            repo: &repo,
            cp,
            pv,
            slot,
            iuse,
            rdepend,
            keywords,
        });
    }
    vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo,
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }]
}

/// One installed instance under `<root>/var/db/pkg` with the given
/// `SLOT` plus caller-supplied field files (`USE`, `IUSE`,
/// `RDEPEND`, `repository`, ...). Mirrors `tests_162::install_162`,
/// which must stay untouched.
fn install_163(root: &Path, cat: &str, pf: &str, slot: &str, files: &[(&str, &str)]) {
    let d = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    for (name, contents) in files {
        std::fs::write(d.join(name), contents.as_bytes()).unwrap();
    }
}
// ---- shared resolve_pretend harness ----

/// Option bundle for `resolve_163`, mirroring `resolve_pretend`'s
/// trailing flags; defaults are the plain top-level selective resolve.
#[derive(Default)]
struct RpOpts163 {
    newuse: bool,
    changed_use: bool,
    update: bool,
    excluded: Vec<String>,
    changed_deps: bool,
    with_bdeps: bool,
    changed_slot: bool,
    selective: bool,
    is_top_level: bool,
    usepkg: bool,
    usepkgonly: bool,
    binpkg_respect_use: bool,
    usepkg_exclude: Vec<String>,
    usepkg_include: Vec<String>,
    rebuilt_binaries: bool,
    rebuilt_binaries_timestamp: Option<u64>,
    newrepo: bool,
    empty: bool,
    getbinpkg: bool,
    autounmask_keywords: bool,
    autounmask_use: bool,
    autounmask_license: bool,
    autounmask_masks: bool,
    extra_constraints: Vec<String>,
}

impl RpOpts163 {
    fn top() -> Self {
        RpOpts163 {
            selective: true,
            is_top_level: true,
            with_bdeps: true,
            ..Default::default()
        }
    }
    fn dep() -> Self {
        RpOpts163 {
            with_bdeps: true,
            ..Default::default()
        }
    }
}

/// Direct `resolve_pretend` call over a scratch repo + scratch vdb;
/// `binpkgs` stages `scanned_binpkgs` records like `ResolveCtx::new`.
fn resolve_163(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    atom: &str,
    o: &RpOpts163,
) -> PretendOutcome {
    resolve_pretend(
        &SelectSource {
            repos,
            root,
            config,
            local_binpkg: &build_local_binpkg_index(config),
        },
        atom,
        &SelectOptions {
            newuse: o.newuse,
            changed_use: o.changed_use,
            update: o.update,
            excluded: &o.excluded,
            changed_deps: o.changed_deps,
            with_bdeps: o.with_bdeps,
            changed_slot: o.changed_slot,
            selective: o.selective,
            is_top_level: o.is_top_level,
            usepkg: o.usepkg,
            usepkgonly: o.usepkgonly,
            binpkg_respect_use: o.binpkg_respect_use,
            usepkg_exclude: &o.usepkg_exclude,
            usepkg_include: &o.usepkg_include,
            rebuilt_binaries: o.rebuilt_binaries,
            rebuilt_binaries_timestamp: o.rebuilt_binaries_timestamp,
            newrepo: o.newrepo,
            empty: o.empty,
            getbinpkg: o.getbinpkg,
            autounmask_keywords: o.autounmask_keywords,
            autounmask_use: o.autounmask_use,
            autounmask_license: o.autounmask_license,
            autounmask_masks: o.autounmask_masks,
            extra_constraints: &o.extra_constraints,
        },
    )
    .unwrap_or_else(|e| panic!("resolve_163({atom}) failed: {e}"))
}

fn binpkg_163(cpv: &str, slot: &str, extra: &[(&str, &str)]) -> HashMap<String, String> {
    let mut m = HashMap::from([
        ("CPV".to_string(), cpv.to_string()),
        ("SLOT".to_string(), slot.to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
    ]);
    for (k, v) in extra {
        m.insert(k.to_string(), v.to_string());
    }
    m
}

/// Each reinstall trigger also fires through the best-version
/// path (past `--update`, which bypasses the `avoid_update`
/// shortcut): the gate and its disjunction arm must agree.
#[test]
fn resolve_pretend_triggers_fire_through_the_best_version_path() {
    // Changed RDEPEND under --update.
    let dir = dir_163("rp-bdeps");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rp", "1.0", "0", "", "dev-libs/other", "amd64")],
    );
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("RDEPEND", "dev-libs/different")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.update = true;
    o.selective = false;
    o.changed_deps = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
    // Changed sub-slot under --update, option on and off.
    for (tag, changed_slot, expected) in [
        (
            "on",
            true,
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: true,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            },
        ),
        ("off", false, installed_163("1.0")),
    ] {
        let dir = dir_163(&format!("rp-bslot-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0/1", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0/0", &[]);
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.update = true;
        o.selective = false;
        o.changed_slot = changed_slot;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Changed repo under --update.
    let dir = dir_163("rp-brepo");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[("repository", "other")]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.update = true;
    o.selective = false;
    o.newrepo = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: true,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each reinstall trigger fires on its own through the
/// `avoid_update` shortcut as well: option-on reinstalls, option-off
/// keeps the installed version.
#[test]
fn resolve_pretend_each_reinstall_trigger_fires_on_its_own() {
    // Changed RDEPEND, option on and off.
    for (tag, changed_deps, deps_changed) in [("on", true, true), ("off", false, false)] {
        let dir = dir_163(&format!("rp-tdeps-{tag}"));
        let repos = repo_pkgs_163(
            &dir,
            &[("dev-libs/rp", "1.0", "0", "", "dev-libs/other", "amd64")],
        );
        install_163(
            &dir,
            "dev-libs",
            "rp-1.0",
            "0",
            &[("RDEPEND", "dev-libs/different")],
        );
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.changed_deps = changed_deps;
        o.selective = false;
        let expected = if deps_changed {
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: true,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            }
        } else {
            installed_163("1.0")
        };
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Changed sub-slot with the same main slot: the `slot_changed`
    // probe compares the full slot. Option on and off.
    for (tag, changed_slot, slot_changed) in [("on", true, true), ("off", false, false)] {
        let dir = dir_163(&format!("rp-tslot-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0/1", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0/0", &[]);
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.changed_slot = changed_slot;
        o.selective = false;
        let expected = if slot_changed {
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: true,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            }
        } else {
            installed_163("1.0")
        };
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Changed repo, option on and off.
    for (tag, newrepo, new_repo) in [("on", true, true), ("off", false, false)] {
        let dir = dir_163(&format!("rp-trepo-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[("repository", "other")]);
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.newrepo = newrepo;
        o.selective = false;
        let expected = if new_repo {
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: true,
                slot_operator_rebuild: false,
            }
        } else {
            installed_163("1.0")
        };
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `--binpkg-changed-deps` drops a binary built against different
/// dependencies: the stale binary must not win over the installed
/// ebuild.
#[test]
fn resolve_pretend_changed_deps_drops_a_stale_binary() {
    let dir = dir_163("rp-bindeps");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-2.0", "0", &[]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163(
        "dev-libs/rp-2.0",
        "1",
        &[("RDEPEND", "dev-libs/other")],
    )]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Reinstall {
            version: "2.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--rebuilt-binaries` with a newer binary build reinstalls with
/// the rebuild flag; without the flag the same shape reinstalls
/// bare (the `--emptytree` arm).
#[test]
fn resolve_pretend_rebuilt_binaries_reinstalls_on_newer_build() {
    let bare = PretendOutcome::Reinstall {
        version: "1.0".to_string(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let rebuilt = PretendOutcome::Reinstall {
        version: "1.0".to_string(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: true,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    for (tag, rebuilt_binaries, expected) in [("rebuilt", true, rebuilt), ("bare", false, bare)] {
        let dir = dir_163(&format!("rp-rebuilt-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[("BUILD_TIME", "1000")]);
        let mut config = cfg_163();
        config.scanned_binpkgs = Some(vec![binpkg_163(
            "dev-libs/rp-1.0",
            "0",
            &[("BUILD_TIME", "2000")],
        )]);
        // The `rebuilt` arm runs selective under `--update` so the flag
        // alone decides; the `bare` arm goes through `--emptytree`.
        let mut o = RpOpts163::top();
        if tag == "bare" {
            o.selective = false;
            o.is_top_level = false;
            o.empty = true;
        }
        o.update = true;
        o.usepkg = true;
        o.rebuilt_binaries = rebuilt_binaries;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
