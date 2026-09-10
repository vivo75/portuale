use super::*;

/// End to end: `--pretend` installs nothing (pretend stays local, with
/// the remote options dropped).
#[test]
fn s8_pretend_installs_nothing() {
    fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let dst_path = dst.join(entry.file_name());
            let ft = entry.file_type().unwrap();
            if ft.is_symlink() {
                let target = std::fs::read_link(entry.path()).unwrap();
                std::os::unix::fs::symlink(target, dst_path).unwrap();
            } else if ft.is_dir() {
                copy_tree(&entry.path(), &dst_path);
            } else if ft.is_file() {
                std::fs::copy(entry.path(), dst_path).unwrap();
            }
        }
    }

    let fixtures = s8_fixtures();
    let base = TempDir::new("portuale-remote-s8-pretend").keep();
    let root = base.join("root");
    copy_tree(&fixtures.join("var"), &root.join("var"));
    let bindir = base.join("bin");
    std::fs::create_dir_all(&bindir).unwrap();
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
    let output = std::process::Command::new(s8_portuale_bin())
        .args([
            "mrg",
            "--pretend",
            "--getbinpkgonly",
            "--remote-hostname",
            "localtest",
            "--remote-transport",
            "local",
            "--remote-root",
            root.to_str().unwrap(),
            "--remote-workdir",
            base.join("work").to_str().unwrap(),
            "--remote-etc-portage",
            &format!("server:{}", clientetc.display()),
            "--remote-portuale-dir",
            bindir.to_str().unwrap(),
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
        .env("FEATURES", "sandbox")
        .output()
        .expect("portuale mrg spawns");
    let stdout = s8_stdout(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries: Vec<String> = std::fs::read_dir(&bindir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        entries.is_empty(),
        "--pretend must install nothing: {entries:?}\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&base);
}
