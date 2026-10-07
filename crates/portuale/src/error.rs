// portuale's own error model, per docs/refactor-01.md §2.1's bounded
// improvement: an internal `Error { kind, detail: Vec<String> }` for the
// CLI-visible error boundary, so error-CLI-formatting can change shape
// per kind without the whole app's `Result<_, String>` internals moving
// (the audit's "not now" refers to converting the internals, not to
// providing the seam). `kind` is a stable classifier (`"resolve"`,
// `"io"`, `"portuale"`); `detail` holds the real payload lines, joined
// with `\n` by `Display` so the emitted text is byte-identical to what
// the String-error design printed.

pub struct Error {
    kind: &'static str,
    detail: Vec<String>,
}

impl Error {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Error {
            kind,
            detail: vec![message.into()],
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail.join("\n"))
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("detail", &self.detail)
            .finish()
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::new("portuale", s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::new("portuale", s.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::new("io", e.to_string())
    }
}

// The one typed error that reaches portuale's boundary today: the depgraph
// resolution step. `kind` = "resolve" classifies it for the CLI formatter
// while `Display` keeps the exact message shape.
impl From<portage_repo::Error> for Error {
    fn from(e: portage_repo::Error) -> Self {
        Error::new("resolve", e)
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn new_debug_and_display_keep_text() {
        let e = Error::new("resolve", "boom");
        assert_eq!(e.to_string(), "boom");
        assert_eq!(
            format!("{e:?}"),
            "Error { kind: \"resolve\", detail: [\"boom\"] }"
        );
    }

    #[test]
    fn strings_cross_as_portuale_kind() {
        assert_eq!(Error::from("boom".to_string()).to_string(), "boom");
        assert_eq!(Error::from("boom").to_string(), "boom");
        assert_eq!(Error::from(std::io::Error::other("x")).to_string(), "x");
    }
}
