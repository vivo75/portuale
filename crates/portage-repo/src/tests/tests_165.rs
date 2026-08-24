/// Graph-input unit tests (mutation cluster 5): walk/installed/vdb,
/// merge-order, blockers, use. Each test observes the function's own
/// return value. Portage grounding: 3.0.82.2 (`3rdparty/portage`).
use crate::*;
use portage_util::TempDir;

/// Fresh unique scratch dir per test (vdb + repo live under it, so
/// the per-root memo caches -- `all_installed_packages`,
/// `installed_candidates` -- never see a reused path).
fn dir_165(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-165-{tag}")).keep()
}

fn install_165(root: &Path, cat: &str, pf: &str, slot: &str, files: &[(&str, &str)]) {
    let d = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    for (name, contents) in files {
        std::fs::write(d.join(name), format!("{contents}\n")).unwrap();
    }
}

fn cfg_165() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}
/// Arguments of [`pending_165`].
struct Pending165Args<'a> {
    atom: &'a str,
    strong: bool,
    buildtime: bool,
    target: (&'a str, &'a str),
    owner: (&'a str, &'a str),
    owner_version: &'a str,
    owner_merging: bool,
    owner_installed: bool,
}

fn pending_165(args: Pending165Args<'_>) -> PendingBlocker {
    let Pending165Args {
        atom,
        strong,
        buildtime,
        target,
        owner,
        owner_version,
        owner_merging,
        owner_installed,
    } = args;
    PendingBlocker {
        atom_str: atom.to_string(),
        strong,
        buildtime,
        target_category: target.0.to_string(),
        target_package: target.1.to_string(),
        owner_key: (owner.0.to_string(), owner.1.to_string()),
        owner_version: owner_version.to_string(),
        owner_merging,
        owner_installed,
    }
}

// ---- `file_blocker_conflicts` and
// `collect_unwalked_installed_blockers` (Portage `_validate_blockers`). ----
/// Arguments of [`filed_165`].
struct Filed165Args<'a> {
    owner: (&'a str, &'a str),
    owner_version: &'a str,
    atom: &'a str,
    strong: bool,
    target: (&'a str, &'a str, &'a str),
    unsolvable: bool,
    satisfied_by: Option<BlockerSatisfiedBy>,
}

// ---- `enqueue_dependencies` (`--deep` AlreadyInstalled recursion). ----
/// Arguments of [`enqueue_165`].
struct Enqueue165Args<'a> {
    repos: &'a [RepoConfig],
    root: &'a Path,
    config: &'a portage_profile::Config,
    cat: &'a str,
    pkg: &'a str,
    ver: &'a str,
    with_bdeps: bool,
    entries: &'a mut Vec<GraphEntry>,
    queue: &'a mut std::collections::VecDeque<QueueItem>,
    pending: &'a mut Vec<PendingBlocker>,
}
