//! A repository without `metadata/md5-cache` gives the same answer as one with
//! it: the experiment asks the `portuale` binary to run the `depend` phase.

use std::path::{Path, PathBuf};
use std::process::Command;

fn target_bin(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/release");
    let bin = dir.join(name);
    assert!(
        bin.is_file(),
        "{} is missing: run `cargo build --release -p portuale -p experiment-pre-sort-atoms` first",
        bin.display()
    );
    bin
}

fn run(root: &Path, depcache: &Path) -> String {
    let out = Command::new(target_bin("experiment-pre-sort-atoms"))
        .args(["--best-only", "--config-root"])
        .arg(root)
        .arg("--root")
        .arg(root)
        .env("PORTAGE_DEPCACHEDIR", depcache)
        .env("PORTUALE_BIN", target_bin("portuale"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn copy_fixtures(to: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
    let status = Command::new("cp")
        .arg("-a")
        .arg(from)
        .arg(to)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn a_repository_without_md5_cache_is_read_through_the_depend_phase() {
    let tmp = tempfile::tempdir_in("/var/tmp").unwrap();
    let (with, without) = (tmp.path().join("with"), tmp.path().join("without"));
    copy_fixtures(&with);
    copy_fixtures(&without);
    std::fs::remove_dir_all(without.join("repo/metadata/md5-cache")).unwrap();
    let expected = run(&with, &tmp.path().join("dep-with"));
    let got = run(&without, &tmp.path().join("dep-without"));
    assert!(!expected.is_empty());
    assert_eq!(got, expected);
    // The phase ran: its results are in the depcachedir.
    assert!(tmp.path().join("dep-without").is_dir());
}
