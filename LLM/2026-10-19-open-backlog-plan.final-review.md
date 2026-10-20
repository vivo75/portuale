# Final review — `2026-10-19-open-backlog-plan.md` (closed 2026-10-20)

Branch `batch/2026-10-19`, portuale `b76ee705..f8961146` (33 commits), pmtest paired commits up to `766f894`. No push.

## Result
- Every open item ≥ #300 is DONE, CLOSED (with evidence, no code: #376 #407 #418) or filed as a residue with a real number (#430–#440).
- Gate on the last tree: fmt, clippy (0 warnings), cargo test, pmtest suite `4147 passed, 37 skipped, 2 xfailed`, failed list empty.
- Reference for every expected value: Portage 3.0.82.2.

## Beds (frozen binary, final run on `f8961146` unless noted)
- G-R: `l0-fixture-oracle` 48 probes / 0 unexplained; `l0-resolver` no newly unexplained cell (one newly clean). Measured on `330dbcf4`; later commits touch only the pubgrub bridge and `--resume --fetchonly`.
- G-PB (pubgrub, five modes): clean cells 61 / 64 / 67 / 14 / 85 against baseline 60 / 63 / 66 / 14 / 84, no exit or `python_targets` findings. The first run on `330dbcf4` found a real regression (see below); the table is the re-run after the fix.
- G-M: reinstall runs of glibc + bash (plain and with `INSTALL_MASK`) 2/2 merged, 0 unexplained; `l31` control 0 unexplained. The plain first run is vacuous (the image already holds those versions); it is not counted.
- G-P: `l2` builder 0 unexplained, `l3` smoke 0 unexplained. The `l3` merge-gate cell compiled gcc for over 3 h and was stopped; it is **not claimed**.

## Found late, fixed
- Bed G-PB, `-pvuD` / `-pvuDN`: #429's rebuild-with-flip planning also fired for alternatives of `||` groups and for deps of kept installed packages, planning `setuptools`/`jinja2` reinstalls and `python_targets` changes (`@system` and `@world` went from rc 0 to rc 1). Fixed in `f8961146`; probe `app-editors/emacs -pvuDN --solver=pubgrub`.
- Keep-going notice wrapping on long hyphenated paths: `wrap_words` now follows Python `textwrap` (`33d635dc`, unit test with Python-computed lines).
- Review R2 (read-only, gemini): `resume --fetchonly` printed the resolution notice under `--quiet`/`--nodeps` (fixed `01c5c03f`); `complete_reuse` without a visibility gate (filed #440); `--quiet` on the keep-going re-plan and the binary verify line (already filed as #437e and #436c).

## Open after close-out
#430–#440 (see `backlog-tasks.md`). Next free number: **#444**.
