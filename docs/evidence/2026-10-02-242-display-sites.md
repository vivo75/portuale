# Slice D S0 — display-site map: `to '<root>'` vs bare, per site (backlog #242)

Status: S0 verdicts. B9 (match real vs keep bare): **match real — no stop.**
The bare form does NOT encode a wider portuale rule: real prints the suffix
in `--columns` too (both arms, probe §1e), and the B0-byte pins re-pin
deterministically (Slice C precedent: runtime f-strings).

Method: real 3.0.82.2 from `3rdparty/portage` (`1d95fc2c5`), run on this host
against a `/tmp/xd-fx` stage built verbatim from `stage.sh`'s deltas
(`PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX`, `PYTHONHASHSEED=0`).
`PORTAGE_RUNNING_ROOT` is NOT a real variable (no reader in real's lib/bin) —
real's running root is always the host `/` when `ROOT != "/"` (`create_trees`,
`portage/__init__.py:497-529`). All S0 bytes below were reproduced locally and
agree with Slice A's checked-in captures; the one surprise (nested rows bare)
is settled by instrumenting real's row painter (§1c).

## 1. Per-site verdicts

### 1a. Merge-list rows, default layout — suffix iff the ROW's own root != `/`

Real `resolver/output.py:841-862` (non-columns arm of `display()`): under
`pkg.root_config.settings["ROOT"] != "/"` the row appends unquoted
`darkgreen("to " + pkg.root)` (`pkg.root` = EROOT); else `_set_no_columns`
(`:514-529`, no suffix). No `ordered`/`merge` gate anywhere on this path.

- `probes/default-g216top.txt`: FX rows `to /tmp/xa-inv-fx/fixtures/`, `/` rows
  bare (`[ebuild N] dev-lang/g216boot-1.0 ` with trailing space).
- `probes/default-blk0-bca.txt`: `blk0x-1` (DEPEND-resolved, `/`) bare,
  `blk0a/b/c-1` (arguments, FX) suffixed. `[uninstall]` bare (`/` — the
  removal follows the `/`-rooted parent's RDEPEND). `[blocks b]` never
  suffixed (appended blocker text, no root concept).
- `probes/default-cyc0z-1.txt`: argument (FX) suffixed, `/` rows bare.

Portuale change: `root_suffix` becomes the per-entry gate
(`entry.targets_running_root ? running_root : target_root`, suffix iff
`!= "/"`). Propagates to every arm that already threads `root_annotation`
(merge `emit`, `emit_nomerge`, `--columns`, uninstall, quiet).

### 1b. Merge-list rows, `--columns` — same per-row gate

`_set_non_root_columns` (`output.py:442-476`): `darkgreen("to " + pkg.root)` in
BOTH the quiet (`:462`) and non-quiet (`:475`) arms. Local probe
(`-p --columns --backtrack=0 blk0b/c/a`): FX rows `to /tmp/xd-fx/fixtures/`
in the column layout, `/` rows bare; no `[uninstall]` row at all
(`output.py:869-870` `continue` — portuale already skips it).

### 1c. Forced verbose-tree re-display (circular abort) — same gate (settled)

The 181 cyc0b-1 capture looked contradictory (FX-staged run, nested rows
bare). Instrumenting real (`set_pkg_info` + `_ordered_tree_display` log):
`handler.merge_list` holds THREE distinct Package objects —
`cyc0b-1@FX` (the argument), `cyc0b-1@/` and `cyc0a-1@/` (EAPI-8 DEPEND
resolved against ESYSROOT → the running tree, which inherits the fixture
repos via `PORTAGE_REPOSITORIES`). Nested display occurrences keep their own
`root_config`, so the gate fires per occurrence: argument suffixed, the
rest bare — exactly the captured bytes. No `ordered` gate exists; none is
needed. (Local repro: `=dev-libs/cyc0b-1` under the stage, byte-identical
shape to `docs/evidence/2026-09-27-181-circular-text/real/`.)

Real hardcyclea (`dev-libs/hardcyclea`, local probe): argument suffixed,
nested `/` rows bare, `Total: 3`. Portuale's pin keeps its own 3-row shape
(leading `[nomerge]`, `Total: 2` — documented) and gains uniform suffixes
under `fixture_env` (single-root); it cannot match real's mixed bytes there.

### 1d. Cycle-node text (`Package.__str__`) — stays bare (brief-directed)

`Package.py:568-608` appends quoted `to '{ROOT}'` for a merge node whose OWN
`root_config` ROOT != `/`; `_prepare_circular_dep_message`
(`circular_dependency.py:76-99`) formats via `f"{pkg}"`. Every pinned cycle
forms in `/` (cross-root DEPEND/BDEPEND — cyc0b1/cyc0w3/cyc0z probes all show
bare nodes), so real is bare on every pinned shape and portuale stays bare
(`circular_node_text` cut kept; its doc comment extended with this verdict).
A hypothetical FX-rooted cycle (pure-RDEPEND ring under staged ROOT) would
carry the quoted suffix in real — no pin covers it, and the brief directs
bare, so the cut stays.

### 1e. Skipped-block detail lines — quoted suffix iff the NODE's own root != `/`

`depgraph.py:1650+` `_show_missed_update_slot_conflicts` renders via
`str(pkg)` (`__str__`, quoted). `probes/default-blk0-bca.txt`: missed
`blk0x-3` (`/`) bare + host-profile `USE="" ABI_X86="(64)"`; parents (FX)
`to '/tmp/xa-inv-fx/fixtures/'` + fixture-profile `USE="" ELIBC="glibc"`.
`probes/hostroots-blk0-bca.txt` (ROOT=`/`): everything bare + ELIBC.
Parent order is root-sensitive (default blk0b-first, hostroots blk0c-first);
D must not sort. Portuale change: attribute skipped nodes/parents to their
resolving root (new `SkippedUpdate` fields), render the quoted suffix per
node, paint each side from its own root's profile (§2).

### 1f. `for <root>` / `in '<root>'` — no change

`for <root>` group headers (`depgraph.py:1616-1664`) belong to autounmask
verbosity portuale does not render per-root; the changed-deps site already
prints `for {root}` (`pretend.rs`). Installed `in '<root>'` already renders
with the real path. Neither moves in D.

### 1g. `--quiet`, `-pv`, `--alphabetical`, JSON — same gate, no new rule

Quiet arms carry the same gate (`output.py:462`); `-pv` only adds
decorations; JSON `builds_against_running_root` is portuale-only (untouched).

## 2. Cross-root USE paint

Real paints every node from its OWN `root_config`: merge rows via
`_display_use` (`output.py:822`, `pkg.use.expand` + `pkg_use_enabled` of that
root), missed lines via `pkg_use_display` (`UseFlagDisplay.py:55`, full
display incl. expand groups). Portuale builds every display from the single
target `Config` today. D threads the running `Config` (loaded from the
running root when it differs from target, else the target clone) into
resolve and picks per node by root attribution:

- Merge rows: running-rooted entries build `use_expand_display[_p]` from the
  running config. Observable nowhere in-contract (no IUSE on any hermetic
  running row; single-root elsewhere) and nil in the bed at `-p` (today's
  oracle is clean with split paints ⇒ no profile-sensitive `-p` content on
  any running row) — but it is real's rule, so it lands.
- Missed line: `skipped_use` from the attributed root's config. Bed-visible:
  with the bed pin dropped on the main list (§3) the missed `blk0x-3` is
  `/`-rooted and paints the container host profile (`ABI_X86="(64)"`),
  retiring `skipped-updates-cross-root-missed-line`. Contract (`fixture_env`
  single-root) keeps fixture paint (`ELIBC="glibc"`) deterministically.
- Parents' `consumer_use`: unchanged in practice (target-rooted), threaded
  for uniformity.

## 3. Bed staging (pmtest half)

`in-container.sh` pins `PORTAGE_RUNNING_ROOT=$FX`, which collapses portuale
to single-root while real (which has no such variable) splits. D adds a
per-list knob dropping that pin for the MAIN list only
(`l0-fixture-oracle.txt`, where the allowlist lives): portuale then resolves
the second root exactly like real (shared fixture repos, fixture-copied
`/var/db/pkg`, host profile). Blast radius is bounded by construction: row
sets/order coincide today with real already split (same repos, same vdb, same
visibility outcomes — else today's oracle would flag them), merge-row
suffixes are comparator-invisible (identity key is `(type, cp, slot)`; `to`
carries no `KEY="…"` pairs), and skipped parent suffixes normalise away in
`_norm_skipped_detail`. What moves in the oracle: the blk0 missed-line USE
(intended) and nothing else; the coordinator's bed run confirms. The other
twelve lists keep the pin (single-root portuale; suffix adds invisible).

Local pre-check (S1): portuale-with-drop vs real on the stage for blk0 order
(parent order is root-sensitive per §1e).

## 4. Comparator/allowlist verdicts (recorded per the brief)

- `_norm_skipped_detail` (`resolve-compare.py:131`, pmtest `2963703`):
  **KEPT** — still load-bearing: stage paths are volatile per run, so quoted
  `to '<root>'` must keep normalising on both sides now that portuale emits
  it too; USE content deliberately un-normalised (the missed-line delta must
  keep firing until the paint lands).
- Dual-row collapse by `(type, cp, slot)`: **KEPT** (general identity rule).
- `skipped-updates-cross-root-missed-line`: **RETIRED** by D (bed confirms).
- Cycle-text bare (`circular_node_text`, `root_suffix` docs, skipped-block
  "stay bare" comment, `what-this-proves.md` #206 paragraph): reopened per
  site per this map; the #206 cut survives ONLY at §1d.

## 5. D-assigned pin list (Conforme pin-table.md §D + this map)

Suffixes: every `-p` stdout merge/uninstall/nomerge row pin under
`fixture_env` (incl. `--columns`, `--tree`, `-pv`, `--quiet` variants),
skipped-block parent lines (quoted), g216 hermetic rows (target rows gain
`to <target>`; running rows keep `to <running>`); hardcyclea/cyc0b-shape
circular re-display rows (uniform suffixes, shape+Total unchanged).
USE: bed blk0 missed line only (contract: unchanged ELIBC). Must-not-move:
L0 (ROOT=`/`), `cyc0w3`/`cyc0b1` stderr blocks, RDEPEND-only oracle cells,
#161 driver tests, corpus (flag drift, NO bless — coordinator's).
