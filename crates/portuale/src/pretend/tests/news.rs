use super::*;

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
