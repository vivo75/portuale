# 05 — brace expansion: fields must not depend on IFS

**Crate:** `brush-core` · **Files:** `src/expansion.rs`, `src/braceexpansion.rs` (unchanged) (+ `brush-shell/tests/cases/compat/word_expansion/braces.yaml`, `tilde.yaml`) · **Patch:** [`patches/05-brace-expansion-ifs.patch`](patches/05-brace-expansion-ifs.patch)

## Symptom

```console
$ brush -c 'f() { local IFS; printf "<%s>" {A..C}; echo; }; f'
<A B C>
$ bash -c 'f() { local IFS; printf "<%s>" {A..C}; echo; }; f'
<A><B><C>
```

`{A..C}` collapsed into one word (`A B C`) whenever IFS was empty (`local
IFS`, `IFS=`) or did not contain a space (`IFS=:`). Real-world impact:
`bin/phase-functions.sh`'s `__filter_readonly_variables` builds bash's
special-variable list as

```bash
mapfile -t bash_vars < <(
    env -i -- "${qemu_env[@]}" "${BASH}" -c \
        "printf %s\\\n $(printf '${!%s*} ' {A..Z} {a..z} _)" | grep -vx -e PATH -e SHELL
)
```

inside a function that has already run `local IFS`. Under brush the `-c`
argument became the single malformed expansion `${!A B C …Z*}`, the hygienic
bash listed nothing, and **no** special variable was filtered: `BASHOPTS`,
`EUID`, `PPID`, `SHELLOPTS`, `UID` were written into the saved
`${T}/environment`, and every later `source` of it printed `declare: cannot
mutate readonly variable`.

## Root cause

`brace_expand_if_needed` flattened `generate_and_combine_brace_expansions`
back into one string with `.join(" ")` and returned it as a single word.
`full_expand_with_splitting` then had to rely on IFS field splitting to
recover the alternatives — which bash never does: brace expansion creates
separate words *before* (and independent of) field splitting.

## Fix

`brace_expand_if_needed` now returns `Option<Vec<String>>` (the alternatives;
`None` = nothing to expand), and `basic_expand` expands each alternative on
its own, moving the resulting fields into the aggregate:

```rust
if let Some(alternatives) = self.brace_expand_if_needed(word)? {
    let mut result: Option<Expansion> = None;
    for alternative in alternatives {
        let expansion = self.expand_one_word(&alternative).await?;
        match &mut result {
            Some(acc) => { acc.fields.extend(expansion.fields); /* … */ }
            None => result = Some(expansion),
        }
    }
    return Ok(result.unwrap_or_default());
}
self.expand_one_word(word).await
```

An empty alternative (`{a,}`) expands to the empty string, contributing no
field — matching bash, which drops the resulting empty unquoted word.
Because each alternative is expanded separately, `~/{a,b}` now tilde-expands
both alternatives too (previously a `known_failure` in the suite, now
unmarked).

## Tests

- `brush-core/src/expansion.rs` — `test_brace_expansion` updated for the new
  return shape; new `test_brace_expansion_does_not_depend_on_ifs` asserts
  `{A..C}` → `A B C` (three fields) and `{a,}` → `a` under both `IFS=` and
  `IFS=:`.
- `brush-shell/tests/cases/compat/word_expansion/braces.yaml` — new oracle
  case "Expansion with curly braces does not depend on IFS" (`IFS=`,
  `IFS=:`, including `{a,}` and `x{1..3}y`).
- `tilde.yaml` — unmarked `Tilde expansion in list`; it passes now.

Full compat suite: 0 unexpected failures (the tilde case moved from
known-fail to pass). Portuale-side,
`brush_phase_env_is_filtered_of_bash_special_variables` asserts the saved
environment carries none of the five special variables and the phase log has
no `cannot mutate readonly variable` / `env: ''` noise.
