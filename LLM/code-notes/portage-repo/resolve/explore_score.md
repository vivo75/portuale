# crates/portage-repo/src/resolve/explore_score.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): Ranking of settled passes for `PORTUALE_BACKTRACK_EXPLORE` (#804).

## struct Explore
- Backlog numbers removed from the comment (original text): `PORTUALE_BACKTRACK_EXPLORE=<n>` (#804): measure what exploring up to `n` settled passes would give, without changing the returned result. The first settle is kept and returned; the rest only feed the report.
