//! Backlog #513: `/etc/portage/virtuals` notice gate. Portage's
//! `validate_ebuild_environment` runs for build, pretend, search, depclean
//! and the other config-using actions, and returns early for `--info`,
//! `--sync`, `--metadata` and `--check-news` (oracle: host Portage 3.0.83,
//! `LLM/evidence/2026-10-24-513/`, shapes 01, 20-29).

use super::*;

fn prints(args: &[&str]) -> bool {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let opts = parse_emerge_args(&args).expect("parses");
    prints_user_virtuals_notice(&opts)
}

#[test]
fn the_notice_follows_the_validating_actions() {
    assert!(prints(&["-p", "dev-libs/dccyca"]));
    assert!(prints(&["-1", "--nodeps", "dev-libs/dccyca"]));
    assert!(prints(&["-s", "dccyca"]));
    assert!(prints(&["--depclean", "-p", "dev-libs/dccyca"]));
    assert!(prints(&["--clean", "-p"]));
    assert!(prints(&["-C", "-p", "dev-libs/dccyca"]));
    assert!(prints(&["--prune", "-p", "dev-libs/dccyca"]));
    assert!(prints(&["--rage-clean", "-p"]));
}

#[test]
fn info_sync_metadata_and_news_stay_silent() {
    assert!(!prints(&["--info"]));
    assert!(!prints(&["--sync"]));
    assert!(!prints(&["--metadata"]));
    assert!(!prints(&["--check-news"]));
    assert!(!prints(&["--list-sets"]));
}
