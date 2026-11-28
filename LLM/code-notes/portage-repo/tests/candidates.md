# crates/portage-repo/src/tests/candidates.rs
Context moved out of the code comments.

## fn split_pf_keeps_a_digit_led_word_in_the_package_name
- Backlog numbers removed from the comment (original text): #127: the public splitter's advertised disambiguation, pinned for the first time -- a digit-led word belongs to the name (`foo-1bar-2.0` -> `foo-1bar`) and a `-r<digits>` word is not a version on its own (`foo-r1-2.0` must not mis-split as `("foo", "r1-2.0")`). Expected values are real `_pkgsplit` (`pkgsplit('foo-1bar-2.0') == ('foo-1bar', '2.0', 'r0')`, `pkgsplit('foo-r1-2.0') == ('foo-r1', '2.0', 'r0')`, system Portage 2026-09-23).
