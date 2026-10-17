//! Portage's short-circuit lines around the phase chain: the stamp skip
//! pair, the WORKDIR freshness lines, and the stale-environment notice.
//! The Rust side prints them before spawning anything, so the vendored
//! bash's own differently-worded skip texts never surface (S5 before S6).
//! Portage: bin/ebuild (stale_env_warning), package/ebuild/doebuild.py
//! (spawnebuild, the WORKDIR block)

use super::feature_token_present;

use std::path::Path;

/// `bin/ebuild`'s `build_dir_phases`: only invocations naming one of these
/// consult `${T}/environment` for the stale-env notice (`pretend` alone and
/// `clean`/`unmerge`/standalone phases never do).
/// Portage: bin/ebuild:254-265
pub(crate) const BUILD_DIR_PHASES: &[&str] = &[
    "setup",
    "unpack",
    "prepare",
    "configure",
    "compile",
    "test",
    "install",
    "package",
    "rpm",
    "merge",
    "qmerge",
];

/// The stamp file Portage checks before spawning `phase`, relative to
/// `${PORTAGE_BUILDDIR}` (`pretend` -> `.pretended`, `unpack` ->
/// `.unpacked`, `configure` -> `.configured`, and so on).
/// Portage: doebuild.py, spawnebuild (`f".{mydo.rstrip('e')}ed"`)
pub(crate) fn stamp_file_name(phase: &str) -> String {
    format!(".{}ed", phase.trim_end_matches('e'))
}

/// Whether `phase` is skipped without spawning: its stamp exists, except
/// `install` under `noauto` (which always re-runs).
/// Portage: doebuild.py, spawnebuild (`if not (mydo == "install" and
/// "noauto" in mysettings.features)`)
pub(crate) fn is_stamped(builddir: &Path, phase: &str, features: &str) -> bool {
    if phase == "install" && feature_token_present(features, "noauto") {
        return false;
    }
    builddir.join(stamp_file_name(phase)).exists()
}

/// The two stdout lines for a skipped phase, in order.
/// Portage: doebuild.py, spawnebuild
pub(crate) fn stamp_skip_lines(phase: &str, pf: &str, builddir: &Path) -> [String; 2] {
    [
        format!(">>> It appears that '{phase}' has already executed for '{pf}'; skipping."),
        format!(
            ">>> Remove '{}' to force {phase}.",
            builddir.join(stamp_file_name(phase)).display()
        ),
    ]
}

/// What the WORKDIR freshness check found.
pub(crate) enum WorkdirCheck {
    /// `WORKDIR` does not exist yet: a fresh build, and Portage prints nothing.
    Fresh,
    /// The stdout lines, in order. A stale tree was already wiped for
    /// recreation; the caller re-creates it with the build dirs.
    Lines(Vec<String>),
}

/// Portage's WORKDIR block for a chain containing `unpack`, checked before
/// any directory is created: a missing `WORKDIR` is silent; without
/// `.unpacked` the dir is recreated (`>>> Not marked as unpacked;
/// recreating WORKDIR...`); otherwise every distfile's mtime is compared
/// against `WORKDIR` (`>>> Checking ...` each, `>>> Timestamp of ... has
/// changed; recreating WORKDIR...` on the first newer one) and only a fully
/// up-to-date tree prints `>>> WORKDIR is up-to-date, keeping...`.
/// Portage: doebuild.py (`>>> Not marked as unpacked`, `>>> WORKDIR is up-to-date`)
pub(crate) fn check_workdir(
    builddir: &Path,
    workdir: &Path,
    distdir: &Path,
    alist: &[String],
) -> WorkdirCheck {
    let workdir_mtime = match std::fs::metadata(workdir).and_then(|meta| meta.modified()) {
        Ok(mtime) => mtime,
        Err(_) => return WorkdirCheck::Fresh,
    };
    if !builddir.join(stamp_file_name("unpack")).exists() {
        remove_workdir(workdir);
        return WorkdirCheck::Lines(vec![
            ">>> Not marked as unpacked; recreating WORKDIR...".to_string(),
        ]);
    }
    let mut lines = Vec::new();
    for file in alist {
        lines.push(format!(">>> Checking {file}'s mtime..."));
        let newer = std::fs::metadata(distdir.join(file))
            .and_then(|meta| meta.modified())
            .is_ok_and(|mtime| mtime > workdir_mtime);
        if newer {
            lines.push(format!(
                ">>> Timestamp of {file} has changed; recreating WORKDIR..."
            ));
            remove_workdir(workdir);
            return WorkdirCheck::Lines(lines);
        }
    }
    lines.push(">>> WORKDIR is up-to-date, keeping...".to_string());
    WorkdirCheck::Lines(lines)
}

/// Wipe `WORKDIR` for recreation. Only this one known in-builddir path is
/// ever removed (never followed when it is a link).
fn remove_workdir(workdir: &Path) {
    if std::fs::remove_dir_all(workdir).is_err() {
        let _ = std::fs::remove_file(workdir);
    }
}

/// Greedy word-wrap at 70 columns, matching Python's `textwrap.wrap(msg,
/// 70)` whenever no single word exceeds the width (so its hyphen rules
/// never fire, as in the stale-env notice).
pub(crate) fn wrap_notice_70(msg: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in msg.split(' ') {
        if !current.is_empty() && current.len() + 1 + word.len() > 70 {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// The stale-env notice bodies (`>>> ` prefix added by the caller):
/// `bin/ebuild` prints them to stderr when `${T}/environment` survives
/// from a previous run (`portage.writemsg` defaults to stderr, and the
/// `${T}` stays literal).
/// Portage: bin/ebuild:363-389 (stale_env_warning)
pub(crate) fn stale_env_lines(pf: &str) -> Vec<String> {
    let msg = format!(
        "Existing ${{T}}/environment for '{pf}' will be sourced. Run 'clean' to start \
         with a fresh environment."
    );
    wrap_notice_70(&msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    use portage_util::TempDir;

    #[test]
    fn stamp_names_match_portages_rstrip_e_rule() {
        let cases = [
            ("pretend", ".pretended"),
            ("setup", ".setuped"),
            ("unpack", ".unpacked"),
            ("prepare", ".prepared"),
            ("configure", ".configured"),
            ("compile", ".compiled"),
            ("test", ".tested"),
            ("install", ".installed"),
        ];
        for (phase, stamp) in cases {
            assert_eq!(stamp_file_name(phase), stamp, "phase {phase}");
        }
    }

    #[test]
    fn skip_lines_match_the_s02_capture_verbatim() {
        // Portage 3.0.82.2, S0 cell `s02-unpack-again` stdout lines 2-3
        // (full argv `ebuild t348-b-phases-8.ebuild unpack`).
        let builddir = Path::new("/tmp/portage/app-misc/t348-b-phases-8");
        let [first, second] = stamp_skip_lines("pretend", "t348-b-phases-8", builddir);
        assert_eq!(
            first,
            ">>> It appears that 'pretend' has already executed for \
             't348-b-phases-8'; skipping."
        );
        assert_eq!(
            second,
            ">>> Remove '/tmp/portage/app-misc/t348-b-phases-8/.pretended' \
             to force pretend."
        );
    }

    #[test]
    fn only_install_under_noauto_ignores_its_stamp() {
        let dir = TempDir::new("stamp-skip-noauto");
        let builddir: &Path = &dir;
        std::fs::write(builddir.join(".installed"), []).unwrap();
        std::fs::write(builddir.join(".unpacked"), []).unwrap();
        assert!(
            is_stamped(builddir, "install", "-sandbox -news"),
            "install without noauto skips"
        );
        assert!(
            !is_stamped(builddir, "install", "-sandbox -news noauto"),
            "install under noauto re-runs (S0 cell s06)"
        );
        assert!(
            is_stamped(builddir, "unpack", "-sandbox noauto"),
            "noauto exempts install only"
        );
        assert!(
            !is_stamped(builddir, "compile", ""),
            "a missing stamp never skips"
        );
    }

    #[test]
    fn stale_env_wraps_exactly_like_the_s02_capture() {
        // Portage 3.0.82.2, S0 cell `s02-unpack-again` stderr lines 9-10.
        assert_eq!(
            stale_env_lines("t348-b-phases-8"),
            vec![
                "Existing ${T}/environment for 't348-b-phases-8' will be \
                 sourced. Run"
                    .to_string(),
                "'clean' to start with a fresh environment.".to_string(),
            ]
        );
    }

    #[test]
    fn missing_workdir_is_silent_like_the_s01_capture() {
        let dir = TempDir::new("stamp-skip-fresh-workdir");
        let builddir = dir.join("portage/cat/pkg-1");
        std::fs::create_dir_all(&builddir).unwrap();
        assert!(matches!(
            check_workdir(&builddir, &builddir.join("work"), &dir, &[]),
            WorkdirCheck::Fresh
        ));
    }

    #[test]
    fn workdir_without_unpacked_stamp_recreates() {
        let dir = TempDir::new("stamp-skip-not-marked");
        let builddir = dir.join("portage/cat/pkg-1");
        let workdir = builddir.join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(workdir.join("stale.txt"), "stale\n").unwrap();
        let WorkdirCheck::Lines(lines) = check_workdir(&builddir, &workdir, &dir, &[]) else {
            panic!("expected the Not-marked line");
        };
        assert_eq!(
            lines,
            vec![">>> Not marked as unpacked; recreating WORKDIR...".to_string()]
        );
        assert!(!workdir.exists(), "the stale tree is wiped for recreation");
    }

    #[test]
    fn stamped_workdir_with_empty_alist_is_up_to_date() {
        // The S0 shape: t348-b-phases-8 has no SRC_URI, so no `Checking`
        // lines precede the verdict (s02 stdout line 1).
        let dir = TempDir::new("stamp-skip-up-to-date");
        let builddir = dir.join("portage/cat/pkg-1");
        let workdir = builddir.join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(builddir.join(".unpacked"), []).unwrap();
        let WorkdirCheck::Lines(lines) = check_workdir(&builddir, &workdir, &dir, &[]) else {
            panic!("expected the up-to-date line");
        };
        assert_eq!(
            lines,
            vec![">>> WORKDIR is up-to-date, keeping...".to_string()]
        );
        assert!(workdir.exists(), "an up-to-date tree is kept");
    }

    #[test]
    fn newer_distfile_recreates_after_a_checking_line() {
        let dir = TempDir::new("stamp-skip-mtime");
        let builddir = dir.join("portage/cat/pkg-1");
        let workdir = builddir.join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(builddir.join(".unpacked"), []).unwrap();
        let distdir = dir.join("dist");
        std::fs::create_dir_all(&distdir).unwrap();
        // Created after the workdir, so strictly newer on any real fs.
        std::fs::write(distdir.join("foo-1.tar.gz"), "new\n").unwrap();
        // Timestamp granularity makes create-order flaky: pin the workdir
        // to 2020 so the just-written distfile is strictly newer.
        set_mtime(&workdir, 1_600_000_000);
        let WorkdirCheck::Lines(lines) =
            check_workdir(&builddir, &workdir, &distdir, &["foo-1.tar.gz".to_string()])
        else {
            panic!("expected the Timestamp lines");
        };
        assert_eq!(
            lines,
            vec![
                ">>> Checking foo-1.tar.gz's mtime...".to_string(),
                ">>> Timestamp of foo-1.tar.gz has changed; recreating WORKDIR...".to_string(),
            ]
        );
        assert!(!workdir.exists(), "the stale tree is wiped for recreation");
    }

    /// Pin `path`'s atime+mtime to `secs` (UTC epoch): coarse filesystems
    /// otherwise make create-ordered mtime comparisons flaky.
    fn set_mtime(path: &Path, secs: libc::time_t) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let raw = CString::new(path.as_os_str().as_bytes()).unwrap();
        let stamp = libc::timespec {
            tv_sec: secs,
            tv_nsec: 0,
        };
        let times = [stamp, stamp];
        // SAFETY: `raw` is a valid NUL-terminated path, `times` a valid pair.
        let rc = unsafe { libc::utimensat(libc::AT_FDCWD, raw.as_ptr(), times.as_ptr(), 0) };
        assert_eq!(rc, 0, "utimensat on {}", path.display());
    }
}
