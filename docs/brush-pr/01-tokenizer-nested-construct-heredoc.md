# 01 — tokenizer: nested `${…}` / `$(…)` on a here-tag line steals the outer here-doc's tokens

**Crate:** `brush-parser` · **File:** `src/tokenizer.rs` · **Patch:** [`patches/01-tokenizer-nested-construct-heredoc.patch`](patches/01-tokenizer-nested-construct-heredoc.patch)

## Symptom

```console
$ brush -c 'f() { cat <<EOF > "${base}.c"
x
EOF
}
declare -f f'
f ()
{
    cat <<EOF
    x
    EOF
     > base "${}.c"          # <-- ${base} destroyed: filename "base" + junk word "${}.c"
}

$ brush -c 'cat <<${VAR}
hi
${VAR}'
error: unterminated here document sequence; tag(s) [VAR] found at: ...   # tag became VAR, not ${VAR}
```

Both are also **execution** bugs, not just serialization: `cat <<${VAR}` writes
to the wrong file, `cat <<EOF > "${base}.c"` redirects to a file literally named
`base`. Real bash handles both.

## Root cause

The tokenizer collects a here-document in stages via `HereState`
(`NextTokenIsHereTag` → `CurrentTokenIsHereTag` → `NextLineIsHereDoc` →
`InHereDocs`). Every token that is *delimited* between the here tag and the
newline ending its line is routed — by `delimit_current_token`'s
`CurrentTokenIsHereTag` / `NextLineIsHereDoc` arms — into that tag's
`pending_tokens_after`, to be replayed after the body.

`consume_nested_construct` (for `$(…)`, `$((…))`, `$[…]`) and the inline `${…}`
loop tokenize their contents by calling `next_token_until(Some(term), …)`
**recursively** and re-assembling the returned fragments into the enclosing
word (`state.append_str(...)`). Those recursive calls also go through
`delimit_current_token`. So when the construct sits on a line that has a pending
*outer* here tag:

1. tokenizing `"${base}.c"` enters the `${` loop;
2. its inner `next_token_until(Some('}'))` reads `base` and delimits it
   (`SpecifiedTerminatingChar`) while `here_state == NextLineIsHereDoc`;
3. `delimit_current_token` takes the `NextLineIsHereDoc` branch and **pushes the
   `base` fragment into the outer here tag's `pending_tokens_after`**, returning
   `Ok(None)`;
4. the `${` loop gets back an empty token, appends nothing, sees the `}`
   terminator, and closes the expansion as `${}`;
5. the stolen `base` fragment resurfaces later as a bogus standalone token.

Same mechanism corrupts `<<${VAR}` (step 2 happens while
`here_state == CurrentTokenIsHereTag`, so `VAR` is registered *as the tag*).

## Fix

Give a nested construct a **fresh here-document context** for the duration of
its own tokenization — exactly what bash's `parse_comsub` does (recursive parse
with its own here-doc list). Two three-line helpers:

```rust
fn suspend_outer_here_docs(&mut self) -> (HereState, Vec<HereTag>) {
    (mem::take(&mut self.cross_state.here_state),
     mem::take(&mut self.cross_state.current_here_tags))
}

fn restore_outer_here_docs(&mut self, suspended: (HereState, Vec<HereTag>)) {
    let (outer_state, outer_tags) = suspended;
    let inner_state = mem::replace(&mut self.cross_state.here_state, outer_state);
    let inner_tags  = mem::replace(&mut self.cross_state.current_here_tags, outer_tags);
    if !inner_tags.is_empty() {
        // bash's "unterminated here-document in command substitution" quirk:
        // a here tag left open inside the construct still gets its body from
        // the lines that follow.
        self.cross_state.current_here_tags.extend(inner_tags);
        if matches!(self.cross_state.here_state, HereState::None) {
            self.cross_state.here_state = inner_state;
        }
    }
}
```

`consume_nested_construct` now wraps a renamed
`consume_nested_construct_inner`; the inline `${…}` loop is extracted into
`consume_braced_parameter_expansion` and wrapped the same way. A here-document
opened *inside* the construct (`$(cat <<X … X)`) is unaffected — it starts from
the empty suspended context and is consumed within the construct's scan; the
existing `pending_here_doc_tokens` machinery in those loops still reassembles
its body text.

Net diff is +helpers / +wrappers and a mechanical extract-method of the `${…}`
body (hence the line count); the loop body itself is unchanged.

## Tests

Two new snapshot tests in `tokenizer.rs`:

- `tokenize_here_doc_line_with_nested_constructs_in_a_later_redirect` —
  `cat <<EOF > "${base}.c"` and `cat <<EOF > "out$((1 + 2)).c" 2>&1` each
  tokenize the redirect target as one `Word`.
- `tokenize_here_tag_that_is_a_braced_parameter_expansion` — `<<${VAR}` keeps
  the literal tag `${VAR}`.

`cargo test -p brush-parser`: 236 passed (0 failed). Full compat suite unchanged
(0 failed). Every function in all 211 Gentoo eclasses that previously failed to
parse now parses (`toolchain-funcs`'s `_tc-has-openmp`, `tc-cpp-is-true`, … —
all use `cat <<-EOF > "${base}.c"`).

## Known limitation (documented in the code)

`cat <<OUTER > $(foo <<INNER … INNER)` — a here-doc opened inside a construct
that is itself on an outer here-tag line — appends `INNER` after `OUTER` in the
tag queue, so the two bodies are read outer-first. bash reads `INNER` first
(inside the comsub). No real eclass does this; left as a comment.
