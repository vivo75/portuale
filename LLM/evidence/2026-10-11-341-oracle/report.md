# Oracle report for #341: Portage 3.0.82.2's EAPI-aware atom grammar

## What these citations point to

- Paths are relative to `3rdparty/portage/lib/` (shortened to `L/`) and `3rdparty/portage/src/`. They mean the files under `/home/vivo/repo/PORTUALE/portuale/3rdparty/portage/`.
- **The vendored tree matches what is installed.**
  - `diff -r` against `/usr/lib/python3.14/site-packages/{portage,_emerge}` differs only in the `@VERSION@` / `@EPREFIX@` / path substitutions in `__init__.py`/`const.py`, the meson files, and two unrelated `_emerge` files (`AbstractEbuildProcess.py`, `create_depgraph_params.py`).
  - The installed tree also has the compiled `portage/dep/_parser.cpython-314-x86_64-linux-gnu.so`.
- **Both places run 3.0.82.2 with the native parser.** On the host and in the container (`localhost/test-portuale:latest`): `portage.VERSION == "3.0.82.2"`, and `from portage.dep import _c_dep_parser` returns `<module 'portage.dep._parser' from '.../_parser.cpython-314-x86_64-linux-gnu.so'>`.
- **The C parser is a fork addition, not upstream Portage.** Its source is `src/dep_parser_core.c` and `src/dep_parser.c`. Setting `PORTAGE_NATIVE_DEP_PARSER=0` turns it off (`L/portage/dep/__init__.py:63-65`).
- **Supported EAPIs:** `portage._supported_eapis` = {0..9, 9-pre1, plus deprecated `*_pre*` strings} (`L/portage/__init__.py:363-393`, `const.EAPI = 9` at `L/portage/const.py:261`). **EAPI 9 is supported.**

All probe scripts and raw transcripts are in this directory: `probe_atoms.py`/`c.txt`/`py.txt`, `probe2.py`, `probe3.py`, `probe_match.py`, `fuzz.py`, `run1..6.sh` and `live1..6.txt`, plus `rs_pairs.txt` (portuale harness output).

---

## The rule, in brief

1. **The grammar is EAPI-independent; EAPI only rejects features afterwards.**
   - `Atom()` matches every string against one grammar with no EAPI input. `_get_atom_re`, `_get_usedep_re` and `_get_atom_wildcard_re` ignore `eapi_attrs` and cache globally.
   - The only EAPI-dependent regex is the slot regex. It switches on `slot_operator`.
   - After parsing, Portage rejects a feature only when **`eapi` is not None** and the EAPI lacks it.
   - Build-id and `::repo` are separate gates controlled by the caller's flags:
     - **`eapi=None`**: both default to *allowed*.
     - **`eapi` given**: `allow_repo` defaults to `repo_deps`, which is **False for every supported EAPI**. `allow_build_id` stays falsy unless the caller passes it explicitly.
2. **Ebuild and binary dependencies are strict.** They are parsed with `eapi=pkg.eapi` and `is_valid_flag=iuse.is_valid_flag`. Any `InvalidAtom` masks the package with `invalid: <KEY>: Invalid atom (<atom>), token N`.
   - **Installed packages are a special case.** Their deps are validated with `eapi=None` and no flag check, which goes through the C fast path. **That path rejects `::repo` and build-id atoms even though the Python path would accept them.**
3. **Config files are lenient and never abort.** Each reader passes its own flags. A bad line is dropped with `--- Invalid atom in <file>: <msg>`.
   - User files: wildcards, repo and build-id allowed; no EAPI.
   - Repo profiles: profile EAPI applies; `::repo` only with `profile-repo-deps`; build-id only with `build-id`; no wildcards.
   - **A profile directory without an `eapi` file defaults to EAPI "0".** That rejects `:slot`, `[use]` and `:=`.

---

## Q1. The atom grammar per EAPI

### 1.1 Where the grammar lives (source)

**Regex fragments** (`L/portage/versions.py:38-54`):

| Fragment | Regex | Line |
|---|---|---|
| `_slot` | `([\w][\w+.-]*)` | :38 |
| `_cat` | `[\w][\w+.-]*` | :42 |
| `_pkg` | `[\w][\w+-]*?` | :47 |
| `_v` | `(\d+)((\.\d+)*)([a-z]?)((_(pre\|p\|beta\|alpha\|rc)\d*)*)` | :49 |
| `_vr` | `_v(-r(\d+))?` | :51 |
| `_cp` | `(cat/pkg(-vr)?)` | :53 |
| `_cpv` | `(_cp-_vr)` | :54 |

- `_cp` deliberately captures an optional "ambiguous" `-vr` tail. That tail is what carries a build-id.
- All regexes are compiled with `re.ASCII`, so `\w` and `\d` match ASCII only.

**Other fragments in `L/portage/dep/__init__.py`:**
- `_slot_loose = ([\w+./*=-]+)` (:243)
- `_use = \[.*\]` (:245)
- `_op = ([=~]|[><]=?)` (:246)
- `_repo_name = [\w][\w-]*` (:249)
- `_extended_cat = [\w+*][\w+.*-]*` (:252)

**Main and helper regexes:**
- **`_get_atom_re`** (:293-326): `(op cpv | =cpv\* | cp)(:slot_loose)?(::repo)?(\[.*\])?`. It is a module global and **ignores `eapi_attrs`** (:294-296).
- **`_get_slot_dep_re`** (:257-278):
  - with `slot_operator`: `((main)(/sub)?)?([*=])?`
  - without: plain `_slot`
- **`_get_usedep_re`** (:378-395): `^[!-]?FLAG((+)|(-))?[?=]?$`, where FLAG is `[A-Za-z0-9][A-Za-z0-9+_@-]*`. A global that ignores EAPI.
- **`_get_atom_wildcard_re`** (:332-372):
  - `simple`: `_extended_cat/[\w+*][\w+*-]*?(-vr)?`
  - `star`: `=cat/pkg-\*\w+\*`
  - Optional `:slot` and `::repo`. **No use-dep group.**

**`Atom.__init__`** (:1900-2153):

1. **Flag defaults** (:1926-1937):
   - `eapi` given: `allow_repo = eapi_attrs.repo_deps` if None. **`allow_build_id` is not defaulted** (stays None, i.e. falsy).
   - `eapi=None`: `allow_repo` and `allow_build_id` default to True.
2. **C fast path** (:1943-1957). Taken when `_c_dep_parser` exists, `_use is None`, `orig_atom is None`, and (`eapi is None` or `slot_operator`).
3. **Blocker prefix** `!`/`!!` (:1959-1967). It is stripped before every grammar, including the wildcard one.
4. **Strict regex first; wildcard regex only if `allow_wildcard`** (:1972-1997).
   - On the wildcard path, `**` is rejected (:1990).
   - A `simple` form with a version tail is rejected (:1988).
5. **`op` arm** (:1998-2022). If the `_cp` ambiguous tail matched, the result is a build-id when `allow_build_id`. Otherwise `InvalidAtom`. A build-id with a leading zero is rejected (:2014-2016).
6. **`star` (`=*`) arm** (:2023-2033). An ambiguous tail always gives `InvalidAtom`, so `=*` can never carry a build-id.
7. **`simple` arm** (:2034-2042). An ambiguous tail gives `InvalidAtom` (`dev-libs/foo-1`).
8. **Slot** (:2055-2076). Parsed with `_get_slot_dep_re`. `:slot*` is rejected (:2068). An empty match is rejected (:2071).
9. **Repo gate** (:2081-2082).
10. **Use deps** (:2084-2090) go to `_use_dep(...)`.
11. **EAPI gates, applied only if `eapi is not None`** (:2101-2153):

    | Check | Line | Error message |
    |---|---|---|
    | `slot_deps` | :2106 | `Slot deps are not allowed in EAPI %s: '%s'` |
    | `use_deps` | :2112 | `Use deps are not allowed…` |
    | `use_dep_defaults` | :2117 | `Use dep defaults are not allowed…` |
    | conditional flags vs `is_valid_flag` | :2125-2144 | `USE flag '%s' referenced in conditional '%s' in atom '%s' is not in IUSE` (category `IUSE.missing`) |
    | `strong_blocks` | :2145-2153 | `Strong blocks are not allowed…` |

    The first four have category `EAPI.incompatible`, as does `strong_blocks`.

**`_use_dep.__init__`** (:1279-1360):
- A token must match `_get_usedep_re`, or the error is `Invalid use dep: '<tok>'` (:1324).
- Accepted operators: `""`, `-`, `?`, `=`, `!=`, `!?`. Anything else, e.g. `-x?`, `-x=` or `!x`, gives `Invalid use dep` (:1330-1343).
- **Default consistency** (:1345-1357), per flag:
  - `(+)` after `(-)` or after a no-default occurrence is rejected.
  - `(-)` after `(+)` or after a no-default occurrence is rejected.
  - A no-default occurrence after either default is rejected.
  - **There is no check that the same flag appears both enabled and disabled.**

**`_c_fast_init`** (:1845-1898) and `_c_fill_atom_fields` (:71-102):
- The fast path performs the same EAPI checks, with the same messages.
- `_use_dep(list(tokens), eapi_attrs)` re-validates use tokens with the Python regex.
- The C scanner, `scan_atom` (`src/dep_parser_core.c:263-426`), wrapped by `py_scan_atom` (`src/dep_parser.c:676-690`), raises `ValueError` unless it consumes the whole string. Repo specs, build-ids and wildcards always fail it, so those fall back to the regex (`dep_parser.c:671-672`, `__init__.py:1939-1952`).

### 1.2 Feature × EAPI table

`eapi_attrs` come from `L/portage/eapi.py:238-329`.
- `eapi=None` and unsupported EAPI strings take the permissive branch (:247-287).
- Supported EAPIs take the computed branch (:288-329).
- Only these attrs are consulted by atom parsing: `slot_operator`, `slot_deps`, `use_deps`, `use_dep_defaults`, `strong_blocks`, and `repo_deps` (the last only to default `allow_repo` when `eapi` is given).

| Feature | eapi=None | EAPI 5 | EAPI 7 | EAPI 8 | EAPI 9 | unsupported str (e.g. "10") | Gate (file:line) |
|---|---|---|---|---|---|---|---|
| `!` weak blocker | yes | yes | yes | yes | yes | yes | grammar only (:1959) |
| `!!` strong blocker | yes | yes (≥2) | yes | yes | yes | yes | `strong_blocks` eapi.py:324, dep:2145 |
| `!!!x` | no | no | no | no | no | no | grammar |
| `= ~ < <= > >=` | yes | yes | yes | yes | yes | yes | `_op` :246 |
| `=cat/pkg-1*` (`=*`) | yes | yes | yes | yes | yes | yes | **not EAPI-gated** (:308, :2023) |
| `>=…*` / `~…*` | no | no | no | no | no | no | grammar |
| `:slot` | yes | yes (≥1) | yes | yes | yes | yes | `slot_deps` eapi.py:318, dep:2106 |
| `:slot/sub` | yes | yes (≥5) | yes | yes | yes | yes | `slot_operator` eapi.py:319, `_get_slot_dep_re` |
| `:=` `:*` `:slot=` `:s/ss=` | yes | yes | yes | yes | yes | yes | `slot_operator`; `:slot*` always no (:2068) |
| `[use]` (6 forms) | yes | yes (≥2) | yes | yes | yes | yes | `use_deps` eapi.py:327 |
| `(+)`/`(-)` defaults | yes | yes (≥4) | yes | yes | yes | yes | `use_dep_defaults` eapi.py:328 |
| `::repo` (default `allow_repo`) | **yes** | **no** | **no** | **no** | **no** | **yes** | `repo_deps=False` eapi.py:314; True :272; dep:1929, :2081 |
| build-id `=c/p-1.0-2` (default `allow_build_id`) | **yes** | **no** | **no** | **no** | **no** | **no** | dep:1936-1937 vs :2009-2022 |
| extended/wildcard | only with `allow_wildcard=True` | same | same | same | same | same | caller flag only (:1973) |
| conditional-flag IUSE check | **skipped** | yes if `is_valid_flag` | yes | yes | yes | yes | only inside `if eapi is not None` (:2101, :2125; C :1860, :1887) |

**Answers to the two direct questions:**
- **`repo_deps` is False for every supported EAPI.** The computed branch hard-codes `repo_deps=False` (eapi.py:314). So `::repo` inside an ebuild's `*DEPEND` is invalid in EAPI 0-9 (LIVE below).
- **When `eapi` is given, `allow_build_id` is effectively False unless the caller passes True.** It is defaulted only in the `eapi is None` branch (dep:1930-1937).
- **An unsupported EAPI string behaves like `eapi=None` for `repo_deps`**, so `::repo` is allowed. Build-id is still rejected, because `eapi` is not None. LIVE row `eX`.
- **EAPI 9 has no atom-grammar change versus 8.** Its new attrs, `profile_eapi_default` and `use_stable` (eapi.py:312, :323), are not consulted by `Atom`.

### 1.3 LIVE probe battery

- **Command:** `python3 probe_atoms.py`, once with the default (C) parser and once with `PORTAGE_NATIVE_DEP_PARSER=0`.
- **Result:** `diff` of the two outputs says **IDENTICAL** (83 strings × 9 flag modes).
- **Note (2026-10-24, backlog #528):** `probe_atoms.py` was later extended to
  86 strings (the three #510 probe atoms appended at the end); this run
  covered the original 83, so its result line is left as recorded.
- **Modes:**
  - `None` = `Atom(s)`
  - `e5/e7/e8/e9` = `Atom(s, eapi="N")`
  - `eX` = `eapi="10"`
  - `e8+bid` = `eapi="8", allow_build_id=True`
  - `None-repo` = `allow_repo=False, allow_build_id=False`
  - `wild` = `allow_wildcard=True, allow_repo=True`
- **Message format:** `ERR[...]` is `str(InvalidAtom)`. A bare atom string means the generic `InvalidAtom(self._string)`.

Rows trimmed to the discriminating ones (full output: `c.txt`):

```
'dev-libs/foo::gentoo'   None=OK(repo=gentoo) | e5..e9=ERR[dev-libs/foo::gentoo] | eX=OK(repo=gentoo) | e8+bid=ERR | None-repo=ERR | wild=OK
'=dev-libs/foo-1.0-2'    None=OK(bid=2,op==) | e5..e9=ERR | eX=ERR | e8+bid=OK(bid=2,op==) | None-repo=ERR | wild=OK(bid=2)
'=dev-libs/foo-1.0-02'   ERR in every mode (leading zero, :2014)
'=dev-libs/foo-1.0-r1-2' None=OK(bid=2) | e*=ERR | e8+bid=OK
'>=dev-libs/foo-1.0-2'   None=OK(bid=2,op=>=) | e*=ERR | e8+bid=OK          # build-id accepted with any operator
'*/foo' 'dev-libs/*' '*/*'   ERR everywhere except wild=OK(ext)
'=dev-libs/foo-1*'       OK in every mode (op==*)
'>=dev-libs/foo-1*'      ERR in every mode
'=dev-libs/foo-*1*'      ERR except wild=OK(ext,op==*)
'dev-libs/foo:'          ERR everywhere
'dev-libs/foo:='  ':0/1='  ':0/1'  ':*'  ':0='   OK everywhere
'dev-libs/foo:0*' ':/1' ':.a' ':-a' ':+a'        ERR everywhere
'dev-libs/foo:_a'  ':a+'                          OK everywhere
'dev-libs/foo[bar(+)]' '[bar(-)?]' '[!bar?]' '[!bar=]' '[bar=]' '[-bar]' '[bar@x]'   OK everywhere
'dev-libs/foo[-bar?]'    ERR[Invalid use dep: '-bar?']
'dev-libs/foo[!bar]'     ERR[Invalid use dep: '!bar']
'dev-libs/foo[bar,bar(+)]'     ERR[Invalid use dep: 'bar(+)']
'dev-libs/foo[bar(+),bar(-)]'  ERR[Invalid use dep: 'bar(-)']
'dev-libs/foo[bar(+),-bar(+)]' OK everywhere   # enabled+disabled same flag is NOT rejected
'dev-libs/foo[_bar]'     ERR[Invalid use dep: '_bar']
'dev-libs/foo[]' '[bar,]'  ERR[Invalid use dep: '']
'dev-libs/foo[bar][baz]' ERR[Invalid use dep: 'bar][baz']
'dev-libs/foo:0[bar]::gentoo'  ERR everywhere (repo must precede [use])
'dev-libs/foo:0::gentoo[bar]'  None/eX/wild=OK(repo) ; e5..e9=ERR
'!!dev-libs/foo' '!!>=dev-libs/foo-1'  OK everywhere ; '!!!dev-libs/foo' ERR
'dev-libs/foo-1' 'dev-libs/foo-1-r1' 'dev-libs/foo-bar-1x' '>=dev-libs/foo'  ERR everywhere
'=dev-libs/foo-1.0_alpha1_beta2-r3' '=dev-libs/foo-01.002' '=dev-libs/foo-1.0a' '=dev-libs/foo-1.0_p' '=dev-libs/foo-1.0-r01'  OK
'=dev-libs/foo-1.0_alpha1_beta2-r3_p1' '=…-1.0ab' '=…-1.0-r'  ERR
'dev-libs/Foo+bar' 'cat/pkg-' 'cat/pkg--' 'cat./pkg' '_cat/_pkg' 'cat/81-libretro'  OK everywhere
'dev-libs/+foo' '+dev/foo'  ERR, but wild=OK(ext)   # wildcard grammar allows a leading '+'
'-cat/pkg' 'cat/-pkg' '.cat/pkg' 'cat/pkg.x' 'cat/pkg-1a'  ERR everywhere
'dev-libs/foo::-foo' '::'  ERR everywhere ; '::foo_bar' '::foo-bar' '::_x'  None/eX/wild=OK, e*=ERR
```

**Extra probe** (`probe2.py`, C and PY output identical):

```
OK  '*/foo::gentoo'  '*/*:0'  'dev-*/foo'  '*-libs/f*o'  '!*/foo'  '*/*::gentoo'      (wild)
OK  '=*/foo-*1*' -> cp=*/foo ver=*1* op==*                                           (wild)
ERR '=dev-libs/fo*-*1.0*'  '=dev-libs/foo-1.0-2*'  'dev-libs/foo-1.0'  '*/foo-1.0'  '*/foo[bar]'  'dev-libs/foo**'  '**/foo'  '*/foo:0*'   (wild)
OK  '=dev-libs/foo-1.0-2:0::r[bar]' -> bid=2 repo=r use=[bar]                         (wild)
OK  '=dev-libs/foo-1.0-0' -> bid=0 ; '=dev-libs/foo-1.0-2147483648999' -> bid=2147483648999 (no bound)
ERR 'dev-libs/foo[b?]' {'eapi':'8', is_valid_flag} -> USE flag 'b' referenced in conditional 'b?' in atom 'dev-libs/foo[b?]' is not in IUSE category=IUSE.missing
OK  'dev-libs/foo[b?]' {is_valid_flag, eapi=None}        # IUSE check skipped without eapi
ERR '!!dev-libs/foo' eapi=1 -> Strong blocks are not allowed in EAPI 1: '!!dev-libs/foo' category=EAPI.incompatible
ERR 'dev-libs/foo:0' eapi=0 -> Slot deps are not allowed in EAPI 0: 'dev-libs/foo:0'
ERR 'dev-libs/foo:0=' eapi=4 / 'dev-libs/foo:0/1' eapi=4 -> bare InvalidAtom (slot regex w/o slot_operator)
ERR 'dev-libs/foo[a(+)]' eapi=3 -> Use dep defaults are not allowed in EAPI 3
ERR 'dev-libs/foo[a]' eapi=1 -> Use deps are not allowed in EAPI 1
OK  'dev-libs/foo:0/1' eapi='9-pre1'
Atom('dev-libs/foo', eapi=8)  -> AttributeError: 'int' object has no attribute 'partition'  (eapi must be str)
```

### 1.4 C fast path versus regex path

**`Atom()` itself: no observable difference.**
- The 83-string battery above is identical.
- A random fuzz (`fuzz.py`, 2 seeds × 60,000 random strings × 3 modes, comparing every parsed field) found **0** differences:
  ```
  seed 7 cases 60000 accepted-in-some-mode 188 C-vs-PY differences 0
  seed 11 cases 60000 accepted-in-some-mode 214 C-vs-PY differences 0
  ```

**`use_reduce()` does differ.** The C path for whole dependency strings, `_c_fast_use_reduce` (`L/portage/dep/__init__.py:228-234`, gate at :1132-1150), is used when all of these hold:
- `token_class is Atom`
- not `is_src_uri`
- not `opconvert`
- **`is_valid_flag is None`**
- no subset, matchnone, masklist or excludeall
- `eapi is None` or `slot_operator`

In that path, the C dependency-list scanner (`scan_item`, `src/dep_parser_core.c:503-619`) **never falls back** to the Python regex. Any atom `scan_atom` cannot consume becomes `ValueError`, then `InvalidDependString(str(e))` (:1149-1150), with a C message and no `errors=` attached. Consequences:

- **`::repo` and build-id atoms are rejected even with `eapi=None`.** The Python path accepts them.
- **The error text differs:** `expected '?'`, `expected atom or group`, `unexpected token`, `invalid use dep token: 'bar'`, instead of `Invalid atom (X), token N`.

LIVE (`probe3.py`, `use_reduce(s, matchall=True, token_class=Atom, flat=True, **kw)`):

```
== C (default)
ERR 'dev-libs/foo::gentoo' {} -> expected '?' | errors: []
ERR 'dev-libs/foo::gentoo' {'eapi': '8', 'is_valid_flag': 'fn'} -> Invalid atom (dev-libs/foo::gentoo), token 1 | errors: [('dev-libs/foo::gentoo', None)]
ERR '=dev-libs/foo-1.0-2' {} -> expected atom or group | errors: []
ERR 'dev-libs/foo:0::gentoo[bar]' {} -> unexpected token | errors: []
ERR 'dev-libs/foo[bar(+),bar]' {} -> invalid use dep token: 'bar' | errors: []
== PY (PORTAGE_NATIVE_DEP_PARSER=0)
OK  'dev-libs/foo::gentoo' {} [('dev-libs/foo::gentoo', 'gentoo', None)]
OK  '=dev-libs/foo-1.0-2' {} [('=dev-libs/foo-1.0-2', None, 2)]
OK  'dev-libs/foo:0::gentoo[bar]' {} [('dev-libs/foo:0::gentoo[bar]', 'gentoo', None)]
ERR 'dev-libs/foo[bar(+),bar]' {} -> Invalid atom (Invalid use dep: 'bar'), token 1
```

The only production caller this affects is installed-package validation (Q2c).

---

## Q2. Every context where Portage builds an Atom from text

The flags below are exactly what each caller passes. Where the eapi is not passed it is None, so `allow_repo` and `allow_build_id` default to True unless passed (dep:1930-1937).

### (a) Ebuild metadata deps (`*DEPEND`, `PDEPEND`, `IDEPEND`)

**Flags** (`L/_emerge/Package.py:312-346`):
- `use_reduce(v, eapi=pkg.eapi, matchall=True, is_valid_flag=self.iuse.is_valid_flag, token_class=Atom, flat=True)`.
- `use_reduce` constructs `Atom(token, eapi=eapi, is_valid_flag=is_valid_flag)` (dep:1029-1031). So `allow_repo` is `repo_deps` (False), `allow_build_id` is None (false), and there are no wildcards.
- `is_valid_flag` is not None, so this is **always the regex path** (gate dep:1139).
- For non-built packages, a `:slot/sub=` "built" slot operator is also rejected (Package.py:347-356): `Improper context for slot-operator "built" atom syntax: …`.

**On `InvalidAtom`:**
1. `use_reduce` raises `InvalidDependString("Invalid atom (%s), token %s", errors=(e,))` (dep:1032-1036).
2. `Package._metadata_exception` (:534-557) records it:
   - If the error has a category (`EAPI.incompatible`, `IUSE.missing`), it is stored under that category with the text `<KEY>: <InvalidAtom msg>`.
   - Otherwise it goes under `dependency.syntax` with `<KEY>: Invalid atom (<atom>), token N`.
3. `masks["invalid"]` makes the package invisible (:432-433, :488-489).
4. Only the first bad token per key is reported, because `use_reduce` raises at the first one.
5. The display is `- cpv::repo (masked by: invalid: <msg>)` (`L/_emerge/depgraph.py:12560-12563`).

LIVE (`run1.sh`, container, overlay `oracle`, EAPI 8 ebuilds). Each command was `emerge -pv --ignore-default-opts <atom>`:

```
=== ARGV: emerge -pv --ignore-default-opts app-misc/badrepo        # RDEPEND="dev-libs/foo::gentoo"
!!! All ebuilds that could satisfy "app-misc/badrepo" have been masked.
!!! One of the following masked packages is required to complete your request:
- app-misc/badrepo-1::oracle (masked by: invalid: RDEPEND: Invalid atom (dev-libs/foo::gentoo), token 1)
=== ARGV: emerge -pv --ignore-default-opts app-misc/badbid         # RDEPEND="=dev-libs/foo-1-2"
- app-misc/badbid-1::oracle (masked by: invalid: RDEPEND: Invalid atom (=dev-libs/foo-1-2), token 1)
=== ARGV: emerge -pv --ignore-default-opts app-misc/badcond        # IUSE="" RDEPEND="dev-libs/foo[x?]"
- app-misc/badcond-1::oracle (masked by: invalid: RDEPEND: USE flag 'x' referenced in conditional 'x?' in atom 'dev-libs/foo[x?]' is not in IUSE)
=== ARGV: emerge -pv --ignore-default-opts app-misc/badblock       # RDEPEND="dev-libs/foo:0* dev-libs/foo-1"
- app-misc/badblock-1::oracle (masked by: invalid: RDEPEND: Invalid atom (dev-libs/foo:0*), token 1)   # only first bad token
=== ARGV: emerge -pv --ignore-default-opts app-misc/goodslot       # RDEPEND="dev-libs/foo:= !!app-misc/badblock"
[ebuild  N     ] dev-libs/foo-1:0/1::oracle  USE="-x" 0 KiB
[ebuild  N     ] app-misc/goodslot-1::oracle  0 KiB
```

### (b) Binary package deps

- **Same code path as (a).** `built=True, installed=False` keeps `dep_eapi = eapi` and `dep_valid_flag = iuse.is_valid_flag` (Package.py:318-327).
- **The only difference:** the "Improper context for slot-operator built" check is skipped for built packages (`if not self.built`, :347).

LIVE (`run3.sh`):
- Built `app-misc/goodslot` as an xpak.
- Rewrote `RDEPEND` to `dev-libs/foo::gentoo` in both the `Packages` index and the xpak.
- Ran `pkgprobe.py`, which constructs `Package(built=True, installed=False, ...)`.

```
binary app-misc/goodslot-1 RDEPEND='dev-libs/foo::gentoo' EAPI=8
  invalid: {'dependency.syntax': ['RDEPEND: Invalid atom (dev-libs/foo::gentoo), token 1']}
  visible: False
=== ARGV: PKGDIR=/var/tmp/oraclepkgs emerge -pv --ignore-default-opts --usepkgonly=y app-misc/goodslot
- app-misc/goodslot-1::oracle (masked by: invalid: RDEPEND: Invalid atom (dev-libs/foo::gentoo), token 1)
```

### (c) Installed (vdb) package deps

**Validation flags (confirmed):** Package.py:321-327 sets `dep_eapi = None` and `dep_valid_flag = None` for installed packages. So EAPI-incompatible atoms and IUSE-missing conditionals are ignored.

**But on this host the C fast path is taken** (all of the gate's conditions hold). So:
- `::repo` and build-id atoms in a vdb `*DEPEND` **do** mark the installed package invalid, with C error text.
- The message carries the file path (:551-557).
- An invalid installed package is invisible (:488-489).
- The depgraph also drops invalid installed packages from match lists (`L/_emerge/depgraph.py:8450-8451`).

LIVE (`run3.sh`). The vdb `RDEPEND` was rewritten and the vdb cache invalidated, then `pkgprobe.py installed` was run with the default parser and with `PORTAGE_NATIVE_DEP_PARSER=0`:

```
RDEPEND=dev-libs/foo::gentoo   C : invalid: {'dependency.syntax': ["RDEPEND: expected '?' in '/var/db/pkg/app-misc/goodslot-1/RDEPEND'"]}  visible: False
                               PY: invalid: False  visible: True
RDEPEND==dev-libs/foo-1-2      C : invalid: {'dependency.syntax': ["RDEPEND: expected atom or group in '/var/db/pkg/app-misc/goodslot-1/RDEPEND'"]}  visible: False
                               PY: invalid: False  visible: True
RDEPEND=dev-libs/foo[nonexistent?]  C and PY: invalid: False  visible: True     # IUSE check skipped
RDEPEND=dev-libs/foo:0*        C : RDEPEND: unexpected token in '…/RDEPEND'
                               PY: RDEPEND: Invalid atom (dev-libs/foo:0*), token 1 in '…/RDEPEND'
```

**Separate traversal path.** When the depgraph actually walks an installed package's deps, it calls `use_reduce(..., is_valid_flag=pkg.iuse.is_valid_flag, opconvert=True, token_class=Atom, eapi=pkg.eapi)` (`depgraph.py:4352-4359`). This is the regex path, with **the real EAPI**.
- On failure for an installed package it retries without `is_valid_flag` (:4360-4376).
- A second failure adds the package to `_masked_installed` and skips its deps (:4377-4379).

### (d) Profile files (repo profiles and repo-level `profiles/`)

**Common reader behaviour.** `grabfile_package` and `grabdict_package` (`L/portage/util/__init__.py:481-607`) call `Atom(k, allow_wildcard=…, allow_repo=…, allow_build_id=…, eapi=eapi)`.
- On `InvalidAtom` they print `--- Invalid atom in %s: %s` (:527-530, :590-593) and **drop the line**.
- With `allow_use=False` (the dict readers for `package.use*` and `package.(accept_)keywords`), an atom carrying `[use]` is dropped with `--- Atom is not allowed to have USE flag(s) in %s: %s` (:532-538).
- `grabfile_package` strips a leading `-` (negation) and, for the `packages` file only, a leading `*` before parsing (:572-581).

**How a profile node's EAPI is chosen** (`L/portage/package/ebuild/_config/LocationsManager.py:204-343`):
1. Default: the repo's `profile_eapi_when_unspecified`, else **"0"** (:220-223).
2. Overridden by the node's `eapi` file (:225-233). An unsupported value raises `ParseError("Profile contains unsupported EAPI …")` (:235-242).
3. `allow_build_id = "build-id" in profile-formats` (:340).
4. `allow_profile_repo_deps(x)` (`L/portage/repository/config.py:1476-1485`) is `eapi_has_repo_deps(eapi)`, which is False for supported EAPIs, **or** `"profile-repo-deps" in profile_formats`.

**Repo default EAPI.** It is `profiles/eapi`, if that EAPI has `profile_eapi_default` (EAPI ≥ 9), else `profile_eapi_when_unspecified`, else "0" (`repository/config.py:524-547`, :1688-1727).
- **The container's gentoo snapshot has no `profile-formats` line** in `metadata/layout.conf`, and all 1109 profile `eapi` files contain `5`.

**Per-file flags:**

| File (repo profile node `x`) | Reader | eapi | wildcard | repo | build-id | use deps |
|---|---|---|---|---|---|---|
| `package.mask`, `package.unmask` | MaskManager.py:171-193 | `x.eapi` | no | `allow_profile_repo_deps(x)` | `x.allow_build_id` | allowed |
| repo `profiles/package.mask` | MaskManager.py:42-53 | `profiles/eapi`, else `repo.eapi` | no | `allow_profile_repo_deps(repo)` | `"build-id" in formats` | allowed |
| repo `profiles/package.unmask` (portage-1 only) | MaskManager.py:142-153 | same | no | same | same | allowed |
| `packages` | config.py:655-666 | `x.eapi` | no (leading `*` stripped) | `allow_profile_repo_deps(x)` | `x.allow_build_id` | allowed |
| `package.keywords` | KeywordsManager.py:25-37 | `x.eapi` | no | as above | `x.allow_build_id` | **dropped** (`allow_use=False`) |
| `package.accept_keywords` | KeywordsManager.py:49-60 | `x.eapi` | no | as above | **not passed → False** | dropped |
| `package.use`, `package.use.mask/force` (`.stable.*`) | UseManager.py:395-426 → `_parse_file_to_dict` :233-293 | `profile.eapi` (+ eapi_filter) | only if `eapi is None and user_config` (:277) | as above | `profile.allow_build_id` | dropped |
| repo `profiles/package.use*` | UseManager.py:381-393 | repo eapi | no | `allow_profile_repo_deps(repo)` | `"build-id" in formats` | dropped |
| `package.provided` | config.py:970-1011 | — | — | `isvalidatom("=" + line)`: no repo, no build-id, no blockers (dep:2660-2702) | | |

`package.provided` is honoured only if `allows_package_provided` (EAPI ≤ 6, or None). Otherwise Portage prints `!!! package.provided not allowed in EAPI %s:` (config.py:974-988). Invalid lines produce `Invalid package name in package.provided: %s` (:994-1000).

**What a profile EAPI changes.** Slot deps need EAPI ≥ 1, use deps ≥ 2, `:=`/sub-slot ≥ 5, `(+)` ≥ 4, `!!` ≥ 2. A profile node without an `eapi` file under a repo with no `profile_eapi_when_unspecified` is **EAPI 0**.

LIVE (`run4.sh`, container). `oprof` has `eapi` = 5; its child `oprof0` has no `eapi` file. Both carry the same `package.mask`; `oprof` also has `package.use` (` x` appended). The make.profile points to `oprof0`. The command each time was `emerge -pv --ignore-default-opts dev-libs/foo`.

First, with `layout.conf` lacking `profile-formats`:
```
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.use: dev-libs/foo::oracle
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.use: =dev-libs/foo-1-2
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.use: */foo
--- Atom is not allowed to have USE flag(s) in /var/db/repos/oracle/profiles/oprof/package.use: dev-libs/foo[x(+)]
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.use: dev-libs/foo-1
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.mask: dev-libs/foo::oracle
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.mask: =dev-libs/foo-1-2
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.mask: */foo
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.mask: dev-libs/foo-1
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: dev-libs/foo::oracle
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: =dev-libs/foo-1-2
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: */foo
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: dev-libs/foo:=
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: Slot deps are not allowed in EAPI 0: 'dev-libs/foo:0'
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: Use deps are not allowed in EAPI 0: 'dev-libs/foo[x(+)]'
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: dev-libs/foo-1
```
(The EAPI 5 `oprof/package.mask` accepted `dev-libs/foo:=`, `dev-libs/foo:0` and `dev-libs/foo[x(+)]`.)

Second, after setting `profile-formats = portage-2 profile-repo-deps build-id`:
```
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.use: */foo
--- Atom is not allowed to have USE flag(s) in /var/db/repos/oracle/profiles/oprof/package.use: dev-libs/foo[x(+)]
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.use: dev-libs/foo-1
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.mask: */foo
--- Invalid atom in /var/db/repos/oracle/profiles/oprof/package.mask: dev-libs/foo-1
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: */foo
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: dev-libs/foo:=
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: Slot deps are not allowed in EAPI 0: 'dev-libs/foo:0'
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: Use deps are not allowed in EAPI 0: 'dev-libs/foo[x(+)]'
--- Invalid atom in /var/db/repos/oracle/profiles/oprof0/package.mask: dev-libs/foo-1
```
`::oracle` and `=…-1-2` are now accepted, even with an explicit `eapi` given, because both flags are passed explicitly. The emerge run itself continues; the bad line is just dropped.

### (e) User config `/etc/portage/*` and the user profile `/etc/portage/profile/*`

**User files.** All use `eapi=None`, `verify_eapi=False`, `allow_wildcard=True`, `allow_repo=True`:

| File | Reader | build-id | `[use]` atoms |
|---|---|---|---|
| `package.mask`, `package.unmask` | MaskManager.py:214-232 | yes | accepted |
| `package.keywords`, `package.accept_keywords` | KeywordsManager.py:73-104 | yes | dropped (`allow_use=False`) |
| `package.use` and friends | UseManager.py:337-348 | yes | dropped |
| `package.license` | LicenseManager.py:37-42 | **no** (`allow_build_id` not passed) | accepted |
| `package.properties`, `package.accept_restrict`, `package.env` | config.py:857-902 | yes | **accepted** |

**`package.env` with a use dep crashes Portage.** A `[use]` atom in `package.env` is accepted, but matching it later raises `AssertionError: setcpv recursion detected` (LIVE below). Treat this as a Portage bug; it is not behaviour to copy.

**User profile node.** `/etc/portage/profile` gets `eapi = read_corresponding_eapi_file(..., default=None)` and `allow_build_id=True` (LocationsManager.py:169-189). Consequences:
- `allow_profile_repo_deps` is True, because `repo_deps(None)` is True.
- Its `package.mask` and `packages` have **no** wildcard support (MaskManager.py:171-180 does not pass `allow_wildcard`).
- Its `package.use*` **does** support wildcards (`extended_syntax = eapi is None and user_config`, UseManager.py:277-288).

LIVE (`run6.sh`, container, output sorted). The command was `emerge -pv --ignore-default-opts dev-libs/foo`. Each file contained the same 8 lines: `*/foo`, `dev-libs/fo*`, `dev-libs/foo::oracle`, `=dev-libs/foo-1-2`, `=dev-libs/foo-1-02`, `dev-libs/foo-1`, `!dev-libs/foo`, `dev-libs/foo[x]`.

```
--- Atom is not allowed to have USE flag(s) in /etc/portage/package.accept_keywords: dev-libs/foo[x]
--- Atom is not allowed to have USE flag(s) in /etc/portage/package.use: dev-libs/foo[x]
--- Atom is not allowed to have USE flag(s) in /etc/portage/profile/package.use: dev-libs/foo[x]
--- Invalid atom in /etc/portage/package.accept_keywords: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/package.accept_keywords: dev-libs/foo-1
--- Invalid atom in /etc/portage/package.env: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/package.env: dev-libs/foo-1
--- Invalid atom in /etc/portage/package.license: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/package.license: =dev-libs/foo-1-2          # build-id not allowed here
--- Invalid atom in /etc/portage/package.license: dev-libs/foo-1
--- Invalid atom in /etc/portage/package.mask: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/package.mask: dev-libs/foo-1
--- Invalid atom in /etc/portage/package.unmask: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/package.unmask: dev-libs/foo-1
--- Invalid atom in /etc/portage/package.use: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/package.use: dev-libs/foo-1
--- Invalid atom in /etc/portage/profile/package.mask: */foo                 # no wildcards in user-profile package.mask
--- Invalid atom in /etc/portage/profile/package.mask: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/profile/package.mask: dev-libs/fo*
--- Invalid atom in /etc/portage/profile/package.mask: dev-libs/foo-1
--- Invalid atom in /etc/portage/profile/package.use: =dev-libs/foo-1-02     # but wildcards OK in user-profile package.use
--- Invalid atom in /etc/portage/profile/package.use: dev-libs/foo-1
--- Invalid atom in /etc/portage/profile/packages: /foo                       # leading '*' stripped from "*/foo"
--- Invalid atom in /etc/portage/profile/packages: =dev-libs/foo-1-02
--- Invalid atom in /etc/portage/profile/packages: dev-libs/fo*
--- Invalid atom in /etc/portage/profile/packages: dev-libs/foo-1
[ebuild  N    #] dev-libs/foo-1:0/1::oracle  USE="x" 0 KiB
=== python: settings.packages / user profile node
profile nodes: [(…/23.0/systemd, '5', False, False), ('/etc/portage/profile', None, True, True)]
packages: ['dev-libs/foo::oracle', '=dev-libs/foo-1-2', '!dev-libs/foo', 'dev-libs/foo[x]']
#### package.env containing "dev-libs/foo[x] foo.conf"
AssertionError: setcpv recursion detected
```

Each `profile/packages` warning appears twice in the raw transcript; the profile is evidently read twice. Blockers (`!dev-libs/foo`) are accepted silently in mask and packages files.

### (f) Command-line atoms and `--exclude`-style options

**Package arguments.**
- Pre-check: `is_valid_package_atom(x, allow_repo=True)` (`actions.py:4244-4262`, and `depgraph.py:5261-5270`). Its definition (`L/_emerge/is_valid_package_atom.py:19-27`) inserts `cat/` for short names and calls `isvalidatom(x, allow_blockers=False, allow_repo=True, allow_build_id=True)`: no eapi, no wildcard, blockers refused.
- Then `Atom(x, allow_repo=True)` with `eapi=None`, so a build-id is allowed (depgraph.py:5278-5285).
- The "contains a conditional which is not allowed" error is raised **only for short names without a category** (:5344-5350), so `dev-libs/foo[x?]` is accepted.
- On failure Portage prints `!!! '<x>' is not a valid package atom.` / `!!! Please check ebuild(5) for full details.` and returns 1.

**Exclude-style options.**
- `--exclude`, `--getbinpkg-exclude/include`, `--reinstall-atoms`, `--rebuild-exclude/ignore`, `--usepkg-exclude/include`, `--useoldpkg-atoms` are checked by `_find_bad_atoms(atoms)` (`L/_emerge/main.py:886-906`, `L/portage/repository/config.py:91-116`).
  - It calls `Atom(atom, allow_wildcard=True, allow_repo=False)`.
  - It rejects any operator, blocker, use dep or repo. That implicitly rejects build-id, which needs an operator.
  - On a bad atom, `parser.error("Invalid Atom(s) in --%s parameter: '%s' (only package names and slot atoms (with wildcards) allowed)")` exits immediately.
- `--buildpkg-exclude` uses `less_strict=True`, which allows operators, repo and build-id (main.py:839-845).
- The depgraph then builds `WildcardPackageSet(atoms[, allow_repo=True])`. Bare names get an implicit `*/` prefix (`L/portage/_sets/base.py:250-260`, `depgraph.py:362-383`).

LIVE (`run5.sh` / `run6.sh`, container):
```
=== ARGV: emerge -pv --ignore-default-opts '=dev-libs/foo-1-2'
emerge: there are no ebuilds to satisfy "=dev-libs/foo-1-2".          # accepted as build-id; ebuilds have none
=== ARGV: emerge -pv --ignore-default-opts '*/foo'
!!! '*/foo' is not a valid package atom.
!!! Please check ebuild(5) for full details.
=== ARGV: emerge -pv --ignore-default-opts 'dev-libs/foo::oracle'   -> [ebuild  N     ] dev-libs/foo-1:0/1::oracle
=== ARGV: emerge -pv --ignore-default-opts '!dev-libs/foo'           -> !!! '!dev-libs/foo' is not a valid package atom.
=== ARGV: emerge -pv --ignore-default-opts 'dev-libs/foo[x?]'        -> [ebuild  N     ] dev-libs/foo-1:0/1::oracle  USE="-x"
=== ARGV: emerge -pv --ignore-default-opts 'foo::oracle'             -> [ebuild  N     ] dev-libs/foo-1:0/1::oracle
=== ARGV: emerge -pv --ignore-default-opts 'dev-libs/foo-1'          -> !!! 'dev-libs/foo-1' is not a valid package atom.
=== ARGV: emerge -pv --ignore-default-opts '=dev-libs/foo-1-02'      -> !!! '=dev-libs/foo-1-02' is not a valid package atom.
=== ARGV: emerge -pv --ignore-default-opts --exclude '>=dev-libs/foo-1' dev-libs/foo
emerge: error: Invalid Atom(s) in --exclude parameter: '>=dev-libs/foo-1' (only package names and slot atoms (with wildcards) allowed)
  (same error for --exclude 'dev-libs/foo::oracle', '=dev-libs/foo-1-2', 'dev-libs/foo[x]', '!dev-libs/foo';
   and for --usepkg-exclude 'dev-libs/foo::oracle')
=== --exclude 'dev-libs/*' | 'foo' | 'dev-libs/foo:0/1' | '*/*:0'  -> accepted, "Total: 0 packages"
=== --buildpkg-exclude '=dev-libs/foo-1-2' | 'dev-libs/foo::oracle' | '>=dev-libs/foo-1' -> accepted
```

### (g) The world (`selected`) file and (h) `/etc/portage/sets/*`

**Readers.** Both `WorldSelectedPackagesSet` and `StaticFileSet` validate each line with `ValidAtomValidator(atom, allow_repo=True)`, which is `isvalidatom` (`L/portage/env/validators.py:6`, `L/portage/_sets/files.py:68-69`, :284-285). So:
- no wildcard, no blocker
- **no build-id** (`isvalidatom` defaults `allow_build_id=False`)
- repo and use deps allowed; `eapi=None`

**On a bad line:** `ItemFileLoader` records `Validation failed at line: N, data X` and drops the line (`L/portage/env/loaders.py:203-221`).
- For a named set, the errors are printed after the merge list (`depgraph.py:11142-11145`).
- **For `@selected`/world, nothing was printed in the probe.** The errors stay on the inner `_pkgset` (files.py:236-255, :292-307), which is not the set the depgraph iterates. This mechanism is an inference; the silence itself is observed.
- `PackageSet._setAtoms` would additionally raise for wildcard or repo atoms when the set disallows them (`base.py:67-85`). Both sets here allow repo.

LIVE (`run5.sh`), using the same 8 lines:
```
=== ARGV: emerge -pv --ignore-default-opts @myset
[ebuild  N     ] dev-libs/foo-1:0/1::oracle  USE="x" 0 KiB
...
/etc/portage/sets/myset: Validation failed at line: 1, data */foo
/etc/portage/sets/myset: Validation failed at line: 2, data dev-libs/fo*
/etc/portage/sets/myset: Validation failed at line: 4, data =dev-libs/foo-1-2
/etc/portage/sets/myset: Validation failed at line: 5, data =dev-libs/foo-1-02
/etc/portage/sets/myset: Validation failed at line: 6, data dev-libs/foo-1
/etc/portage/sets/myset: Validation failed at line: 7, data !dev-libs/foo
=== ARGV: emerge -pv --ignore-default-opts @selected      (world file = same 8 lines)
[ebuild  N     ] dev-libs/foo-1:0/1::oracle  USE="x" 0 KiB     # bad lines silently dropped; [x] honoured
```
Lines 3 (`::oracle`) and 8 (`[x]`) were accepted.

Profile `packages` sets (`L/portage/_sets/profiles.py:19`, :46-47, and `ProfilePackageSet.py:15`, :36-37) reuse the profile flags from (d).

### (i) `package.env`

Covered in (e): user-only, flags as listed there. Portage has no profile-level `package.env` reader.

### (j) `REQUIRED_USE` and `IUSE`

Skipped as instructed. The `use.force`/`use.mask` files contain flags, not atoms.

---

## Q3. Matching semantics (`L/portage/dep/__init__.py:2887-3205`)

**Normalising the dependency.**
- A string `mydep` is re-parsed as `Atom(…, allow_wildcard=True, allow_repo=True)` after `lstrip("!")`.
- A blocker Atom is re-parsed without its blocker (:2901-2906).
- Build-id is taken from `mydep.build_id` (:2910).

**Build-id is consulted only in the `=` branch** (:2994-3008).
- The candidate must be a `_pkg_str` with an equal `build_id`. A `_pkg_str` whose build_id is None, i.e. any ebuild, does **not** match.
- A plain-string candidate matches regardless of build-id.
- `>=`, `~`, `<` and similar with a build-id parse fine with `eapi=None`, but **the build-id is ignored** in matching.
- `=*` can never carry a build-id: `=cat/pkg-1.0-2*` is `InvalidAtom` (dep:2032-2033; LIVE above).

**Extended syntax** (:2934-2972).
- The cp is matched with `extended_cp_match`: `re.escape(cp)` with `\*` turned into `[^/]*` (:2352-2365).
- For `=*` extended atoms, `\*VER\*` is a **substring test** on the candidate version (`ver in x_ver`), and `x_ver` includes `-rN`.

**`=*` on a normal atom** (:3010-3055). It is a prefix match on the version after leading-zero normalisation, constrained to version-component boundaries ("1*" does not match "10").

**Filters applied after the operator branch:**
- slot / sub-slot (:3115-3141)
- use deps, only for Package objects (:3143-3188)
- repo (:3190-3203): a candidate whose repo is None or `__unknown__` passes.

**Ranking** (`best_match_to_list`, :2796-2884):
- `=` 6, `~` 5, `=*` 4, a slotted atom 3, `> < >= <=` 2, bare cp 1.
- Extended atoms: `=*` 0, slotted −1, plain −2. **The code values differ from the docstring.**
- Ties among extended atoms keep the first one seen.

LIVE (`probe_match.py`). Candidates are `_pkg_str`: foo-1.0 with build_id 1, foo-1.0 with build_id 2, foo-1.0 with build_id None, foo-1.0-r1 with build_id 3, foo-10 with build_id 4, and foobar-1.12 with repo x.
```
=dev-libs/foo-1.0-2      -> ['dev-libs/foo-1.0#2']
=dev-libs/foo-1.0        -> ['dev-libs/foo-1.0#1', 'dev-libs/foo-1.0#2', 'dev-libs/foo-1.0#None']
>=dev-libs/foo-1.0-2     -> [all foo incl. #1, #None, 1.0-r1#3, 10#4]          # build-id ignored
~dev-libs/foo-1.0-2      -> ['…1.0#1', '…1.0#2', '…1.0#None', '…1.0-r1#3']    # build-id ignored
=dev-libs/foo-1*         -> foo-1.0 ×3, foo-1.0-r1 (not foo-10)
*/foo*                   -> all foo + foobar
=*/foo-*1*               -> foo-1.0 ×3, 1.0-r1, foo-10
=dev-libs/foo*-*12*      -> ['dev-libs/foobar-1.12#None']
dev-libs/foo::x          -> []          ; */*::x -> ['dev-libs/foobar-1.12#None']
plain-string candidates, =dev-libs/foo-1.0-2 -> ['dev-libs/foo-1.0', 'dev-libs/foo-1.0-r0']
best_match_to_list(foo-1.0#2, [*/*, dev-libs/*])            = */*        (first wins among wildcards)
best_match_to_list(..., + dev-libs/foo, */*:0, =*/foo-*1*)  = dev-libs/foo
best_match_to_list(..., + =dev-libs/foo-1.0-2, =dev-libs/foo-1.0) = =dev-libs/foo-1.0-2   (tie at 6: first kept)
```

---

## Q4. Where portuale's EAPI-agnostic superset diverges

**Method.**
- I read `crates/portage-dep/src/lib.rs`:
  - `CAT/PKG/VER/SLOT/REPO/USEFLAG` at :33-43
  - `atom_regex` :157-172
  - `parse_use_deps` :189-231
  - `parse_atom_uncached` :256-336
  - `parse_wildcard_atom` :832-856
- I ran the already-built `target/release/atom-harness`. Built 2026-10-11 01:10; there are no `crates/portage-dep` or `crates/atom-harness` commits after that. Command: `atom-harness batch < rs_in.txt`. Output is in `rs_pairs.txt`.

**Parse disagreements** (portuale versus Portage's `Atom(s)` and `Atom(s, eapi="8")`):
```
'dev-libs/foo::gentoo'        portuale=OK  Portage: None=OK e8=ERR e5=ERR wild=OK
'dev-libs/foo:0::gentoo[bar]' portuale=OK  Portage: None=OK e8=ERR
'=dev-libs/foo-1.0-2'         portuale=ERR Portage: None=OK(bid) e8=ERR
'>=dev-libs/foo-1.0-2'        portuale=ERR Portage: None=OK(bid) e8=ERR
'=dev-libs/foo-1-2'           portuale: |=|dev-libs|foo-1|2|   Portage None: cp=dev-libs/foo ver=1 bid=2 ; e8: ERR
'>=dev-libs/foo-1-2' '~dev-libs/foo-1-2'   portuale: pkg foo-1 ver 2 ; Portage None: bid=2 ; e8 ERR
'=dev-libs/foo-1-2*'          portuale: |=*|dev-libs|foo-1|2|  Portage: ERR (both modes)
'dev-libs/foo:_a'             portuale=ERR Portage=OK (all modes)
'=dev-libs/foo-١' (U+0661)    portuale=OK  Portage=ERR (all modes)
```

### Per-context consequences

**1. Ebuild and binary deps.** These are the highest-value items.
- **(a) portuale accepts what Portage rejects:**
  - `::repo` in any `*DEPEND` (Portage masks: `invalid: RDEPEND: Invalid atom (…::repo), token N`).
  - Versioned atoms whose package name ends in `-<version>` with an operator (`=cat/foo-1-2`, `>=…`, `~…`, `=…-1-2*`). Portage rejects these in an EAPI context; portuale silently reads them as package `foo-1`.
- **(b) portuale does not mask packages with syntactically invalid dep atoms at all.**
  - `first_invalid_conditional` (`crates/portage-repo/src/masked_deps.rs:375-377`) does `let Some(atom) = parse_atom(t) else { continue }`.
  - I found no producer of `Invalid atom (…), token N` or `dependency.syntax`.
  - This is from grep across `crates/`, not exhaustive; treat it as a strong guess.
  - The same masking would be needed for `EAPI.incompatible` atoms (e.g. `:=` in EAPI < 5), but portuale's EAPI floor of 7 makes that moot for ebuilds.
- **(c)** The IUSE-missing conditional mask is implemented (masked_deps.rs:296-411) and matches the Portage text.

**2. Installed (vdb) deps.**
- Portage, through the C path, marks an installed package with `::repo` or a build-id in its vdb `*DEPEND` as `invalid` and invisible, with C error text.
- portuale accepts these. This is rare in practice: Portage never writes such atoms into the vdb.

**3. Profiles, by profile EAPI and `profile-formats`.**
- portuale emits no `--- Invalid atom in …` warnings (no such string in `crates/`) and drops nothing.
- In a profile node without an `eapi` file (EAPI "0" unless the repo sets `profile_eapi_when_unspecified`), portuale accepts `:slot`, `[use]` and `:=`. Portage drops those lines.
- portuale accepts `::repo` without `profile-repo-deps`.
- portuale rejects build-id lines that Portage accepts under the `build-id` format.
- portuale accepts `*/foo`-style wildcards. `matches_config_entry` (`crates/portage-repo/src/config_match.rs:8-21`) falls back to `parse_wildcard_atom` for every package.* list. **Unverified:** whether every portuale profile reader routes through it. Portage rejects wildcards in repo-profile files, and in the user profile's `package.mask` and `packages`.
- `[use]` atoms in `package.use*` and `package.(accept_)keywords`: Portage drops them with "not allowed to have USE flag(s)". What portuale does with them was not checked.

**4. User config.**
- portuale's wildcard grammar covers only `*/*`, `cat/*` and `*/pkg` (lib.rs:808-856). Portage's extended syntax additionally accepts:
  - partial globs (`dev-libs/fo*`, `dev-*/foo`, `*-libs/f*o`)
  - a leading `+` in a wildcard name
  - wildcard plus slot (`*/*:0`)
  - wildcard plus repo (`*/*::gentoo`)
  - `=*/foo-*1*` substring-version atoms
  - blockers
- portuale rejects build-id lines everywhere Portage accepts them: all user `package.*` files except `package.license`, and the user profile.

**5. Command line.**
- Portage accepts `=cat/pkg-1.0-2` as a build-id. It matches only binaries with that build-id, otherwise prints `there are no ebuilds to satisfy`.
- portuale's `parse_atom` returns None, and it would report an invalid atom. The exact portuale message was not probed.
- `--exclude` and similar: Portage aborts with `emerge: error: Invalid Atom(s) in --X parameter: '…' (only package names and slot atoms (with wildcards) allowed)` for operators, repo (except `--buildpkg-exclude`), use deps or blockers. Whether portuale validates these options was not checked.

**6. World file and sets.**
- Portage drops wildcard, blocker and build-id lines. It prints `Validation failed at line: N, data X` for named sets and nothing for world.
- portuale behaviour was not checked.

**7. Grammar-level nits that apply in every context:**
- portuale's `SLOT` (`[A-Za-z0-9][…]`, lib.rs:38) rejects a slot starting with `_`, which Portage's `[\w]` accepts.
- portuale's `VER` uses `\d`, which is Unicode-aware in Rust's `regex` crate unless `(?-u)` is set, so it accepts non-ASCII digits. Portage uses `re.ASCII`.
- Use-dep default-conflict rules are equivalent (`seen_defaults`, lib.rs:198-221, versus `_use_dep` :1345-1357). Both accept `[bar(+),-bar(+)]`.

---

## Escapes and edge cases

- **`PORTAGE_NATIVE_DEP_PARSER=0`** turns off both C paths (dep:63-65). The installed-package divergence in Q2c then disappears.
- **The C `use_reduce` path is disabled** by any of: non-None `is_valid_flag`, `opconvert`, `is_src_uri`, subset, masklist, excludeall, matchnone, a `token_class` other than `Atom`, or `eapi` without `slot_operator` (dep:1132-1146). So ebuild and binary validation and depgraph traversal always use the regex path.
- **An explicit `allow_repo` or `allow_build_id` from the caller overrides the EAPI default** even when `eapi` is given (dep:1928). Profile readers rely on this.
- **`Atom(eapi=8)`** with an int instead of a str raises `AttributeError` from `Eapi.__init__` (eapi.py:225), not `TypeError`.
- **`isvalidatom`** returns False for blockers unless `allow_blockers` is set (dep:2698). This affects the command line, sets and world.
- **`grabfile_package` returns string forms, not Atoms, for negated lines** (`-atom`) and for `packages` lines with `*` (util:595-606).

## What I could NOT establish

1. **Which other `use_reduce` callers hit the C path with repo or build-id atoms.** I verified only `Package._validate_deps` for installed packages and the depgraph traversal (`depgraph.py:4352`, which uses the regex path). I did not audit every `use_reduce` call in `portage/` (e.g. `dbapi/`, `_sets/`, `vartree`).
2. **Why world-file validation errors are not printed.** The mechanism (errors kept on the inner `_pkgset`) is inferred from the source; only the silence is observed.
3. **The behaviour of the portuale binary, as opposed to its parser, in each Q2 context.** Not probed. The Q4 items 1(b), 3, 4 (beyond wildcards), 5 and 6 are source-grep inferences or explicitly unverified.
4. **Unsupported-EAPI ebuilds.** I did not probe whether `_validate_deps` output or the `EAPI.unsupported` mask is what the user sees, or in what order.
5. **The repo-level `profiles/package.mask` live case.** I probed profile-node files, not the repo-level `profiles/package.mask` and the gentoo `profiles/eapi` default. The source is cited (MaskManager.py:42-53).
