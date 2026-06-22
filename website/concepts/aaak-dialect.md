# AAAK Dialect

AAAK is a lossy abbreviation dialect designed to pack repeated entities and relationships into fewer tokens at scale. It is readable by any LLM — Claude, GPT, Gemini, Llama, Mistral — without a decoder.

::: warning Experimental
AAAK is a separate compression layer, **not the storage default**. The 96.6% benchmark score comes from raw verbatim mode. AAAK mode currently scores 84.2% R@5 — a 12.4 point regression. We're iterating.
:::

## What AAAK Is

- **Lossy, not lossless.** Uses regex-based abbreviation, not reversible compression.
- **A structured summary format.** Extracts entities, topics, key sentences, emotions, and flags from plain text.
- **Readable by any LLM.** No decoder needed — models read it naturally.
- **Designed for scale.** Saves tokens when the same entities appear hundreds of times.

## What AAAK Is Not

- **Not lossless compression.** The original text cannot be reconstructed.
- **Not efficient at small scale.** Short text already tokenizes efficiently — AAAK overhead costs more than it saves.
- **Not the default storage format.** MemPalace stores raw verbatim text in the drawer store.

## Format

```
Header:   FILE_NUM|PRIMARY_ENTITY|DATE|TITLE
Zettel:   ZID:ENTITIES|topic_keywords|"key_quote"|WEIGHT|EMOTIONS|FLAGS
Tunnel:   T:ZID<->ZID|label
Arc:      ARC:emotion->emotion->emotion
```

### Entity Codes

Three-letter uppercase codes: `ALC=Alice`, `KAI=Kai`, `MAX=Max`.

### Emotion Codes

| Code | Meaning | Code | Meaning |
|------|---------|------|---------|
| `vul` | vulnerability | `joy` | joy |
| `fear` | fear | `trust` | trust |
| `grief` | grief | `wonder` | wonder |
| `rage` | rage | `love` | love |
| `hope` | hope | `despair` | despair |
| `peace` | peace | `humor` | humor |
| `tender` | tenderness | `raw` | raw honesty |
| `doubt` | self-doubt | `relief` | relief |
| `anx` | anxiety | `exhaust` | exhaustion |

### Flags

| Flag | Meaning |
|------|---------|
| `ORIGIN` | Origin moment (birth of something) |
| `CORE` | Core belief or identity pillar |
| `SENSITIVE` | Handle with absolute care |
| `PIVOT` | Emotional turning point |
| `GENESIS` | Led directly to something existing |
| `DECISION` | Explicit decision or choice |
| `TECHNICAL` | Technical architecture detail |

## Example

**Input:**
```
We decided to use GraphQL instead of REST because the frontend team needs
flexible queries. Kai recommended it after researching both options. The team
was excited about the schema-first approach.
```

**AAAK output:**
```
0:KAI|graphql_rest_decided|"decided to use GraphQL instead of REST"|determ+excite|DECISION+TECHNICAL
```

## Usage

### Compress drawers via CLI

```bash
# Preview compression
mpr compress --wing myapp --dry-run

# Compress and store
mpr compress --wing myapp
```

The `--config` flag points at an entity mapping file (see below).

### Entity config format

```json
{
  "entities": {"Alice": "ALC", "Bob": "BOB"},
  "skip_names": ["Gandalf", "Sherlock"]
}
```

### Rust API

```rust
use std::collections::HashMap;
use mempalace_core::dialect::{compress, compress_with_metadata, Dialect};

// Top-level helper
let mut people = HashMap::new();
people.insert("Alice".to_string(), "ALC".to_string());
let text = "We decided to use GraphQL instead of REST because the frontend team needs flexible queries. Kai recommended it after researching both options.";
let compressed = compress(text, &people);

// With metadata (wing/room context)
let mut metadata = serde_json::Map::new();
metadata.insert("wing".into(), serde_json::json!("myapp"));
metadata.insert("room".into(), serde_json::json!("arch"));
let compressed_with_meta = compress_with_metadata(text, &people, &metadata);

// Full Dialect struct
let dialect = Dialect::new();
let stats = dialect.compression_stats(text, &compressed);
println!("ratio: {}", stats.ratio);
```

## When to Use AAAK

AAAK is most useful when:
- You have **many repeated entities** across thousands of sessions
- You need to **compress context** for local models with small windows
- You want **structured summaries** pointing back to verbatim drawers

For most users, raw verbatim mode is the better default.
