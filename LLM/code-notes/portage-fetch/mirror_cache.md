# crates/portage-fetch/src/mirror_cache.rs
Portage's `${DISTDIR}/.mirror-cache.json` round-trip plus the URL helpers `async_mirror_url` builds on.

## module
- Ported from `fetch.py::async_mirror_url` (cache shape, URL helpers).
- Pure: no I/O and no clock here; `portuale/src/fetch.rs` reads/writes the file and passes `now` in.

## fn parse_mirror_cache
- Anything that isn't Portage's shape yields an empty cache: invalid JSON, a non-object, an entry that isn't `[number, [[str...]...]]`.

## fn serialize_mirror_cache
- Matches Python's default `json.dump` separators and `ensure_ascii` escaping; entries keep their original order, new ones appended (a Python dict keeps insertion order).

## mod tests
- `REAL_CACHE`: a cache file as Portage wrote it on this project's dev host (`/var/cache/distfiles/.mirror-cache.json`, 2026-09-14).
