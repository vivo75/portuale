# 02 — AST `Display`: `declare -f` of a here-document is unparseable and non-idempotent

**Crate:** `brush-parser` · **File:** `src/ast.rs` (+ `brush-shell/tests/cases/compat/builtins/declare.yaml`) · **Patch:** [`patches/02-declare-f-heredoc-serialization.patch`](patches/02-declare-f-heredoc-serialization.patch)

Five related serialization fixes. Each stands alone; grouped because they share
the "`declare -f` should round-trip" goal and the same file. Natural commit
split noted per section.

---

## (a) Here-document body placement — the load-bearing fix

### Symptom

```console
$ brush -c 'f() { cat <<EOF > /tmp/o
hi
EOF
echo after; }
declare -f f'
f ()
{
    cat <<EOF
    hi                 # (1) body indented by the { } block
    EOF                # (2) terminator indented -> never matches on re-parse
     > /tmp/o          # (3) redirect emitted AFTER the body, on its own line
    echo after
}
```

bash:

```
f ()
{
    cat <<EOF > /tmp/o
hi
EOF

    echo after
}
```

`source`-ing brush's output fails: `<<EOF` requires its terminator at column 0
(`<<-` strips leading *tabs* only, and brush indents with spaces).

### Root cause

`IoRedirect::HereDocument`'s `Display` emits the operator, tag, **body and
terminator** as one inline unit. So:

- (3) in a `RedirectList` of `[HereDocument, File]` the body is written before
  the `File` redirect is even reached;
- (1)/(2) the whole thing is written through the
  `indenter::indented(f).with_str(DISPLAY_INDENT)` wrapper that
  `BraceGroupCommand` / `IfClauseCommand` / … put around their bodies, which
  prefixes every line.

A here-document body fundamentally belongs *after the end of the current
logical line, at column 0*, regardless of nesting depth — bash handles this
with a deferred-heredoc queue in `print_cmd.c`.

### Fix

A thread-local `DEFERRED_HEREDOCS` queue:

- `IoRedirect::HereDocument`'s `Display` now writes only `[fd]<<[-]<tag>` inline
  and pushes `"<body><terminator>\n"` onto the queue.
- `flush_deferred_heredocs(f)` drains the queue: one leading `\n` (ends the
  command's line) then the blocks back-to-back (each already `\n`-terminated).
- `CompoundList::fmt_items` calls it after each item that opened a here-document
  (in place of that item's `;` separator — matching bash, which drops the
  separator and lets the following newline + body + blank line stand in).
  `FunctionDefinition` / `Program` flush at the end as a safety net (function
  body trailing redirect; guard against leaking into a later render).

The `indenter` dependency is replaced by a local `ShellIndent` adapter (same
"indent each non-empty line after a newline" behaviour) that additionally
**does not indent a line whose starting newline was emitted inside a verbatim
span** — a deferred here-doc body, or (see (b)) the interior of a multi-line
word. The *first* line of such a span — the one carrying the `<<tag` or the
opening quote — is still indented normally.

**Commit boundary:** self-contained. `indenter` drops out of
`brush-parser/Cargo.toml` (not in this patch — one extra line + `Cargo.lock`).

---

## (b) Multi-line words are re-indented → `declare -f` is not a fixpoint

### Symptom

```console
# toolchain-funcs.eclass's tc-get-compiler-type has: local code='<newline>#if ...<newline>'
$ brush ... declare -f tc-get-compiler-type   # then eval, then declare -f again, ...
round 1: maxindent=12
round 2: maxindent=12
round 3: maxindent=16      # <-- the single-quoted string's interior gains 4 spaces
round 4: maxindent=20      #     every round-trip, forever
```

bash indents the string interior **once** (when parsing from source), then its
`declare -f` output is a fixpoint. Over portage's save-env-per-phase loop,
brush's drift bloats `${T}/environment` without bound.

### Root cause

The block indenter cannot tell a structural newline from a newline inside a
`Word`'s literal value (a multi-line quoted string, or `$( … )` spanning
lines). It indents both. bash prints words verbatim.

### Fix

`Word::Display` wraps its `write!` in a `RawSpan` RAII guard
(`RAW_SPAN_DEPTH` thread-local counter). `ShellIndent` treats a line whose
starting newline landed while `RAW_SPAN_DEPTH > 0` as verbatim — no indent.
Combined with the "first line still indented" rule from (a), `local code='` is
indented but the `#if …` continuation lines and the closing `'` are not,
exactly matching bash.

**Commit boundary:** depends on (a)'s `ShellIndent` / `in_verbatim_span`.

---

## (c) Process substitution — double parentheses, compounding

### Symptom

```console
# verify-sig.eclass: tee >(tar -xf - || die)
brush declare -f, round 1:  tee >(( tar -xf - || die ))
brush declare -f, round 2:  tee >(( ( tar -xf - || die ) ))     # grows
```

### Root cause

`CommandPrefixOrSuffixItem::ProcessSubstitution` (and
`IoFileRedirectTarget::ProcessSubstitution`) render as
`{kind}({subshell_command})`, but `SubshellCommand`'s own `Display` already adds
`( … )`. So `>(list)` → `>(` + `( list )` + `)`.

### Fix

Render the inner list directly: `write!(f, "{kind}({})", subshell_command.list)`.
(brush stores a process substitution's body as a `SubshellCommand` for grammar
reuse; that's an AST detail, not the surface syntax.)

**Commit boundary:** independent one-liner ×2.

---

## (d) Pipeline separator spacing

`cat foo | grep x` was rendered `cat foo |grep x`. Bash puts spaces around the
pipe. One-char change in `Pipeline::Display` (`" |"` → `" | "`).

**Commit boundary:** independent one-liner.

---

## (e) fd-duplication redirect spacing

`2>&1` was rendered `2>& 1`. A shell prints `>&1` / `<&-` with **no** space
between the operator and the target; every other file redirection keeps the
space. `IoRedirect::File`'s `Display` now selects the separator on
`IoFileRedirectKind::{DuplicateInput,DuplicateOutput}`.

**Commit boundary:** independent, ~5 lines.

---

## Tests

`brush-shell/tests/cases/compat/builtins/declare.yaml` — 4 new cases
(here-document, process substitution, multi-line string, and an explicit
`d=$(declare -f f); eval "$d"; [ "$d" = "$(declare -f f)" ]` idempotency
check). All byte-identical to the bash oracle.

Full compat suite: 0 failed (unchanged). Every function in all 211 Gentoo
eclasses now round-trips `declare -f` → `eval` → `declare -f` idempotently
(with (a)+(b)+(c); (d)/(e) close the last byte-level gaps against bash for the
`toolchain-funcs` functions specifically).
