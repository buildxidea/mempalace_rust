//! Streaming Markdown exporter for the palace.
//!
//! Exports drawers from SQLite to a browsable folder of Markdown files
//! suitable for Obsidian or static-site generators.
//!
//! ## Layout
//!
//! ```text
//! {output_dir}/
//!   index.md               # wing × room summary table
//!   {wing}/{room}.md        # one file per room
//! ```
//!
//! Each `{room}.md` contains a heading per drawer with timestamp and
//! source_file metadata.
//!
//! ## Security
//!
//! All write paths use the same symlink-hardened helpers as the root
//! `exporter.rs`: directory-level `reject_symlink` check and per-file
//! `O_NOFOLLOW` open.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::Context;

use crate::drawer_store::DrawerStore;

// ---------------------------------------------------------------------------
// Symlink security helpers (mirrored from export/exporter.rs)
// ---------------------------------------------------------------------------

/// Refuse to write into a path that is itself a symlink.
fn reject_symlink(path: &Path, label: &str) -> anyhow::Result<()> {
    if std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        anyhow::bail!(
            "refusing to export: {} is a symbolic link ({}). \
             Remove the symlink or choose a different output path.",
            label,
            path.display()
        );
    }
    Ok(())
}

/// Open a file for writing, refusing to follow a symlink at the target path.
fn safe_open_for_write(path: &Path, append: bool) -> anyhow::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true);
        if append {
            opts.append(true);
        } else {
            opts.truncate(true);
        }
        opts.custom_flags(libc::O_NOFOLLOW);
        match opts.open(path) {
            Ok(f) => Ok(f),
            Err(e) => {
                if e.raw_os_error() == Some(libc::ELOOP) {
                    anyhow::bail!("refusing to write: {} is a symbolic link.", path.display());
                }
                Err(e.into())
            }
        }
    }
    #[cfg(not(unix))]
    {
        if std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            anyhow::bail!("refusing to write: {} is a symbolic link.", path.display());
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true);
        if append {
            opts.append(true);
        } else {
            opts.truncate(true);
        }
        Ok(opts.open(path)?)
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Sanitize a string for use as a filesystem path component.
///
/// Replaces characters that are illegal on Windows/macOS with underscores.
fn safe_path_component(name: &str) -> String {
    let result: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let trimmed = result.trim().to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}

// ---------------------------------------------------------------------------
// Export stats
// ---------------------------------------------------------------------------

#[non_exhaustive]
pub struct ExportStats {
    pub wings: usize,
    pub rooms: usize,
    pub drawers: usize,
}

// ---------------------------------------------------------------------------
// Core export
// ---------------------------------------------------------------------------

/// Stream drawers from SQLite to a browsable Markdown directory.
///
/// Each wing gets a subdirectory; each room within a wing gets a `.md` file.
/// A top-level `index.md` summarizes wings, rooms, and drawer counts.
///
/// When `wing_filter` is `Some(name)`, only drawers belonging to that wing
/// are exported.
pub fn export_markdown(
    palace_path: &Path,
    output_dir: &Path,
    wing_filter: Option<&str>,
) -> anyhow::Result<ExportStats> {
    let store = DrawerStore::open(palace_path)?;

    if store.is_empty() {
        println!("  Palace is empty -- nothing to export.");
        return Ok(ExportStats {
            wings: 0,
            rooms: 0,
            drawers: 0,
        });
    }

    // Reject symlinks at the output root
    reject_symlink(output_dir, "output_dir")?;
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("creating output dir {}", output_dir.display()))?;

    // -----------------------------------------------------------------------
    // Stream drawers from SQLite, grouped by wing → room
    // -----------------------------------------------------------------------
    let export_rows = store.get_all_for_export(wing_filter)?;

    // wing → (room → Vec<DrawerRow>)
    let mut grouped: BTreeMap<String, BTreeMap<String, Vec<DrawerRow>>> = BTreeMap::new();

    for (id, content, wing, room, source_file, filed_at) in export_rows {
        grouped
            .entry(wing)
            .or_default()
            .entry(room)
            .or_default()
            .push(DrawerRow {
                id,
                content,
                source_file,
                filed_at,
            });
    }

    if grouped.is_empty() {
        println!(
            "  No drawers found{}.",
            wing_filter
                .map(|w| format!(" in wing '{w}'"))
                .unwrap_or_default()
        );
        return Ok(ExportStats {
            wings: 0,
            rooms: 0,
            drawers: 0,
        });
    }

    // -----------------------------------------------------------------------
    // Write per-room .md files
    // -----------------------------------------------------------------------
    let mut total_wings = 0usize;
    let mut total_rooms = 0usize;
    let mut total_drawers = 0usize;

    // index.md summary data: wing → (room_count, drawer_count)
    let mut index_data: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for (wing, rooms) in &grouped {
        let safe_wing = safe_path_component(wing);
        let wing_dir = output_dir.join(&safe_wing);

        reject_symlink(&wing_dir, &format!("wing directory '{}'", safe_wing))?;
        std::fs::create_dir_all(&wing_dir)
            .with_context(|| format!("creating wing dir {}", wing_dir.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&wing_dir) {
                let mode = meta.permissions().mode();
                let _ = std::fs::set_permissions(
                    &wing_dir,
                    std::fs::Permissions::from_mode(mode | 0o700),
                );
            }
        }

        let mut room_drawer_count = 0usize;

        for (room, drawers) in rooms {
            let safe_room = safe_path_component(room);
            let room_file = wing_dir.join(format!("{}.md", safe_room));

            reject_symlink(&room_file, &format!("room file '{}'", safe_room))?;

            let mut file = safe_open_for_write(&room_file, false)?;

            // Room heading
            writeln!(file, "# {} / {}\n", wing, room)?;

            for drawer in drawers {
                let source = drawer
                    .source_file
                    .as_deref()
                    .unwrap_or("unknown");
                writeln!(file, "## {}", drawer.id)?;
                writeln!(file)?;
                writeln!(file, "> {}\n", drawer.content)?;
                writeln!(file, "| Field | Value |")?;
                writeln!(file, "|-------|-------|")?;
                writeln!(file, "| Source | {} |", source)?;
                writeln!(file, "| Filed | {} |", drawer.filed_at)?;
                writeln!(file)?;
                writeln!(file, "---\n")?;

                total_drawers += 1;
                room_drawer_count += 1;
            }
        }

        index_data.insert(
            wing.clone(),
            (rooms.len(), room_drawer_count),
        );
        total_wings += 1;
        total_rooms += rooms.len();
    }

    // -----------------------------------------------------------------------
    // Write index.md
    // -----------------------------------------------------------------------
    let index_path = output_dir.join("index.md");
    reject_symlink(&index_path, "index.md")?;

    let today = chrono::Local::now().format("%Y-%m-%d");
    let mut index_lines = vec![
        format!("# Palace Export -- {}\n", today),
        String::new(),
        "| Wing | Rooms | Drawers |".to_string(),
        "|------|-------|---------|".to_string(),
    ];

    for (wing, (room_count, drawer_count)) in &index_data {
        let safe_wing = safe_path_component(wing);
        index_lines.push(format!(
            "| [{}]({}/) | {} | {} |",
            wing, safe_wing, room_count, drawer_count,
        ));
    }

    {
        let mut f = safe_open_for_write(&index_path, false)?;
        f.write_all(index_lines.join("\n").as_bytes())?;
    }

    println!(
        "\n  Exported {} drawers across {} wings, {} rooms",
        total_drawers, total_wings, total_rooms,
    );
    println!("  Output: {}", output_dir.display());

    Ok(ExportStats {
        wings: total_wings,
        rooms: total_rooms,
        drawers: total_drawers,
    })
}

// ---------------------------------------------------------------------------
// Internal row type
// ---------------------------------------------------------------------------

struct DrawerRow {
    id: String,
    content: String,
    source_file: Option<String>,
    filed_at: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawer_store::DrawerStore;
    use std::collections::HashMap;
    use serde_json::Value;

    fn make_store(temp: &tempfile::TempDir) -> DrawerStore {
        DrawerStore::open(temp.path()).unwrap()
    }

    fn insert_test_drawer(
        store: &DrawerStore,
        id: &str,
        content: &str,
        wing: &str,
        room: &str,
        source: Option<&str>,
        filed_at: &str,
    ) {
        let mut meta = HashMap::new();
        if let Some(s) = source {
            meta.insert("source_file".to_string(), Value::String(s.to_string()));
        }
        meta.insert("filed_at".to_string(), Value::String(filed_at.to_string()));
        store
            .insert(id, content, &meta, wing, room, source, None)
            .unwrap();
    }

    #[test]
    fn test_export_creates_index_and_room_files() {
        let temp = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let store = make_store(&temp);

        insert_test_drawer(&store, "d1", "hello", "wing_a", "room_1", Some("src.rs"), "2026-01-01");
        insert_test_drawer(&store, "d2", "world", "wing_a", "room_2", Some("src.rs"), "2026-01-02");
        insert_test_drawer(&store, "d3", "foo", "wing_b", "room_1", Some("lib.rs"), "2026-01-03");

        let stats = export_markdown(temp.path(), output.path(), None).unwrap();
        assert_eq!(stats.wings, 2);
        assert_eq!(stats.rooms, 3);
        assert_eq!(stats.drawers, 3);

        assert!(output.path().join("index.md").exists());
        assert!(output.path().join("wing_a").exists());
        assert!(output.path().join("wing_a/room_1.md").exists());
        assert!(output.path().join("wing_a/room_2.md").exists());
        assert!(output.path().join("wing_b/room_1.md").exists());

        let index = std::fs::read_to_string(output.path().join("index.md")).unwrap();
        assert!(index.contains("wing_a"));
        assert!(index.contains("wing_b"));
        assert!(index.contains("Palace Export"));
    }

    #[test]
    fn test_export_room_file_has_heading_and_drawer() {
        let temp = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let store = make_store(&temp);

        insert_test_drawer(&store, "abc", "test content", "proj", "decisions", Some("a.md"), "2026-06-15");

        export_markdown(temp.path(), output.path(), None).unwrap();

        let room_md = std::fs::read_to_string(output.path().join("proj/decisions.md")).unwrap();
        assert!(room_md.contains("# proj / decisions"));
        assert!(room_md.contains("## abc"));
        assert!(room_md.contains("test content"));
        assert!(room_md.contains("Source"));
        assert!(room_md.contains("a.md"));
        assert!(room_md.contains("Filed"));
        // filed_at uses SQLite datetime('now') so just check the table row exists
        assert!(room_md.contains("| Filed |"));
    }

    #[test]
    fn test_export_with_wing_filter() {
        let temp = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let store = make_store(&temp);

        insert_test_drawer(&store, "d1", "one", "keep", "r1", None, "2026-01-01");
        insert_test_drawer(&store, "d2", "two", "skip", "r1", None, "2026-01-02");

        let stats = export_markdown(temp.path(), output.path(), Some("keep")).unwrap();
        assert_eq!(stats.wings, 1);
        assert_eq!(stats.drawers, 1);

        assert!(output.path().join("keep/r1.md").exists());
        assert!(!output.path().join("skip").exists());
    }

    #[test]
    fn test_export_empty_palace() {
        let temp = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let _store = make_store(&temp);

        let stats = export_markdown(temp.path(), output.path(), None).unwrap();
        assert_eq!(stats.wings, 0);
        assert_eq!(stats.drawers, 0);
    }

    #[test]
    fn test_export_wing_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let store = make_store(&temp);

        insert_test_drawer(&store, "d1", "content", "proj", "r1", None, "2026-01-01");

        let stats = export_markdown(temp.path(), output.path(), Some("nonexistent")).unwrap();
        assert_eq!(stats.wings, 0);
        assert_eq!(stats.drawers, 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_safe_open_for_write_blocks_symlinked_file() {
        let temp = tempfile::tempdir().unwrap();
        let real_target = temp.path().join("real_target.md");
        let link = temp.path().join("link.md");
        std::os::unix::fs::symlink(&real_target, &link).unwrap();

        let err = safe_open_for_write(&link, false)
            .expect_err("should refuse symlinked target");
        let msg = format!("{}", err);
        assert!(msg.contains("symbolic link"), "unexpected error: {msg}");
    }

    #[cfg(unix)]
    #[test]
    fn test_reject_symlink_blocks_symlinked_dir() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("real");
        std::fs::create_dir_all(&target).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = reject_symlink(&link, "output_dir").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("symbolic link"), "unexpected error: {msg}");
        assert!(msg.contains("output_dir"), "unexpected error: {msg}");
    }

    #[test]
    fn test_safe_path_component() {
        assert_eq!(safe_path_component("hello"), "hello");
        assert_eq!(safe_path_component("a/b:c"), "a_b_c");
        assert_eq!(safe_path_component(""), "unknown");
        assert_eq!(safe_path_component("  "), "unknown");
    }

    #[test]
    fn test_export_index_md_content() {
        let temp = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let store = make_store(&temp);

        insert_test_drawer(&store, "d1", "c1", "alpha", "beta", None, "2026-03-01");
        insert_test_drawer(&store, "d2", "c2", "alpha", "beta", None, "2026-03-02");
        insert_test_drawer(&store, "d3", "c3", "gamma", "delta", None, "2026-03-03");

        export_markdown(temp.path(), output.path(), None).unwrap();

        let index = std::fs::read_to_string(output.path().join("index.md")).unwrap();
        // Heading
        assert!(index.contains("# Palace Export"));
        // Table header
        assert!(index.contains("| Wing | Rooms | Drawers |"));
        // alpha has 1 room, 2 drawers
        assert!(index.contains("[alpha](alpha/)"));
        assert!(index.contains("| 1 | 2 |"));
        // gamma has 1 room, 1 drawer
        assert!(index.contains("[gamma](gamma/)"));
        assert!(index.contains("| 1 | 1 |"));
    }
}
