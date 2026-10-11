# crates/portuale/src/helpers/filter_env.rs
Native `filter-bash-environment` helper: byte-level port of Portage's environment filter.

## module
- #326 S3, D1/D3/D4/D7: a byte-level port of `bin/filter-bash-environment.py`'s `filter_bash_environment()` (all 164 lines of it), routed from the `python` dispatcher (D1): a first argument whose basename is `filter-bash-environment.py` comes here with the remaining args. The vendored script itself is untouched (the call site stays upstream's; in D2 `real` mode `PORTAGE_PYTHON=/usr/bin/python` still runs it).
- Behaviour is Portage's, in Portage's order, over raw bytes: stdin is split exactly like Python binary-file iteration (lines end at `\n` inclusive; the last line may lack it), stdin streams to stdout (`BufReader`/`BufWriter`), and there is no UTF-8 decoding anywhere. Per line, first match wins: a multi-line-quote continuation line (written with `\x01` stripped when its variable is kept; it clears the quote when `have_end_quote` says so); then, only when not in a here-doc or function, a variable assignment (a kept line gets the `declare -r` rewrite and the `\x01` strip) or a `declare` without assignment (a kept line gets the rewrite but NO `\x01` strip); then a here-doc delimiter line, a here-doc start line, a function body line, a function start line, else pass-through. The end-quote search skips the start quote exactly like Portage: `line[end(name)+2..]` (past `=` and the opening quote).
- Regex translation (Python `re` on bytes -> `regex::bytes::Regex`): `(?-u)` everywhere, so `\w \s \d \W` are ASCII-only, like Python bytes patterns (on bytes Python never matches non-ASCII word bytes). `.` never matches `\n` in either engine; `(?s)` is NOT set. Python `$` (no MULTILINE) matches at end or just before a final `\n`; every line here carries its `\n`, so each trailing `$` is written `\n?\z`, consistently, including the runtime here-doc delimiter `^DELIM$` -> `\AD ELIM\n?\z` (DELIM is `\w+`, so, as in Portage, nothing is escaped). Python `.match` anchors at the start only: every anchored pattern gets `\A` (`close_quote_re.search` stays unanchored). Full translation table (Python bytes with `.match` unless noted -> Rust `regex::bytes`): `.*\s<<[-]?(\w+)$` -> `(?-u)\A.*\s<<[-]?(\w+)\n?\z`; `^[-\w]+\s*\(\)\s*$` -> `(?-u)\A[-\w]+\s*\(\)\s*\n?\z`; `^\}$` -> `(?-u)\A\}\n?\z`; `(^|^declare\s+-\S+\s+|^declare\s+|^export\s+)([^=\s]+)=("|\')?.*$` (groups 1=prefix 2=name 3=quote) -> `(?-u)\A(?:declare\s+-\S+\s+|declare\s+|export\s+)?([^=\s]+)=(["'])?.*\n?\z` (groups 1=name 2=quote); `(\\"|"|\')\s*$` (`.search`) -> `(?-u)(\\"|["'])\s*\n?\z` (search); `^declare\s+-(\S*)r(\S*)\s+` unchanged apart from the `(?-u)\A` prefix; `^declare(\s+-\S+)?\s+([^=\s]+)\s*$` (groups 1=flags 2=name) -> `(?-u)\Adeclare(?:\s+-\S+)?\s+([^=\s]+)\s*\n?\z` (group 1=name); runtime `^DELIM$` -> `(?-u)\ADELIM\n?\z`; var `^(t1|..\|\d.*|.*\W.*)$` (`.match(name)`) -> `(?-u)\A(?:t1|..\|\d.*|.*\W.*)\n?\z` (`is_match(name)`).
- The PATTERN argument uses Python regex syntax (Portage joins `argv[1].split()` -- an ASCII-whitespace split, empties dropped -- plus `\d.*` and `.*\W.*` with `|`). Accepted: everything `regex::bytes` compiles with identical meaning to Python `re` on bytes -- literal names, `.`, `*`/`+`/`?`/`{m,n}`, `.*`, `\d \D \s \S \w \W`, `[...]`, `|` (Portage joins the tokens with it), `(...)`/`(?:...)`/`(?P<name>...)`, `^ $ \A \z \b \B`, `\\` escapes, `(?i)`-style scoped flags with Python-identical meaning. Loud failure (stderr message + exit 1, empty stdout, the `error-bad-pattern` rc): anything the Rust engine rejects (lookarounds, backrefs, `\Z`, `\C`, octal escapes -- all Python-`re`-only), plus anything Rust would accept with a different meaning than Python-on-bytes: `\p`/`\P` (no Python `re` equivalent) and `(?u`/`(?U` flag runs outside a character class (bytes patterns cannot set UNICODE in Python; Rust would flip `\w` et al. to Unicode); a `[` inside a class (`[a[b]]`, POSIX `[[:alpha:]]`: Rust nests, Python reads it literally) and `&&`/`--`/`~~` inside a class (Rust set operators, Python literals or a bad range); `(?<name>` (a Rust named group, a Python error). PATTERN bytes outside ASCII become `\xHH` escapes (a literal-byte match in both engines); a `\`-escape of a non-ASCII byte is a bad escape in Python, so it fails loudly too.
- argv (Portage's `__main__`): `-h`/`--help` anywhere in the args after the script -> `usage: <script-basename> PATTERN` on stdout, exit 0; anything but exactly one PATTERN arg -> the usage plus `Exactly one PATTERN argument required.` on stderr, exit 2.

- Python-to-Rust pattern table (verbatim from the original comment):
```
| Python (bytes, `.match` unless noted) | Rust (`regex::bytes`)             |
| `.*\s<<[-]?(\w+)$`                    | `(?-u)\A.*\s<<[-]?(\w+)\n?\z`      |
| `^[-\w]+\s*\(\)\s*$`                  | `(?-u)\A[-\w]+\s*\(\)\s*\n?\z`     |
| `^\}$`                                | `(?-u)\A\}\n?\z`                   |
| `(^|^declare\s+-\S+\s+|^declare\s+    | `(?-u)\A(?:declare\s+-\S+\s+       |
|   |^export\s+)([^=\s]+)=("|\')?.*$`   |   |declare\s+|export\s+)?          |
|                                       |   ([^=\s]+)=(["'])?.*\n?\z`       |
|   (groups 1=prefix 2=name 3=quote)    |   (groups 1=name 2=quote)          |
| `(\\"|"|\')\s*$` (`.search`)          | `(?-u)(\\"|["'])\s*\n?\z` (search) |
| `^declare\s+-(\S*)r(\S*)\s+`          | `(?-u)\Adeclare\s+-(\S*)r(\S*)\s+` |
| `^declare(\s+-\S+)?\s+([^=\s]+)\s*$`  | `(?-u)\Adeclare(?:\s+-\S+)?\s+     |
|   (groups 1=flags 2=name)             |   ([^=\s]+)\s*\n?\z` (group 1=name)|
| runtime `^DELIM$`                     | `(?-u)\ADELIM\n?\z`                |
| var `^(t1|..\|\d.*|.*\W.*)$`          | `(?-u)\A(?:t1|..\|\d.*|.*\W.*)\n?\z`|
|   (`.match(name)`)                    |   (`is_match(name)`)                |
```

## fn run
- Entry from the `python` dispatcher: `script` is the argv element whose basename is `filter-bash-environment.py` (Portage's `sys.argv[0]`, the usage basename), `args` is everything after it (Portage's `sys.argv[1:]`). Stdout stays empty on every non-zero exit (the failure paths return before the filter runs, as in Portage, where `re.compile` throws first).

## fn compile_var_pattern
- Portage's `__main__` pattern build: `argv[1].split()` (an ASCII-whitespace split, empties dropped) plus the appended `\d.*` (drop digit-leading names: not valid bash) and `.*\W.*` (drop names with non-bash characters), compiled as `^(...)$`.

## fn pattern_is_divergent
- True when the joined token bytes would compile in Rust with a different meaning than Python `re` on bytes (see the module list): `\p`/`\P` anywhere an escape is active, and a `u`/`U` flag inside an inline `(?...)` group outside a character class.

## fn rust_pattern_source
- Translate raw PATTERN bytes into the Rust pattern string: ASCII bytes pass through (every regex metacharacter is ASCII, with the same meaning in both engines); each non-ASCII byte becomes a `\xHH` escape (a literal-byte match in both). `None` when the bytes cannot mean the same in both engines: a `\`-escape of a non-ASCII byte is a bad escape in Python `re`.

## struct LineRes
- The static line patterns (the translation table lives in the module notes). Group numbering differs from Portage's where a prefix group became non-capturing.

## fn have_end_quote
- Portage's `have_end_quote`: the line ends (modulo trailing whitespace) with the opening quote byte. `\\"` (backslash-quote) at the end is NOT the end quote -- group 1 is then two bytes, never equal to the one-byte quote.

## fn filter_declare_readonly_opt
- Portage's `filter_declare_readonly_opt`: drop the `r` from a `declare -<opts>r<opts>` flag run (`declare -ar` -> `declare -a`, bare `declare -r` -> `declare `). Both `(\S*)` groups always participate.

## fn strip_ctrl_a
- Portage's `line.replace(b"\1", b"")` (bug 222091: bash multiplies `\x01` bytes on every env save).

## fn here_doc_delimiter
- Portage builds the delimiter matcher as `re.compile(b"^" + group + b"$")` (nothing escaped); the group is `\w+`, so it is regex-safe by construction.

## fn run_filter
- Portage's `filter_bash_environment`, in Portage's order (see the module notes). Lines are raw byte slices ending at `\n` inclusive, except a final line without one. Skip-the-start-quote detail: past `=` and the opening quote, so a newline right after it does not read as an immediate end. No `\x01` strip on the bare-declare path, like Portage. Here-documents come before functions, so doc content cannot read as a function end.

## tests: native_filter_matches_every_fixture_case_byte_identical
- #326 S3/D4: every `fixtures/helpers/filter-env/<case>` oracle -- run the native filter as a child process (the shim's call shape) and assert stdout is BYTE-IDENTICAL, rc matches, and (for rc 0) stderr is empty. `error-bad-pattern` pins only rc and empty stdout (the Python traceback text is not reproduced).
- The helper runs the native filter exactly like the shim would: `portuale __helper python filter-bash-environment.py <args>`, stdin bytes verbatim.
- The `args` fixture file bytes ARE argv[1]: no trailing newline is added, none is present, so this matches `"$(cat args)"` byte for byte. A pipe could deadlock past one pipe buffer (both sides stream), so stdin goes through a temp file while stdout/stderr stream. Inputs over 64 KiB are bzip2-compressed (`stem.bz2`).

## tests: dollar_matches_before_a_final_newline
- `$` (no MULTILINE) matches at end OR just before a final `\n`: the function-end and here-doc-delimiter patterns accept both spellings and reject a non-delimiter line either way.

## tests: word_class_is_ascii_only
- `\w` is ASCII-only on both sides: a here-doc redirect into a UTF-8 word is NOT a here-doc start (the `(\w+)$` cannot reach the line end past the non-ASCII bytes), while a plain ASCII delimiter is. `<<-` (strip-tabs) variant still matches ASCII delimiters.

## tests: non_ascii_name_bytes_match_dot_star_backslash_capital_w
- Portage appends `.*\W.*` to the alternation, and `\W` is ASCII-only on bytes: a name with a non-ASCII byte matches (every non-ASCII byte is a non-word byte), so Portage drops it (see the `edge-utf8-name` fixture). `compile_var_pattern(b"MY_KEEP")`: listed names match (dropped); clean ASCII names do not; appended `\d.*` drops digit-leading names.

## tests: a_last_line_without_newline_round_trips
- A last line without trailing newline round-trips byte-identical (kept: no newline added; dropped: no output), like Python binary-file iteration.

## tests: help_flag_prints_usage_and_exits_zero / wrong_argc_prints_usage_to_stderr_and_exits_two / bad_pattern_exits_one_with_empty_stdout
- `-h`/`--help` anywhere prints the usage to stdout and exits 0. Anything but exactly one PATTERN arg prints the usage plus the `Exactly one PATTERN argument required.` line to stderr, exit 2. An uncompilable PATTERN fails loudly: exit 1, empty stdout (the Python traceback text is not reproduced -- only the rc pins the `error-bad-pattern` fixture). A divergent-but-compilable pattern fails the same way.

## tests: patterns_with_rust_only_meaning_are_refused
- Patterns Rust compiles with a meaning Python `re` does not share are refused (Python 3, probed live 2026-10-08: `[[:alpha:]]` matches `a]`, `[a&&b]` matches `&`, `[a~~b]` is the class {a,~,b}, `(?<n>a)` is "unknown extension"); the Python-identical shapes that look similar are still accepted (`[]a]`, `[^]a]`, `[a-z_]*`, `(?P<n>a)`, `(?i)abc`, `A\[B`, `BASH_.* __.*`). `[]a]` means {], a} in both engines.
