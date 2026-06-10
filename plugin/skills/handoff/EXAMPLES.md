# Examples

## Resume current project

```
User: /handoff
```

Agent determines project path from `cwd`, calls `memory_sessions`, picks the most recent session whose `cwd` matches. Checks for unanswered questions, then summarizes.

Expected output:
```
Resuming session: a1b2c3d4 — "Config restructure"
Started: 2026-06-10T14:32:00Z  Status: completed

Unanswered question from last session: "Should we keep the ~/.mempalace fallback?"

Key files touched:
  - crates/core/src/config.rs (Config::config_dir)
  - plugin/skills/recall/SKILL.md

Key decisions:
  - XDG_CONFIG_HOME as primary directory resolution
  - ~/.mempalace fallback kept for backward compatibility

Key errors:
  - None

Next step: Resolve the fallback question and decide on migration strategy.
```

## Resume with cwd override

```
User: /handoff /path/to/specific/project
```

Agent uses the provided path instead of `cwd`. All other logic is the same.

## Session with unanswered question

If the session's last observation is a question ending with `?`, it's surfaced first:
```
There's an unanswered question from the last session:
  "Should we keep the ~/.mempalace fallback?"
```

## Handoff with no matching session

```
User: /handoff
```

If no session matches the current project `cwd`, the agent falls back to the single most recent session overall:
```
No sessions match this project. Falling back to most recent session overall:
  a1b2c3d4 — "Config restructure" (from ~/projects/mempalace_rust)
```

## Handoff with zero observations

If the most recent session has zero observations:
```
Most recent session a1b2c3d4 has no observations. Ready to start fresh.
```

## Gotchas

- **CWD matching uses directory-boundary check**: `session.cwd == projectPath` OR `session.cwd.startsWith(projectPath + sep)` OR vice versa. Simple prefix matching is not used to avoid false positives across repos like `/repo-a` vs `/repo-a-staging`.
- **Session status preference**: Sessions with `status: completed` are preferred over `abandoned`. If only abandoned sessions exist, the most recent one is used.
- **Unanswered question detection**: Looks for the last few observations of type `conversation` with `narrative` ending in `?`. Only one question is surfaced.
- **MCP fallback**: If MCP tools are unavailable, falls back to HTTP: `GET $MEMPALACE_URL/sessions` and `POST $MEMPALACE_URL/smart_search`. Make sure the REST server is running with `mpr serve --http`.
