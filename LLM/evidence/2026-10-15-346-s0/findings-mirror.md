# Findings mirror (staged for `../pmtest/differential-test-bed/findings/`)

> Staged copy of the S0 verdicts for the review rule on Portage expectations
> (plan G1). Source: `portuale/LLM/evidence/2026-10-15-346-s0/{SUMMARY,matrix}.md`.
> Copy this file to `../pmtest/differential-test-bed/findings/2026-10-15-346-s0.md`
> (pmtest-side act, not done by the S0 worker).

- Oracle Portage 3.0.82.2 vs portuale `f221c03f` (mtime 2026-10-15 19:18:07 UTC).
  104 scenarios, hermetic `PORTAGE_CONFIGROOT`s, all `emerge` cells with
  `--package-moves=n` except the two UPD-coupling demos.
- Confirmed divergences (fix slices): user-profile node (S1; 11 use/keywords/
  sets rows), `package.keywords` user+profile (S2; warning text captured),
  build-id in accept_keywords/package.use (S3), make.conf TMPDIR/DISTDIR/
  FEATURES + trailing-slash + REPO_NAME + ARCH-filter + direct USE (S4),
  stamp/skip/environment/WORKDIR/Manifest lines (S5), `>>>` markers + stdout
  tee + `build.log` + PORT_LOGDIR (S6), all-masked abort preamble (S7, narrowed
  to the top-level all-masked arm), argparse error lines + trailing blank
  (S8), getbinpkg exclude/include + per-repo keys + partly-excluded skip (S9).
- Owner decisions with evidence: merge/unmerge listing is direct-path-only
  (S10), auto-clean fires only on same-CP+slot older-counter leftovers with
  user `AUTOCLEAN=yes` (S11; forced-yes after global updates must NOT be
  reproduced — l00).
- Cuts confirmed with text: argparse `usage:` (hash-order, seed/noseed md5s),
  `Performing Global Updates` (g00/l00), news (not probed, out of scope).
- New for backlog #371–#373: empty-set rc 2 (`no package atoms…`), un-sourced
  bashrcs, unread per-repo usepkg keys (filing only; first reproduced in-cell).
