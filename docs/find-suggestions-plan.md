# Plan: `_find_suggestions` — the circular-dependency USE-flag heuristic

*Working plan — 2026-09-03. Move to `docs/history/` once complete.*

## The gap

When portuale hits an unbreakable build-time dependency cycle it prints
the `* Error: circular dependencies:` block (the shortest cycle as a
growing-indent `<pkg> depends on` / `<pkg> (buildtime)` chain) and then
the **generic** advisory:

```
 * Note that circular dependencies can often be avoided by temporarily
 * disabling USE flags that trigger optional dependencies.
```

Real portage only prints that generic text as the `else` branch. When
`circular_dependency_handler._find_suggestions()` finds a concrete fix it
prints instead:

```

It might be possible to break this cycle
by applying the following change:
- dev-libs/foo-1.0 (Change USE: -bar)

Note that this change can be reverted, once the package has been installed.
```

(`by applying any of the following changes:` — with `any of` bold — when
there is more than one; a per-suggestion
` (This change might require USE changes on parent packages.)` line when
`followup_change`; and a "the dependency graph contains a lot of cycles"
trailer when `large_cycle_count`.)

`pretend.rs` (`run`, the circular block ~line 9051) and
`emerge_pretend_reference.py` (`run`, ~line 15358) both carry the cut in
a comment: *"minus … `_find_suggestions`'s USE-flag heuristic (portuale
always hits the generic-advisory else branch)."*

## Real source

- `3rdparty/portage/lib/_emerge/resolver/circular_dependency.py`
  — `circular_dependency_handler`, esp. `_find_suggestions()`
  (lines 114–297, ~180 lines) and `_get_autounmask_changes` /
  `_get_use_mask_and_force`.
- `3rdparty/portage/lib/portage/dep/__init__.py::extract_affecting_use`
  (lines 3525–3656) — the bracket-stack parser `_find_suggestions`
  leans on. Test corpus:
  `3rdparty/portage/lib/portage/tests/dep/test_extract_affecting_use.py`
  (23 pass cases + 15 malformed-`xfail`).
- `3rdparty/portage/lib/_emerge/depgraph.py::_show_circular_deps`
  (10425–10486) — the render sequence that consumes `handler.suggestions`
  / `handler.large_cycle_count`.

### `_find_suggestions` algorithm (per edge `pos` of the shortest cycle; `pkg = cycle[pos]`, `parent = cycle[pos-1]`)

1. Every hard-cycle edge is build-time, so
   `dep = "<parent DEPEND> <parent BDEPEND>"` (`Package._buildtime_keys`).
2. `parent_atom` = the atom `parent` used to pull `pkg`
   (`all_parent_atoms[pkg]` filtered to `ppkg == parent`), taken
   `.unevaluated_atom` (so `foo[bar=]` stays `foo[bar=]`). soname deps →
   `continue`.
3. `affecting_use = extract_affecting_use(dep, parent_atom, eapi)` — the
   flags whose `flag?` conditionals gate `parent_atom` inside `dep`.
4. `untouchable = use.mask ∪ use.force ∪ _get_autounmask_changes(parent)`;
   `affecting_use -= untouchable`.
5. REQUIRED_USE entanglement: if `affecting_use` intersects
   `get_required_use_flags(parent.REQUIRED_USE)`, widen
   `affecting_use = (affecting_use ∪ required_use_flags) − untouchable`
   **iff** `len ≤ MAX_AFFECTING_USE` (10).
6. `affecting_use` empty → `continue`. `len > 10` → keep only
   currently-enabled flags; still `> 10` → `continue` (bug #555698).
7. `solutions = set()`. For every `use_state ∈ product({disabled,
   enabled}, repeat=len(affecting_use))`:
   - `current_use = _pkg_use_enabled(parent)` with each affecting flag
     forced to its `use_state`.
   - `reduced = use_reduce(dep, uselist=current_use, flat=True)`.
   - if `parent_atom ∉ reduced` **and**
     `check_required_use(parent.REQUIRED_USE, current_use, …)`:
     record the *minimal diff* solution
     `{(flag, True) : enabled & not currently on} ∪ {(flag, False) :
     disabled & currently on}` as a `frozenset`.
8. Superset-prune `solutions` (drop any strict superset of another).
9. Grandparent-atom conflict: for each `(ppkg, atom)` in
   `_parent_atoms[parent]`, `atom = atom.unevaluated_atom`; skip if
   `not atom.use`. For each `(flag, _)` in the solution: if
   `flag ∈ atom.use.enabled ∪ atom.use.disabled` → drop the solution;
   else if `atom.use.conditional` names the flag → `followup_change =
   True`.
10. Surviving solution → append
    `- {parent.cpv} (Change USE: {±flags})\n` (`+flag` red, `-flag`
    blue in real) plus, when `followup_change`,
    ` (This change might require USE changes on parent packages.)`.

`large_cycle_count = len(self.cycles) > 3` (needs full cycle
enumeration, a separate cut — portuale has one cycle, so this is always
`False` for now).

## Approach

Keep it in the render layer, next to the block that already prints the
generic advisory (`pretend.rs::run` + the Python `run` mirror) — no
`GraphResult` change. A single helper

```rust
fn circular_dep_suggestions(
    cycle: &[String],                 // result.circular_deps[0], cpv strings
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    autounmask_use_changes: &[AutounmaskChange],
) -> Vec<String>                       // the `- cpv (Change USE: …)` lines
```

re-derives everything from the repo (parent md5-cache `DEPEND`/`BDEPEND`,
`effective_use_flags`, `IUSE`, `REQUIRED_USE`) — portuale already does
this kind of md5-cache re-read in `refresh_entry_use_display` and the
`'parent_flip` block.

**Primitives** — have: `portage_use_reduce::use_reduce_flat` (step 7's
`use_reduce(…, flat=True)`), `portage_required_use::check_required_use`
(step 7), `forced_or_masked_flags` (step 4's `use.mask ∪ use.force`),
`effective_use_flags` (step 7's `_pkg_use_enabled`, sans autounmask —
see cuts). **Need**: `extract_affecting_use` (Slice 1) and a
`required_use_flag_names(&str) -> HashSet<String>` token scan (~10
lines, Slice 2).

## Decisions (2026-09-03)

- **Colour: match real.** The `Change USE:` flags render coloured —
  `+flag` `color::c("red", …)` (`\x1b[31;01m`), `-flag`
  `color::c("blue", …)` (`\x1b[34;01m`), and `any of` `color::c("bold",
  …)` (`\x1b[01m`) — the same bytes real emits (verified against
  `color.rs`; the `BAD` prefix in this block is already coloured this
  way). `resolve_havecolor` already matches real's `NO_COLOR` / `--color`
  / tty gating, and the contract suite runs non-tty so both sides take
  the no-colour path by default; add one `--color y` contract case to
  pin the ANSI.
- **Two slices, not three.** The grandparent-atom conflict filter +
  `followup_change` note fold into Slice 2 — one commit for the whole
  faithful `_find_suggestions`.

## Slices (each: both sides + verified byte-identical, committed on request)

1. **Port `extract_affecting_use`.** New `pub fn extract_affecting_use(
   dep: &str, atom: &str) -> Result<HashSet<String>, ParseError>` in
   `portage-dep` (it's a dep-string primitive). Faithful transcription of
   the bracket-stack parser. All 23 pass cases + 15 malformed cases as
   Rust unit tests; a Python mirror + harness test over the same corpus.
   **No behaviour change** — nothing calls it yet.
2. **The full `_find_suggestions` + wire it in.** The heuristic helper
   (all of steps 1–10, grandparent check included), and the
   `_show_circular_deps` render branch: replace the unconditional generic
   advisory with real's `if suggestions: "It might be possible to break
   this cycle" / "by applying the following change:" | "by applying
   <bold>any of</bold> the following changes:" / <lines> / "Note that
   this change can be reverted…"  else: <generic advisory>`. Grandparent
   step (9): re-derive `parent`'s own puller atoms (same md5-cache scan
   as step 2, over `result.entries[parent].required_by`), drop solutions
   a grandparent's `[flag]`/`[-flag]` use-dep forbids, add
   ` (This change might require USE changes on parent packages.)` for a
   conditional-only clash. Fixtures: `dev-libs/usecyclea` + `usecycleb`
   where `x? ( <the cycle DEPEND> )` gates the cycle (`-x` breaks it),
   and a second pair with a grandparent `dep[flag]` for the step-9 path.
   Contract test (byte-identical Rust==Python) + `portage-repo` unit
   tests on the helper. The existing `hardcyclea` fixture (no IUSE)
   keeps hitting the `else` branch unchanged.

## Simplifications / cuts (call out in code + `what-this-proves.md`)

- **`_pkg_use_enabled` sans autounmask** (Slice 2): step 7 uses
  `effective_use_flags(config, …)` without folding
  `result.autounmask_use_changes` — a cycle that only survives *because*
  of an autounmask USE change is a corner of a corner. `_get_autounmask_
  changes` (step 4's `untouchable`) is still honoured from
  `autounmask_use_changes`.
- **One cycle, not `get_cycles`** — `large_cycle_count` stays `False`;
  the "lot of cycles" trailer never prints. Separate backlog cut (full
  elementary-cycle enumeration).
- **soname deps** — `parent_atom.package` is always true for portuale
  (no soname graph), so the `continue` at real line 146 is unreachable;
  no handling needed.
- The **forced cycle-only `--verbose --tree` re-display**
  (`self.display(handler.merge_list)` at `depgraph.py:10435`) is a
  *separate* cut and not in scope here — it's an independent addition to
  the same block.

## Risk

- Only ever runs on the already-fatal circular-deps path (exit 1), which
  today has exactly one contract test (`hardcyclea`) — so the blast
  radius is small, but the corpus to regression-check against is also
  small. Slice 1's 38-case `extract_affecting_use` corpus is the real
  confidence.
- `product(2, repeat=n)` with `n ≤ 10` → ≤ 1024 `use_reduce_flat` calls
  per cycle edge; cheap, and only on a failing resolve.
- The md5-cache re-derivation of `parent_atom` must agree with what the
  walk actually queued — same class of risk as `refresh_entry_use_
  display` / `'parent_flip`; mitigated by finding the atom the same way
  (`use_reduce_flat` over the buildtime keys under the parent's
  effective USE, first token whose `cp` matches `pkg`).
