---
name: portuale-ebuild-merge
description: Use when changing how portuale executes ebuild phases or puts files on disk — EAPI behaviour, the config chain that feeds a phase, unmerge and same-version reinstall, preserved libraries, the VDB write order, binpkgs and gpkg, sandbox/namespace/umask of a demoted phase, or the glibc+bash merge safety gate.
---

# Ebuild execution and the merge path

This is the part of portuale where a mistake is not a wrong message but a
broken machine. Where the notes below say "Portage does X", it is the
compatibility constraint of sharing one VDB and one set of installed files with
Portage, or a safety rule, not parity for its own sake (charter property 4:
removal and replacement must not break running programs). Two packages — `sys-libs/glibc` and `app-shells/bash` — have
files that every running process maps; a merge that gets them wrong has no
undo.

## The safety gate before anything else

Any change touching the merge or unmerge path is not done until a **successful
test merge of `sys-libs/glibc` and `app-shells/bash`** passes, after *every*
slice that touches that path. The merge path is
`crates/portuale/src/ebuild_merge/`, `emerge_build/`, `emerge_getbinpkg.rs` and
`ebuild_unmerge.rs` (the list lives in `LLM/tools/pair_check.sh`, which fails a
commit that touches it without `LLM/evidence/merge-gate-<branch>.md`). The
authoritative text is `LLM/agent-context.md`, "Merge-path safety gate"; the
commands, run from `../pmtest`, are:

```
differential-test-bed/run/l1-merge-from-binpkg.sh \
    differential-test-bed/atomlists/l1-merge-gate.txt      # first run builds the pair
L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 \
    differential-test-bed/run/l1-merge-from-binpkg.sh \
    differential-test-bed/atomlists/l1-merge-gate.txt      # re-merge over the running pair
```

Both the sqlite and the redb VDB backends must merge them with identical
counters (a passing run reports `merged 2/2, 0 hard / 0 unexplained`).

## The VDB is a database with a write order

- **Lock it.** The merge holds an exclusive lock on the VDB for its whole
  duration (`VdbLock` in `crates/portage-vdb/src/files_write.rs`, Portage's
  `lockdir`), so portuale and Portage can share one database safely.
- **Delete the old entry, then rename the new one in.** Not the other way
  round (`V:5219` then `V:5238` in the same source).
- **Restore directory mtimes *before* writing the stamp.** Applying
  `utimensat` after stamping invalidates the metadata file that consumers use
  to validate caches.
- **Bump the mtime** on the category directory and the VDB root before and
  after modifications — that is what consumers read.
- **The COUNTER is never reused**, not even on a same-version reinstall.

## Unmerge and same-version reinstall

The rule that everything else hangs off: on a reinstall the **new image is
merged first**, the old instance is then unmerged with `others_in_slot=[new]`,
so every path the new image owns is left alone — including type changes such as
object → symlink (`LLM/evidence/2026-10-14-354-s0/matrix.md`).

From there:

- Files present in both old and new `CONTENTS` are left alone.
- Config-protected files are deleted only when unmodified; a modified one is
  left on disk, owned by no package.
- Unprotected orphans are deleted unconditionally by default; with
  `-unmerge-orphans` only when mtime/md5 still match.
- Preserve-libs decides from the **old installed instance's** `RESTRICT`, not
  the new package's, and copies the dropped soname's `CONTENTS` line into the
  new instance while recording the dependency.
- On a same-version reinstall the just-preserved paths are keyed by the same
  string as the exclusion, so a naive linkage map drops them and the library is
  deleted. Keep two owner sources and do not dedupe across them
  (`LLM/02.355-reinstall-unmerge-parity.opus.md`).

`CONTENTS` parsing is shared, not per-site: one parser in `portage-vdb` with
Portage's `_contents_re` rules, where `sym` uses a **greedy** match so the
split is at the *last* ` -> ` (`vartree.py:1951-1958`). That greedy split looks
wrong and is correct; a unit test pins it.

## The config chain that feeds a phase

- The **user profile is a real chain node**, appended only when a
  `make.profile` resolves and the directory exists. Remove the special-case
  reads but keep their warning ranks.
- **Resolved** `FEATURES` drives the gates (sandbox, test), not just the
  exported string — following the `emerge` path precedent.
- The incremental fold cannot distinguish "negated" from "never set". When
  `make.globals` was not read, seed a synthetic base layer under the stack, or
  a `make.conf` append form silently loses its base.
- EPREFIX is not supported in the phase environment: `phase_env.rs` exports it
  empty and `ED` collapses to `D`. `PORTAGE_OVERRIDE_EPREFIX` is read only on
  the binpkg path (`emerge_getbinpkg.rs`). Do not assume it works elsewhere.
- The direct `ebuild … merge/qmerge` path must consult `make.conf` for
  `PORTAGE_TMPDIR`; falling back to a host default is a known divergence.

## The phase environment

- A standalone phase env must export `SLOT`, the declared `IUSE` in
  `IUSE_EFFECTIVE`, and `BASH_FUNC____in_portage_iuse%%`; without them
  `in_iuse` fails where Portage succeeds and `.keep` filenames lose their SLOT.
- An empty `src_prepare` with no `eapply_user`/`default` dies rc 1 with
  `eapply_user (or default) must be called in src_prepare()!`
- A helper exiting **126 does not fail** the install phase — both sides swallow
  it, because the last command's status is not treated as the phase outcome.
- EAPI 7 and EAPI 8 environments are identical except the EAPI value itself
  and PV-derived paths. EAPI 8 strips empty directories from the image
  (`strict-keepdir`), so they are absent from both ROOT and `CONTENTS`.

## Demoted phases: namespaces, loopback, umask

A phase that drops privileges keeps Portage's namespaces by unsharing **raw**
(no user namespace) in `pre_exec` while still root, then demoting in the same
child. Three details are unit-pinned because each one silently breaks parity:

- **umask `022`** must be set after the demotion, or portuale creates
  `0644`/`0755` files where Portage does not.
- **Loopback** comes up via raw netlink inside `pre_exec` (`lo` + `10.0.0.1/8`,
  plus `fd::1/8` where IPv6 works).
- **The pid-drop is a second fork**: without re-exec it fails with ENOMEM, and
  that is a safety rule, not an implementation choice.

## Binary packages

- `pkg_pretend` runs **before** merging a binary, so metadata checks happen
  before the image is installed.
- A digest failure renames the partial file rather than re-downloading
  immediately.
- The pretend pass fetches remote binaries before any merge and reuses the
  fetched file if verification succeeds; a verification failure aborts before
  anything merges, leaving no VDB entry and no payload.

## Traps

- **Plumbing a fix "properly" can turn an arm off everywhere.** Making the
  orphan arm explicit would have switched it off in every pmtest root and
  flipped unrelated unmerge pins; the shape of the fix is part of the design.
- **A harness that compares only post-merge `CONTENTS` cannot see a deletion
  bug** — a file one side removes and the other keeps is in neither.
- **Tests that lack pending entries pass through the live fallback** and
  discriminate nothing.
- **Four unmerge implementations exist** (the local one, the remote merge
  script, the shell loop, and the VDB ebuild). A change in one is not a change
  in all.
- **`PORTUALE_BIN_DIR` set but wrong is an error, never a fallthrough** to the
  build tree or the embedded copy.

## Where to look

| Question | Source |
|---|---|
| What "install into the filesystem" means, and the merge gate | `LLM/agent-context.md` |
| Reinstall/unmerge parity, arm by arm | `LLM/02.355-reinstall-unmerge-parity.opus.md` |
| Config-chain parity | `LLM/02.346-ebuild-config-options-parity.opus.md` |
| EAPI phase audit | `LLM/02.348-eapi-phase-audit.opus.md` |
| VDB write order | `LLM/evidence/305-s0-vartree-write-order.md` |
| Sandbox, namespaces, umask | `LLM/superpowers/plans/2026-10-30-630-namespaces.md`, `…-631-umask.md` |
| The bed that proves it | `../pmtest/differential-test-bed/` |

## Provenance

The documents this skill was distilled from, and how many verified facts each
contributed: [`references/sources.md`](references/sources.md).
