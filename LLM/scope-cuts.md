# Scope-cut registry

Every deferred cut named in `LLM/what-this-proves.md` gets one entry here.
The proof file is append-only and is never edited to add a `CUT-n` marker,
so this registry is where orphan cuts are acknowledged. Checked by
`python3 LLM/tools/cut_registry_check.py` (exit 0 when clean).

## Entry format

One entry per line, `- CUT-<n>: <one line> → <target>`, where `<n>` is
unique and `<target>` is exactly one of:

- `#<backlog>`: the cut is open and its follow-up is that backlog number.
- `closed #<backlog>`: the cut was open, and its follow-up is now DONE.
- `none (accepted): <reason>`: no follow-up. Give the reason.

## Acknowledging a marker in what-this-proves.md

A marker (`Still deferred`, `remains open`, `still open)`) on line `L` of
`what-this-proves.md` is acknowledged when either:

1. a registry line cites the line as `what-this-proves.md:<L>` (the cited
   number must be the marker's own line), or
2. a `CUT-<n>` reference sits within 3 lines of it in the proof file.

## Rules enforced

- (a) an open entry (`→ #<backlog>`) whose backlog entry is DONE is an
  error. Write `→ closed #<backlog>` once the follow-up has shipped.
- (b) an unacknowledged marker in `what-this-proves.md` is an error.
- (c) an entry naming a backlog number absent from `backlog-tasks.md` is an
  error. Duplicate `CUT-<n>` numbers and entries without a `→` target are
  errors too.

## Entries

- CUT-1: `emerge -pc` in args mode, `world_sets` `@`-refs as roots (what-this-proves.md:760) → none (accepted): niche (depclean with `@set` atoms on the command line). Not checked against real Portage here. Candidate for a backlog item if an owner needs it.
- CUT-2: depclean slot-operator rebuild edges (what-this-proves.md:782) → none (accepted): deliberately left out of the depclean increments, and the proof file still lists them as out at what-this-proves.md:736 and 915. No backlog item. Candidate for one.
- CUT-3: `package.use` stacking, which the paragraph at what-this-proves.md:1577 calls open (what-this-proves.md:1577) → none (accepted): superseded. The repo-plus-profile `package.use` stacking was closed by the later follow-up paragraph at what-this-proves.md:2023.
- CUT-4: IUSE-aware `_*` wildcard expansion (what-this-proves.md:2804) → none (accepted): superseded. what-this-proves.md:8305 re-analysed it (such tokens only originate from `USE_EXPAND` folding or the `package.use` shorthand) and implemented the expansion in `effective_use_flags` where IUSE is visible, so no per-package IUSE lookup at config depth is needed.
- CUT-5: recursively building a new package against the running root (what-this-proves.md:7000) → none (accepted): partly shipped as the deliberately non-recursive `--root-deps` increment, whose section is cited from the marker's own paragraph. The recursive remainder has no backlog item. Candidate for one.
- CUT-6: standalone `ebuild <file> <phase>` USE and flags (what-this-proves.md:16061) → none (accepted): backlog #100 (DONE 2026-09-21) made standalone `ebuild` export the full resolved config env. The USE half of the sentence was not re-verified here, so a check of the USE half is worth doing before closing it.
- CUT-7: upstream brush PRs unopened (what-this-proves.md:16095) → #5: the backlog entry "brush PRs: submit staged fixes" is the follow-up. It needs the owner's GitHub auth, so it is not a portuale code gap.
- CUT-8: `--binpkg-respect-user-patches` is parsed and stored but changes nothing, no patch digests are compared (what-this-proves.md, backlog #503 paragraph) → closed #509
- CUT-9: `--circular-deps-report=json` is parsed and stored, the report keeps one format (what-this-proves.md, backlog #503 paragraph) → closed #507
- CUT-10: `--merge-wait-scope` is parsed and stored, the scheduler does not use it (what-this-proves.md, backlog #503 paragraph) → #512
