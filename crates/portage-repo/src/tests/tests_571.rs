//! The category allow-list everywhere, not just `--regen`.
//!
//! Portage's `settings.categories` (the incremental union of every repo's
//! `profiles/categories` plus `/etc/portage/categories`, `config.py:948-960`)
//! gates ebuild discovery: `cp_all` walks only listed categories
//! (`porttree.py:1097-1128`), `cp_list` prints `!!! '<cp>' has a category
//! that is not listed in ...` once and yields nothing for an unlisted one
//! with ebuilds on disk (`porttree.py:1156,1216-1225`), and short-name
//! expansion iterates the allowed categories (`cpv_expand.py`,
//! `depgraph.py:4948-4975`). Installed/binary databases keep categories
//! derived from their own contents (`dbapi/__init__.py:38-49`).
//!
//! Portage host captures agree: `p571-hidden` (valid ebuild, listed
//! nowhere) is unsatisfiable everywhere with the `!!!` on direct/dep
//! lookups and silence on short-name/`--search`; `p571-shared` (ebuild
//! in the main repo, listed only by the overlay) merges — the union
//! covers every repo.

use super::*;
use md5::Digest as _;
use portage_util::TempDir;

/// One minimal valid ebuild plus its md5-cache entry (real `_md5_`, like
/// every committed fixture entry). `extra` appends metadata lines
/// (`RDEPEND=...` for the needer).
fn write_p571_ebuild(repo: &std::path::Path, cat: &str, pkg: &str, extra: &str) {
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let body =
        format!("EAPI=8\nDESCRIPTION=\"571 {pkg}\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n{extra}");
    std::fs::write(dir.join(format!("{pkg}-1.0.ebuild")), &body).unwrap();
    let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(
        cachedir.join(format!("{pkg}-1.0")),
        format!("DEFINED_PHASES=-\nDESCRIPTION=571 {pkg}\nEAPI=8\nKEYWORDS=amd64\nSLOT=0\n{extra}_md5_={md5}\n"),
    )
    .unwrap();
}

fn repo_config_571(name: &str, location: std::path::PathBuf, is_main: bool) -> RepoConfig {
    RepoConfig {
        name: name.to_string(),
        location,
        priority: if is_main { -1000 } else { 0 },
        is_main,
        masters: Vec::new(),
        profile_formats: Vec::new(),
        cache_formats: Vec::new(),
        aliases: Vec::new(),
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: Vec::new(),
    }
}

/// A main repo listing only `listed` (but carrying `hidden` and `shared`
/// ebuilds too) plus an overlay listing only `shared`: the allow-list
/// shape in miniature. Returns the base dir, the repos, and the config root
/// (no user categories file).
fn mini_571() -> (TempDir, Vec<RepoConfig>, std::path::PathBuf) {
    let base = TempDir::new("portuale-571-mini");
    let _ = std::fs::remove_dir_all(&base);
    let main = base.join("main");
    let overlay = base.join("overlay");
    let cfg = base.join("cfg");
    for (repo, cats) in [(&main, "listed\n"), (&overlay, "shared\n")] {
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::write(repo.join("profiles").join("categories"), cats).unwrap();
    }
    write_p571_ebuild(&main, "listed", "listed", "");
    write_p571_ebuild(&main, "listed", "need", "RDEPEND=hidden/hidden\n");
    write_p571_ebuild(&main, "hidden", "hidden", "");
    write_p571_ebuild(&main, "shared", "shared", "");
    let repos = vec![
        repo_config_571("main571", main, true),
        repo_config_571("overlay571", overlay, false),
    ];
    (base, repos, cfg)
}

/// `all_cp` walks only listed categories: the unlisted `hidden` dir never
/// surfaces, while the overlay-listed `shared` does. Portage:
/// `portdbapi.cp_all(categories=settings.categories)`.
#[test]
fn all_cp_lists_only_listed_categories() {
    let (_base, repos, cfg) = mini_571();
    let categories = Categories::load(&repos, &cfg);
    assert_eq!(
        all_cp(&repos, &categories.set),
        vec![
            "listed/listed".to_string(),
            "listed/need".to_string(),
            "shared/shared".to_string(),
        ]
    );
    let _ = std::fs::remove_dir_all(&_base);
}

/// The gate mirrors `cp_list`: a listed cp yields candidates; an unlisted
/// cp with ebuilds yields nothing (marking the once-per-cp record the
/// `!!!` line keys on); an unlisted cp with no ebuilds yields nothing
/// without marking (Portage prints no notice then either).
#[test]
fn gate_returns_empty_for_unlisted_categories() {
    let (_base, repos, cfg) = mini_571();
    let categories = Categories::load(&repos, &cfg);
    let gate = CategoryGate::new(&repos, &categories, &cfg);
    assert_eq!(gate.repo_candidates("shared", "shared").unwrap().len(), 1);
    assert!(categories.warned.borrow().is_empty());
    assert!(gate.repo_candidates("hidden", "hidden").unwrap().is_empty());
    assert_eq!(
        categories.warned.borrow().clone(),
        HashSet::from([("hidden".to_string(), "hidden".to_string())])
    );
    // Second lookup of the same cp: still empty (the record suppresses
    // the repeat notice).
    assert!(gate.repo_candidates("hidden", "hidden").unwrap().is_empty());
    assert_eq!(categories.warned.borrow().len(), 1);
    // No ebuilds on disk: empty and silent.
    assert!(
        gate.repo_candidates("hidden", "nosuchpkg")
            .unwrap()
            .is_empty()
    );
    assert_eq!(categories.warned.borrow().len(), 1);
    let _ = std::fs::remove_dir_all(&_base);
}

/// Portage normalises `PORTAGE_CONFIGROOT` to exactly one trailing slash
/// before the unlisted-category line renders `%setc/portage/categories`
/// from it: `/` prints `/etc/portage/categories`, a root with a trailing
/// slash prints unchanged. Portage: LocationsManager.py:79-83,
/// porttree.py:1216-1225.
#[test]
fn unlisted_category_line_renders_config_root_with_one_trailing_slash() {
    let (_base, repos, cfg) = mini_571();
    let categories = Categories::load(&repos, &cfg);
    for (root, want) in [
        ("/", "/etc/portage/categories"),
        ("/tmp/cfg", "/tmp/cfg/etc/portage/categories"),
        ("/tmp/cfg/", "/tmp/cfg/etc/portage/categories"),
    ] {
        let gate = CategoryGate::new(&repos, &categories, std::path::Path::new(root));
        assert_eq!(
            gate.unlisted_category_line("hidden", "hidden"),
            format!("\n!!! 'hidden/hidden' has a category that is not listed in {want}")
        );
    }
    std::fs::remove_dir_all(&_base).unwrap();
}

/// Two walks sharing one `Categories` fire the `!!!` once: the second
/// walk's lookup finds the cp in the shared once-per-cp record and stays
/// silent. Portage: the frozen `xcache["cp-list"]` memoises `cp_list` per
/// process (porttree.py:1138-1151).
#[test]
fn two_walks_sharing_categories_warn_once() {
    let (_base, repos, cfg) = mini_571();
    let categories = Categories::load(&repos, &cfg);
    let first = CategoryGate::new(&repos, &categories, &cfg);
    let second = CategoryGate::new(&repos, &categories, &cfg);
    assert!(
        first
            .repo_candidates("hidden", "hidden")
            .unwrap()
            .is_empty()
    );
    assert_eq!(categories.warned.borrow().len(), 1);
    assert!(
        second
            .repo_candidates("hidden", "hidden")
            .unwrap()
            .is_empty()
    );
    assert_eq!(categories.warned.borrow().len(), 1);
    std::fs::remove_dir_all(&_base).unwrap();
}

/// End to end at the resolver level: a second walk built from a cloned
/// request shares the first walk's record, so the `!!!` fires once across
/// both walks. Portage: the frozen `xcache["cp-list"]` memoises `cp_list`
/// per process (porttree.py:1138-1151).
#[test]
fn cloned_request_shares_the_warning_record_across_walks() {
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let new_req = || {
        ResolveRequest::new(
            &root,
            &root,
            &["p571-hidden/p571-hidden".to_string()],
            &config,
            &root.join("distfiles"),
        )
    };
    let req = new_req();
    let ctx1 = ResolveCtx::new(&req).expect("first walk builds");
    let mut req2 = new_req();
    req2.categories = Some(std::rc::Rc::clone(&ctx1.categories));
    let ctx2 = ResolveCtx::new(&req2).expect("second walk builds");
    assert!(
        ctx1.gate()
            .repo_candidates("p571-hidden", "p571-hidden")
            .unwrap()
            .is_empty()
    );
    assert!(
        ctx2.gate()
            .repo_candidates("p571-hidden", "p571-hidden")
            .unwrap()
            .is_empty()
    );
    assert_eq!(ctx1.categories.warned.borrow().len(), 1);
}

/// End to end over the shared fixtures (whose committed
/// `profiles/categories` files list every category but `p571-hidden`):
/// the unlisted direct target aborts with no entries, the overlay-listed
/// one merges, and the dependency on the unlisted cp aborts.
/// End to end over the shared fixtures (whose committed
/// `profiles/categories` files list every category but `p571-hidden`):
/// the unlisted direct target and the dependency on it fail resolve
/// with the plain miss, while the overlay-listed one merges.
#[test]
fn resolve_honours_the_allow_list() {
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let resolve = |atom: &str| {
        ResolveRequest::new(
            &root,
            &root,
            &[atom.to_string()],
            &config,
            &root.join("distfiles"),
        )
        .resolve()
    };
    let hidden = resolve("p571-hidden/p571-hidden").unwrap_err();
    assert!(
        hidden.to_string().contains("no ebuilds"),
        "unlisted direct target misses: {hidden}"
    );
    let shared = resolve("p571-shared/p571-shared").expect("overlay-listed merges");
    assert_eq!(shared.entries.len(), 1);
    assert_eq!(
        shared.entries[0].outcome,
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
    let need = resolve("p571-listed/p571-need").expect("dep miss aborts, not errors");
    assert!(matches!(need.outcome, ResolveOutcome::Aborted { .. }));
    assert!(
        need.entries.iter().any(|e| e.category == "p571-hidden"
            && matches!(e.outcome, PretendOutcome::NoVisibleCandidate)),
        "the unlisted dep has no visible candidate"
    );
}
