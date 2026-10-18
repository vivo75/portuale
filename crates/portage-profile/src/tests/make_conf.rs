use super::*;

#[test]
fn logical_lines_joins_multiline_quotes_and_skips_apostrophe_comments() {
    // A `#` comment containing a lone `'` must not open a quote that
    // swallows the rest of the file (the real cnf/make.globals bug).
    let text = "# Don't edit this\n\
                FEATURES=\"a b\n\
                c d\"\n\
                # it's fine\n\
                X=\"y\" # trailing note\n\
                Z=1\n";
    let lines = logical_lines(text);
    assert_eq!(lines, vec!["FEATURES=\"a b c d\"", "X=\"y\"", "Z=1"]);
}

/// The escape matrix of real `getconfig` + `varexpand`, captured with
/// `portage.util.getconfig` on portage 3.0.82.2 (2026-09-16); the
/// `expand` map held `B=HELLO`. `\"`/`\\` lose their backslash in
/// the shlex stage, `\$` survives it and becomes a literal `$`, and
/// a quote that survives shlex (here from the opposite quote kind)
/// suspends expansion.
#[test]
fn shlex_and_varexpand_escape_handling_matches_portage_getconfig() {
    let scalars: HashMap<String, String> = [("B".to_string(), "HELLO".to_string())]
        .into_iter()
        .collect();
    let expand = |line: &str| {
        let (_, raw) = parse_kv_line(line).expect("kv line");
        substitute(&raw, &scalars)
    };
    assert_eq!(expand(r##"A="x \$y""##), "x $y");
    assert_eq!(expand(r##"A="x \"y\"""##), "x \"y\"");
    assert_eq!(expand(r##"A="x \\y""##), "x \\y");
    assert_eq!(expand(r##"A="x \\${B} z""##), "x ${B} z");
    assert_eq!(expand(r##"A='x \$y'"##), "x $y");
    assert_eq!(expand(r##"A='${B}'"##), "HELLO");
    assert_eq!(expand(r##"A="'${B}'""##), "'${B}'");
    assert_eq!(expand(r##"A=${B}"##), "HELLO");
    assert_eq!(expand(r##"A=$B"##), "HELLO");
    assert_eq!(expand(r##"A="$B""##), "HELLO");
    assert_eq!(expand(r##"A="\$""##), "$");
    assert_eq!(expand(r##"A="\\""##), "\\");
    assert_eq!(expand(r##"A="\$B""##), "$B");
    assert_eq!(expand(r##"A="\\$B""##), "$B");
    assert_eq!(expand(r##"A="a\nb""##), "a\\nb");
}

/// `\`+newline inside quotes, captured with `portage.util.getconfig`
/// (expand=True) on Portage 3.0.82.2 / Python 3.14.8 (2026-10-18,
/// `LLM/evidence/2026-10-18-409-s0/oracle.txt`): the pair vanishes and
/// the text on both sides is kept as is; a bare newline becomes a space.
#[test]
fn backslash_newline_in_quotes_is_removed_like_portage_getconfig() {
    let scalars = HashMap::new();
    let value = |text: &str| {
        let lines = logical_lines(text);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let (_, raw) = parse_kv_line(&lines[0]).expect("kv line");
        substitute(&raw, &scalars)
    };
    assert_eq!(value("A=\"a \\\nb\"\n"), "a b");
    assert_eq!(value("B='a \\\nb'\n"), "a b");
    assert_eq!(value("D=\"x\\\n\"\n"), "x");
    assert_eq!(value("E=\"p\\\\\nq\"\n"), "pq");
    assert_eq!(value("G=\"a\nb\"\n"), "a b");
    assert_eq!(value("H=\"a  \\\n   b\"\n"), "a     b");
}

/// The owner's `llvm-lto` env file (#409): `package.env` values keep no
/// stray `\` from a continued `RUSTFLAGS`.
#[test]
fn env_file_continuation_lines_expand_like_portage() {
    let dir = portage_util::TempDir::new("portage-profile-test-env-continuation");
    let env = dir.path().join("llvm-lto");
    fs::write(
        &env,
        "COMMON_FLAGS=\"-O3\"\nCFLAGS=\"${COMMON_FLAGS}\"\n\
         RUSTFLAGS=\"-C target-cpu=znver3 -C opt-level=3 \\\n\
         -Clinker=clang -Clinker-plugin-lto \\\n\
         -Clink-arg=-fuse-ld=lld\"\n",
    )
    .unwrap();
    let out = expand_package_env_files(
        dir.path(),
        &["llvm-lto".to_string()],
        dir.path(),
        &HashMap::new(),
    );
    let get = |k: &str| out.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("CFLAGS"), Some("-O3"));
    assert_eq!(
        get("RUSTFLAGS"),
        Some(
            "-C target-cpu=znver3 -C opt-level=3 -Clinker=clang -Clinker-plugin-lto -Clink-arg=-fuse-ld=lld"
        )
    );
}
