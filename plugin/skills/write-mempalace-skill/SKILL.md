---
name: write-mempalace-skill
description: How to author new mempalace skills for Claude Code
---

## Skill anatomy

A mempalace skill is a directory under `plugin/skills/<name>/` containing:

- `SKILL.md` -- the skill definition (required, under 100 lines for action skills)
- `EXAMPLES.md` -- usage examples with expected output (action skills only)
- `_shared/TROUBLESHOOTING.md` -- common failures and fixes (action skills only)

Reference skills have only `SKILL.md`.

## SKILL.md format (action skill)

Required structure with YAML frontmatter, quick start, why, workflow, and anti-patterns sections. All code fences must have a language identifier (markdownlint MD040). See any existing action skill (e.g., `recall/`, `forget/`) for the canonical example.

Top-level sections:

```text
---
name: my-skill
description: One-line description of what the skill does
argument-hint: "[args]"
user-invocable: true
---

Context prompt for the agent: $ARGUMENTS

## Quick start

Brief 1-2 line example.

## Why

One sentence explaining the purpose.

## Workflow

1. Step one
2. Step two

## Anti-patterns

**WRONG** -- bad approach:

// Bad code example

**RIGHT** -- good approach:

// Good code example

> See TROUBLESHOOTING.md and EXAMPLES.md for more.
```

## SKILL.md format (reference skill)

Reference skills are simpler -- only SKILL.md with frontmatter and content:

```text
---
name: mempalace-xxx
description: Brief description
---

## Content

Reference data, tables, links.
```

## Naming convention

- Action skills: short names (`recall`, `forget`, `handoff`)
- Reference skills: `mempalace-` prefix (`mempalace-config`, `mempalace-mcp-tools`)

## Rules

- All code fences must have a language identifier (markdownlint MD040).
- Action skills must be under 100 lines in `SKILL.md`.
- Frontmatter is YAML with `name`, `description`, `argument-hint`, `user-invocable`.
- Reference skills may omit `argument-hint` and `user-invocable`.
- Never use the `agentmemory-` prefix -- use `mempalace-`.
- The `$ARGUMENTS` placeholder in the body is replaced with the user's input at invocation time.
