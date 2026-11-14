# Charter

## What portuale is

`portuale` is a package manager for Gentoo, written in Rust. It started as a
reimplementation of Portage's `emerge` and `ebuild`; `mrg` was added later
as a second front end with the same role as `emerge` and extra features and
options. Both resolve dependencies, fetch sources, run the ebuild phases
(with a real `bash` or the embedded `brush` shell), merge and unmerge files,
and handle binary packages in the xpak and gpkg formats.

`mrg` is not bound to `emerge`. It may use its own solver (or several),
and shares crates and code with `emerge` where that helps.

## Relation to Portage

Portage is where portuale comes from, and it remains a teacher: we keep
watching how it evolves and learn from it, with a pragmatic eye.
It is no longer a goal and no longer the oracle.

- Portuale is not a fork and does not try to replace Portage. We hope Portage
  is maintained for a long time and kept in good health.
- `portuale emerge` keeps the same options, with the same meaning and
  intentions. It may differ from Portage in the order it builds packages
  and in the messages it shows the user.
- Matching Portage bug for bug became too expensive, because portuale does
  not use the same algorithm. We do not copy Portage's bugs or accidents.
- Changes made in portuale may not be portable back to Portage.
- We keep helping Portage where we can, for example with the `pmtest` suite.

## What counts as correct

Behaviour is derived from the Package Manager Specification (PMS) and the
EAPIs, and from the Gentoo Devmanual. Where those are silent, the owner decides.

The main goal is a healthy Linux installation. Concretely, a plan or a
finished merge must satisfy these properties, and we want each of them
checked by a program:

1. **Dependencies hold.** Every package that stays installed has its runtime
   and post-merge dependencies satisfied. No blocker is violated, no
   `REQUIRED_USE` is broken, no slot holds two packages.
2. **Order is legal.** Build-time dependencies are installed before the
   package that needs them to build; runtime dependencies are present by the
   end.
   2a. **Cycles are not forbidden.** Some dependency loops cannot be
   serialized (A needs B and B needs A, or longer loops). We drop the
   self-imposed limit that a package is built only once: breaking such a
   loop by building a package twice is allowed, as long as the final state
   meets every property here. `mrg --plan-world --sort depend` does this:
   it cuts a cycle at a runtime-only edge that carries no build-time need,
   and where every runtime edge carries one it builds the back-edge's
   consumer a second time, right after the cycle. It also verifies the cut
   result with a usability closure and builds the first package whose
   build-time need chain stays unmet a second time, after the closure
   becomes satisfied. The health checker
   excuses the need the first build missed when the identical package is
   merged again later (`cycle-noted`), and still fails a slot that would
   hold two different versions.
3. **Packages are as new as possible.** Each package ends at the best
   visible version that fits the constraints of all the others.
4. **Removal does not break running programs.** The user may remove a package
   even when that leaves other packages with unsatisfied dependencies;
   portuale warns, but does not forbid it. What it must do, as Portage does,
   is keep the shared libraries (`.so` files) that remaining programs still
   link against, by preserving them temporarily, so those programs keep
   working until they are rebuilt.
5. **Messages help.** Every error names the cause (which package, which atom,
   which flag) and says at least one thing the user can do about it.

Until such checks exist for a behaviour, code that merely copies Portage stays.
It is removed gradually, once a check shows that doing without it is at least
as good.

## How the code must look

Almost all of the code is written by LLMs. It must still be readable by humans,
because a project other people cannot review and enjoy would only ever work on
one machine.

- Comments and documentation are in international English. The `LLM/` folder
  is the exception and is left to the LLMs.
- Functions have a clear, well-defined behaviour and are small enough to hold
  in the head.
- A change that improves features and also shrinks the code (tests excluded)
  is worth more than one that adds the same features by only adding code.

## Open points

- The `pmtest` pins and the harvested corpus record what Portage printed.
  They are useful as a regression and comparison tool, no longer as a
  statement of what is right.
