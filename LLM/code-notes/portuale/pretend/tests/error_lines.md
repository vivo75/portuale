# crates/portuale/src/pretend/tests/error_lines.rs
Context moved out of the code comments.

## fn parser_error_keeps_portages_trailing_newline_rule
- Backlog numbers removed from the comment (original text): Plan #347 S8 (347-ERR): option-parsing failures print Portage's `emerge: error:` line without the `usage:` block (347-USG cut). The trailing-newline rule (D4) is byte-exact: messages Portage passes to `parser.error` carry their own trailing `\n` and gain argparse's, ending in a blank line; argparse-native errors end in one `\n`. Oracle: Portage 3.0.82.2 C cells `LLM/evidence/2026-10-15-346-s0/c/` (`c01`, `c06`, `c12` below).

## fn bundled_x_and_j_l_clusters_parse_like_portage
- Backlog numbers removed from the comment (original text): Backlog #405: a bundled `-X` takes the rest of the bundle or the next argument as its value; `-j`/`-l` followed by letters is the bare option plus more short flags. Oracle: Portage 3.0.82.2, `LLM/evidence/2026-10-18-405-s0/compare.txt` (`-pX` errors, `-jabc` is `-j -a -b -c`).

## fn fetchonly_and_fetch_all_uri_parse_alone_and_in_bundles
- Backlog numbers removed from the comment (original text): Backlog #401: `-f`/`-F` parse as options and bundle letters; `-F` implies `-f`. Portage: `_emerge/actions.py` (`fetchonly`, `fetch_all_uri`).
