# Contradiction Detection

::: tip Status
The fact-checking primitives live in `crates/core/src/fact_checker.rs` and the time-aware query helpers in `crates/core/src/knowledge_graph.rs`. They are used by `mempalace_kg_query` and the consolidation pipeline, but there isn't a single end-to-end "check this assertion" MCP tool exposed to AI agents yet — the examples below show how the underlying components are intended to compose.
:::

## What It Does

Checks assertions against entity facts in the knowledge graph. When enabled, it catches contradictions like:

```
Input:  "Soren finished the auth migration"
Output: 🔴 AUTH-MIGRATION: attribution conflict — Maya was assigned, not Soren

Input:  "Kai has been here 2 years"
Output: 🟡 KAI: wrong_tenure — records show 3 years (started 2023-04)

Input:  "The sprint ends Friday"
Output: 🟡 SPRINT: stale_date — current sprint ends Thursday (updated 2 days ago)
```

## How It Works

Facts are checked against the knowledge graph:
- **Attribution conflicts** — the wrong person credited for a task
- **Temporal errors** — wrong dates, tenures, or durations
- **Stale information** — facts that have been superseded

Ages, dates, and tenures are calculated dynamically from the entity's recorded facts — not hardcoded.

## Rust API

The fact checker is a pure function over plain text. Use it as a building block in your own consolidation or reflection passes:

```rust
use mempalace_core::fact_checker::check_text;

let issues = check_text("Soren finished the auth migration in 2024.");
for issue in issues {
    println!("{:?} {:?}: {}", issue.severity, issue.kind, issue.message);
}
```

For more precise queries (by entity, time-bounded), combine it with the knowledge graph directly:

```rust
use mempalace_core::knowledge_graph::KnowledgeGraph;

let kg = KnowledgeGraph::open(std::path::Path::new("~/.mempalace/knowledge.db"))?;
let facts = kg.query_entity("Maya", Some("2026-01-20"), "both")?;
```

The `as_of` parameter is the key — pass the date at which you want to evaluate the assertion, and the KG returns only facts that were valid at that point.

## Status

The current codebase includes the temporal knowledge graph primitives and the fact checker module needed for this direction, but there isn't a single `mempalace_check_assertion` tool wired through MCP yet. If you need it today, compose `mempalace_kg_query` with `fact_checker::check_text` in your own tooling.
