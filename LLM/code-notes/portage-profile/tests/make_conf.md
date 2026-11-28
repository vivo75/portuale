# crates/portage-profile/src/tests/make_conf.rs
Context moved out of the code comments.

## fn backslash_newline_in_an_unquoted_word_reads_as_a_space
- Backlog numbers removed from the comment (original text): Unquoted `C=a\<newline>b`: the same capture, case C, reads `a b`; the token must not stop at the joined space (backlog #410).

## fn env_file_continuation_lines_expand_like_portage
- Backlog numbers removed from the comment (original text): The owner's `llvm-lto` env file (#409): `package.env` values keep no stray `\` from a continued `RUSTFLAGS`.
