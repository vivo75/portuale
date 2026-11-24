---
title: Archify diagrams are regenerated from candidate JSON and cite only committed paths
author: Francesco
date: 2026-11-24
branch: main
cost: { minutes: 9, tokens: 329966 }
id: l-93b00a93
scope: repo
status: active
src: agent
touches:
  - docs/archify/README.md
  - docs/archify/architecture.delivery.json
  - docs/archify/architecture.html
  - docs/archify/class.delivery.json
  - docs/archify/class.html
  - docs/archify/dataflow.delivery.json
  - docs/archify/dataflow.html
  - docs/archify/erd.delivery.json
  - docs/archify/erd.html
  - docs/archify/lifecycle.delivery.json
---

## Decided
The ten diagrams under docs/archify are built from hand-written candidate JSON and delivered with finalize. Architecture omits portuale's direct links to leaf crates (a card says so) to cut route crossings from 8 to 3. Lifecycle collapses the pretend..install phases into one Building state. The waterfall is marked illustrative; the timeline uses commit dates.

## Tried and ruled out
A 13-state lifecycle (one state per phase) failed the width gate. Citing the fixtures symlink failed repository evidence.

## Watch out
Source citations are checked against the pinned commit, so a symlink or uncommitted path is rejected. Tree ids must start with a letter (3rdparty was refused). Sequence participant labels are capped near 15 text units. The generator script was a scratch file and is not in the repo; edit the JSON in docs/archify/src and rerun finalize with --repo-root.

In the code: docs/archify/src/*.json, docs/archify/README.md; command: node .claude/skills/archify/bin/archify.mjs finalize <type> <json> <html> --repo-root . --quality showcase
