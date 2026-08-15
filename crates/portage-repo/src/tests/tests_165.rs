/// Graph-input unit tests (mutation cluster 5): walk/installed/vdb,
/// merge-order, blockers, use. Each test observes the function's own
/// return value. Portage grounding: 3.0.82.2 (`3rdparty/portage`).
use crate::*;

fn cfg_165() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}
