# S0 #645 — real-tree perl-5.44 cells still diverge after #572 + #643

**Status:** S0 record, no product change. Branch `backlog/645-real-tree-s0`
(portuale; pmtest untouched).
**Bed log:** `../pmtest/differential-test-bed/logs/l0-20261103T001814Z/`
(graded binary `7b7a4e5f-dirty`, both #572 + #643 fixes in).
**Bed tree snapshot:** `../pmtest/differential-test-bed/repos/gentoo` @
`e07309ba6` (`metadata/timestamp.chk`: `Thu, 29 Oct 2026 04:00:57 +0000`),
identical to the log's `fingerprint.tsv:3`. All ebuild quotes below are
verbatim from that snapshot.
**Oracle:** Portage 3.0.83 (`3rdparty/portage`, `ResolverPlayground`).
**Probe:** `probe645_mutations.py` (same dir, `M0`..`M6b`;
`python3 probe645_mutations.py <MUT>`), Portage-oracle only.
**Prior S0s:** `LLM/evidence/2026-10-25-567/` (#567 tree-side cause),
`LLM/evidence/2026-10-28-572/REPORT.md` (#572 seed feed),
`LLM/evidence/2026-11-02-643/REPORT.md` (#643 consumer-side expansion).

## 1. The single trigger

**The installed virtual's tree state is multi-version** — SLU ships 4 tree
ebuilds (`1.630.0-r1`, `1.680.0`, `1.680.100_rc`, `1.700.0`), Encode ships 2
(`3.210.0-r2`, `3.240.0`) — tripping the single-tree-version cut **first** in
code order in **both** mechanisms:

- `virtual_expansion_seeds` (`crates/portage-repo/src/resolve/rev_dep_or.rs:706`):
  `if ebuilds.len() != 1 || ebuilds[0].version != *version { continue; }`
- `expand_new_style_virtual_arm`
  (`crates/portage-repo/src/resolve/rev_dep_or.rs:823`): the same cut.

Each firing is independently sufficient (WA_count / WB_count worlds: purity
held, yet silent — Step 4). One trigger explains both mechanisms' silence on
all three cells (same missing pair, same co-scheduled perl upgrade — Task 1,
§5 below). This is the fourth the trace names: not reachability order, not
versioned-arm form, not USE-conditional wrapping.

## 2. Consumer path (verbatim depstrings, libreoffice cell)

Request: `app-office/libreoffice` atom (`meta.tsv`:
`app-office_libreoffice atom 1 1`; `run.log` rc=1/1 — both sides abort
identically on the autounmask backtrack; the abort is not this S0).

### Leg A — Try-Tiny → dead nested `||` → Sub-Name (real-only)

| # | Package (real LO line) | Verbatim depstring reaching the next leg |
|---|---|---|
| 1 | `app-office/libreoffice-25.2.7.2-r1` (real 236) | `BDEPEND="… dev-util/intltool …"` (`libreoffice-25.2.7.2-r1.ebuild:273`) |
| 2 | `dev-util/intltool-0.51.0-r3` (real 104) | `BDEPEND="dev-lang/perl dev-perl/XML-Parser"`, `RDEPEND="${BDEPEND} sys-devel/gettext"` (`intltool-0.51.0-r3.ebuild:14-18`) |
| 3 | `dev-perl/XML-Parser-2.470.0-r1` (real 103) | `RDEPEND=">=dev-libs/expat-1.95.1-r1 dev-perl/libwww-perl"` (`XML-Parser-2.470.0-r1.ebuild:15-18`) |
| 4 | `dev-perl/libwww-perl-6.810.0` (real 101, `USE="ssl -test"`) | bare top-level `dev-perl/Try-Tiny` in `RDEPEND` (`libwww-perl-6.810.0.ebuild:26`); no USE wrap, no version. The `test?`-wrapped Try-Tiny in `dev-perl/HTTP-Message-7.10.0`'s `BDEPEND` (lines 32–42) is inactive (`USE="-test"`): libwww-perl is the sole in-cell live Try-Tiny referrer. |
| 5 | `dev-perl/Try-Tiny-0.320.0` (real 95, `USE="-minimal -test"` → `!minimal?` TRUE) | `RDEPEND=" !<=dev-perl/Try-Tiny-Except-0.10.0  !minimal? ( \|\| ( >=virtual/perl-Scalar-List-Utils-1.400.0 dev-perl/Sub-Name ) )  virtual/perl-Carp  >=virtual/perl-Exporter-5.570.0"` (`Try-Tiny-0.320.0.ebuild:17-24`). The `||` is NESTED (RDEPEND → `!minimal?(…)` → `||`), first arm versioned `>=`, second arm bare. |
| 6 | installed `virtual/perl-Scalar-List-Utils-1.680.100_rc` (`vdb-list.txt:309`) | `RDEPEND=" \|\| ( =dev-lang/perl-5.42* ~perl-core/${PN#perl-}-${PV} )  dev-lang/perl:=  !<perl-core/…  !>perl-core/…-r999"` (`perl-Scalar-List-Utils-1.680.100_rc.ebuild:10-15`, top-level `||`, first arm `=`-pinned to old perl, second arm `~`). Tree `perl-core/Scalar-List-Utils/` holds only `1.630.0` + `1.680.0` — NO `1.680.100_rc` ebuild, so the `~` arm matches nothing in-tree. |
| 7 | `dev-perl/Sub-Name-0.280.0-r1` (real 94, immediately before Try-Tiny 95) | fall-through arm, real-only. Tree-wide grep for `dev-perl/Sub-Name` intersected with this cell's merge list leaves Try-Tiny as the only in-cell referrer. |

### Leg B — Encode-Locale → virtual/perl-Encode → perl-core/Encode (real-only)

| # | Package (real LO line) | Verbatim depstring reaching the next leg |
|---|---|---|
| 4′ | `dev-perl/libwww-perl-6.810.0` (real 101) | bare top-level `dev-perl/Encode-Locale` (`libwww-perl-6.810.0.ebuild:17`); second path via `>=dev-perl/HTTP-Message-6.180.0` → `HTTP-Message-7.10.0` RDEPEND `>=dev-perl/Encode-Locale-1.0.0` (line 22). Intersected with this cell's merge list, only HTTP-Message and libwww-perl remain. |
| 5′ | `dev-perl/Encode-Locale-1.50.0-r1` (real 34 / portuale 79, both `USE="-test"`) | `RDEPEND=">=virtual/perl-Encode-2.0.0"` (`Encode-Locale-1.50.0-r1.ebuild:15`) — top-level, versioned, no USE wrap, no `||`. |
| 6′ | installed `virtual/perl-Encode-3.210.0-r2` (`vdb-list.txt:297`) | `RDEPEND=" \|\| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40* ~perl-core/Encode-3.210.0 )  dev-lang/perl:=  !<perl-core/Encode-3.210.0 !>perl-core/Encode-3.210.0-r999"` (`perl-Encode-3.210.0-r2.ebuild:10-15`, top-level `||`, two `=`-pinned old-perl arms + `~` arm). |
| 7′ | `perl-core/Encode-3.210.0` (real 33, immediately before Encode-Locale 34) | the `~perl-core/Encode-3.210.0` arm expanded, real-only. |

Installed side (`vdb-list.txt`, 320 pkgs): `dev-lang/perl-5.42.2` installed
(line 116); both virtuals installed at exactly the tree versions quoted
above, so the quoted tree RDEPENDs are the installed RDEPENDs.

## 3. Per-mechanism gate that skips (with the cell string that hits it)

Both gates are the single-tree-version cut; each fires first in code order
in its function, before purity/seed-arm logic is reached.

**Seed feed** — `virtual_expansion_seeds`
(`crates/portage-repo/src/resolve/rev_dep_or.rs:671`), gate at **:706**:

```rust
if ebuilds.len() != 1 || ebuilds[0].version != *version {
    continue;
}
```

- Leg A: virtual entry `virtual/perl-SLU AlreadyInstalled 1.680.100_rc`,
  tree count **4** (scratch world WA_full mirrors M6b). Skipped here; the
  fall-through arm `dev-perl/Sub-Name` never becomes a seed.
- Leg B: virtual entry `virtual/perl-Encode AlreadyInstalled 3.210.0-r2`,
  tree count **2** (scratch world WB_full mirrors M5). Skipped here; the
  `~perl-core/Encode-3.210.0` arm never becomes a seed. Plain consumer atom
  `>=virtual/perl-Encode-2.0.0` never reaches the consumer expander by design
  (Portage expands it inline, `dep_check.py:287-288`) — leg B is seed-feed
  territory only.

**Consumer expansion** — `expand_new_style_virtual_arm`
(`crates/portage-repo/src/resolve/rev_dep_or.rs:798`), gate at **:823**:

```rust
if ebuilds.len() != 1 || ebuilds[0].version != version {
    return None;
}
```

- Leg A: consumer arm `>=virtual/perl-Scalar-List-Utils-1.400.0` (Try-Tiny
  RDEPEND `!minimal? ( || ( >=virtual/perl-Scalar-List-Utils-1.400.0
  dev-perl/Sub-Name ) )`; `best_installed_matching` = `Some("1.680.100_rc")`)
  hits the 4-version cut → `None`, arm kept byte-identical, no conjunction,
  no fall-through.

**Recorded mandatory second lift** (not the trigger — fires only once the
count cut is lifted): with the count held at 1 (WA_pure) the conjunction
extras (`dev-lang/perl:=`, `!<`, `!>`) trip
`virtual_rdepend_is_expandable` (`rev_dep_or.rs:656-658`,
`!split.is_flat() && split.plain_atoms().is_empty()`, `:847-848` call site)
while Portage expands that conjunction shape (M3 falls through). The cut
overfits the S0-mixed DISJUNCTION shape (`A || (...)`,
`probe572_mixed.py`) it was written for. The follow-up must lift both.

## 4. Killed candidates (one line each)

- **Reachability order: DEAD** — WA_full/WB_full entries show the virtual
  `AlreadyInstalled` with `required_by=[consumer]` and the consumer
  merge-bound (`New`); portuale held the node and skipped it at the shape
  gates. Playground side: Portage sets `virt_parent` and still falls through
  (S0 #643 §2a).
- **Versioned-arm form: DEAD** — every probe uses `>=virtual/...` versioned
  arms; M0/M6a/M6b fall through in Portage and WA_wrap merges in portuale.
- **USE-conditional wrapping: DEAD** — M1 falls through in Portage; WA_wrap
  MERGES in portuale (expansion descends through `flag?`:
  `expand_seq_into` recurses with `arm_position=false`,
  `rev_dep_or.rs:946-954`).
- **Nested-`||` verbatim cut (the #644 suspect from the ledger): DEAD** —
  the real `||` is a direct child of `!minimal?(...)`, not of another `||`;
  only an `||` inside an `||` arm copies verbatim
  (`rev_dep_or.rs:930-931`). WA_wrap proves the wrapped position expands.
- **Conjunction extras as the SOLE trigger: DEAD** — WA_count/WB_count stay
  silent with purity held. Recorded as the follow-up's mandatory second
  lift (§3), not the cause.

## 5. The other two cells: SAME pair + SAME upgrade, no STOP

- `gnome-base/gnome-shell` (real 556 lines / portuale 546): sorted merge-row
  set diff is exactly the same two real-only rows
  (`Sub-Name-0.280.0-r1`, `perl-core/Encode-3.210.0`); perl `5.42.2→5.44.0`
  U-row both sides (real 126 / portuale 50); Try-Tiny both sides (real 151,
  adjacent to Sub-Name 150 / portuale 126); Totals 455 vs 453 (delta 2 =
  the pair). `run.log`: rc=1/1.
- `media-gfx/gimp` (real 249 / portuale 250): same two real-only rows, same
  perl U-row both sides (real 52 / portuale 22), Try-Tiny both sides (real
  71 adjacent to Sub-Name 70 / portuale 60), Totals 184 vs 182. `run.log`:
  rc=1/1.

Verdict: all three cells carry the identical `[missing]` pair with the
identical co-scheduled perl upgrade — libreoffice's trigger covers
gnome-shell and gimp; the plan's defined STOP exit does not fire. (Ruling 1
respected.)

## 6. Follow-up slice — #647

**#647 (shaped and numbered by this S0, filed at close-out; verified free across `LLM/backlog-tasks.md` and
`LLM/backlog-tasks-2026-10.md`, whose highest number is #646): lift the
single-tree-version cut in both expansion mechanisms to Portage's
per-version expansion, plus the conjunction-extras lift.**

### Exact product sites / signatures

All in `crates/portage-repo/src/resolve/rev_dep_or.rs`:

1. `pub(crate) fn virtual_expansion_seeds(ctx: &ResolveCtx<'_>, pass: &PassResult, grown: &BacktrackParams) -> Vec<((String, String, String), String)>` (:671) —
   lift the `:696-708` single-version gate (`ebuilds.len() != 1`) to expand
   EACH matching tree virtual à la Portage (`dep_check.py:196-290`),
   including the `1.700.0`/`3.240.0` `=perl-5.44*` conjunctions.
2. `pub(crate) fn expand_new_style_virtual_arm(args: ExpandNewStyleVirtualArmArgs<'_>) -> Option<Vec<String>>` (:798) —
   lift the same cut at `:813-825` identically.
3. `pub(crate) fn virtual_rdepend_is_expandable(split: &OrSplit) -> bool`
   (:656) — mandatory second lift: the non-`||` conjuncts
   (`dev-lang/perl:=`, `!<`, `!>`) must not veto expansion once the count
   cut is gone (WA_pure/WB evidence: count 1, purity held, yet silent;
   Portage M3 expands the same conjunction).

### Exact fixture / request

Hermetic fixture family mirroring M6b (leg A) + M5 (leg B): multi-version
virtuals with the VERBATIM real RDEPENDs (SLU `1.630.0-r1`/`1.680.0` with NO
`||` at all, `1.680.100_rc`, `1.700.0` naming `=perl-5.44*`; Encode
`3.210.0-r2`/`3.240.0`), Try-Tiny consumer with the verbatim wrapped
`!minimal? ( || ( >=virtual/perl-SLU-1.400.0 dev-perl/Sub-Name ) )`,
Encode-Locale consumer with plain `>=virtual/perl-Encode-2.0.0`, vdb
installed at the real versions, real md5-cache entries — resolved with
**atom requests** (`dev-perl/Try-Tiny` + `=dev-lang/perl-5.44.0` form, as in
M6b/M5). MUST NOT use `@world --update --deep`: M4/M5b document the
request-shape effect (virtual upgrades instead of fall-through), which is
not the cell.

### Oracle

Portage 3.0.83 playground: M6b mergelist
`[Sub-Name-0.280.0-r1, perl-5.44.0, Try-Tiny-0.320.0]` (virtual stays at
installed `1.680.100_rc`) and M5 mergelist
`[perl-5.44.0, perl-core/Encode-3.210.0, Encode-Locale-1.50.0-r1]` (virtual
stays installed) — reproduce exactly, including NO virtual upgrade. Bed
proof: the three L0 cells' `[missing]` rows gone with a follow-up binary.

### Tripwires the follow-up must not move

Q19 skip rows, libc-silence, nodeps/onlydeps (seeds gated on full walks),
deep/emptytree-once-only, backtrack count deliberately unasserted
(portuale 1/20 feed vs Portage 0/20 inline — #572 precedent). Do NOT touch
#644 (nested-`||`-verbatim cut, mixed-`||` cuts — different family) or #646
(no-upgrade nested-virtual pin — the tie the other way); the M4/M5b
request-shape effect must stay pinned as not-the-cell.

## 7. Bed verdict

Both `known-divergences.yaml` entries
(`perl-544-stabilization-version-selection-{encode,sub-name}`) **stay** until
a bed with the #647 fix proves them gone. No yaml moved by this S0; no
product code, no fixture commits, no pmtest changes.
