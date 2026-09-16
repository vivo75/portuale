# GLEP compliance review (2026-09-15)

Audit of portuale's current implementation against the 12 GLEPs flagged as
technical/tooling-relevant, cross-referenced against both portuale's Rust
source and the vendored real Portage (`3rdparty/portage/`) it is validated
against. Read `docs/glep/glep-NNNN.rst` for full text; this doc extracts only
the normative requirements and portuale's compliance status.

Key framing established while researching this: several of these GLEPs
specify behavior that lives entirely in `emerge --sync` (tree/rsync
verification) or in `repoman`/`pkgcheck` (tree QA), neither of which portuale
implements or intends to — `--sync`/`--metadata` are recorded as **permanent
non-goals** (see memory `whole-emerge-actions-complete`). Where a GLEP's
requirement falls inside that boundary, it's marked N/A below rather than a
gap, and I verified the boundary in the vendored real source rather than
assuming it.

---

## GLEP 78 — Gentoo binary package container format

Specifies the `gpkg` container: an uncompressed outer `.tar` holding, in
order, a `gpkg-1` marker file, a `metadata.tar${comp}` (+ optional `.sig`),
an `image.tar${comp}` (+ optional `.sig`), and a `Manifest` (optionally
clear-text OpenPGP-signed). Mandates member-compression support (not
whole-archive), OpenPGP detached signatures per member plus a Manifest
signature to prevent replace/reuse attacks, duplicate-file rejection, and
regular-files-only extraction.

**Status: Full.**

- Implemented in `rust/portuale/src/binpkg.rs`: `gpkg-1` marker detection
  (`binpkg.rs:151-170`), per-member compression classification including zstd
  and xz (`classify_inner_member`, tested `binpkg.rs:1331-1344`), Manifest
  `DATA` line parsing with a **duplicate-name reject**
  (`binpkg.rs:526` `"Manifest lists {name} more than once"`), a **member-not-
  in-Manifest reject** (`binpkg.rs:540`), and full OpenPGP verification of
  both the clear-signed Manifest (`verify_clearsigned_manifest`,
  `binpkg.rs:1308`) and detached member `.sig` files via `GpgVerify`
  (`binpkg.rs:106-135`), gated by a `binpkg-ignore-signature`-style knob.
  Hash support is BLAKE2B + SHA512 (`binpkg.rs:1594,1601`), matching real's
  own `MANIFEST2_HASH_DEFAULTS`.
- **Regular-files-only extraction is enforced since 2026-09-15 (#56).**
  `outer_regular_member`/`outer_prefix_dir` (`binpkg.rs:102-169`,
  `symlink_metadata`/`FileType`, no follow) and the shared
  `walk_outer_members` run at all three outer-container sites
  (`read_gpkg_metadata`, `verify_gpkg_manifest`, `extract_gpkg_member`):
  the prefix must be a real directory and every trusted name (`gpkg-1`,
  `Manifest`, `metadata.tar*`, `image.tar*`, `.sig`) must be a regular
  file — anything else is an error naming the member and its type. The
  three outer `tar -xf` calls also pass `--no-same-owner`.
- The audit's own follow-up ("real's enforcement wasn't independently
  re-verified") is answered by #56 S0's crafted-archive matrix
  (`TEST/findings/l2.md` "## #56 S0"): real's `tarfile.extractfile` never
  leaves the archive (a symlink resolves inside it or raises `KeyError`;
  every special member fails the Manifest size/structure checks first),
  while the pre-fix portuale listed the host `/etc` through a symlinked
  prefix and blocked on a FIFO member — so this closed a real divergence,
  not just theoretical hardening.
- **Scope note on "Full":** GLEP 78's "only regular files are permitted"
  names the **outer** container, which is what #56 hardened and what this
  status covers. The same class one level down — a symlinked
  `metadata/<KEY>` member *inside* the inner `metadata.tar`, which
  portuale's `Path::is_file` + `fs::read` walk still follows while real's
  `unpack_metadata` raises — is outside the GLEP's wording but is a live
  portuale-only host read, open as backlog **#58** (branch `backlog/58`).

## GLEP 82 — Repository configuration file (layout.conf)

Defines the `metadata/layout.conf` key-value format and a fixed set of keys:
`masters` (mandatory), plus optional `manifest-hashes`,
`manifest-required-hashes`, `use-manifests`, `update-changelog`,
`cache-formats`, `eapis-deprecated/banned/testing`,
`profile-eapis-deprecated/banned`, `repo-name`, `aliases`, `thin-manifests`,
`sign-commits`, `sign-manifests`, `properties-allowed`, `restrict-allowed`,
`profile-formats`.

**Status: Partial — every key real `emerge` itself consults is now
honoured.** `masters`, `aliases`, `repo-name` and `profile-formats` were
already read; `cache-formats` was the last genuinely actionable gap and is
closed since 2026-09-15 (#55, below). Every other key is parsed as raw
text and ignored, matching the GLEP's own "unknown keys should be
ignored" rule; their real-world effect is nil for `emerge` (dead in the
vendored tree, or `repoman`/`pkgcheck`/`--sync` territory).

- `parse_layout_conf` (`rust/portage-repo/src/lib.rs:1036`) returns the
  raw `key = value` map; the repo-config pass that consumes it
  (`lib.rs:1259-1300`) extracts `masters`, `aliases`, `repo-name`,
  `profile-formats` and — since #55, 2026-09-15 — `cache-formats`
  (`lib.rs:1276-1293`, resolved onto `RepoConfig::cache_formats`). Every
  other key is silently ignored (parsed as raw text, per the GLEP's own
  "unknown keys should be ignored" rule, but never consulted).
- Verified in the vendored real source that the effect of this is uneven:
  - `eapis-deprecated`/`eapis-banned`/`eapis-testing` and their `profile-`
    counterparts are **dead code in real `emerge` itself** — grepping all of
    `3rdparty/portage/` finds `RepoConfig.eapi_is_banned()`/
    `.eapi_is_deprecated()` defined in `repository/config.py:570-574` but
    called from nowhere in the vendored tree (they're consumed only by
    `repoman`/`pkgcheck`, which aren't vendored and aren't in portuale's
    scope). **Not a real backlog candidate.**
  - `cache-formats` (`config.py:578,1564-1577`) genuinely affects real's
    cache-format selection (`md5-dict` vs legacy `pms`) — portuale's
    `has_usable_md5_cache` (now `lib.rs:1840`) instead auto-detected by probing
    for a `metadata/md5-cache` directory. This matched real's own
    implementation-defined default and the near-universal state of the
    Gentoo repo (every mainstream repo has used `md5-dict` since ~2012), so
    practical risk was low, but an explicit `cache-formats = pms` repo
    (forcing legacy cache) would have silently diverged. **Closed
    2026-09-15 by #55:** the key is resolved on `RepoConfig` (lowercase +
    split; empty -> auto-detect `md5-dict` then `pms`), the read path takes
    the first **known** format and skips the md5-cache rung when it is not
    `md5-dict` (or `FEATURES=metadata-transfer`), and `--regen` refuses to
    write `metadata/md5-cache` for a `pms`-only repo. The one narrowing is
    the absent `pms` reader: a `pms`-first repo falls back to the
    depcachedir/depend phase, which yields the same metadata a valid `pms`
    cache would (slower, not different).
  - `thin-manifests` (defaulting `false` in the GLEP, but `true` in the
    Gentoo repo and virtually every modern repo) governs whether per-package
    Manifests list `EBUILD`/`AUX`/`MISC` entries. portuale's
    `parse_manifest` (`rust/portage-fetch/src/lib.rs:184`) only ever reads
    `DIST` lines and ignores everything else — this happens to match
    `thin-manifests = true` behavior unconditionally, which is correct for
    every repo layout portuale is validated against, so **not a gap** in
    practice.
  - `manifest-hashes`/`manifest-required-hashes`/`use-manifests`/
    `sign-manifests`/`update-changelog`/`properties-allowed`/
    `restrict-allowed`/`sign-commits` are all either purely
    tree-authoring/dev-tooling concerns (repoman/pkgdev territory) or,
    per GLEP 74's own text, part of the full-tree-verification path that
    real delegates to `gemato` during `--sync` (see GLEP 74 below) — outside
    portuale's `--sync`-is-a-non-goal boundary.

## GLEP 74 — Full-tree verification using Manifest files

Specifies whole-repository integrity/authenticity verification: a signed
top-level `Manifest` referencing nested sub-Manifests via `MANIFEST` entries,
`IGNORE` entries, `TIMESTAMP` freshness checks, and a defined checksum/
compression algorithm table.

**Status: N/A — inside the `--sync` non-goal boundary**, confirmed against
real source rather than assumed.

- Verified in `3rdparty/portage/lib/portage/sync/modules/rsync/rsync.py:32-
  36,154,402-421`: real's own full-tree verification is implemented by
  shelling out to the external `gemato` Python library
  (`gemato.recursiveloader.ManifestRecursiveLoader`) from inside the rsync
  sync module, gated by `sync-rsync-verify-metamanifest`. `emerge` itself
  contains no independent implementation — real *also* just delegates this
  to a separate tool. Since portuale doesn't implement `emerge --sync` at
  all (users run real `emerge --sync` / a VCS pull for tree updates before
  invoking portuale), this GLEP's core mechanism sits entirely outside
  portuale's scope, matching the existing product boundary decision.
- The one place this GLEP's *data format* (not its full-tree algorithm)
  matters to portuale is per-package `DIST` verification during fetch,
  which is implemented (`parse_manifest`, thin-manifest-shaped, `DIST`-only)
  and gpkg's own internal Manifest (GLEP 78, above) — both covered
  separately.

## GLEP 79 — Gentoo OpenPGP Authority Keys

Specifies Infrastructure-side key-signing automation (L1/L2 Authority Keys)
for validating Gentoo developer OpenPGP identities via LDAP. Entirely a
social/infrastructure process — no package-manager-executable behavior is
specified (the GLEP's own text: "exact details regarding creating and
verifying signatures... are outside the scope").

**Status: N/A.** No action for portuale; its gpkg/Manifest signature
verification (GLEP 78) already just calls the user's configured `gpg`
keyring, which is the correct integration point — Authority Keys are how
*that keyring* gets populated with trustworthy Gentoo developer keys, not
something the package manager itself does.

## GLEP 81 — User and group management via dedicated packages

Specifies `acct-user`/`acct-group` category packages that create system
users/groups via `pkg_preinst`, using ordinary `DEPEND`/`RDEPEND` for
tracking. Explicitly states the proposal "avoid[s]... any changes in the
package manager."

**Status: N/A — confirmed by design, not by omission.** Grepped for
`acct-user`/`acct-group`/`acct_user`/`acct_group` across `rust/`: zero hits,
which is *correct* per the GLEP's own text. The mechanism relies entirely on
generic package-manager primitives portuale already has: ordinary dependency
resolution (pulling in the acct-* package), ordinary reverse-dependency
depclean pruning when no installed package depends on it anymore, and
ordinary `pkg_preinst` phase execution (the `useradd`/`groupadd` calls live
in the ebuild's `acct-user.eclass`/`acct-group.eclass` inherit, which
portuale executes the same as any other phase script). No acct-specific
special-casing is needed or would be correct to add.

## GLEP 53 — Keywording scheme

Defines the `arch[-os]` two-field KEYWORDS token syntax (e.g. `amd64`,
`sparc-fbsd`), explicitly as a *backwards-compatible clarification* of
existing string-based keyword usage, not a new enforced structure.

**Status: Full.** portuale's keyword matching (`keywords_accepted`,
`rust/portage-repo/src/lib.rs:4975`, with `~`-prefix/`-*`/`*`/`~*` semantics
tested at `lib.rs:32843-33067`) treats keywords as opaque strings exactly as
real Portage does — real likewise never parses out the arch/os halves for
matching purposes (confirmed: no `arch.*os` split logic found in the
vendored keyword-matching code either). Since the GLEP mandates the *syntax*
be transparent to string equality (that's the whole backwards-compatibility
point of the GLEP), portuale's plain-string approach is exactly compliant,
not merely coincidentally working.

## GLEP 59 — Manifest2 hash policies and security implications

A 2008-era security analysis recommending retirement of MD5/SHA1/RIPEMD160
in favor of SHA512 and WHIRLPOOL as they became available, and generally
"verify the strongest available hash first."

**Status: Full — matches real's current (already-migrated) state.** Real
Portage today ships `MANIFEST2_HASH_DEFAULTS = frozenset(("BLAKE2B",
"SHA512"))` — WHIRLPOOL was itself later superseded by BLAKE2B in real's own
evolution, which this GLEP's "add stronger hashes as available" principle
anticipates. portuale implements exactly BLAKE2B + SHA512
(`rust/portage-fetch/src/lib.rs:502-503`, `rust/portuale/src/binpkg.rs:1594,
1601`) — the deprecated MD5/SHA1/RIPEMD160 hashes this GLEP calls to retire
are correctly absent.

## GLEP 61 — Manifest2 compression

Specifies transparent gzip/bzip2/xz/lzma compressed-Manifest fallback for
*large, tree-wide* Manifests (empirically, per-package Manifests essentially
never hit the 32KiB suggested cutoff; this targets the top-level
MetaManifest).

**Status: N/A — same `--sync` boundary as GLEP 74.** Compressed Manifest
handling is meaningless without the full-tree verification GLEP 74
specifies, and that's real's `gemato`-in-`--sync` territory, not `emerge`'s.
portuale's per-package `parse_manifest` reads small, always-uncompressed
per-package `DIST`-only Manifests, which were never in scope for this GLEP's
32KiB cutoff to begin with.

## GLEP 64 — Export PMS's cached VDB information

Specifies that the PM cache, per installed package: evaluated build-time
metadata (PMS 13.2 cache vars), a full file listing with type/checksum/
mtime, ELF/Mach-O/COFF linking info (NEEDED, SONAME, RPATH), build flags
(CHOST/CFLAGS/etc.), USE/KEYWORDS, DEPEND/RDEPEND/PDEPEND, and misc.
(build time, repository, DEFINED_PHASES, EAPI, INHERITED, SLOT) — all in a
common, externally-readable layout (i.e. `/var/db/pkg/<cat>/<pf>`,
consumable by non-Portage tools).

**Status: Full.** This is portuale's core design constraint, not an add-on:
it writes the *same* `/var/db/pkg` layout real does, byte-for-byte
compatible per the `emerge --info` and vdb-`environment` parity work already
verified in this project (memory `emerge-info-config-layer-complete`,
`getbinpkg-vdb-entry-fix`). Specifically checked for this review:
`NEEDED.ELF.2` generation and consumption is fully ported
(`rust/portuale/src/needed_elf.rs`, `rust/portuale/src/ebuild_merge.rs:1047-
1401`, driven by a real, unmodified `scanelf` invocation exactly as real
does — see the end-to-end test at `ebuild_merge.rs:6472`), and `CONTENTS`
(file listing with type/checksum/mtime) is written by the same merge path.
No gap found; this GLEP essentially describes what portuale already had to
build to be a drop-in `emerge` replacement.

## GLEP 68 — Package and category metadata (metadata.xml)

Specifies the XML schema for per-category/per-package `metadata.xml`
(maintainers, long descriptions, USE flag docs, upstream info, slot docs).

**Status: N/A — confirmed by checking real, not assumed.** Grepped
`3rdparty/portage/` for any `metadata.xml` consumption in the `emerge`/
resolver/build code paths: none found. `metadata.xml` is exclusively
consumed by QA/presentation tooling (`repoman`/`pkgcheck`/soko/
packages.gentoo.org), never by `emerge` itself for dependency resolution,
masking, or merging. portuale correctly has no `metadata.xml` parser either.

## GLEP 83 — EAPI deprecation

An *informational* GLEP: Council timing criteria (24/48-month windows, 5%
usage threshold) for when to deprecate/ban an EAPI. Not itself a technical
format spec.

**Status: N/A**, and verified the one place it could have mattered doesn't:
real's `_eapi_is_deprecated()` (`3rdparty/portage/lib/portage/__init__.py:
364-388`) — which *does* feed into per-package masking via
`getmaskingstatus.py:91` — is driven by `_deprecated_eapis`, a hardcoded set
of **pre-release EAPI identifiers only** (`"3_pre1"`, `"7_pre1"`, etc.), not
stable EAPI numbers like `"5"` or `"6"`. No real ebuild in a normal tree
uses a pre-release EAPI string, so this masking branch is realistically dead
in both real and portuale's differential test beds. Separately, the
per-repo `layout.conf` `eapis-deprecated`/`eapis-banned` keys this GLEP's
mechanism could *also* have been routed through are themselves dead code in
`emerge` (see GLEP 82 above) — confirmed by the same grep. No action
warranted.

## GLEP 84 — Standard format for package.mask files

Specifies a *comment/documentation* convention for `profiles/package.mask`
entries (author line, explanation paragraphs, last-rite epilogue, bug-list
syntax) layered on top of the existing PMS 4.4/5.2.8 raw-entry format. The
GLEP is explicit that it "does not break the raw entries format specified in
PMS" and is opt-in via a header comment tools can detect.

**Status: N/A.** This is a tooling convention for humans and mask-authoring
tools (`pkgdev mask`, `soko`), not something the package manager parses
differently — `package.mask` entries are, and remain, plain PMS atom lists
with `#`-prefixed comment lines the PM already skips. portuale's
`package.mask` handling doesn't need special-casing for this GLEP's comment
structure, and adding any would be incorrect (the PM must never treat
comment content as meaningful).

---

## Prioritized backlog candidates

Two genuine, narrow gaps survived cross-referencing against what real
`emerge` itself actually consults (as opposed to `repoman`/`pkgcheck`/
`--sync`/gemato territory, all out of scope). Both are now closed:

1. **`cache-formats` layout.conf key (GLEP 82) — DONE 2026-09-15 (#55).**
   `RepoConfig::cache_formats` resolves the key exactly like real
   (`parse_layout`), the read path honours the first known format plus
   `FEATURES=metadata-transfer` (`porttree.py:322`), and `--regen` follows
   `egencache`'s writer targets for the `md5-dict` half. A `pms`-first repo
   is a documented narrowing: no `pms` reader, so it takes the
   depcachedir/depend fallback (same metadata, no pregen speedup). Evidence:
   `TEST/findings/l2.md` "## #55 S0"; `docs/backlog-tasks.md` #55.

2. **Outer gpkg container tar entry-type check (GLEP 78) — DONE
   2026-09-15 (#56).** The three outer-container sites now require a real
   prefix directory and regular files at every trusted name, via
   `outer_regular_member`/`outer_prefix_dir` and the shared
   `walk_outer_members` (`read_gpkg_metadata`, `verify_gpkg_manifest`,
   `extract_gpkg_member`); non-regular members are errors naming the
   member and its type. The audit's "real's enforcement wasn't
   independently re-verified" caveat was resolved by #56 S0's
   crafted-archive matrix: real's in-archive `tarfile.extractfile` never
   leaves the container (`KeyError` for a symlink target, size/structure
   failures for every special member), while the pre-fix portuale followed
   a symlinked prefix to the host `/etc`, read a host file through a
   symlinked `Manifest`, and blocked on a FIFO member — so this closed a
   real divergence, not just theoretical hardening. Evidence:
   `TEST/findings/l2.md` "## #56 S0"; `scripts/gpkg_crafted_members.py`;
   `docs/backlog-tasks.md` #56 (whose close-out also files the inner
   `metadata.tar` member-type residue as #58).

Everything else — GLEP 74/59/61's full-tree/hash/compression machinery
(delegated to `gemato` inside real's own `--sync`, itself a portuale
non-goal), GLEP 79/81/83's process/convention-only specs, GLEP 68's
metadata.xml (unused by `emerge` itself), and GLEP 84's mask-file prose
formatting — needs no portuale change and should stay a deliberate
non-goal, not a backlog item. The `eapis-*` layout.conf keys inside GLEP 82
fall in this same "dead in real emerge" bucket despite being under a GLEP
that's otherwise partially actionable.
