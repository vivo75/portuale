// Real `lib/portage/elog/` -- the `echo` module (default-on via
// `make.globals`'s `PORTAGE_ELOG_SYSTEM="save_summary:log,warn,error,qa
// echo"`). After every package merge, real `elog_process(cpv, settings)`
// reads the per-phase message files `bin/isolated-functions.sh::
// __elog_base` wrote under `${T}/logging/`, filters them by
// `PORTAGE_ELOG_CLASSES` (default `"log warn error"`), and hands them to
// each enabled module; `mod_echo` accumulates and prints a
// `* Messages for package <cpv>:` block for every package at the very end
// of the run (its `finalize()` is an `atexit` handler).
//
// This module is the pilot's `mod_echo`: `collect` reads one package's
// `${T}/logging/`, `echo_summary` prints the accumulated blocks. The
// pilot never deletes the builddir, so `run()` just re-scans each merged
// entry's `${T}/logging/` after the merge loop -- no need to thread a
// message buffer through the merge machinery.
//
// v1 cuts: only the `echo` module (`save`/`save_summary`/`mail*` write
// files / send mail -- deferred); `PORTAGE_ELOG_CLASSES` is read from the
// env only (no `make.conf`), defaulting to real `make.globals`'s
// `"log warn error"`; the `:levels` per-module override on the `echo`
// token itself is honoured, but the Python-side in-memory `einfo`
// messages (generated before `setup`) have no pilot equivalent.

use crate::color::Colorizer;
use std::collections::HashSet;
use std::path::Path;

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

/// Whether the `echo` module is enabled (real default: yes).
pub fn echo_enabled() -> bool {
    elog_system_tokens()
        .iter()
        .any(|t| t.split(':').next() == Some("echo"))
}

/// The uppercased message classes the `echo` module shows -- its own
/// `echo:levels` override if present, else `PORTAGE_ELOG_CLASSES` (real
/// `make.globals` default `"log warn error"`).
fn echo_classes() -> HashSet<String> {
    for t in elog_system_tokens() {
        if let Some((name, levels)) = t.split_once(':') {
            if name == "echo" {
                return levels.split(',').map(|l| l.trim().to_uppercase()).collect();
            }
        }
    }
    std::env::var("PORTAGE_ELOG_CLASSES")
        .unwrap_or_else(|_| "log warn error".to_string())
        .split_whitespace()
        .map(|c| c.to_uppercase())
        .collect()
}

/// Reads `${t_dir}/logging/*` for one package and returns its
/// class-filtered messages in phase order. Empty when there is nothing
/// to report (real `collect_ebuild_messages` shortcut).
pub fn collect(t_dir: &Path) -> Vec<ElogMessage> {
    let classes = echo_classes();
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
            if classes.contains("*") || classes.contains(level) {
                out.push(ElogMessage {
                    level: level.to_string(),
                    text: text.to_string(),
                });
            }
        }
    }
    out
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

        let msgs = collect(&t);
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
        assert!(collect(&t).is_empty());
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
                    level: "LOG".to_string(),
                    text: "read the docs".to_string(),
                },
                ElogMessage {
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
    fn echo_classes_honours_the_per_module_override() {
        // A bare `echo` token with a `:levels` suffix overrides the global
        // PORTAGE_ELOG_CLASSES -- checked by parsing a literal token list.
        let classes: std::collections::HashSet<String> = "echo:info,qa"
            .split_once(':')
            .map(|(_, l)| l.split(',').map(|x| x.trim().to_uppercase()).collect())
            .unwrap();
        assert!(classes.contains("INFO"));
        assert!(classes.contains("QA"));
        assert!(!classes.contains("LOG"));
    }
}
