/// Result/display-assembly unit tests: each test observes the
/// assembled structures directly, never a contract pin's text.
use crate::*;
use portage_util::TempDir;

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

/// Fixed "amd64"-only, no-overrides config, mirroring `tests_162`'
/// `cfg_162` (a `KEYWORDS="amd64"` scratch ebuild is always
/// visible).
fn cfg_163() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
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
