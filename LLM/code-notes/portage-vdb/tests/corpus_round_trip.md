# crates/portage-vdb/tests/corpus_round_trip.rs
Corpus round trip: files -> sqlite/redb -> files must `verify` clean, and index queries must match the scan.

## module
- The corpus comes from backlog #305 (`LLM/evidence/305-s0-corpus.md`, plan steps S2.8 and S5.3).

## fn index_queries_match_the_scan
- Covers plan steps S8.1 and S8.2 on the corpus.
