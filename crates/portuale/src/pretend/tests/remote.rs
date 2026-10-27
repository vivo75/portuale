use super::*;

#[test]
#[cfg(unix)]
fn remote_success_prints_no_post_merge_notice_like_portage() {
    // Portage: post_emerge.py. The remote early return skips the tail; the local
    // vdb never changes, so the post notice stays silent. Exactly one notice fires.
    let portuale_bin = built_portuale_bin();
    let fixtures = fixtures_root();
    let base = TempDir::new("remote_news");
    let root = base.join("root");
    copy_tree(&fixtures.join("var"), &root.join("var"));
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
    let md5 = {
        use md5::Digest as _;
        md5::Md5::digest(std::fs::read(binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2")).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    std::fs::write(
        binhost.join("Packages"),
        format!(
            "TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/binpkgrmpkg-1.0\nDEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgrmpkg-1.0.tbz2\nREPO: testrepo\nSIZE: {size}\nMD5: {md5}\nSLOT: 0\nUSE:\n"
        ),
    )
    .unwrap();
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
            // Unsigned test binhost: opt out of the verifying default, so
            // the `.tbz2` entry is served.
            "[tmpbinhost]\nsync-uri = file://{}\npriority = 1\nverify-signature = false\n",
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
        .env("PORTAGE_OVERRIDE_EPREFIX", &root)
        .env("PORTAGE_RUNNING_ROOT", &root)
        .env("DISTDIR", fixtures.join("distfiles"))
        .env("PORTAGE_TMPDIR", base.join("pt"))
        .env("FEATURES", "news")
        // Pin the child's repo set to the rewritten clientetc text, so
        // the host's installed system repos.conf cannot add its
        // `gentoo` news items to this exact-output pin (same technique as
        // `news_resolve_env`).
        .env(
            "PORTAGE_REPOSITORIES",
            repos_conf_dir_text(&clientetc.join("etc/portage/repos.conf")),
        )
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
