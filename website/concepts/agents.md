# Specialist Agents

MemPalace supports **agent diaries** and **lessons** through MCP tools, plus dedicated agent wings for partitioning memory. The practical model is simple: give an agent a stable name, and write/read diary entries and lessons under that agent's identity.

::: tip Current Scope
Agent diaries are stable. Lessons (`mempalace_lesson_save` / `mempalace_lesson_recall`) and reflections (`mempalace_reflect`) build on top of the same wing model. There is no separate agent registry on disk — agents are identified by the `agent_name` parameter on diary and lesson tools.
:::

## What Agents Get

Each named agent:

- **Has a wing** — `wing_<agent_name>` automatically created the first time you write a diary or lesson entry
- **Keeps a diary** — observations, findings, and recurring patterns, persisted in AAAK format
- **Saves lessons** — durable learnings that survive across sessions and are surfaced during wake-up
- **Can read recent history** — useful for patterns, continuity, and follow-up work

## Agent Diary

The diary is a lightweight memory stream for one named agent: observations, findings, decisions, and recurring patterns.

### Writing Entries

```text
MCP tool: mempalace_diary_write
  arguments: {
    "agent_name": "reviewer",
    "entry": "PR#42|auth.bypass.found|missing.middleware.check|pattern:3rd.time.this.quarter",
    "topic": "auth-bypass"
  }
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `agent_name` | **Yes** | Agent name — defaults to `wing_{agent_name}` |
| `entry` | **Yes** | Diary entry (AAAK format recommended) |
| `topic` | No | Topic tag (default: `"general"`) |
| `wing` | No | Override the target wing |

### Reading History

```text
MCP tool: mempalace_diary_read
  arguments: { "agent_name": "reviewer", "last_n": 10 }
  → returns last 10 entries (in AAAK)
```

### MCP Tools

| Tool | Description |
|------|-------------|
| `mempalace_diary_write` | Write an AAAK diary entry |
| `mempalace_diary_read` | Read recent diary entries |

## Lessons & Reflections

Lessons are longer-lived than diary entries — they're surfaced on wake-up and intentionally persistent.

| Tool | Description |
|------|-------------|
| `mempalace_lesson_save` | Save a reusable lesson the AI has learned |
| `mempalace_lesson_recall` | Recall lessons by topic or recent N |
| `mempalace_reflect` | Run a reflection pass over recent observations |

```text
MCP tool: mempalace_lesson_save
  arguments: {
    "agent_name": "reviewer",
    "lesson": "When the deploy script exits 137, it was OOM-killed. Bump the memory limit and retry."
  }
```

## How It Works

Each named agent maps to its own wing in the palace:

- `wing_reviewer` — the reviewer's diary, findings, patterns, lessons
- `wing_architect` — the architect's decisions, tradeoffs, lessons
- `wing_ops` — the ops agent's incidents, deploys, lessons

All diary entries go into a `diary` room within the wing; lessons go into a `lessons` room. Both are tagged with topic, timestamp, and agent name.

## Specialization

Separate diary streams let you keep different working contexts apart. A reviewer can keep bug patterns, an architect can keep decisions, and an ops agent can keep incident notes without mixing them into one shared log.

::: tip
If you use multiple specialist prompts or toolchains, keep the agent names stable so each one writes back to the same diary wing over time. Different `agent_name` values produce different wings.
:::
