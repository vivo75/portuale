# crates/portage-repo/src/tests/license.rs
LICENSE string parsing and masking unit tests.

## fn parse_license_tree_flattens_a_plain_group_at_top_level
- Expected value verified directly against Portage's `portage.dep.use_reduce(..., opconvert=True)`: `"GPL-2 MIT"` -> `['GPL-2', 'MIT']`. A bare top-level list already has AND semantics, so a plain `"(...)"` there adds nothing structurally worth keeping.

## fn parse_license_tree_keeps_any_of_members_flat
- Expected value verified against Portage: `"GPL-2 || ( MIT BSD )"` -> `['GPL-2', ['||', 'MIT', 'BSD']]` -- the `||` group's own members sit directly in its own list, not double-nested.

## fn parse_license_tree_nests_a_plain_group_directly_inside_any_of
- Expected value verified against Portage: `"|| ( ( GPL-2 MIT ) BSD )"` -> `[['||', ['GPL-2', 'MIT'], 'BSD']]` -- a plain sub-group sitting directly inside a `||` group's own member list stays a genuine nested "this whole bundle is one alternative" unit, unlike the top-level case above.

## fn parse_license_tree_resolves_a_use_conditional
- Expected value verified against Portage: `"|| ( foo? ( GPL-2 MIT ) BSD )"` with foo enabled -> `[['||', ['GPL-2', 'MIT'], 'BSD']]`; with foo disabled -> `['BSD']` (the conditional contributes nothing at all, and the now-single-alternative `||` itself collapses away too -- Portage's `use_reduce` behavior, verified directly).
- Portage's `use_reduce` collapses a single-alternative `||` wrapper; portuale's parser keeps it (see the module doc comment on why that specific collapse isn't replicated). Masking-wise this is equivalent either way, since an `AnyOf` of one alternative and that alternative alone make an identical accept/reject decision.

## fn has_masked_license_any_of_alternative_bundle_needs_the_whole_bundle_accepted
- `"|| ( ( GPL-2 MIT ) BSD )"`: accepting only MIT (not GPL-2) must NOT satisfy the (GPL-2 AND MIT) bundle -- only accepting BSD, or both GPL-2 and MIT together, satisfies this.
