use super::*;

#[test]
fn news_item_format_and_valid_gate_the_1x_vs_2x_atom_split() {
    // Portage: news.py, DisplayInstalledRestriction. Format 1.x allows plain atoms
    // only; 2.x allows slots and USE deps.
    let item =
        |fmt: &str, atom: &str| format!("News-Item-Format: {fmt}\nDisplay-If-Installed: {atom}\n");
    assert_eq!(news_item_format("x"), None);
    assert_eq!(news_item_format("News-Item-Format: 1.0\n"), Some(true));
    assert_eq!(news_item_format("News-Item-Format: 2.13\n"), Some(false));
    // First matching header wins.
    assert_eq!(
        news_item_format("News-Item-Format: 2.0\nNews-Item-Format: 1.0\n"),
        Some(false)
    );

    for a in [
        "dev-libs/foo",
        "dev-libs/foo:2",
        "dev-libs/foo:2/3",
        "dev-libs/foo:=",
        "dev-libs/foo:*",
        "dev-libs/foo[bar]",
        "dev-libs/foo[bar(+)]",
    ] {
        assert!(news_item_valid(&item("2.0", a)), "2.x {a} valid");
    }
    assert!(news_item_valid(&item("1.0", "dev-libs/foo")));
    for a in [
        "dev-libs/foo:2",
        "dev-libs/foo:2/3",
        "dev-libs/foo:=",
        "dev-libs/foo:*",
        "dev-libs/foo[bar]",
        "dev-libs/foo[bar(+)]",
    ] {
        assert!(!news_item_valid(&item("1.0", a)), "1.x {a} invalid");
    }
    assert!(!news_item_valid(&item("2.0", "dev-libs/foo[[bad")));
    assert!(!news_item_valid("Display-If-Installed: dev-libs/foo\n"));
}

#[test]
#[cfg(unix)]
fn ask_read_news_no_skips_eselect_and_continues() {
    // "No" skips eselect and continues into the resolve.
    ask_read_news_declined("no", ["--ask", "--read-news"]);
}

#[test]
#[cfg(unix)]
fn ask_read_news_true_spellings_prompt_like_portage() {
    // Portage `true_y_or_n`: the `=True` spellings prompt like the bare pair.
    let stdout = ask_read_news_declined("true_spellings", ["--ask=True", "--read-news=True"]);
    assert!(stdout.contains("Quitting."), "{stdout}");
}
