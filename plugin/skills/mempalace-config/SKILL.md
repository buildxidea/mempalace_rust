---
name: mempalace-config
description: Configuration options, environment variables, and precedence for mempalace
---

## Config file

Located at `~/.mempalace/config.json` (XDG-compliant: `$XDG_CONFIG_HOME/mempalace/config.json`):

```json
{
  "palace_path": "~/.mempalace/palace",
  "collection_name": "mempalace_drawers",
  "people_map": {},
  "embedding_model": "naive",
  "languages": [],
  "consolidation_enabled": true,
  "graph_extraction_enabled": null,
  "rerank_enabled": null,
  "bm25_weight": null,
  "vector_weight": null,
  "graph_weight": null
}
```

## Environment variables

| Variable | Description |
|----------|-------------|
| `MEMPALACE_PALACE_PATH` | Override palace path (same as `--palace`) |
| `MEMPAL_DIR` | Directory for auto-mining in hooks |
| `MEMPALACE_HTTP_PORT` | Port for REST API server (default: 6969) |
| `MEMPALACE_URL` | Base URL for REST API fallback |
| `MEMPALACE_SECRET` | Bearer token for REST API auth |
| `XDG_CONFIG_HOME` | Config directory (default: `~/.config`) |
| `XDG_DATA_HOME` | Data directory override |
| `XDG_STATE_HOME` | State directory override |

## Precedence (highest to lowest)

1. CLI flags (`--palace <path>`, `--wing`, `--limit`, etc.)
2. Environment variables (`MEMPALACE_PALACE_PATH`, etc.)
3. Config file (`~/.mempalace/config.json`)
4. Application defaults

## Feature flags

| Config key | Default | Effect |
|-----------|---------|--------|
| `consolidation_enabled` | `true` | Enable LLM consolidation pipeline |
| `auto_compress` | `null` | Auto-compress new observations |
| `graph_extraction_enabled` | `null` | Enable knowledge graph extraction |
| `rerank_enabled` | `null` | Enable BM25 reranking |
| `snapshot_enabled` | `null` | Enable snapshot system |
| `vision_enabled` | `null` | Enable vision search |

## Embedding models

| Model | Description |
|-------|-------------|
| `naive` (default) | Word overlap similarity, no external model |
| `paraphrase-multilingual-MiniLM-L12-v2` | Multilingual embeddings |
| `all-MiniLM-L6-v2` | Fast English embeddings |
