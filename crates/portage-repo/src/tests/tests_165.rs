/// Graph-input unit tests (mutation cluster 5): walk/installed/vdb,
/// merge-order, blockers, use. Each test observes the function's own
/// return value. Portage grounding: 3.0.82.2 (`3rdparty/portage`).
use crate::*;
use portage_util::TempDir;

fn cfg_165() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
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
