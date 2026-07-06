//! Obsidian export — full parity with upstream `obsidian-export.ts`.
//!
//! Exports memories, observations, lessons, crystals, and sessions as
//! Obsidian-compatible markdown files with rich YAML frontmatter, inline
//! `#tags`, `[[wikilinks]]`, and a Map of Content (MOC) index file.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// What to export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportType {
    Memories,
    Observations,
    Lessons,
    Crystals,
    Sessions,
}

impl std::fmt::Display for ExportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memories => write!(f, "memories"),
            Self::Observations => write!(f, "observations"),
            Self::Lessons => write!(f, "lessons"),
            Self::Crystals => write!(f, "crystals"),
            Self::Sessions => write!(f, "sessions"),
        }
    }
}

impl std::str::FromStr for ExportType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "memories" | "memory" => Ok(Self::Memories),
            "observations" | "observation" => Ok(Self::Observations),
            "lessons" | "lesson" => Ok(Self::Lessons),
            "crystals" | "crystal" => Ok(Self::Crystals),
            "sessions" | "session" => Ok(Self::Sessions),
            "all" => Ok(Self::Memories), // fallback for "all"
            _ => Err(format!(
                "unknown export type: {s} (expected: memories, observations, lessons, crystals, sessions)"
            )),
        }
    }
}

/// Configuration for Obsidian export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObsidianExportConfig {
    pub output_dir: String,
    pub include_frontmatter: bool,
    pub include_tags: bool,
    pub include_links: bool,
    pub tag_prefix: String,
    pub date_format: String,
    /// Generate a Map of Content (MOC) index file linking all exported items.
    pub generate_moc: bool,
    /// When true, inline `#tags` appear in the markdown body (not just YAML).
    pub inline_tags: bool,
    /// Filter: only export these types. Empty = export all requested.
    pub export_types: Vec<ExportType>,
}

impl Default for ObsidianExportConfig {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            include_frontmatter: true,
            include_tags: true,
            include_links: true,
            tag_prefix: "mempalace/".to_string(),
            date_format: default_date_format(),
            generate_moc: true,
            inline_tags: true,
            export_types: vec![],
        }
    }
}

/// Resolve the default output directory from env or fallback.
fn default_output_dir() -> String {
    std::env::var("MEMPALACE_OBSIDIAN_EXPORT_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "./memory-export".to_string())
}

/// Resolve the default date format from env or fallback.
fn default_date_format() -> String {
    std::env::var("MEMPALACE_OBSIDIAN_DATE_FORMAT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "%Y-%m-%d %H:%M".to_string())
}

/// A single entry in the MOC index.
#[derive(Debug, Clone, Serialize)]
pub struct MocEntry {
    pub title: String,
    pub path: String,
    pub entry_type: String,
    pub date: Option<String>,
    pub tags: Vec<String>,
}

/// Result of an export operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObsidianExportResult {
    pub exported_count: usize,
    pub output_dir: String,
    pub files: Vec<String>,
    pub moc_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Null-record safety helpers
// ---------------------------------------------------------------------------

/// Null-record safe-ID normalizer: returns a sanitized string suitable for
/// templates or an empty string if null/missing, never panics.
fn safe_id(v: &Option<String>, label: &str) -> String {
    v.as_deref()
        .unwrap_or_else(|| {
            tracing::warn!("Obsidian export: null or missing {label}, using placeholder");
            ""
        })
        .to_string()
}

/// Null-record safe-timestamp formatter.
fn safe_timestamp(dt: &DateTime<Utc>, format: &str) -> String {
    dt.format(format).to_string()
}

/// Null-record safe-string normalizer.
fn safe_str(v: &str) -> &str {
    v.trim()
}

// ---------------------------------------------------------------------------
// Tag and filename sanitisation
// ---------------------------------------------------------------------------

pub(crate) fn sanitize_tag(tag: &str) -> String {
    tag.chars()
        .map(|c| match c {
            ' ' => '-',
            c if c.is_alphanumeric() || c == '-' || c == '_' => c,
            _ => '_',
        })
        .collect::<String>()
        .to_lowercase()
}

pub(crate) fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    // Collapse consecutive underscores and trim
    let collapsed = s
        .split('_')
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    collapsed.trim().to_string()
}

/// Escape YAML string value if it contains characters that need quoting.
fn yaml_str(value: &str) -> String {
    if value.is_empty()
        || value.starts_with('{')
        || value.starts_with('[')
        || value.starts_with('"')
        || value.starts_with('\'')
        || value.contains(':')
        || value.contains('#')
        || value.starts_with('*')
        || value.starts_with('?')
        || value.starts_with('-')
        || value.contains('\n')
        || value.contains('"')
        || value.contains('\'')
    {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

// ---------------------------------------------------------------------------
// YAML frontmatter builder
// ---------------------------------------------------------------------------

fn build_frontmatter(
    id: &str,
    title: &str,
    entry_type: &str,
    date: &DateTime<Utc>,
    config: &ObsidianExportConfig,
    extra_fields: &[(&str, String)],
    tags: &[String],
    aliases: &[String],
) -> String {
    let mut fm = String::from("---\n");
    fm.push_str(&format!("id: {}\n", id));
    fm.push_str(&format!("title: {}\n", yaml_str(title)));
    fm.push_str(&format!("type: {}\n", entry_type));
    fm.push_str(&format!(
        "date: {}\n",
        safe_timestamp(date, &config.date_format)
    ));

    if config.include_tags && !tags.is_empty() {
        fm.push_str("tags:\n");
        for tag in tags {
            fm.push_str(&format!("  - {}\n", sanitize_tag(tag)));
        }
    }

    if !aliases.is_empty() {
        fm.push_str("aliases:\n");
        for alias in aliases {
            fm.push_str(&format!("  - {}\n", yaml_str(alias)));
        }
    }

    for (key, value) in extra_fields {
        fm.push_str(&format!("{}: {}\n", key, value));
    }

    fm.push_str("---\n\n");
    fm
}

/// Render inline `#tag` line for the markdown body.
fn render_inline_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = tags
        .iter()
        .map(|t| format!("#{}", sanitize_tag(t)))
        .collect();
    format!("{}\n\n", rendered.join(" "))
}

/// Render `[[wikilinks]]` section for related items.
fn render_wikilinks(title: &str, ids: &[(String, &str)]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let mut md = format!("## {}\n\n", title);
    for (id, label) in ids {
        md.push_str(&format!("- [[{}|{}]]\n", id, label));
    }
    md.push('\n');
    md
}

// ---------------------------------------------------------------------------
// Memory export
// ---------------------------------------------------------------------------

/// Export a single memory as Obsidian markdown.
pub fn memory_to_obsidian_md(
    memory: &crate::types::Memory,
    config: &ObsidianExportConfig,
) -> String {
    let mut md = String::new();
    let id = safe_id(&Some(memory.id.clone()), "memory.id");
    let title = safe_str(&memory.title).replace('\n', " ");
    let tags: Vec<String> = memory.concepts.clone();

    // Aliases: wikilinks to related IDs
    let aliases: Vec<String> = memory
        .related_ids
        .iter()
        .chain(memory.source_observation_ids.iter())
        .cloned()
        .collect();

    // Extra frontmatter fields
    let mut extra: Vec<(&str, String)> = vec![
        ("memory_type", memory.memory_type.to_string()),
        ("strength", format!("{:.2}", memory.strength)),
        ("version", memory.version.to_string()),
        ("project", yaml_str(&memory.project)),
    ];
    if let Some(ref parent) = memory.parent_id {
        extra.push(("parent_id", parent.clone()));
    }
    if !memory.files.is_empty() {
        let files_str: Vec<String> = memory.files.iter().map(|f| format!("\"{}\"", f)).collect();
        extra.push(("files", format!("[{}]", files_str.join(", "))));
    }

    if config.include_frontmatter {
        md.push_str(&build_frontmatter(
            &id,
            &title,
            "memory",
            &memory.created_at,
            config,
            &extra,
            &tags,
            &aliases,
        ));
    }

    // Title
    md.push_str(&format!("# {}\n\n", title));

    // Content
    md.push_str(&memory.content);
    md.push_str("\n\n");

    // Inline tags
    if config.inline_tags && !tags.is_empty() {
        md.push_str(&render_inline_tags(&tags));
    }

    // Wikilinks
    if config.include_links {
        let related: Vec<(String, &str)> = memory
            .related_ids
            .iter()
            .map(|id| (id.clone(), "related"))
            .collect();
        let sources: Vec<(String, &str)> = memory
            .source_observation_ids
            .iter()
            .map(|id| (id.clone(), "source"))
            .collect();
        let supersedes: Vec<(String, &str)> = memory
            .supersedes
            .iter()
            .map(|id| (id.clone(), "superseded"))
            .collect();

        if !related.is_empty() {
            md.push_str(&render_wikilinks("Related", &related));
        }
        if !sources.is_empty() {
            md.push_str(&render_wikilinks("Sources", &sources));
        }
        if !supersedes.is_empty() {
            md.push_str(&render_wikilinks("Supersedes", &supersedes));
        }
    }

    md
}

// ---------------------------------------------------------------------------
// Observation export
// ---------------------------------------------------------------------------

/// Export a single observation as Obsidian markdown.
pub fn observation_to_obsidian_md(
    obs: &crate::types::CompressedObservation,
    config: &ObsidianExportConfig,
) -> String {
    let mut md = String::new();
    let id = safe_id(&Some(obs.id.clone()), "obs.id");
    let title = safe_str(&obs.title);
    let tags: Vec<String> = obs.concepts.clone();

    let aliases: Vec<String> = vec![obs.session_id.clone()];

    let mut extra: Vec<(&str, String)> = vec![
        ("observation_type", obs.observation_type.to_string()),
        ("importance", obs.importance.to_string()),
        ("confidence", format!("{:.2}", obs.confidence)),
        ("session", obs.session_id.clone()),
    ];
    if let Some(ref agent) = obs.agent_id {
        extra.push(("agent_id", agent.clone()));
    }

    if config.include_frontmatter {
        md.push_str(&build_frontmatter(
            &id,
            title,
            "observation",
            &obs.timestamp,
            config,
            &extra,
            &tags,
            &aliases,
        ));
    }

    // Title
    md.push_str(&format!("# {}\n\n", title));

    if let Some(subtitle) = &obs.subtitle {
        md.push_str(&format!("> {}\n\n", subtitle));
    }

    // Narrative
    md.push_str(&obs.narrative);
    md.push_str("\n\n");

    // Inline tags
    if config.inline_tags && !tags.is_empty() {
        md.push_str(&render_inline_tags(&tags));
    }

    // Facts
    if !obs.facts.is_empty() {
        md.push_str("## Facts\n\n");
        for fact in &obs.facts {
            md.push_str(&format!("- {}\n", fact));
        }
        md.push('\n');
    }

    // Files
    if !obs.files.is_empty() {
        md.push_str("## Files\n\n");
        for file in &obs.files {
            md.push_str(&format!("- `{}`\n", file));
        }
        md.push('\n');
    }

    md
}

// ---------------------------------------------------------------------------
// Lesson export
// ---------------------------------------------------------------------------

/// Export a single lesson as Obsidian markdown.
pub fn lesson_to_obsidian_md(
    lesson: &crate::types::Lesson,
    config: &ObsidianExportConfig,
) -> String {
    let mut md = String::new();
    let id = &lesson.id;
    let title = lesson.content.chars().take(80).collect::<String>();
    let tags: Vec<String> = lesson.tags.clone();

    let aliases: Vec<String> = lesson.source_ids.iter().cloned().collect();

    let mut extra: Vec<(&str, String)> = vec![
        ("confidence", format!("{:.2}", lesson.confidence)),
        ("retention", format!("{:.2}", lesson.retention)),
        (
            "reinforcement_count",
            lesson.reinforcement_count.to_string(),
        ),
    ];
    if let Some(ref ctx) = lesson.context {
        extra.push(("context", yaml_str(ctx)));
    }
    if let Some(ref proj) = lesson.project {
        extra.push(("project", yaml_str(proj)));
    }
    if let Some(ref source) = lesson.source {
        extra.push(("source", yaml_str(source)));
    }

    if config.include_frontmatter {
        md.push_str(&build_frontmatter(
            id,
            &title,
            "lesson",
            &lesson.created_at,
            config,
            &extra,
            &tags,
            &aliases,
        ));
    }

    // Title
    md.push_str(&format!("# Lesson: {}\n\n", title));

    // Content body
    md.push_str(&lesson.content);
    md.push_str("\n\n");

    // Context
    if let Some(ref ctx) = lesson.context {
        md.push_str(&format!("> Context: {}\n\n", ctx));
    }

    // Inline tags
    if config.inline_tags && !tags.is_empty() {
        md.push_str(&render_inline_tags(&tags));
    }

    // Related source memories
    if config.include_links && !lesson.source_ids.is_empty() {
        let links: Vec<(String, &str)> = lesson
            .source_ids
            .iter()
            .map(|id| (id.clone(), "source memory"))
            .collect();
        md.push_str(&render_wikilinks("Related Memories", &links));
    }

    md
}

// ---------------------------------------------------------------------------
// Crystal export
// ---------------------------------------------------------------------------

/// Export a single crystal as Obsidian markdown.
pub fn crystal_to_obsidian_md(
    crystal: &crate::types::Crystal,
    config: &ObsidianExportConfig,
) -> String {
    let mut md = String::new();
    let id = &crystal.id;

    // Use first line of narrative as title, or fallback to id
    let title = crystal
        .narrative
        .lines()
        .next()
        .unwrap_or(id)
        .chars()
        .take(80)
        .collect::<String>();

    let mut extra: Vec<(&str, String)> = vec![];
    if let Some(ref proj) = crystal.project {
        extra.push(("project", yaml_str(proj)));
    }
    if let Some(ref sid) = crystal.session_id {
        extra.push(("session", sid.clone()));
    }
    if !crystal.key_outcomes.is_empty() {
        let outcomes: Vec<String> = crystal
            .key_outcomes
            .iter()
            .map(|o| format!("\"{}\"", o.replace('"', "\\\"")))
            .collect();
        extra.push(("key_outcomes", format!("[{}]", outcomes.join(", "))));
    }
    if !crystal.files_affected.is_empty() {
        let files: Vec<String> = crystal
            .files_affected
            .iter()
            .map(|f| format!("\"{}\"", f))
            .collect();
        extra.push(("files_affected", format!("[{}]", files.join(", "))));
    }
    if !crystal.lessons.is_empty() {
        let lessons: Vec<String> = crystal
            .lessons
            .iter()
            .map(|l| format!("\"{}\"", l.replace('"', "\\\"")))
            .collect();
        extra.push(("lessons", format!("[{}]", lessons.join(", "))));
    }

    let aliases: Vec<String> = crystal.action_ids.clone();
    let tags = crystal.lessons.clone();

    if config.include_frontmatter {
        md.push_str(&build_frontmatter(
            id,
            &title,
            "crystal",
            &crystal.created_at,
            config,
            &extra,
            &tags,
            &aliases,
        ));
    }

    // Title
    md.push_str(&format!("# {}\n\n", title));

    // Narrative
    md.push_str(&crystal.narrative);
    md.push_str("\n\n");

    // Inline tags
    if config.inline_tags && !tags.is_empty() {
        md.push_str(&render_inline_tags(&tags));
    }

    // Key outcomes
    if !crystal.key_outcomes.is_empty() {
        md.push_str("## Key Outcomes\n\n");
        for outcome in &crystal.key_outcomes {
            md.push_str(&format!("- {}\n", outcome));
        }
        md.push('\n');
    }

    // Files affected
    if !crystal.files_affected.is_empty() {
        md.push_str("## Files Affected\n\n");
        for file in &crystal.files_affected {
            md.push_str(&format!("- `{}`\n", file));
        }
        md.push('\n');
    }

    // Lessons extracted
    if !crystal.lessons.is_empty() {
        md.push_str("## Lessons\n\n");
        for lesson in &crystal.lessons {
            md.push_str(&format!("- {}\n", lesson));
        }
        md.push('\n');
    }

    // Wikilinks to actions
    if config.include_links && !crystal.action_ids.is_empty() {
        let links: Vec<(String, &str)> = crystal
            .action_ids
            .iter()
            .map(|id| (id.clone(), "action"))
            .collect();
        md.push_str(&render_wikilinks("Actions", &links));
    }

    md
}

// ---------------------------------------------------------------------------
// Session export
// ---------------------------------------------------------------------------

/// Export a single session as Obsidian markdown.
pub fn session_to_obsidian_md(
    session: &crate::types::Session,
    config: &ObsidianExportConfig,
) -> String {
    let mut md = String::new();
    let id = &session.id;
    let title = session
        .first_prompt
        .as_deref()
        .unwrap_or("Untitled Session")
        .chars()
        .take(80)
        .collect::<String>();

    let tags: Vec<String> = session.tags.clone();
    let aliases = vec![session.project.clone()];

    let mut extra: Vec<(&str, String)> = vec![
        ("project", yaml_str(&session.project)),
        ("status", session.status.clone()),
        ("observation_count", session.observation_count.to_string()),
    ];
    if let Some(ref model) = session.model {
        extra.push(("model", model.clone()));
    }
    if let Some(ref agent) = session.agent_id {
        extra.push(("agent_id", agent.clone()));
    }
    if let Some(ref ended) = session.ended_at {
        extra.push(("ended_at", safe_timestamp(ended, &config.date_format)));
    }
    if !session.commit_shas.is_empty() {
        let shas: Vec<String> = session
            .commit_shas
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect();
        extra.push(("commits", format!("[{}]", shas.join(", "))));
    }

    if config.include_frontmatter {
        md.push_str(&build_frontmatter(
            id,
            &title,
            "session",
            &session.started_at,
            config,
            &extra,
            &tags,
            &aliases,
        ));
    }

    // Title
    md.push_str(&format!("# Session: {}\n\n", title));

    // Summary
    if let Some(ref summary) = session.summary {
        md.push_str("## Summary\n\n");
        md.push_str(summary);
        md.push_str("\n\n");
    }

    // First prompt
    if let Some(ref prompt) = session.first_prompt {
        md.push_str("## First Prompt\n\n");
        md.push_str("> ");
        md.push_str(prompt);
        md.push_str("\n\n");
    }

    // Inline tags
    if config.inline_tags && !tags.is_empty() {
        md.push_str(&render_inline_tags(&tags));
    }

    // Metadata section
    md.push_str("## Metadata\n\n");
    md.push_str(&format!("- **Project**: {}\n", session.project));
    md.push_str(&format!("- **Status**: {}\n", session.status));
    md.push_str(&format!(
        "- **Observations**: {}\n",
        session.observation_count
    ));
    if let Some(ref model) = session.model {
        md.push_str(&format!("- **Model**: {}\n", model));
    }
    md.push('\n');

    md
}

// ---------------------------------------------------------------------------
// MOC (Map of Content) index
// ---------------------------------------------------------------------------

/// Generate a Map of Content (MOC) index file that links to all exported items.
pub fn generate_moc(entries: &[MocEntry], output_dir: &str) -> Result<String> {
    let path = Path::new(output_dir).join("00 - Index.md");

    let mut md = String::from("---\ntitle: Map of Content\ntype: moc\ndate: ");
    md.push_str(&Utc::now().format(&default_date_format()).to_string());
    md.push_str("\n---\n\n");
    md.push_str("# MemPalace Map of Content\n\n");
    md.push_str("> Auto-generated index of all exported memories, observations, and more.\n\n");

    // Group entries by type
    let mut by_type: HashMap<String, Vec<&MocEntry>> = HashMap::new();
    for entry in entries {
        by_type
            .entry(entry.entry_type.clone())
            .or_default()
            .push(entry);
    }

    // Sort types for stable output
    let mut type_keys: Vec<String> = by_type.keys().cloned().collect();
    type_keys.sort();

    for type_key in &type_keys {
        let items = &by_type[type_key];
        let heading = match type_key.as_str() {
            "memory" => "Memories",
            "observation" => "Observations",
            "lesson" => "Lessons",
            "crystal" => "Crystals",
            "session" => "Sessions",
            other => other,
        };
        md.push_str(&format!("## {} ({})\n\n", heading, items.len()));

        for entry in items {
            let date_str = entry
                .date
                .as_deref()
                .map(|d| format!(" ({})", d))
                .unwrap_or_default();
            let tag_str = if entry.tags.is_empty() {
                String::new()
            } else {
                let t: Vec<String> = entry.tags.iter().map(|t| format!("`{}`", t)).collect();
                format!(" {}", t.join(" "))
            };
            md.push_str(&format!(
                "- [[{}|{}{}]]{}\n",
                sanitize_filename(&entry.title),
                entry.title,
                date_str,
                tag_str,
            ));
        }
        md.push('\n');
    }

    std::fs::write(&path, &md).context("failed to write MOC index")?;
    Ok(path.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Batch export functions
// ---------------------------------------------------------------------------

/// Export all memories to Obsidian format.
pub fn export_memories(
    memories: &[crate::types::Memory],
    config: &ObsidianExportConfig,
) -> Result<ObsidianExportResult> {
    let output_dir = Path::new(&config.output_dir);
    std::fs::create_dir_all(output_dir)?;

    let mut files = Vec::new();
    let mut moc_entries = Vec::new();
    let mut errors = 0usize;

    for memory in memories {
        if memory.id.trim().is_empty() {
            tracing::warn!("Skipping memory with empty id: title={:?}", memory.title);
            continue;
        }
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<String> {
                Ok(memory_to_obsidian_md(memory, config))
            }));
        let md = match result {
            Ok(Ok(md)) => md,
            Ok(Err(e)) => {
                tracing::warn!("Obsidian export failed for memory {}: {e}", memory.id);
                errors += 1;
                continue;
            }
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                tracing::warn!("Obsidian export panicked for memory {}: {msg}", memory.id);
                errors += 1;
                continue;
            }
        };
        let filename = sanitize_filename(&memory.title);
        let path = output_dir.join(format!("{}.md", filename));
        if let Err(e) = std::fs::write(&path, &md) {
            tracing::warn!("Obsidian export write failed for memory {}: {e}", memory.id);
            errors += 1;
            continue;
        }
        moc_entries.push(MocEntry {
            title: memory.title.clone(),
            path: path.to_string_lossy().to_string(),
            entry_type: "memory".to_string(),
            date: Some(safe_timestamp(&memory.created_at, &config.date_format)),
            tags: memory.concepts.clone(),
        });
        files.push(path.to_string_lossy().to_string());
    }

    tracing::debug!(
        "Obsidian export memories: {} OK, {} errors",
        files.len(),
        errors
    );

    let moc_path = if config.generate_moc && !moc_entries.is_empty() {
        Some(generate_moc(&moc_entries, &config.output_dir)?)
    } else {
        None
    };

    Ok(ObsidianExportResult {
        exported_count: files.len(),
        output_dir: config.output_dir.clone(),
        files,
        moc_path,
    })
}

/// Export all observations to Obsidian format.
pub fn export_observations(
    observations: &[crate::types::CompressedObservation],
    config: &ObsidianExportConfig,
) -> Result<ObsidianExportResult> {
    let output_dir = Path::new(&config.output_dir);
    std::fs::create_dir_all(output_dir.join("observations"))?;

    let mut files = Vec::new();
    let mut moc_entries = Vec::new();
    let mut errors = 0usize;

    for obs in observations {
        if obs.id.trim().is_empty() {
            tracing::warn!("Skipping observation with empty id: title={:?}", obs.title);
            continue;
        }
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<String> {
                Ok(observation_to_obsidian_md(obs, config))
            }));
        let md = match result {
            Ok(Ok(md)) => md,
            Ok(Err(e)) => {
                tracing::warn!("Obsidian export failed for observation {}: {e}", obs.id);
                errors += 1;
                continue;
            }
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                tracing::warn!("Obsidian export panicked for observation {}: {msg}", obs.id);
                errors += 1;
                continue;
            }
        };
        let filename = sanitize_filename(&obs.title);
        let path = output_dir
            .join("observations")
            .join(format!("{}.md", filename));
        if let Err(e) = std::fs::write(&path, &md) {
            tracing::warn!(
                "Obsidian export write failed for observation {}: {e}",
                obs.id
            );
            errors += 1;
            continue;
        }
        moc_entries.push(MocEntry {
            title: obs.title.clone(),
            path: path.to_string_lossy().to_string(),
            entry_type: "observation".to_string(),
            date: Some(safe_timestamp(&obs.timestamp, &config.date_format)),
            tags: obs.concepts.clone(),
        });
        files.push(path.to_string_lossy().to_string());
    }

    tracing::debug!(
        "Obsidian export observations: {} OK, {} errors",
        files.len(),
        errors
    );

    let moc_path = if config.generate_moc && !moc_entries.is_empty() {
        Some(generate_moc(&moc_entries, &config.output_dir)?)
    } else {
        None
    };

    Ok(ObsidianExportResult {
        exported_count: files.len(),
        output_dir: config.output_dir.clone(),
        files,
        moc_path,
    })
}

/// Export all lessons to Obsidian format.
pub fn export_lessons(
    lessons: &[crate::types::Lesson],
    config: &ObsidianExportConfig,
) -> Result<ObsidianExportResult> {
    let output_dir = Path::new(&config.output_dir);
    std::fs::create_dir_all(output_dir.join("lessons"))?;

    let mut files = Vec::new();
    let mut moc_entries = Vec::new();
    let mut errors = 0usize;

    for lesson in lessons {
        let md = lesson_to_obsidian_md(lesson, config);
        let label = lesson.content.chars().take(60).collect::<String>();
        let filename = sanitize_filename(&label);
        let path = output_dir.join("lessons").join(format!("{}.md", filename));
        if let Err(e) = std::fs::write(&path, &md) {
            tracing::warn!("Obsidian export write failed for lesson {}: {e}", lesson.id);
            errors += 1;
            continue;
        }
        moc_entries.push(MocEntry {
            title: label,
            path: path.to_string_lossy().to_string(),
            entry_type: "lesson".to_string(),
            date: Some(safe_timestamp(&lesson.created_at, &config.date_format)),
            tags: lesson.tags.clone(),
        });
        files.push(path.to_string_lossy().to_string());
    }

    tracing::debug!(
        "Obsidian export lessons: {} OK, {} errors",
        files.len(),
        errors
    );

    let moc_path = if config.generate_moc && !moc_entries.is_empty() {
        Some(generate_moc(&moc_entries, &config.output_dir)?)
    } else {
        None
    };

    Ok(ObsidianExportResult {
        exported_count: files.len(),
        output_dir: config.output_dir.clone(),
        files,
        moc_path,
    })
}

/// Export all crystals to Obsidian format.
pub fn export_crystals(
    crystals: &[crate::types::Crystal],
    config: &ObsidianExportConfig,
) -> Result<ObsidianExportResult> {
    let output_dir = Path::new(&config.output_dir);
    std::fs::create_dir_all(output_dir.join("crystals"))?;

    let mut files = Vec::new();
    let mut moc_entries = Vec::new();
    let mut errors = 0usize;

    for crystal in crystals {
        let md = crystal_to_obsidian_md(crystal, config);
        let title = crystal
            .narrative
            .lines()
            .next()
            .unwrap_or(&crystal.id)
            .chars()
            .take(60)
            .collect::<String>();
        let filename = sanitize_filename(&title);
        let path = output_dir.join("crystals").join(format!("{}.md", filename));
        if let Err(e) = std::fs::write(&path, &md) {
            tracing::warn!(
                "Obsidian export write failed for crystal {}: {e}",
                crystal.id
            );
            errors += 1;
            continue;
        }
        moc_entries.push(MocEntry {
            title,
            path: path.to_string_lossy().to_string(),
            entry_type: "crystal".to_string(),
            date: Some(safe_timestamp(&crystal.created_at, &config.date_format)),
            tags: crystal.lessons.clone(),
        });
        files.push(path.to_string_lossy().to_string());
    }

    tracing::debug!(
        "Obsidian export crystals: {} OK, {} errors",
        files.len(),
        errors
    );

    let moc_path = if config.generate_moc && !moc_entries.is_empty() {
        Some(generate_moc(&moc_entries, &config.output_dir)?)
    } else {
        None
    };

    Ok(ObsidianExportResult {
        exported_count: files.len(),
        output_dir: config.output_dir.clone(),
        files,
        moc_path,
    })
}

/// Export all sessions to Obsidian format.
pub fn export_sessions(
    sessions: &[crate::types::Session],
    config: &ObsidianExportConfig,
) -> Result<ObsidianExportResult> {
    let output_dir = Path::new(&config.output_dir);
    std::fs::create_dir_all(output_dir.join("sessions"))?;

    let mut files = Vec::new();
    let mut moc_entries = Vec::new();
    let mut errors = 0usize;

    for session in sessions {
        let md = session_to_obsidian_md(session, config);
        let title = session
            .first_prompt
            .as_deref()
            .unwrap_or("Untitled Session")
            .chars()
            .take(60)
            .collect::<String>();
        let filename = sanitize_filename(&title);
        let path = output_dir.join("sessions").join(format!("{}.md", filename));
        if let Err(e) = std::fs::write(&path, &md) {
            tracing::warn!(
                "Obsidian export write failed for session {}: {e}",
                session.id
            );
            errors += 1;
            continue;
        }
        moc_entries.push(MocEntry {
            title,
            path: path.to_string_lossy().to_string(),
            entry_type: "session".to_string(),
            date: Some(safe_timestamp(&session.started_at, &config.date_format)),
            tags: session.tags.clone(),
        });
        files.push(path.to_string_lossy().to_string());
    }

    tracing::debug!(
        "Obsidian export sessions: {} OK, {} errors",
        files.len(),
        errors
    );

    let moc_path = if config.generate_moc && !moc_entries.is_empty() {
        Some(generate_moc(&moc_entries, &config.output_dir)?)
    } else {
        None
    };

    Ok(ObsidianExportResult {
        exported_count: files.len(),
        output_dir: config.output_dir.clone(),
        files,
        moc_path,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CompressedObservation, Crystal, Lesson, Memory, MemoryType, ObservationType,
    };

    fn test_memory(id: &str) -> Memory {
        Memory {
            id: id.into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            memory_type: MemoryType::Semantic,
            title: format!("Test Memory {}", id),
            content: "This is test content.".into(),
            concepts: vec!["rust".into(), "testing".into()],
            files: vec!["src/main.rs".into()],
            session_ids: vec!["s-1".into()],
            strength: 0.8,
            version: 1,
            parent_id: None,
            supersedes: vec![],
            related_ids: vec![],
            source_observation_ids: vec!["o-1".into()],
            is_latest: true,
            forget_after: None,
            image_ref: None,
            agent_id: None,
            project: "test".into(),
        }
    }

    fn test_obs(id: &str) -> CompressedObservation {
        CompressedObservation {
            id: id.into(),
            session_id: "s-1".into(),
            timestamp: chrono::Utc::now(),
            observation_type: ObservationType::FileEdit,
            title: format!("Test Observation {}", id),
            subtitle: Some("A test subtitle".into()),
            facts: vec!["Fact 1".into(), "Fact 2".into()],
            narrative: "This is the observation narrative.".into(),
            concepts: vec!["rust".into()],
            files: vec!["src/lib.rs".into()],
            importance: 5,
            confidence: 0.9,
            image_ref: None,
            image_description: None,
            modality: "text".into(),
            agent_id: None,
        }
    }

    fn test_lesson(id: &str) -> Lesson {
        Lesson {
            id: id.into(),
            content: "Always use Rust for new projects".into(),
            context: Some("project setup".into()),
            retention: 0.9,
            confidence: 0.85,
            project: Some("test".into()),
            source: Some("manual".into()),
            source_ids: vec!["m-1".into()],
            tags: vec!["rust".into(), "best-practice".into()],
            last_reinforced: None,
            reinforcement_count: 2,
            decay_rate: 0.05,
            last_decayed_at: None,
            updated_at: chrono::Utc::now(),
            deleted: false,
            created_at: chrono::Utc::now(),
        }
    }

    fn test_crystal(id: &str) -> Crystal {
        Crystal {
            id: id.into(),
            action_ids: vec!["a-1".into(), "a-2".into()],
            narrative: "Implemented memory export system".into(),
            key_outcomes: vec!["Full Obsidian parity".into(), "MOC index generated".into()],
            files_affected: vec!["obsidian_export.rs".into()],
            lessons: vec!["use MOC for navigation".into()],
            session_id: Some("s-1".into()),
            project: Some("test".into()),
            created_at: chrono::Utc::now(),
        }
    }

    fn test_session(id: &str) -> crate::types::Session {
        crate::types::Session {
            id: id.into(),
            project: "test".into(),
            cwd: "/tmp".into(),
            started_at: chrono::Utc::now(),
            ended_at: None,
            status: "active".to_string(),
            observation_count: 5,
            model: Some("claude-sonnet-4".into()),
            tags: vec!["coding".into()],
            first_prompt: Some("Implement obsidian export".into()),
            summary: Some("Implemented the obsidian export feature".into()),
            commit_shas: vec!["abc123".into()],
            agent_id: None,
        }
    }

    #[test]
    fn test_memory_frontmatter_has_date_field() {
        let memory = test_memory("m-1");
        let config = ObsidianExportConfig::default();
        let md = memory_to_obsidian_md(&memory, &config);

        assert!(md.contains("---"));
        assert!(md.contains("id: m-1"));
        assert!(md.contains("title:"));
        assert!(md.contains("type: memory"));
        assert!(md.contains("date:"));
        assert!(md.contains("tags:"));
        assert!(md.contains("aliases:"));
        assert!(md.contains("strength: 0.80"));
        assert!(md.contains("version: 1"));
    }

    #[test]
    fn test_memory_inline_tags() {
        let memory = test_memory("m-1");
        let config = ObsidianExportConfig::default();
        let md = memory_to_obsidian_md(&memory, &config);

        assert!(md.contains("#rust"));
        assert!(md.contains("#testing"));
    }

    #[test]
    fn test_memory_wikilinks() {
        let mut memory = test_memory("m-1");
        memory.related_ids = vec!["m-2".into(), "m-3".into()];
        let config = ObsidianExportConfig::default();
        let md = memory_to_obsidian_md(&memory, &config);

        assert!(md.contains("[[m-2|related]]"));
        assert!(md.contains("[[m-3|related]]"));
        assert!(md.contains("## Related"));
        assert!(md.contains("[[o-1|source]]"));
        assert!(md.contains("## Sources"));
    }

    #[test]
    fn test_observation_frontmatter() {
        let obs = test_obs("o-1");
        let config = ObsidianExportConfig::default();
        let md = observation_to_obsidian_md(&obs, &config);

        assert!(md.contains("---"));
        assert!(md.contains("id: o-1"));
        assert!(md.contains("type: observation"));
        assert!(md.contains("date:"));
        assert!(md.contains("observation_type:"));
        assert!(md.contains("importance: 5"));
        assert!(md.contains("confidence: 0.90"));
        assert!(md.contains("session: s-1"));
        assert!(md.contains("## Facts"));
        assert!(md.contains("Fact 1"));
    }

    #[test]
    fn test_observation_inline_tags() {
        let obs = test_obs("o-1");
        let config = ObsidianExportConfig::default();
        let md = observation_to_obsidian_md(&obs, &config);

        assert!(md.contains("#rust"));
    }

    #[test]
    fn test_lesson_to_obsidian_md() {
        let lesson = test_lesson("lsn-1");
        let config = ObsidianExportConfig::default();
        let md = lesson_to_obsidian_md(&lesson, &config);

        assert!(md.contains("---"));
        assert!(md.contains("id: lsn-1"));
        assert!(md.contains("type: lesson"));
        assert!(md.contains("confidence: 0.85"));
        assert!(md.contains("# Lesson:"));
        assert!(md.contains("Always use Rust"));
        assert!(md.contains("> Context:"));
        assert!(md.contains("#rust"));
        assert!(md.contains("#best-practice"));
        assert!(md.contains("[[m-1|source memory]]"));
    }

    #[test]
    fn test_crystal_to_obsidian_md() {
        let crystal = test_crystal("crystal-1");
        let config = ObsidianExportConfig::default();
        let md = crystal_to_obsidian_md(&crystal, &config);

        assert!(md.contains("---"));
        assert!(md.contains("id: crystal-1"));
        assert!(md.contains("type: crystal"));
        assert!(md.contains("## Key Outcomes"));
        assert!(md.contains("Full Obsidian parity"));
        assert!(md.contains("## Files Affected"));
        assert!(md.contains("obsidian_export.rs"));
        assert!(md.contains("## Lessons"));
        assert!(md.contains("[[a-1|action]]"));
        assert!(md.contains("[[a-2|action]]"));
    }

    #[test]
    fn test_session_to_obsidian_md() {
        let session = test_session("s-1");
        let config = ObsidianExportConfig::default();
        let md = session_to_obsidian_md(&session, &config);

        assert!(md.contains("---"));
        assert!(md.contains("id: s-1"));
        assert!(md.contains("type: session"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("## First Prompt"));
        assert!(md.contains("Implement obsidian export"));
        assert!(md.contains("## Metadata"));
        assert!(md.contains("**Project**: test"));
        assert!(md.contains("**Model**:"));
        assert!(md.contains("#coding"));
    }

    #[test]
    fn test_moc_generation() {
        let entries = vec![
            MocEntry {
                title: "Memory A".into(),
                path: "Memory A.md".into(),
                entry_type: "memory".into(),
                date: Some("2026-01-01".into()),
                tags: vec!["rust".into()],
            },
            MocEntry {
                title: "Observation B".into(),
                path: "observations/Observation B.md".into(),
                entry_type: "observation".into(),
                date: Some("2026-01-02".into()),
                tags: vec![],
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = generate_moc(&entries, &dir.path().to_string_lossy()).unwrap();
        assert!(Path::new(&path).exists());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Map of Content"));
        assert!(content.contains("Memories (1)"));
        assert!(content.contains("Observations (1)"));
        assert!(content.contains("[[Memory A|Memory A"));
    }

    #[test]
    fn test_export_memories_to_files() {
        let dir = tempfile::tempdir().unwrap();
        let memories = vec![test_memory("m-1"), test_memory("m-2")];
        let config = ObsidianExportConfig {
            output_dir: dir.path().to_string_lossy().to_string(),
            generate_moc: true,
            ..Default::default()
        };

        let result = export_memories(&memories, &config).unwrap();
        assert_eq!(result.exported_count, 2);
        assert!(result.moc_path.is_some());

        for file in &result.files {
            assert!(Path::new(file).exists());
        }

        // Verify MOC was written
        let moc = std::fs::read_to_string(result.moc_path.as_ref().unwrap()).unwrap();
        assert!(moc.contains("Map of Content"));
        assert!(moc.contains("Memories"));
    }

    #[test]
    fn test_export_observations_to_files() {
        let dir = tempfile::tempdir().unwrap();
        let observations = vec![test_obs("o-1")];
        let config = ObsidianExportConfig {
            output_dir: dir.path().to_string_lossy().to_string(),
            generate_moc: true,
            ..Default::default()
        };

        let result = export_observations(&observations, &config).unwrap();
        assert_eq!(result.exported_count, 1);
        assert!(result.moc_path.is_some());

        for file in &result.files {
            assert!(Path::new(file).exists());
        }
    }

    #[test]
    fn test_export_lessons_to_files() {
        let dir = tempfile::tempdir().unwrap();
        let lessons = vec![test_lesson("lsn-1")];
        let config = ObsidianExportConfig {
            output_dir: dir.path().to_string_lossy().to_string(),
            generate_moc: true,
            ..Default::default()
        };

        let result = export_lessons(&lessons, &config).unwrap();
        assert_eq!(result.exported_count, 1);
        assert!(result.moc_path.is_some());

        for file in &result.files {
            assert!(Path::new(file).exists());
        }
    }

    #[test]
    fn test_export_crystals_to_files() {
        let dir = tempfile::tempdir().unwrap();
        let crystals = vec![test_crystal("crystal-1")];
        let config = ObsidianExportConfig {
            output_dir: dir.path().to_string_lossy().to_string(),
            generate_moc: true,
            ..Default::default()
        };

        let result = export_crystals(&crystals, &config).unwrap();
        assert_eq!(result.exported_count, 1);
        assert!(result.moc_path.is_some());

        for file in &result.files {
            assert!(Path::new(file).exists());
        }
    }

    #[test]
    fn test_export_sessions_to_files() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = vec![test_session("s-1")];
        let config = ObsidianExportConfig {
            output_dir: dir.path().to_string_lossy().to_string(),
            generate_moc: true,
            ..Default::default()
        };

        let result = export_sessions(&sessions, &config).unwrap();
        assert_eq!(result.exported_count, 1);
        assert!(result.moc_path.is_some());

        for file in &result.files {
            assert!(Path::new(file).exists());
        }
    }

    #[test]
    fn test_sanitize_tag() {
        assert_eq!(sanitize_tag("rust programming"), "rust-programming");
        assert_eq!(sanitize_tag("Special/Chars!"), "special_chars_");
        assert_eq!(sanitize_tag("UPPERCASE"), "uppercase");
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("test/file:name"), "test_file_name");
        assert_eq!(sanitize_filename("valid-name"), "valid-name");
    }

    #[test]
    fn test_yaml_str() {
        assert_eq!(yaml_str("hello"), "hello");
        assert_eq!(yaml_str("has: colon"), "\"has: colon\"");
        assert_eq!(yaml_str("has \"quotes\""), "\"has \\\"quotes\\\"\"");
        assert_eq!(yaml_str("has#hash"), "\"has#hash\"");
        assert_eq!(yaml_str("{json}"), "\"{json}\"");
        assert_eq!(yaml_str("[list]"), "\"[list]\"");
    }

    #[test]
    fn test_memory_no_frontmatter() {
        let memory = test_memory("m-1");
        let config = ObsidianExportConfig {
            include_frontmatter: false,
            ..Default::default()
        };
        let md = memory_to_obsidian_md(&memory, &config);

        assert!(!md.starts_with("---"));
        assert!(md.contains("# Test Memory m-1"));
    }

    #[test]
    fn test_memory_no_links() {
        let memory = test_memory("m-1");
        let config = ObsidianExportConfig {
            include_links: false,
            ..Default::default()
        };
        let md = memory_to_obsidian_md(&memory, &config);

        assert!(!md.contains("## Related"));
        assert!(!md.contains("## Sources"));
    }

    #[test]
    fn test_memory_no_inline_tags() {
        let memory = test_memory("m-1");
        let config = ObsidianExportConfig {
            inline_tags: false,
            ..Default::default()
        };
        let md = memory_to_obsidian_md(&memory, &config);

        assert!(!md.contains("#rust\n"));
        // Tags still in frontmatter
        assert!(md.contains("tags:"));
    }

    #[test]
    fn test_export_type_from_str() {
        assert_eq!(
            "memories".parse::<ExportType>().unwrap(),
            ExportType::Memories
        );
        assert_eq!(
            "observations".parse::<ExportType>().unwrap(),
            ExportType::Observations
        );
        assert_eq!(
            "lessons".parse::<ExportType>().unwrap(),
            ExportType::Lessons
        );
        assert_eq!(
            "crystals".parse::<ExportType>().unwrap(),
            ExportType::Crystals
        );
        assert_eq!(
            "sessions".parse::<ExportType>().unwrap(),
            ExportType::Sessions
        );
        assert_eq!("all".parse::<ExportType>().unwrap(), ExportType::Memories);
        assert!("invalid".parse::<ExportType>().is_err());
    }

    #[test]
    fn test_export_result_serialization() {
        let result = ObsidianExportResult {
            exported_count: 5,
            output_dir: "./export".to_string(),
            files: vec!["file1.md".to_string()],
            moc_path: Some("./export/00 - Index.md".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ObsidianExportResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.exported_count, 5);
        assert!(parsed.moc_path.is_some());
    }

    #[test]
    fn test_observation_aliases_to_session() {
        let obs = test_obs("o-1");
        let config = ObsidianExportConfig::default();
        let md = observation_to_obsidian_md(&obs, &config);

        assert!(md.contains("aliases:"));
        assert!(md.contains("  - s-1"));
    }

    #[test]
    fn test_lesson_no_inline_tags_when_disabled() {
        let lesson = test_lesson("lsn-1");
        let config = ObsidianExportConfig {
            inline_tags: false,
            ..Default::default()
        };
        let md = lesson_to_obsidian_md(&lesson, &config);

        assert!(!md.contains("#rust\n"));
        assert!(md.contains("tags:")); // still in frontmatter
    }

    #[test]
    fn test_crystal_links_to_actions() {
        let crystal = test_crystal("crystal-1");
        let config = ObsidianExportConfig {
            include_links: true,
            ..Default::default()
        };
        let md = crystal_to_obsidian_md(&crystal, &config);

        assert!(md.contains("## Actions"));
        assert!(md.contains("[[a-1|action]]"));
        assert!(md.contains("[[a-2|action]]"));
    }

    #[test]
    fn test_session_project_alias() {
        let session = test_session("s-1");
        let config = ObsidianExportConfig::default();
        let md = session_to_obsidian_md(&session, &config);

        assert!(md.contains("aliases:"));
        assert!(md.contains("  - test"));
    }

    #[test]
    fn test_observation_importance_and_confidence() {
        let obs = test_obs("o-1");
        let config = ObsidianExportConfig::default();
        let md = observation_to_obsidian_md(&obs, &config);

        assert!(md.contains("importance: 5"));
        assert!(md.contains("confidence: 0.90"));
    }

    #[test]
    fn test_empty_observations_export() {
        let dir = tempfile::tempdir().unwrap();
        let config = ObsidianExportConfig {
            output_dir: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let result = export_observations(&[], &config).unwrap();
        assert_eq!(result.exported_count, 0);
        assert!(result.files.is_empty());
        assert!(result.moc_path.is_none());
    }
}
