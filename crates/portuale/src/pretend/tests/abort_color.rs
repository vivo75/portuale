use super::*;

const RESET: &str = "\x1b[39;49;00m";

/// Backlog #453 A: the resolver-built abort strings colourise Portage's
/// `_show_unsatisfied_dep` spans and nothing else (the resolver library
/// itself stays ANSI-free). Oracle: Portage 3.0.82.2 pipe captures of
/// `--pretend dev-libs/maskedpkg` / `dev-libs/missingdep`.
#[test]
fn abort_report_colours_the_masked_and_miss_spans() {
    let colour = Colorizer::new(true);
    // All-masked: BAD / INFORM / BAD.
    assert_eq!(
        colorize_abort_report(
            "\n!!! All ebuilds that could satisfy \"dev-libs/maskedpkg\" have been masked.\n",
            &colour
        ),
        format!(
            "\n!!! \x1b[31;01mAll ebuilds that could satisfy {RESET}\
             \x1b[32m\"dev-libs/maskedpkg\"{RESET}\
             \x1b[31;01m have been masked.{RESET}\n"
        )
    );
    // Plain miss: green atom; chain node INFORM.
    assert_eq!(
        colorize_abort_report(
            "there are no ebuilds to satisfy \"dev-libs/foo\".\n\
             (dependency required by \"dev-libs/bar-1.0::testrepo\" [ebuild])\n",
            &colour
        ),
        format!(
            "there are no ebuilds to satisfy \x1b[32;01m\"dev-libs/foo\"{RESET}.\n\
             (dependency required by \"\x1b[32mdev-libs/bar-1.0::testrepo{RESET}\" [ebuild])\n"
        )
    );
    // REQUIRED_USE header: BAD / INFORM / BAD.
    assert_eq!(
        colorize_abort_report(
            "\n!!! The ebuild selected to satisfy \"dev-libs/rqu0f-1\" has unmet requirements.\n",
            &colour
        ),
        format!(
            "\n!!! \x1b[31;01mThe ebuild selected to satisfy {RESET}\
             \x1b[32m\"dev-libs/rqu0f-1\"{RESET}\
             \x1b[31;01m has unmet requirements.{RESET}\n"
        )
    );
    // Anything else (tails, `- ` rows, Handbook) stays plain.
    let plain_tail = "!!! One of the following masked packages is required to complete your request:\n\
        - dev-libs/maskedpkg-1.0::testrepo (masked by: ~amd64 keyword)\n";
    assert_eq!(colorize_abort_report(plain_tail, &colour), plain_tail);
    // Disabled colour is the identity.
    let off = Colorizer::new(false);
    let msg = "\n!!! All ebuilds that could satisfy \"dev-libs/maskedpkg\" have been masked.\n";
    assert_eq!(colorize_abort_report(msg, &off), msg);
}
