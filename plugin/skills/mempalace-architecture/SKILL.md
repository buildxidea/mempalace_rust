---
name: mempalace-architecture
description: Overview of mempalace architecture including wings, halls, rooms, BBBL, and palace structure
---

## Palace structure

A mempalace instance contains all stored observations organized in a hierarchy:

```text
/path/to/palace/
  mpr_drawers.json        -- main embedvec collection (observations)
  mpr_compressed.json     -- compressed versions (AAAK dialect)
  tunnels.json            -- mesh peer connections
```

## Wings, halls, and rooms

Data is organized by topic (wing) and sub-topic (hall/room), following the memory palace metaphor:

- **Wing** -- broad topic area (e.g., "emotions", "technical", "identity"). Detected from directory names during `mpr init`.
- **Hall** -- category within a wing ("bug-fixes", "decisions", "patterns").
- **Room** -- specific context or file within a hall.

Default wings: emotions, consciousness, memory, technical, identity, family, creative.

## BBBL (Basic-Building-Block-Language)

AAAK Dialect for compressing observations to ~3-5% of original size:

```text
@KAI @PRI
&project (mempalace_rust | config)
=KAI We decided to use XDG_CONFIG_HOME ... (config-resolution | 2026-06-10)
```

This is an identity-aware shorthand where people are mapped to codes (KAI, PRI) and observations are compressed with type markers (`@`, `&`, `=`).

## Memory layers

- **L0 Identity** (`identity.txt`): AI's self-concept, loaded every session.
- **L1 Wake-up**: Recent context from the last few sessions (~600-900 tokens).
- **L2 Recall**: On-demand search via hybrid BM25 + vector + graph.
- **L3 Consolidation**: Periodic LLM-based compression of observations into memories.
- **Graph**: Knowledge graph with entities, relations, and graph-expanded search.

## Storage

- **Config**: `~/.mempalace/config.json` (XDG-compliant, `$XDG_CONFIG_HOME/mempalace/`)
- **Palace data**: Default `~/.mempalace/palace/` (configurable via `palace_path`)
- **Identity**: `~/.mempalace/identity.txt`
- **Entity registry**: `~/.mempalace/entity_registry.json`
- **People map**: `~/.mempalace/people_map.json`
