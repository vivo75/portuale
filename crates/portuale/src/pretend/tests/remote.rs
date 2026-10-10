use super::*;

#[test]
#[cfg(unix)]
fn remote_success_prints_no_post_merge_notice_like_portage() {
    // Backlog #231 (b): the remote-execution early return
    // (`take_remote_exec` dispatch) skips `post_emerge`'s tail --
    // correctly so. The remote plan merges onto the *remote* host;
    // the local `${ROOT}/var/db/pkg` is never touched, so real's
    // `_pkgs_changed` gate (`post_emerge.py:112-117`) evaluates
    // false and -- the run being non-`--pretend` -- the tail would
    // print nothing. Local-transport end to end (`mrg
    // --remote-transport local`, the same shape the contract
    // suite's `test_mrg_remote_resolve_merges_a_binhost_binary`
    // uses): the pre-resolution notice still prints once, before
    // `Calculating...`, and no second notice follows the merge.
    let portuale_bin = built_portuale_bin();
    let fixtures = fixtures_root();
    let base = TempDir::new("remote_news").keep();
    // Client ROOT: a copy of the fixtures' `var` (the hermetic
    // 5-count vdb), so the pre notice has something to print.
    let root = base.join("root");
    copy_tree(&fixtures.join("var"), &root.join("var"));
    // Tmp `file://` binhost serving the hook-ordering fixture
    // binary with a minimal `Packages` index.
    let binhost = base.join("binhost");
    std::fs::create_dir_all(binhost.join("dev-libs")).unwrap();
    std::fs::copy(
        fixtures.join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
        binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"),
    )
    .unwrap();
    let size = std::fs::metadata(binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"))
        .unwrap()
        .len();
    std::fs::write(
        binhost.join("Packages"),
        format!(
            "TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/binpkgrmpkg-1.0\nDEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgrmpkg-1.0.tbz2\nREPO: testrepo\nSIZE: {size}\nSLOT: 0\nUSE:\n"
        ),
    )
    .unwrap();
    // Client `/etc/portage`: the fixture config repointed at the
    // tmp binhost (absolute repo locations, like the contract
    // suite's `_write_tmp_clientetc`).
    let clientetc = base.join("clientetc");
    copy_tree(
        &fixtures.join("etc/portage"),
        &clientetc.join("etc/portage"),
    );
    let profile_link = clientetc.join("etc/portage/make.profile");
    let _ = std::fs::remove_file(&profile_link);
    std::os::unix::fs::symlink(fixtures.join("repo/profiles/default"), &profile_link).unwrap();
    std::fs::write(
        clientetc.join("etc/portage/binrepos.conf"),
        format!(
            "[tmpbinhost]\nsync-uri = file://{}\npriority = 1\n",
            binhost.display()
        ),
    )
    .unwrap();
    for entry in std::fs::read_dir(clientetc.join("etc/portage/repos.conf")).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            let mut line = line.to_string();
            if line.trim_start().starts_with("location") && line.contains('=') {
                let value = line
                    .split('=')
                    .nth(1)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if !value.is_empty() && !value.starts_with('/') {
                    line = line.replace(
                        value.as_str(),
                        fixtures.join(&value).display().to_string().as_str(),
                    );
                }
            }
            lines.push(line);
        }
        lines.push(String::new());
        std::fs::write(entry.path(), lines.join("\n")).unwrap();
    }
    let work = base.join("work");
    let output = std::process::Command::new(&portuale_bin)
        .args([
            "mrg",
            "--getbinpkgonly",
            "--remote-hostname",
            "localtest",
            "--remote-transport",
            "local",
            "--remote-root",
            root.to_str().unwrap(),
            "--remote-workdir",
            work.to_str().unwrap(),
            "--remote-etc-portage",
            &format!("server:{}", clientetc.display()),
            "dev-libs/binpkgrmpkg",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("PORTAGE_CONFIGROOT", &clientetc)
        .env("ROOT", &root)
        .env("PORTAGE_RUNNING_ROOT", &fixtures)
        .env("DISTDIR", fixtures.join("distfiles"))
        .env("PORTAGE_TMPDIR", base.join("pt"))
        .env("FEATURES", "news")
        .output()
        .expect("portuale mrg spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(">>> Remote merged dev-libs/binpkgrmpkg-1.0"),
        "{stdout}"
    );
    assert!(
        root.join("var/db/pkg/dev-libs/binpkgrmpkg-1.0/CONTENTS")
            .is_file(),
        "the remote unit really merged into the client root"
    );
    // Exactly one notice: the pre-resolution one, standing before
    // `Calculating...` -- no post-merge notice follows the merge.
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    assert!(
        stdout.find("need reading").unwrap() < stdout.find("Calculating").unwrap(),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&base);
}
