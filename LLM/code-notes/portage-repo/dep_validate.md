# crates/portage-repo/src/dep_validate.rs
Ebuild and binary `*DEPEND` validation (#341 S4).

## enum DepErrorCategory
- Which `_invalid` bucket a dep failure files under: `Syntax` (= Portage's `dependency.syntax`), `IuseMissing` (`IUSE.missing`), `EapiIncompatible` (`EAPI.incompatible`). Portage: `lib/_emerge/Package.py:534-557` (`_metadata_exception`).

## fn validate_dep_string
- One dep string like `use_reduce(matchall=True, flat=True, token_class=Atom, is_valid_flag=…)` with `eapi=pkg.eapi`: the `portage-use-reduce` walk with a per-atom-token hook (`parse_atom_with` with `AtomOptions { eapi, ..Default::default() }`, then `check_conditional_flags`). Returns the parsed atoms, or Portage's error text for the first failure.
- `check_conditional_flags` runs per atom right after its parse because Portage runs it inside `Atom.__init__`, before the strong-blocks result surfaces (dep:2125-2144). The order is not observable (strong blocks are off only in EAPI 0/1, where use deps already fail first — review I-1, proven live), but the error itself is.
- An EAPI Portage itself cannot parse (`3_pre1`, padded) has no validation behaviour (eapi.py:224-229 raises there); reported as the first token's syntax failure with the S1 cut text (`invalid EAPI '<e>'`).
- No new memo: `is_visible` is already memoised (visibility.rs:236); a `(repo_location, cpv)` key would collide for binaries (all have an empty `repo_location`) and for different configs.

## fn invalid_dep_reasons
- Portage `Package._validate_deps` + `_metadata_exception` for a repo/binary candidate, rendered `invalid: <KEY>: <text>`, grouped by category in first-insertion order (Package.py:534-557 files each error under its category; depgraph.py:12560-12563 iterates the buckets in that order — so BDEPEND IUSE.missing + DEPEND syntax + RDEPEND IUSE.missing prints BDEPEND, RDEPEND, DEPEND; S0 cell `t341-mixcat`).
- Keys visited in `_dep_keys` order (`BDEPEND DEPEND IDEPEND PDEPEND RDEPEND`, Package.py:93); each key's first error files under its category. Categorized errors (IUSE.missing, EAPI.incompatible) print `<KEY>: <message>` without the `Invalid atom (…), token N` wrapper.
- The `Improper context` loop runs per key that parsed cleanly (`else:` branch, Package.py:330-356), one `Syntax` reason per atom with `slot_operator == Equals && sub_slot.is_some()` (`Atom.slot_operator_built`, dep:2156-2162) — quoted via `atom_to_string`, which rebuilds the original token. Only for unbuilt (ebuild) candidates (`if not self.built`); binaries skip it (S0 `F-binary-rewritten` shape). Reopens backlog #15 (see the plan §0.4).
- Only ever called for repo/binary candidates, never vdb ones (Portage passes `dep_eapi=None, dep_valid_flag=None` for installed packages, Package.py:315-328); the exemption holds structurally.
