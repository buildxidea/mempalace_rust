# Mempalace Rust: 1:1 AgentMemory Feature Parity Plan

## Context

**Goal:** Implement all features from [rohitg00/agentmemory](https://github.com/rohitg00/agentmemory) (TypeScript, ~50k lines) as a 1:1 Rust port in mempalace_rust. AgentMemory is a persistent, searchable, versioned memory database for AI agents with 4-tier consolidation, hybrid search (BM25+Vector+Graph via RRF), LLM-driven compression, multi-agent coordination, and vision support.

**Current state:** mempalace_rust has ~62 source files, 552 tests, 19 MCP tools, 4 embedding backends, SQLite+HNSW storage, full knowledge graph, and BM25+Vector+PPR search. Missing: LLM pipeline, compression, consolidation, retention/decay, versioning, multi-agent features, vision, smart search, and ~35 other agentmemory functions.

**Approach:** 8 phases, ordered by dependency. Each phase produces a compilable, tested increment.

---

## Phase 0: Foundation — New Types & LLM Provider Abstraction

### 0.1 — AgentMemory-equivalent types in Rust

**File: `crates/core/src/types.rs` (new)**

Map ALL agentmemory types to Rust structs/enums:

```rust
// Core enums (1:1 from agentmemory types.ts)
pub enum ObservationType {
    FileRead, FileWrite, FileEdit, CommandRun, Search, WebFetch,
    Conversation, Error, Decision, Discovery, Subagent, Notification,
    Task, Image, Other,
}

pub enum HookType {
    SessionStart, PromptSubmit, PreToolUse, PostToolUse, PostToolFailure,
    PreCompact, SubagentStart, SubagentStop, Notification,
    TaskCompleted, Stop, SessionEnd,
}

pub enum MemoryType {
    Pattern, Preference, Architecture, Bug, Workflow, Fact,
}

pub enum ConsolidationTier {
    Working, Episodic, Semantic, Procedural,
}

pub enum GraphNodeType {
    File, Function, Concept, Error, Decision, Pattern, Library,
    Person, Project, Preference, Location, Organization, Event,
}

pub enum GraphEdgeType {
    Uses, Imports, Modifies, Causes, Fixes, DependsOn, RelatedTo,
    WorksAt, Prefers, BlockedBy, CausedBy, OptimizesFor, Rejected,
    Avoids, LocatedIn, SucceededBy,
}

pub enum ActionEdgeType {
    Requires, Unlocks, SpawnedBy, GatedBy, ConflictsWith,
}

pub enum ActionStatus { Pending, Active, Done, Blocked, Cancelled }
pub enum SignalType { Info, Request, Response, Alert, Handoff }
pub enum CheckpointType { Ci, Approval, Deploy, External, Timer }
pub enum CheckpointStatus { Pending, Passed, Failed, Expired }
pub enum SentinelType { Webhook, Timer, Threshold, Pattern, Approval, Custom }
pub enum CircuitState { Closed, Open, HalfOpen }
pub enum AgentScopeMode { Shared, Isolated }
pub enum TeamMode { Shared, Private }
```

Core structs (with Serde, 1:1 field mapping from agentmemory):
- `Session` — id, project, cwd, started_at, ended_at, status, observation_count, model, tags, first_prompt, summary, commit_shas, agent_id
- `RawObservation` — id, session_id, timestamp, hook_type, tool_name, tool_input, tool_output, user_prompt, assistant_response, raw, modality, image_data, agent_id
- `CompressedObservation` — id, session_id, timestamp, type, title, subtitle, facts, narrative, concepts, files, importance (1-10), confidence, image_ref, image_description, modality, agent_id
- `Memory` — id, created_at, updated_at, type, title, content, concepts, files, session_ids, strength, version, parent_id, supersedes, related_ids, source_obs_ids, is_latest, forget_after, image_ref, agent_id, project
- `SemanticMemory` — id, fact, confidence, source_session_ids, source_memory_ids, access_count, last_accessed_at, strength
- `ProceduralMemory` — id, name, steps, trigger_condition, expected_outcome, frequency, source_session_ids, tags, concepts, strength
- `MemoryRelation` — type (supersedes/extends/derives/contradicts/related), source_id, target_id, confidence
- `RetentionScore` — memory_id, source, score, salience, temporal_decay, reinforcement_boost, last_accessed, access_count
- `DecayConfig` — lambda (0.01), sigma (0.3), tier_thresholds (hot=0.7, warm=0.4, cold=0.15)
- `ContextBlock` — type, content, tokens, recency, source_ids

Multi-agent structs:
- `Action` — id, title, description, status, priority (1-10), created_at, updated_at, created_by, assigned_to, project, tags, source_obs_ids, source_memory_ids, result, parent_id, metadata, sketch_id, crystallized_into
- `ActionEdge` — id, type, source_action_id, target_action_id, metadata
- `Lease` — id, action_id, agent_id, acquired_at, expires_at, renewed_at, status
- `Checkpoint` — id, name, description, status, type, created_at, resolved_at, resolved_by, result, expires_at, linked_action_ids
- `Signal` — id, from, to, thread_id, reply_to, type, content, metadata, created_at, read_at, expires_at
- `Routine` — id, name, description, steps, created_at, updated_at, frozen, tags, source_procedural_ids
- `RoutineStep` — order, title, description, action_template, depends_on
- `RoutineRun` — id, routine_id, status, started_at, completed_at, action_ids, step_status, initiated_by
- `Sketch` — id, title, description, status, action_ids, project, created_at, expires_at
- `Crystal` — id, narrative, key_outcomes, files_affected, lessons, source_action_ids, session_id, project
- `Lesson` — id, content, context, confidence, reinforcements, source, source_ids, project, tags, decay_rate
- `Insight` — id, title, content, confidence, reinforcements, source_concept_cluster, source_memory_ids, decay_rate
- `Facet` — id, target_id, target_type, dimension, value
- `Sentinel` — id, name, type, status, config, result, linked_action_ids
- `MemorySlot` — label, content, size_limit, description, pinned, read_only, scope, created_at, updated_at
- `ProjectProfile` — project, updated_at, top_concepts, top_files, conventions, common_errors, session_count, total_observations
- `MeshPeer` — id, url, name, last_sync_at, status, shared_scopes
- `AuditEntry` — id, timestamp, operation, user_id, function_id, target_ids, details, quality_score
- `ExportData` — version, exported_at, sessions, observations, memories, summaries, profiles, graph_nodes, graph_edges, semantic_memories, procedural_memories, actions, action_edges, routines, signals, checkpoints, sentinels, sketches, crystals, facets, lessons, insights, access_logs

### 0.2 — LLM Provider Trait

**File: `crates/core/src/llm/provider.rs` (new)**

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn complete(&self, system: &str, user: &str) -> anyhow::Result<String>;
    async fn describe_image(&self, image_base64: &str, mime: &str, prompt: &str) -> anyhow::Result<String>;
}
```

**File: `crates/core/src/llm/circuit_breaker.rs` (new)**

```
Constants: failure_threshold=3, failure_window_ms=60_000, recovery_timeout_ms=30_000
State machine: Closed -> Open (on >=3 failures in 60s) -> HalfOpen (after 30s) -> Closed (on success)
is_allowed() -> bool
record_success() / record_failure()
```

**File: `crates/core/src/llm/fallback_chain.rs` (new)**

Try providers in configured order. On error, try next. Circuit breaker wraps each provider.

**File: `crates/core/src/llm/openai_compat.rs` (new)**

OpenAI-compatible provider (works with OpenAI, Ollama, any /v1/chat/completions endpoint). Uses `reqwest` (already a dep). Config via env: `OPENAI_API_KEY`, `OPENAI_MODEL`, `OPENAI_BASE_URL`.

**File: `crates/core/src/llm/anthropic_provider.rs` (new)**

Anthropic Claude provider. Uses Messages API. Config: `ANTHROPIC_API_KEY`, `ANTHROPIC_MODEL`.

**File: `crates/core/src/llm/noop_provider.rs` (new)**

No-op provider for when no LLM keys are configured. Returns empty string.

**File: `crates/core/src/llm/mod.rs` (new)**

Module re-exports + factory function `create_llm_provider_from_env()` that auto-detects from available API keys.

### 0.3 — Extend Palace struct

**File: `crates/core/src/palace.rs` (modify)**

Add to Palace struct:
```rust
pub struct Palace {
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn PalaceStore>,
    llm: Option<Arc<dyn LlmProvider>>,       // NEW
    sessions: Arc<SessionStore>,              // NEW
}
```

Update `PalaceBuilder` to accept `Option<Arc<dyn LlmProvider>>`.

### 0.4 — Session Store

**File: `crates/core/src/session.rs` (new)**

SQLite-backed session tracking. CRUD for `Session` records. Scopes observations by session_id. Methods: `create_session`, `get_session`, `list_sessions`, `end_session`, `add_observation`, `get_observations(session_id)`, `list_all_observations(project?)`.

### Tests for Phase 0
- All types serialize/deserialize round-trip (serde_json)
- LlmProvider trait: mock provider returns canned response
- Circuit breaker: state transitions (closed->open->halfopen->closed)
- Fallback chain: first provider fails, second succeeds
- Noop provider returns empty
- PalaceBuilder accepts optional LLM provider

---

## Phase 1: Compression Pipeline (mem::compress)

### 1.1 — XML Prompt Templates

**File: `crates/core/src/prompts/compression.rs` (new)**

Exact 1:1 port of agentmemory's `COMPRESSION_SYSTEM` prompt. Output XML schema: `<type>`, `<title>`, `<subtitle>`, `<facts><fact>`, `<narrative>`, `<concepts><concept>`, `<files><file>`, `<importance>`.

`build_compression_prompt(observation)` assembles user prompt from: timestamp, hook_type, tool_name, tool_input (truncated 4000), tool_output (truncated 4000), user_prompt (truncated 2000).

### 1.2 — XML Parser

**File: `crates/core/src/prompts/xml.rs` (new)**

`get_xml_tag(xml: &str, tag: &str) -> Option<String>` — extract first text content.
`get_xml_children(xml: &str, parent: &str, child: &str) -> Vec<String>` — extract all child tag texts.
Valid tag pattern: `^[a-zA-Z_][a-zA-Z0-9_-]*$`.

### 1.3 — Compression Engine

**File: `crates/core/src/compress.rs` (new)**

Algorithm (1:1 from agentmemory compress.ts):
1. If observation has image data AND LLM provider supports vision -> describe image first, fall back to text-only on failure
2. Build compression prompt from observation fields
3. Call `llm.complete(COMPRESSION_SYSTEM, prompt)`
4. Parse XML response via `parse_compression_xml(xml)`
5. Validate: type in ObservationType enum, importance clamped [1,10]
6. Quality score (max 100): +25 if facts>0, +10 if facts>=3, +20 if narrative>=20, +5 if >=50, +15 if title in [5,120], +15 if concepts>0, +10 if importance in [1,10]
7. confidence = quality_score / 100
8. If validation fails: retry once with stricter suffix appended to system prompt
9. Return `CompressedObservation`

### 1.4 — Synthetic Compression (Zero-LLM fallback)

**File: `crates/core/src/compress_synthetic.rs` (new)**

1:1 port of `inferType()` and `extractFiles()`:
- `infer_type(tool_name, hook_type)`: maps hook_type -> observation type, then normalizes tool_name (camelCase/kebab -> snake_case) and pattern-matches substrings
- `extract_files(input)`: scan for keys `file_path`, `filepath`, `path`, `filePath`, `file`, `pattern`; return unique values <512 chars
- Output: CompressedObservation with title=truncate(tool_name,80), importance=5, confidence=0.3

### 1.5 — Wire into add_drawer

**File: `crates/core/src/palace.rs` (modify)**

In `MemoryProvider::add_drawer()`: if drawer.kind == Raw and LLM provider available, run compression pipeline before storing. If no LLM, use synthetic compression.

### Tests for Phase 1
- XML parser: extract tags, children, invalid tag names rejected
- Compression: mock LLM returns valid XML -> parsed CompressedObservation with correct fields
- Compression: mock LLM returns invalid XML -> retry with stricter prompt
- Synthetic: infer_type maps all known tool_name patterns
- Synthetic: extract_files finds file_path variants
- Quality scoring: verify all score components

---

## Phase 2: Consolidation & Memory Lifecycle

### 2.1 — Consolidation Engine

**File: `crates/core/src/consolidation.rs` (new)**

**Concept grouping algorithm** (1:1 from agentmemory consolidate.ts):
1. Fetch all sessions (optionally filtered by project)
2. Load observations, filter: has title AND importance >= 5
3. Build `concept_groups: HashMap<String, Vec<CompressedObservation>>` grouped by lowercase concept
4. Sort groups by count desc, filter >= 3 observations per group
5. Cap at 10 LLM calls per consolidation run
6. For each group: take top 8 observations by importance, build prompt, call LLM
7. Parse XML: `<type>`, `<title>`, `<content>`, `<concepts>`, `<files>`, `<strength>` (1-10)

**Versioning / isLatest logic**:
- Find existing memory with same title (case-insensitive) + project scope
- If found: set `existing.is_latest = false`, create new with `version = existing.version + 1`, `parent_id = existing.id`, `supersedes = [existing.id, ...existing.supersedes]`
- If not: create new with `version = 1`, `is_latest = true`

### 2.2 — Consolidation Pipeline (4-Tier)

**File: `crates/core/src/consolidation_pipeline.rs` (new)**

Three-stage pipeline:
1. **Semantic tier**: fetch session summaries (>=5 required), take 20 most recent, LLM extracts facts via `<fact>...</fact>` XML, create SemanticMemory with confidence
2. **Procedural tier**: collect repeated patterns (frequency >= 2), LLM extracts procedures via `<procedure><trigger><steps>` XML, create ProceduralMemory
3. **Decay**: `apply_decay(items, decay_days)` — compute `days_since`, `decay_periods = floor(days_since / decay_days)`, `strength = max(0.1, strength * 0.9^decay_periods)`

### 2.3 — Memory Versioning (mem::evolve)

**File: `crates/core/src/memory_lifecycle.rs` (new)**

`evolve(memory_id, new_content, new_title?)`:
- Load existing memory
- Set `is_latest = false`
- Create new version: `version += 1`, `parent_id = old.id`, `supersedes = [old.id, ...old.supersedes]`
- Both stored, old marked superseded

### 2.4 — Retention Scoring (mem::retention-score)

**File: `crates/core/src/retention.rs` (new)**

Exact formula from agentmemory:
```
salience = type_weight(memory.type) + min(0.2, access_count * 0.02)
type_weights = { architecture: 0.9, preference: 0.85, pattern: 0.8, bug: 0.7, workflow: 0.6, fact: 0.5 }
temporal_decay = exp(-lambda * deltaT)    // lambda = 0.01
reinforcement_boost = sigma * sum(1/days_since_access_i)  // sigma = 0.3
score = min(1, salience * temporal_decay + reinforcement_boost)
```

Tier classification: hot >= 0.7, warm >= 0.4, cold >= 0.15, evictable < 0.15

### 2.5 — Auto-Forget (mem::auto-forget)

**File: `crates/core/src/auto_forget.rs` (new)**

Three-phase algorithm:
1. **TTL expiry**: delete memories where `now > forget_after`
2. **Contradiction detection** (Jaccard > 0.9): tokenize content (words >2 chars), compute Jaccard similarity on pairs sharing concepts, mark older as `is_latest = false`
3. **Low-value eviction**: observations >180 days AND importance <= 2 -> delete

### 2.6 — Eviction (mem::evict)

**File: `crates/core/src/evict.rs` (new)**

Multi-strategy: stale sessions (>30 days), low-importance old observations, expired memories, non-latest old memories, per-project observation caps.

### Tests for Phase 2
- Consolidation: 3 observations with shared concept -> grouped, LLM called, memory created
- Versioning: evolve existing memory -> old marked !is_latest, new has version+1
- Retention: verify score formula matches expected values for known inputs
- Auto-forget: TTL expiry removes correct memories
- Auto-forget: Jaccard contradiction detection marks older as !is_latest
- Decay: verify `strength * 0.9^decay_periods` calculation
- Pipeline: semantic + procedural + decay stages execute in order

---

## Phase 3: Search Enhancement

### 3.1 — RRF Fusion (replace current hybrid)

**File: `crates/core/src/search/rrf.rs` (new)**

Exact RRF formula from agentmemory:
```
RRF_K = 60
combined_score = W_bm25 * (1/(RRF_K + bm25_rank)) + W_vector * (1/(RRF_K + vector_rank)) + W_graph * (1/(RRF_K + graph_rank))
Defaults: bm25_weight=0.4, vector_weight=0.6, graph_weight=0.3
```

Weight normalization: when a stream has no results, set its effective weight to 0, then normalize all weights so they sum to 1.0.

### 3.2 — Diversification

**File: `crates/core/src/search/diversify.rs` (new)**

`diversify_by_session(results, max_per_session=3)`: group results by session_id, take max N per session, then backfill remaining slots.

### 3.3 — Query Expansion

**File: `crates/core/src/search/query_expansion.rs` (new)**

LLM-driven: "You are a query expansion engine..." Output XML: `<reformulations><query>`, `<temporal><query>`, `<entities><entity>`. 3-5 reformulations, extract named entities.

Heuristic entity extraction: quoted strings `"..."`, capitalized words (excluding stop words).

### 3.4 — Smart Search (mem::smart-search)

**File: `crates/core/src/search/smart_search.rs` (new)**

Two modes:
- **Expand mode** (expand_ids): fetch up to 20 observations by ID
- **Compact mode** (query): over-fetch 3x (cap 300), run hybrid search + lesson recall in parallel, filter by agent_id, return `CompactSearchResult`

### 3.5 — Reranker

**File: `crates/core/src/search/reranker.rs` (new)**

Cross-encoder reranking using tract-onnx (already a dep). Load `ms-marco-MiniLM-L-6-v2` ONNX model.
```
rerank(query, results, top_k=20):
  for each candidate: text = "{query} [SEP] {title} {narrative}".truncate(512)
  score = model(text)
  sort by score desc
```
Feature flag: `rerank-cross-encoder`.

### 3.6 — Update Searcher

**File: `crates/core/src/searcher.rs` (modify)**

Replace current fusion with RRF from 3.1. Add diversification from 3.2. Add optional reranking from 3.5.

### Tests for Phase 3
- RRF: verify combined score matches expected values for known rank inputs
- RRF: weight normalization when vector stream empty
- Diversification: max 3 results per session, backfill correct
- Query expansion: mock LLM returns XML -> parsed reformulations + entities
- Smart search: compact mode returns expected result shape
- Reranker: mock model returns scores, results sorted correctly

---

## Phase 4: Knowledge Graph Enhancement & Temporal

### 4.1 — Graph Extraction

**File: `crates/core/src/graph_extraction.rs` (new)**

LLM-driven extraction from observations (1:1 from agentmemory graph-extraction.ts):
- Prompt: "You are a knowledge graph extraction engine..." Output XML: `<entities><entity><type><name><properties>`, `<relationships><rel><type><source><target><weight>`
- Node types: file, function, concept, error, decision, pattern, library, person, project, preference, location, organization, event
- Edge types: uses, imports, modifies, causes, fixes, depends_on, related_to, works_at, prefers, blocked_by, caused_by, optimizes_for, rejected, avoids, located_in, succeeded_by
- Batch extraction: process 10 observations per LLM call (configurable via `GRAPH_EXTRACTION_BATCH_SIZE`)

### 4.2 — Graph Retrieval

**File: `crates/core/src/graph_retrieval.rs` (new)**

`GraphRetrieval` struct with methods:
- `search_by_entities(entities, depth=2, limit)` — traverse from entity nodes
- `expand_from_chunks(top_obs, depth=1, limit=5)` — expand from vector search results
- Returns `GraphRetrievalResult { obs_id, session_id, score, graph_context, path_length }`

### 4.3 — Temporal Graph

**File: `crates/core/src/temporal_graph.rs` (new)**

`TemporalQuery { entity_name, as_of, from, to, include_history }`
`TemporalState { entity, current_edges, historical_edges, timeline }`

Bi-temporal tracking: valid_from/valid_to + commit_time. History of edge changes.

### 4.4 — Wire into existing KG

**File: `crates/core/src/knowledge_graph.rs` (modify)**

Add graph node/edge types from 4.1. Add `extract_from_observations(observations)` that calls graph extraction + stores results. Wire `related()` stub in Palace to use graph retrieval.

### 4.5 — Relations (mem::relate)

**File: `crates/core/src/relations.rs` (new)**

CRUD for `MemoryRelation` with types: supersedes, extends, derives, contradicts, related.
`get_related(memory_id, max_hops, min_confidence)` — graph traversal.

### Tests for Phase 4
- Graph extraction: mock LLM returns XML -> parsed entities + edges
- Graph extraction: all 13 node types and 16 edge types recognized
- Graph retrieval: entity-based traversal returns expected results
- Temporal graph: query as_of date returns correct edge state
- Relations: create, query, multi-hop traversal

---

## Phase 5: Multi-Agent Coordination

### 5.1 — Actions & Dependencies

**File: `crates/core/src/coordination/actions.rs` (new)**

CRUD for `Action` + `ActionEdge`. Dependency graph logic:
- If action has `requires` edge: initial status = Blocked
- `propagate_completion(action_id)`: for each `requires`/`unlocks` edge, check if ALL deps done -> unblock

### 5.2 — Frontier

**File: `crates/core/src/coordination/frontier.rs` (new)**

Scoring formula:
```
score = priority * 10 + min(age_hours * 0.5, 20) + unlock_count * 5 + spawned_by_bonus(3) + active_bonus(15)
```
Algorithm: skip done/cancelled/blocked/leased-by-others, score remaining, sort desc.

### 5.3 — Leases

**File: `crates/core/src/coordination/leases.rs` (new)**

Constants: `DEFAULT_LEASE_TTL = 10min`, `MAX_LEASE_TTL = 60min`

Operations:
- `acquire(action_id, agent_id, ttl)`: validate action, check existing leases, create lease, set action Active
- `release(lease_id, result?)`: if result -> action Done; else -> action Pending, clear assigned_to
- `renew(lease_id, extend)`: new_expiry = max(now, current_expiry) + extend
- `cleanup()`: expire past-due leases, reset orphaned actions to Pending

### 5.4 — Signals

**File: `crates/core/src/coordination/signals.rs` (new)**

`send(from, content, to?, thread_id?, reply_to?, type, expires_in_ms?)` — create Signal.
`read(agent_id, unread_only?, thread_id?, type?)` — filter + auto-mark read.
`threads()` — group by thread_id, count messages, track participants.

### 5.5 — Checkpoints

**File: `crates/core/src/coordination/checkpoints.rs` (new)**

`create(name, description, type, expires_at?, linked_action_ids?)` — link to actions via `gated_by` edges, set actions to Blocked.
`resolve(checkpoint_id, status, resolved_by, result?)` — on Passed: check ALL gates for each linked action -> unblock.
`expire()` — set Expired for pending past expires_at.

### 5.6 — Routines

**File: `crates/core/src/coordination/routines.rs` (new)**

`run(routine_id, overrides?)`: for each step, create Action from template. If step has depends_on -> Blocked. Create `requires` edges. Create RoutineRun record.
`status(run_id)`: poll all actions, map to step status. All done -> completed. Any cancelled -> failed.

### 5.7 — Team & Mesh

**File: `crates/core/src/coordination/team.rs` (new)**

Team-scoped memory sharing. `share(team_id, item)` stores TeamSharedItem. `profile(team_id)` aggregates top concepts/files/patterns.

**File: `crates/core/src/coordination/mesh.rs` (new)**

P2P sync via HTTP. LWW merge: for each item, compare timestamps, keep newer. Shared scopes: memories, actions, semantic, procedural, relations, graph nodes/edges. Security: block private IPs.

### 5.8 — Audit Log

**File: `crates/core/src/audit.rs` (new)**

Record all mutations as AuditEntry: operation type, target IDs, details, quality score. Used by governance and diagnostics.

### Tests for Phase 5
- Actions: CRUD, dependency propagation (action unblocked when deps done)
- Frontier: scoring formula matches expected, blocked/leased filtered
- Leases: acquire/release/renew lifecycle, conflict detection, cleanup
- Signals: send/read/threads with filtering
- Checkpoints: create with gated_by, resolve unblocks actions
- Routines: run creates actions with dependency edges
- Mesh: LWW merge keeps newer items
- Audit: all mutations produce audit entries

---

## Phase 6: Context, Sessions & Smart Features

### 6.1 — Context Injection (mem::context)

**File: `crates/core/src/context.rs` (new)**

Block assembly algorithm:
1. Pinned slots -> rendered
2. Project profile: top 8 concepts, top 5 files, conventions, common errors
3. Lessons: filter by project, score = `(project_match ? 1.5 : 1) * confidence`, take top 10
4. Session summaries: last 10 sessions for project, prefer summaries, fallback to top 5 important observations per session
5. Sort blocks by recency desc
6. Greedy fill until token budget exhausted (token estimate = ceil(len/3))
7. Format as XML: `<agentmemory_context project="..." tokens="...">...</agentmemory_context>`

### 6.2 — Session Summarization

**File: `crates/core/src/summarize.rs` (new)**

LLM-driven: take all observations for a session, generate `SessionSummary` with title, narrative, key_decisions, files_modified, concepts.

### 6.3 — Working Memory / Sliding Window

**File: `crates/core/src/working_memory.rs` (new)**

Core memory with eviction scoring:
```
score = importance/10 * 0.5 + recency_score * 0.3 + access_score * 0.2
recency_score = 1/(1 + recency_days * 0.1)
access_score = log2(access_count + 1) / 10
```

Auto-page: when core exceeds 30% budget, move lowest-scored entries to archival.

### 6.4 — Pinned Memory Slots

**File: `crates/core/src/slots.rs` (new)**

CRUD for MemorySlot: create, get, list, append, replace, delete. Project-scoped + global slots.

### 6.5 — Project Profiles

**File: `crates/core/src/profile.rs` (new)**

Auto-compute from observations: top concepts (frequency), top files, conventions (extracted), common errors, session count. Store as ProjectProfile.

### 6.6 — Timeline

**File: `crates/core/src/timeline.rs` (new)**

Chronological observation listing with relative positioning. Filter by project, anchor point, before/after offsets.

### 6.7 — Patterns, Reflection, Crystallization, Lessons, Insights

**File: `crates/core/src/patterns.rs` (new)** — Recurring pattern extraction from observations.
**File: `crates/core/src/reflect.rs` (new)** — LLM-driven reflection on memories.
**File: `crates/core/src/crystallize.rs` (new)** — Crystallize action outcomes into Crystal records.
**File: `crates/core/src/lessons.rs` (new)** — CRUD for Lesson with confidence tracking.
**File: `crates/core/src/insights.rs` (new)** — Derived insights from concept clusters + memories + lessons.

### 6.8 — Sketches, Facets, Sentinels

**File: `crates/core/src/sketches.rs` (new)** — Draft action proposals with expiry.
**File: `crates/core/src/facets.rs` (new)** — Multi-dimensional tagging on actions/memories/observations.
**File: `crates/core/src/sentinels.rs` (new)** — Event-driven watchers (webhook, timer, threshold, pattern, approval).

### 6.9 — Skill Extraction, Branch-Aware, Replay, File Index

**File: `crates/core/src/skill_extract.rs` (new)** — Extract skills from sessions.
**File: `crates/core/src/branch_aware.rs` (new)** — Git branch-scoped memory operations.
**File: `crates/core/src/replay.rs` (new)** — Session replay by re-injecting observations.
**File: `crates/core/src/file_index.rs` (new)** — File-level indexing and lookup.

### Tests for Phase 6
- Context: block assembly respects token budget, includes all block types
- Summarize: mock LLM returns valid SessionSummary
- Working memory: eviction scoring, auto-page moves low-score to archival
- Slots: CRUD operations
- Profile: top concepts/files computed from observations
- Timeline: chronological ordering with relative positions
- Each smart feature: basic CRUD + edge cases

---

## Phase 7: Vision, Export & Advanced Features

### 7.1 — Vision/Image Embedding

**File: `crates/core/src/vision/mod.rs` (new)**

Image embedding provider trait:
```rust
#[async_trait]
pub trait ImageEmbedder: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn dim(&self) -> usize;
    async fn embed_image(&self, image_bytes: &[u8]) -> anyhow::Result<Vec<f32>>;
}
```

CLIP-based implementation using tract-onnx (already a dep). Load CLIP ViT-B/32 ONNX model.

### 7.2 — Vision Search

**File: `crates/core/src/vision/search.rs` (new)**

`embed_image(image_ref)` — validate image path, embed via CLIP, store in image_embeddings KV.
`search(query_text?, query_image?, top_k=10)` — text or image query, cosine similarity against stored embeddings.

### 7.3 — Image Quota & Reference Counting

**File: `crates/core/src/vision/image_refs.rs` (new)**

Track image references, cleanup orphaned images, enforce disk quota.

### 7.4 — Export/Import

**File: `crates/core/src/export_import.rs` (new)**

Full ExportData serialization (all types from Phase 0). Import with strategies: merge, replace, skip.
Version compatibility: handle all export format versions.

### 7.5 — Obsidian Export

**File: `crates/core/src/obsidian_export.rs` (new)**

Export memories as Obsidian vault: one markdown file per memory, frontmatter with metadata, wikilink cross-references.

### 7.6 — Snapshot (Git-versioned)

**File: `crates/core/src/snapshot.rs` (new)**

`create(message)` — serialize all data, commit to git repo in snapshot dir. `list()` — enumerate snapshots. `diff(from, to)` — show added/removed counts.

### 7.7 — Claude Bridge Sync

**File: `crates/core/src/claude_bridge.rs` (new)**

Sync memories to Claude Code's MEMORY.md file. Line budget (default 200). Bidirectional sync.

### 7.8 — Disk Size Manager

**File: `crates/core/src/disk_size.rs` (new)**

Track disk usage across all KV stores, enforce limits, trigger eviction when over budget.

### 7.9 — Diagnostics & Governance

**File: `crates/core/src/diagnostics.rs` (new)** — Diagnostic checks for all subsystems.
**File: `crates/core/src/governance.rs` (new)** — Filter/audit/query across all data types.

### 7.10 — Dedup, Enrich, Verify

**File: `crates/core/src/dedup.rs` (new)** — Cross-session deduplication.
**File: `crates/core/src/enrich.rs` (new)** — LLM enrichment of existing observations.
**File: `crates/core/src/verify.rs` (new)** — Memory verification against source data.

### Tests for Phase 7
- Vision: embed image returns vector of correct dimension
- Vision search: text query returns ranked image results
- Export/Import: round-trip preserves all data
- Obsidian: markdown output has correct frontmatter
- Snapshot: create + list + diff
- Diagnostics: all checks produce pass/warn/fail

---

## Phase 8: MCP Server Expansion & CLI

### 8.1 — Expand MCP Server to 51 Tools

**File: `crates/core/src/mcp_server.rs` (modify)**

Add 32 new MCP tools to match agentmemory's 51:

**Core (14 total, add 5):**
- `memory_compress_file` — compress a file into observations
- `memory_file_history` — file modification history
- `memory_sessions` — list/filter sessions
- `memory_commits` — git commit lookup
- `memory_commit_lookup` — commit SHA -> session mapping

**Consolidation (add 3):**
- `memory_consolidate` — trigger consolidation
- `memory_consolidation_pipeline` — run 4-tier pipeline
- `memory_retention_score` — score all memories

**Graph (add 3):**
- `memory_graph_query` — entity-based graph query
- `memory_temporal_graph` — temporal graph query
- `memory_graph_extraction` — extract graph from observations

**Multi-Agent (add 11):**
- `memory_action_create` / `memory_action_update` — CRUD actions
- `memory_frontier` — priority-ranked actions
- `memory_lease` — acquire/release lease
- `memory_signal_send` / `memory_signal_read` — inter-agent messaging
- `memory_checkpoint` — condition gates
- `memory_routine_run` — execute routine
- `memory_mesh_sync` — P2P sync
- `memory_team_share` / `memory_team_feed` — team memory

**Smart Features (add 8):**
- `memory_slot_create/get/list/append/replace/delete` — pinned slots
- `memory_sketch_create/promote` — draft proposals
- `memory_crystallize` — outcome crystallization
- `memory_sentinel_create/trigger` — event watchers
- `memory_facet_tag/query` — multi-dimensional tags
- `memory_lesson_save/recall` — learned lessons
- `memory_reflect` — memory reflection
- `memory_insight_list` — derived insights

**Other (add 3):**
- `memory_obsidian_export` — Obsidian vault export
- `memory_snapshot_create` — git-versioned snapshot
- `memory_diagnose` / `memory_heal` — diagnostics

### 8.2 — CLI Expansion

**File: `crates/core/src/cli.rs` (modify)**

New subcommands:
- `mpr consolidate [--project] [--tier]` — run consolidation
- `mpr compress <session-id>` — compress session observations
- `mpr context [--budget]` — inject context
- `mpr sessions [--project]` — list sessions
- `mpr actions` — list/manage actions
- `mpr frontier` — show action frontier
- `mpr signals` — read signals
- `mpr export [--format json|obsidian]` — export data
- `mpr import <file>` — import data
- `mpr snapshot [create|list|diff]` — manage snapshots
- `mpr profile` — show project profile
- `mpr diagnose` — run diagnostics
- `mpr forget [--dry-run]` — auto-forget
- `mpr evolve <memory-id> --content <text>` — evolve memory
- `mpr mesh [sync|status]` — P2P sync
- `mpr vision [embed|search]` — vision operations

### Tests for Phase 8
- Each new MCP tool: schema validation, dispatch, read-only blocking for mutations
- Each new CLI subcommand: arg parsing, handler invocation
- Integration: full observe->compress->consolidate->search->context pipeline via MCP

---

## Cross-Cutting Concerns

### Configuration

**File: `crates/core/src/config.rs` (modify)**

Add new config fields matching agentmemory env vars:
- `llm_provider` (auto-detect from API keys)
- `consolidation_enabled`, `consolidation_decay_days` (30)
- `auto_compress` (default false)
- `graph_extraction_enabled`, `graph_extraction_batch_size` (10)
- `rerank_enabled` (default false)
- `snapshot_enabled`, `snapshot_interval` (3600)
- `token_budget` (2000)
- `max_obs_per_session` (500)
- `agent_id`, `agent_scope` (shared/isolated)
- `team_id`, `team_mode`
- `vision_enabled` (default false)
- `bm25_weight` (0.4), `vector_weight` (0.6), `graph_weight` (0.3)

### Feature Flags

**File: `crates/core/Cargo.toml` (modify)**

Add feature flags:
```toml
llm-openai = ["dep:reqwest"]           # OpenAI-compatible LLM
llm-anthropic = ["dep:reqwest"]        # Anthropic Claude LLM
coordination = []                       # Multi-agent features
vision = ["dep:tract-onnx"]            # CLIP image embeddings
rerank = ["dep:tract-onnx"]            # Cross-encoder reranking
full = ["llm-openai", "llm-anthropic", "coordination", "vision", "rerank", "store-usearch", "embed-fastembed"]
```

### Storage

All new data types use SQLite for persistence (extend existing PalaceDb patterns). The existing `EmbedvecStore` and `UsearchSqliteStore` handle drawer embeddings. New tables:
- `sessions`, `observations`, `memories`, `semantic_memories`, `procedural_memories`
- `actions`, `action_edges`, `leases`, `signals`, `checkpoints`, `routines`, `routine_runs`
- `relations`, `slots`, `profiles`, `sketches`, `crystals`, `lessons`, `insights`
- `facets`, `sentinels`, `audit_log`, `image_refs`, `image_embeddings`
- `mesh_peers`, `team_shared`, `snapshots`

### Error Handling

Extend `MempalaceError` with variants for new subsystems:
```rust
Compression(String), Consolidation(String), Retention(String),
Coordination(String), Vision(String), ExportImport(String),
LlmProvider(String), CircuitBreakerOpen,
```

---

## File Inventory Summary

**New files (~45):**
- `types.rs`, `session.rs`, `audit.rs`
- `llm/mod.rs`, `llm/provider.rs`, `llm/circuit_breaker.rs`, `llm/fallback_chain.rs`, `llm/openai_compat.rs`, `llm/anthropic_provider.rs`, `llm/noop_provider.rs`
- `prompts/mod.rs`, `prompts/compression.rs`, `prompts/consolidation.rs`, `prompts/graph_extraction.rs`, `prompts/vision.rs`, `prompts/xml.rs`
- `compress.rs`, `compress_synthetic.rs`
- `consolidation.rs`, `consolidation_pipeline.rs`, `memory_lifecycle.rs`
- `retention.rs`, `auto_forget.rs`, `evict.rs`
- `search/rrf.rs`, `search/diversify.rs`, `search/query_expansion.rs`, `search/smart_search.rs`, `search/reranker.rs`
- `graph_extraction.rs`, `graph_retrieval.rs`, `temporal_graph.rs`, `relations.rs`
- `coordination/mod.rs`, `coordination/actions.rs`, `coordination/frontier.rs`, `coordination/leases.rs`, `coordination/signals.rs`, `coordination/checkpoints.rs`, `coordination/routines.rs`, `coordination/team.rs`, `coordination/mesh.rs`
- `context.rs`, `summarize.rs`, `working_memory.rs`, `slots.rs`, `profile.rs`, `timeline.rs`
- `patterns.rs`, `reflect.rs`, `crystallize.rs`, `lessons.rs`, `insights.rs`
- `sketches.rs`, `facets.rs`, `sentinels.rs`
- `skill_extract.rs`, `branch_aware.rs`, `replay.rs`, `file_index.rs`
- `vision/mod.rs`, `vision/search.rs`, `vision/image_refs.rs`
- `export_import.rs`, `obsidian_export.rs`, `snapshot.rs`, `claude_bridge.rs`, `disk_size.rs`, `diagnostics.rs`, `governance.rs`
- `dedup.rs`, `enrich.rs`, `verify.rs`

**Modified files (~8):**
- `palace.rs` — extend Palace struct, wire new subsystems
- `palace/builder.rs` — accept LLM provider, new config
- `config.rs` — new config fields
- `searcher.rs` — RRF fusion, diversification, reranking
- `mcp_server.rs` — 32 new tools
- `cli.rs` — new subcommands
- `knowledge_graph.rs` — graph extraction, temporal
- `Cargo.toml` — new feature flags, dependencies

---

## Verification Plan

### Phase 0 Tests
```bash
cargo test --lib types llm:: circuit_breaker fallback_chain session
```

### Phase 1 Tests
```bash
cargo test --lib compress compress_synthetic prompts::xml
```

### Phase 2 Tests
```bash
cargo test --lib consolidation consolidation_pipeline retention auto_forget evict memory_lifecycle
```

### Phase 3 Tests
```bash
cargo test --lib search::rrf search::diversify search::query_expansion search::smart_search
```

### Phase 4 Tests
```bash
cargo test --lib graph_extraction graph_retrieval temporal_graph relations
```

### Phase 5 Tests
```bash
cargo test --lib coordination::
```

### Phase 6 Tests
```bash
cargo test --lib context summarize working_memory slots profile timeline
```

### Phase 7 Tests
```bash
cargo test --lib vision export_import obsidian_export snapshot
```

### Phase 8 Tests
```bash
cargo test --lib mcp_server cli
```

### Full regression
```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo fmt --check
```

### End-to-end verification
1. Start MCP server: `mpr serve`
2. Observe hook fires -> compress -> store
3. Consolidate -> create episodic memory
4. Search via hybrid RRF -> verify results
5. Context injection -> verify token budget
6. Multi-agent: create action, acquire lease, send signal
7. Export -> import on fresh instance -> verify data preserved
