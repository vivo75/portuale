// Real `lib/portage/elog/`. After every package merge, real
// `elog_process(cpv, settings)` reads the per-phase message files
// `bin/isolated-functions.sh::__elog_base` wrote under `${T}/logging/`,
// filters them by `PORTAGE_ELOG_CLASSES` (default `"log warn error"`),
// and hands them to each module named in `PORTAGE_ELOG_SYSTEM` (default
// `"save_summary:log,warn,error,qa echo"`).
//
// This module ports:
//   - `mod_echo` -- `collect` reads one package's `${T}/logging/`,
//     `echo_summary` prints the accumulated `* Messages for package
//     <cpv>:` blocks (real `finalize()`, an atexit handler).
//   - `mod_save` -- one `<logdir>/elog/[<cat>/]<pf>:<ts>.log` file per
//     package (`save_process`).
//   - `mod_save_summary` -- append every package's messages to a single
//     `<logdir>/elog/summary.log` (`save_summary_process`). This one is
//     ON by default (it's in `make.globals`'s `PORTAGE_ELOG_SYSTEM`).
//
// The pilot never deletes the builddir, so the driver (`pretend.rs`)
// re-scans each entry's `${T}/logging/` after the merge loop (and after
// the `emerge -C`/`--depclean`/`--prune` removal loop, filtered to the
// `prerm`/`postrm` phases -- real `dblink.unmerge`'s own
// `_elog_process(phasefilter=...)`) via `process_batch` -- no message
// buffer threaded through the (un)merge machinery.
//
// v1 cuts: `mail` / `mail_summary` / `syslog` / `custom` are NOT ported
// (a real SMTP client + MIME assembly is not "light"; `mail*` prints a
// one-line "unsupported" notice and is skipped -- see `pretend.rs`).
// `PORTAGE_ELOG_CLASSES` / `PORTAGE_ELOG_SYSTEM` are read from the env
// only (no `make.conf`), defaulting to `make.globals`. The `logdir` is
// `$PORTAGE_LOGDIR` else `<root>/var/log/portage` -- root-relative, a
// deliberate divergence from real `mod_save`'s `<BROOT>/var/log/portage`
// (`BROOT` is `/`, needs privileges), matching the pilot's other
// `<root>`-relative path choices for a relocatable tree. The real
// uid/gid/mode chmod dance on the log dir/files is a documented cut, like
// every other privilege-preserving `chown` in this pilot.

use crate::color::Colorizer;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Real `portage.const.EBUILD_PHASES`, in order -- `mod_echo._finalize`
/// walks this so messages print in phase order regardless of file mtime.
const EBUILD_PHASES: &[&str] = &[
    "pretend",
    "setup",
    "unpack",
    "prepare",
    "configure",
    "compile",
    "test",
    "install",
    "package",
    "instprep",
    "preinst",
    "postinst",
    "prerm",
    "postrm",
    "nofetch",
    "config",
    "info",
    "other",
];

/// One `<TYPE> <message>` line from a `${T}/logging/<phase>` file.
pub struct ElogMessage {
    /// Which `${T}/logging/<phase>` file this came from -- real
    /// `collect_ebuild_messages` keys on it, and `_combine_logentries`
    /// emits a `<LEVEL>: <phase>` header on every phase/level change.
    pub phase: String,
    /// `LOG` / `INFO` / `WARN` / `ERROR` / `QA`.
    pub level: String,
    pub text: String,
}

/// One package's worth of collected, class-filtered elog messages.
pub struct ElogPackage {
    /// `cat/pkg-ver` (real `mod_echo`'s `key`).
    pub cpv: String,
    /// The `ROOT` the package merged to (`/` renders the short header).
    pub root: String,
    pub messages: Vec<ElogMessage>,
}

fn elog_system_tokens() -> Vec<String> {
    std::env::var("PORTAGE_ELOG_SYSTEM")
        .unwrap_or_else(|_| "save_summary:log,warn,error,qa echo".to_string())
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Whether the module named `name` (`echo`, `save`, `save_summary`, …)
/// is listed in `PORTAGE_ELOG_SYSTEM` -- its token is either the bare
/// name or `name:levels`. `-` is accepted for `_` (real `elog_process`:
/// `s = s.replace("-", "_")`), so `save-summary` == `save_summary`.
pub fn module_enabled(name: &str) -> bool {
    elog_system_tokens()
        .iter()
        .any(|t| t.split(':').next().map(|m| m.replace('-', "_")).as_deref() == Some(name))
}

/// Whether the `echo` module is enabled (real default: yes).
pub fn echo_enabled() -> bool {
    module_enabled("echo")
}

/// `PORTAGE_ELOG_CLASSES` (real `make.globals` default `"log warn
/// error"`), uppercased.
fn portage_elog_classes() -> HashSet<String> {
    std::env::var("PORTAGE_ELOG_CLASSES")
        .unwrap_or_else(|_| "log warn error".to_string())
        .split_whitespace()
        .map(|c| c.to_uppercase())
        .collect()
}

/// The uppercased message classes module `name` shows -- its own
/// `name:levels` override in `PORTAGE_ELOG_SYSTEM` if present (real
/// `elog_process`'s per-module `filter_loglevels(..., levels)`), else
/// `PORTAGE_ELOG_CLASSES`.
fn module_classes(name: &str) -> HashSet<String> {
    for t in elog_system_tokens() {
        if let Some((m, levels)) = t.split_once(':') {
            if m.replace('-', "_") == name {
                return levels
                    .split(',')
                    .map(|l| l.trim().to_uppercase())
                    .filter(|l| !l.is_empty())
                    .collect();
            }
        }
    }
    portage_elog_classes()
}

/// Reads `${t_dir}/logging/*` for one package -- every `LOG`/`INFO`/
/// `WARN`/`ERROR`/`QA` line, in real `EBUILD_PHASES` order, unfiltered
/// (real `collect_ebuild_messages`). Empty when there is nothing there.
pub fn collect_all(t_dir: &Path) -> Vec<ElogMessage> {
    let logging = t_dir.join("logging");
    let mut out = Vec::new();
    for phase in EBUILD_PHASES {
        let Ok(content) = std::fs::read_to_string(logging.join(phase)) else {
            continue;
        };
        for line in content.split('\n') {
            if line.is_empty() {
                continue;
            }
            let Some((level, text)) = line.split_once(' ') else {
                continue;
            };
            if !matches!(level, "ERROR" | "INFO" | "LOG" | "QA" | "WARN") {
                continue;
            }
            out.push(ElogMessage {
                phase: (*phase).to_string(),
                level: level.to_string(),
                text: text.to_string(),
            });
        }
    }
    out
}

/// `collect_all`, restricted to the named phases -- real
/// `_elog_process(phasefilter=...)`, which `dblink.unmerge()` calls with
/// `("prerm", "postrm")` so a package's stale install-time `${T}/logging`
/// files (the pilot never cleans the builddir) don't resurface on removal.
pub fn collect_all_phases(t_dir: &Path, phases: &[&str]) -> Vec<ElogMessage> {
    collect_all(t_dir)
        .into_iter()
        .filter(|m| phases.contains(&m.phase.as_str()))
        .collect()
}

/// `collect_all` filtered to `classes` (real `filter_loglevels`).
fn filter_by_classes<'a>(
    msgs: &'a [ElogMessage],
    classes: &HashSet<String>,
) -> Vec<&'a ElogMessage> {
    msgs.iter()
        .filter(|m| classes.contains("*") || classes.contains(&m.level))
        .collect()
}

/// `collect_all` filtered by the `echo` module's classes -- the shape
/// `echo_summary` consumes. `phases` optionally restricts which
/// `${T}/logging/<phase>` files are read (real `phasefilter`).
pub fn collect(t_dir: &Path, phases: Option<&[&str]>) -> Vec<ElogMessage> {
    let classes = module_classes("echo");
    let all = match phases {
        Some(p) => collect_all_phases(t_dir, p),
        None => collect_all(t_dir),
    };
    all.into_iter()
        .filter(|m| classes.contains("*") || classes.contains(&m.level))
        .collect()
}

/// Real `_combine_logentries`: one flat string, phases in `EBUILD_PHASES`
/// order, a `<LEVEL>: <phase>` header emitted whenever the (phase, level)
/// pair changes, a trailing blank line when anything was written.
/// `msgs` must already be in `collect_all`'s phase order.
fn combine_logentries(msgs: &[&ElogMessage]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut prev: Option<(&str, &str)> = None;
    for m in msgs {
        let cur = (m.phase.as_str(), m.level.as_str());
        if prev != Some(cur) {
            lines.push(format!("{}: {}", m.level, m.phase));
            prev = Some(cur);
        }
        lines.push(m.text.trim_end_matches('\n').to_string());
    }
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.join("\n")
}

/// `$PORTAGE_LOGDIR` if set, else `<root>/var/log/portage` -- see this
/// module's own doc comment on the `<BROOT>` divergence.
pub fn logdir(root: &Path) -> PathBuf {
    std::env::var_os("PORTAGE_LOGDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("var/log/portage"))
}

/// UTC `%Y%m%d-%H%M%S` (real `mod_save`'s `time.strftime(..., time.gmtime())`).
fn utc_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days (Howard Hinnant's algorithm) -- no chrono dep.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}

/// Real `mod_save.process`: write one package's `fulltext` to
/// `<logdir>/elog/<pf>:<utc-stamp>.log` (or, with `FEATURES=split-elog`,
/// `<logdir>/elog/<cat>/<pf>:<stamp>.log`; otherwise the `<cat>:` is
/// prefixed onto the filename). `key` is `cat/pkg-ver`. Returns the path
/// written. Skipped entirely by the caller when the package has no
/// class-filtered messages.
pub fn save_process(
    logdir: &Path,
    key: &str,
    fulltext: &str,
    split_elog: bool,
) -> Result<PathBuf, String> {
    let (cat, pf) = key.split_once('/').unwrap_or(("", key));
    let stamp = utc_stamp();
    let (subdir, filename) = if split_elog {
        (logdir.join("elog").join(cat), format!("{pf}:{stamp}.log"))
    } else {
        (logdir.join("elog"), format!("{cat}:{pf}:{stamp}.log"))
    };
    std::fs::create_dir_all(&subdir).map_err(|e| format!("{}: {e}", subdir.display()))?;
    let path = subdir.join(filename);
    std::fs::write(&path, fulltext).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Real `mod_save_summary.process`: append one package's block to
/// `<logdir>/elog/summary.log` -- a `>>> Messages generated by process
/// <pid> on <local-time> for package <key>:\n\n` header, then `fulltext`,
/// then `\n`. The pilot uses the same UTC stamp `mod_save` does (with a
/// `UTC` suffix) rather than real `time.localtime()` + `%Z`, for a
/// deterministic, timezone-independent line.
pub fn save_summary_process(logdir: &Path, key: &str, fulltext: &str) -> Result<PathBuf, String> {
    use std::io::Write as _;
    let elogdir = logdir.join("elog");
    std::fs::create_dir_all(&elogdir).map_err(|e| format!("{}: {e}", elogdir.display()))?;
    let path = elogdir.join("summary.log");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    write!(
        f,
        ">>> Messages generated by process {} on {} UTC for package {key}:\n\n{fulltext}\n",
        std::process::id(),
        utc_stamp(),
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Real `mod_save` / `mod_save_summary` for one merged package: build the
/// per-module `fulltext` from `all_msgs` (unfiltered `collect_all`) and
/// hand it to whichever of `save` / `save_summary` is enabled. Each
/// module's own `filter_loglevels` runs here (`save_summary`'s default
/// token carries `:log,warn,error,qa`). A module is skipped for this
/// package when the filter leaves nothing (real `if len(mod_logentries)
/// == 0: continue`). Returns the paths written, for the caller's log line.
pub fn save_modules_process(
    logdir: &Path,
    key: &str,
    all_msgs: &[ElogMessage],
    split_elog: bool,
) -> Result<Vec<PathBuf>, String> {
    let mut written = Vec::new();
    for (name, is_summary) in [("save", false), ("save_summary", true)] {
        if !module_enabled(name) {
            continue;
        }
        let filtered = filter_by_classes(all_msgs, &module_classes(name));
        if filtered.is_empty() {
            continue;
        }
        let fulltext = combine_logentries(&filtered);
        let path = if is_summary {
            save_summary_process(logdir, key, &fulltext)?
        } else {
            save_process(logdir, key, &fulltext, split_elog)?
        };
        written.push(path);
    }
    Ok(written)
}

/// Real `elog_process` over a batch of packages that just merged or
/// unmerged. Each item is `(cpv, t_dir)` where `t_dir` is that package's
/// `${T}` (`${PORTAGE_BUILDDIR}/temp`). `phases` restricts which
/// `${T}/logging/<phase>` files are read (real
/// `_elog_process(phasefilter=...)`): `None` after a merge (every
/// phase), `Some(&["prerm", "postrm"])` for `dblink.unmerge()`.
/// `root_display` is the `ROOT` string for the `echo` header (`/` gives
/// the short form).
///
/// The `save` / `save_summary` modules run immediately per package (real
/// `mod_save.process`), printing the `Elog messages ... written to ...`
/// line; `echo` is accumulated and printed once at the end (real
/// `mod_echo._finalize`, an atexit handler). The `mail`/`mail_summary`
/// "unsupported" notice prints once. A no-op when no module is enabled
/// or nothing has messages. The pilot never cleans the builddir, so the
/// caller re-scans `${T}/logging/` here rather than threading a message
/// buffer through the (un)merge machinery.
pub fn process_batch(
    logdir: &Path,
    root_display: &str,
    items: &[(String, PathBuf)],
    phases: Option<&[&str]>,
    color: &Colorizer,
) {
    let echo = echo_enabled();
    let save_any = module_enabled("save") || module_enabled("save_summary");
    let mail_any = module_enabled("mail") || module_enabled("mail_summary");
    if mail_any {
        eprintln!(
            " {} elog `mail`/`mail_summary` is not supported by portuale \
             (SMTP delivery is out of scope) -- messages still go to \
             `echo`/`save`/`save_summary`",
            color.c("WARN", "*")
        );
    }
    if !(echo || save_any) {
        return;
    }
    let split_elog = std::env::var("FEATURES")
        .unwrap_or_default()
        .split_whitespace()
        .any(|f| f == "split-elog");
    let mut packages = Vec::new();
    for (cpv, t_dir) in items {
        if save_any {
            let all = match phases {
                Some(p) => collect_all_phases(t_dir, p),
                None => collect_all(t_dir),
            };
            if !all.is_empty() {
                match save_modules_process(logdir, cpv, &all, split_elog) {
                    Ok(paths) => {
                        for p in paths {
                            println!(
                                "{}Elog messages for {cpv} written to {}",
                                color.c("INFO", " * "),
                                p.display()
                            );
                        }
                    }
                    Err(e) => eprintln!("elog: {e}"),
                }
            }
        }
        if echo {
            let messages = collect(t_dir, phases);
            if !messages.is_empty() {
                packages.push(ElogPackage {
                    cpv: cpv.clone(),
                    root: root_display.to_string(),
                    messages,
                });
            }
        }
    }
    if echo {
        echo_summary(&packages, color);
    }
}

/// Real `mod_echo._finalize`: the `* Messages for package <cpv>:` block
/// for every accumulated package, all message types on stdout, each line
/// `<colour> * </colour><msg>` (`EOutput.e{info,log,warn,error,qawarn}`).
pub fn echo_summary(packages: &[ElogPackage], color: &Colorizer) {
    for pkg in packages {
        println!();
        let key = color.c("INFORM", &pkg.cpv);
        let star = color.c("INFO", " * ");
        if pkg.root == "/" {
            println!("{star}Messages for package {key}:");
        } else {
            println!("{star}Messages for package {key} merged to {}:", pkg.root);
        }
        println!();
        for msg in &pkg.messages {
            let style = match msg.level.as_str() {
                "INFO" => "INFO",
                "LOG" => "LOG",
                "WARN" => "WARN",
                "ERROR" => "ERR",
                _ => "QAWARN", // QA
            };
            println!("{}{}", color.c(style, " * "), msg.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "elog_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("logging")).unwrap();
        d
    }

    #[test]
    fn collect_reads_phase_files_in_order_and_filters_by_class() {
        // Default classes ("log warn error") -- INFO/QA are dropped, and
        // phase order (install before postinst) is honoured.
        let t = tmpdir();
        std::fs::write(
            t.join("logging/postinst"),
            "WARN later warning\nINFO postinst info\n",
        )
        .unwrap();
        std::fs::write(
            t.join("logging/install"),
            "LOG first message\nQA a qa note\nERROR an error\n",
        )
        .unwrap();

        let msgs = collect(&t, None);
        let got: Vec<(&str, &str)> = msgs
            .iter()
            .map(|m| (m.level.as_str(), m.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("LOG", "first message"),
                ("ERROR", "an error"),
                ("WARN", "later warning"),
            ]
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn collect_is_empty_when_there_are_no_message_files() {
        let t = tmpdir();
        assert!(collect(&t, None).is_empty());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn collect_phasefilter_keeps_only_the_named_phases() {
        // A builddir the pilot never cleaned: install-time logs still
        // sit next to the removal-time ones. `dblink.unmerge`'s
        // `phasefilter=("prerm","postrm")` must ignore the install log.
        let t = tmpdir();
        std::fs::write(t.join("logging/install"), "LOG built fine\n").unwrap();
        std::fs::write(t.join("logging/prerm"), "WARN leaving config behind\n").unwrap();
        std::fs::write(t.join("logging/postrm"), "LOG run revdep-rebuild\n").unwrap();

        let filtered = collect_all_phases(&t, &["prerm", "postrm"]);
        let got: Vec<(&str, &str)> = filtered
            .iter()
            .map(|m| (m.phase.as_str(), m.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("prerm", "leaving config behind"),
                ("postrm", "run revdep-rebuild"),
            ]
        );
        // The echo-filtered variant drops the WARN only if classes say so;
        // default classes keep it, so both survive here.
        assert_eq!(collect(&t, Some(&["prerm", "postrm"])).len(), 2);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn echo_summary_renders_the_real_message_block_shape() {
        let color = Colorizer::new(false);
        let pkg = ElogPackage {
            cpv: "dev-libs/foo-1.0".to_string(),
            root: "/".to_string(),
            messages: vec![
                ElogMessage {
                    phase: "postinst".to_string(),
                    level: "LOG".to_string(),
                    text: "read the docs".to_string(),
                },
                ElogMessage {
                    phase: "postinst".to_string(),
                    level: "WARN".to_string(),
                    text: "watch out".to_string(),
                },
            ],
        };
        // No colour -> plain " * " prefixes; capture is via a real run in
        // test_portuale.py, this just exercises the non-panicking path.
        echo_summary(std::slice::from_ref(&pkg), &color);
    }

    #[test]
    fn module_classes_honours_the_per_module_override() {
        // `save_summary:log,warn,error,qa` (the make.globals default token)
        // overrides the bare PORTAGE_ELOG_CLASSES for that module only.
        temp_env(
            &[(
                "PORTAGE_ELOG_SYSTEM",
                Some("save_summary:log,warn,error,qa echo"),
            )],
            || {
                let sc = module_classes("save_summary");
                assert!(sc.contains("QA"));
                assert!(sc.contains("LOG"));
                // `echo` has no override -> the bare default {LOG,WARN,ERROR}.
                let ec = module_classes("echo");
                assert!(!ec.contains("QA"));
                assert!(ec.contains("WARN"));
            },
        );
    }

    #[test]
    fn combine_logentries_matches_real_combine_shape() {
        let msgs = [
            ElogMessage {
                phase: "install".to_string(),
                level: "LOG".to_string(),
                text: "a".to_string(),
            },
            ElogMessage {
                phase: "install".to_string(),
                level: "LOG".to_string(),
                text: "b".to_string(),
            },
            ElogMessage {
                phase: "install".to_string(),
                level: "WARN".to_string(),
                text: "c".to_string(),
            },
            ElogMessage {
                phase: "postinst".to_string(),
                level: "WARN".to_string(),
                text: "d".to_string(),
            },
        ];
        let refs: Vec<&ElogMessage> = msgs.iter().collect();
        assert_eq!(
            combine_logentries(&refs),
            "LOG: install\na\nb\nWARN: install\nc\nWARN: postinst\nd\n"
        );
        assert_eq!(combine_logentries(&[]), "");
    }

    #[test]
    fn save_and_save_summary_write_the_expected_files() {
        let t = tmpdir();
        let logs = t.join("logs");
        temp_env(
            &[
                (
                    "PORTAGE_ELOG_SYSTEM",
                    Some("save save_summary:log,warn,error,qa echo"),
                ),
                ("PORTAGE_ELOG_CLASSES", Some("log warn error")),
            ],
            || {
                let msgs = [
                    ElogMessage {
                        phase: "install".to_string(),
                        level: "LOG".to_string(),
                        text: "hello".to_string(),
                    },
                    ElogMessage {
                        phase: "install".to_string(),
                        level: "QA".to_string(),
                        text: "a qa note".to_string(),
                    },
                ];
                let written =
                    save_modules_process(&logs, "dev-libs/foo-1.0", &msgs, false).unwrap();
                assert_eq!(written.len(), 2);

                // save: one <cat>:<pf>:<stamp>.log, `fulltext` only has the
                // LOG line (QA filtered out by PORTAGE_ELOG_CLASSES).
                let elog = logs.join("elog");
                let saved: Vec<_> = std::fs::read_dir(&elog)
                    .unwrap()
                    .filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("dev-libs:foo-1.0:"))
                    .collect();
                assert_eq!(saved.len(), 1);
                assert_eq!(
                    std::fs::read_to_string(elog.join(&saved[0])).unwrap(),
                    "LOG: install\nhello\n"
                );

                // save_summary: its :log,warn,error,qa override keeps the QA line.
                let summary = std::fs::read_to_string(elog.join("summary.log")).unwrap();
                assert!(summary.contains("for package dev-libs/foo-1.0:"));
                assert!(summary.contains("LOG: install\nhello\nQA: install\na qa note\n"));
            },
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn save_modules_are_off_when_not_in_portage_elog_system() {
        let t = tmpdir();
        temp_env(&[("PORTAGE_ELOG_SYSTEM", Some("echo"))], || {
            let msgs = [ElogMessage {
                phase: "install".to_string(),
                level: "LOG".to_string(),
                text: "x".to_string(),
            }];
            let written =
                save_modules_process(&t.join("logs"), "dev-libs/foo-1.0", &msgs, false).unwrap();
            assert!(written.is_empty());
            assert!(!t.join("logs").exists());
        });
        let _ = std::fs::remove_dir_all(&t);
    }

    /// Minimal process-env scoping for these serial tests -- `env::var`
    /// reads are process-global, so run one closure at a time.
    fn temp_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
    }
}
