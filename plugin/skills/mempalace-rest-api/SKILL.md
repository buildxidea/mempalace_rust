---
name: mempalace-rest-api
description: REST API endpoint reference for the mempalace HTTP server
---

The REST API is served on port 6969 by default (`MEMPALACE_HTTP_PORT`). Start with `mpr serve --http`.

## Endpoints

### Health & info

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Simple health check |
| GET | `/healthz` | Full health report (requires `health` feature) |
| GET | `/livez` | Lightweight liveness probe |
| GET | `/tools` | List available tools |

### Memories

| Method | Path | Description |
|--------|------|-------------|
| GET | `/memories` | List memories |
| POST | `/memories` | Save a memory |
| GET | `/memories/{id}` | Get memory by ID |
| POST | `/memories/{id}` | Delete memory |

### Sessions & commits

| Method | Path | Description |
|--------|------|-------------|
| GET | `/sessions` | List sessions |
| GET | `/commits` | List commits |
| GET | `/commits/{hash}` | Look up commit |
| POST | `/session/start` | Start a session |
| POST | `/session/end` | End a session |

### Search

| Method | Path | Description |
|--------|------|-------------|
| POST | `/search` | Search memories |
| POST | `/smart_search` | Hybrid BM25 + vector search |

### Knowledge graph

| Method | Path | Description |
|--------|------|-------------|
| POST | `/kg/query` | Query KG |
| POST | `/kg/add` | Add KG fact |
| POST | `/kg/invalidate` | Invalidate KG fact |
| GET | `/kg/stats` | KG statistics |
| POST | `/kg/timeline` | KG timeline |
| POST | `/kg/traverse` | Traverse KG |
| GET | `/graph/stats` | Graph statistics |
| POST | `/graph/search` | Graph search |
| POST | `/graph/expand` | Graph expand |

### Slots

| Method | Path | Description |
|--------|------|-------------|
| GET | `/slots` | List slots |
| POST | `/slots` | Create slot |
| GET | `/slots/{id}` | Get slot |
| POST | `/slots/{id}` | Delete slot |
| POST | `/slots/{id}/append` | Append to slot |
| POST | `/slots/{id}/replace` | Replace slot |

### Sentinels, checkpoints, actions

| Method | Path | Description |
|--------|------|-------------|
| GET/POST | `/sentinels` | List/create sentinels |
| POST | `/sentinels/{id}` | Delete sentinel |
| POST | `/sentinels/{id}/trigger` | Trigger sentinel |
| GET/POST | `/checkpoints` | List/create checkpoints |
| POST | `/checkpoints/{id}/resolve` | Resolve checkpoint |
| GET/POST | `/actions` | List/create actions |
| POST | `/actions/{id}` | Update action |

### Other

`POST /observe`, `POST /enrich`, `POST /consolidate`, `POST /reflect`, `POST /migrate`, `GET /status`, `POST /context/build`, `POST /timeline`, `POST /patterns`, `GET /audit`, `POST /relations`, `GET /profile`, `POST /skill/extract`, `POST /retention/score`, `GET /access/stats`, `POST /vision/search`, `POST /sketches`, `POST /session/start`, `POST /session/end`, `POST /summarize`, `POST /forget`, `POST /remember`, `GET /livez`, `GET /viewer/`, `POST /team/share`, `GET /team/feed`, `POST /lesson/save`, `GET /lesson/recall`, `POST /crystallize`, POST ` /diagnose`, `POST /heal`, `POST /verify`.
