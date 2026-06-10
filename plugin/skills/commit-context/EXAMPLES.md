# Examples

## Trace a file to its agent session

```
User: /commit-context crates/core/src/config.rs
```

Agent runs `git log -n 1 -- crates/core/src/config.rs`, gets SHA `abc123def456`. Calls `memory_commit_lookup` with `sha: "abc123def456"`.

Expected output:
```
Commit: abc123def456 (abc123d)
  Branch: main
  Author: Tran Quang Dang
  Message: feat: add XDG config directory resolution with platform fallback

Linked session: a1b2c3d4
  Project: mempalace_rust
  Started: 2026-06-10T14:32:00Z
  Ended: 2026-06-10T15:15:00Z
  Observations: 12

Top observations from that session:
  decision - "Adopted XDG_CONFIG_HOME over platform ProjectDirs"
    > config_dir() now checks XDG_CONFIG_HOME first
```

## Trace a function to its commit

```
User: /commit-context normalize_wing_name in config.rs
```

Agent runs `git log -L :normalize_wing_name:crates/core/src/config.rs`, gets the SHA. Same output format.

## Commit with no linked session

If git shows a SHA but `memory_commit_lookup` returns `commit: null`:

```
Commit: def789ghi012 (def789g)
  Branch: main
  Author: Bot
  Message: chore: update deps

No linked session found. This commit predates session linking.
Git changes:
  - Updated Cargo.toml dependency versions
```

## MCP tool unavailable

If `memory_commit_lookup` is unavailable, falls back to HTTP: `GET $MEMPALACE_URL/commits/{sha}` with `Authorization: Bearer $MEMPALACE_SECRET`.

## Gotchas

- **Blame strategy differs by input type**: File path gets `git log -n 1 -- <file>`. Function name gets `git log -L :<function>:<file>`. Line range gets `git blame -L <start>,<end> <file>`.
- **Commit must be linked via hooks**: A commit only has a linked session if the agent hooks were active when it was made. Old commits won't have links.
- **Fabrication risk**: If `memory_commit_lookup` returns empty, the skill explicitly says the commit predates session linking. Do not invent a session.
- **SHA must be full**: The MCP tool expects a full SHA. The skill extracts it from `git log` or `git blame` output.
