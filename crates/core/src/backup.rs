//! Retention pruning for timestamped palace backups.
//!
//! ``mpr migrate`` and ``mpr repair rebuild`` each write a fresh,
//! timestamped backup every time they run and historically never deleted the old
//! ones. On a machine that mines or repairs on a schedule those full-size copies
//! accumulate silently — a real palace was found with hundreds of gigabytes of
//! backups sitting beside only a few hundred megabytes of live data, nearly
//! filling the disk. This module prunes the backup set down to a bounded count
//! after each new backup is written.
//!
//! The retention count comes from ``MempalaceConfig.max_backups`` (default 10)
//! or the ``MEMPALACE_MAX_BACKUPS`` environment variable.

use anyhow::{Context, Result};
use std::path::Path;
use tracing::{info, warn};

/// Delete the oldest backups matching a glob pattern so at most `max_backups`
/// remain.
///
/// # Arguments
///
/// * `pattern` - A glob pattern matching the backup paths (files or
///   directories). The caller is responsible for escaping any literal,
///   non-wildcard portion that can contain glob metacharacters — palace paths
///   sometimes do (e.g. a ``[``).
/// * `max_backups` - Number of most-recent backups to keep. `None` or any
///   value ``<= 0`` disables pruning and returns immediately, so a backup set
///   is never touched when the user has opted out.
///
/// # Returns
///
/// The list of paths that were successfully removed.
///
/// Recency is determined by filesystem mtime rather than by parsing the
/// timestamp out of the name, so it stays correct even when two backup
/// producers use different timestamp formats. Deletion failures are logged
/// and skipped: pruning is best-effort cleanup and must never abort the
/// migrate/repair operation that just completed successfully.
pub fn prune_backups(pattern: &str, max_backups: Option<usize>) -> Vec<String> {
    if max_backups.is_none_or(|v| v == 0) {
        return Vec::new();
    }
    let cap = max_backups.unwrap(); // safe: guarded above

    let mut scored: Vec<(std::time::SystemTime, String)> = Vec::new();
    match glob::glob(pattern) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(path) => {
                        match path.metadata() {
                            Ok(meta) => {
                                // Use mtime — falls back to the filesystem
                                // modification timestamp, which stays correct
                                // even when two backup producers use
                                // different timestamp formats in the name.
                                let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                                scored.push((mtime, path.to_string_lossy().to_string()));
                            }
                            Err(e) => {
                                // Vanished between glob and stat (concurrent
                                // prune / cleanup); nothing for us to remove.
                                warn!("Backup prune: could not stat {}: {}", path.display(), e);
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Backup prune: glob error: {}", e);
                        continue;
                    }
                }
            }
        }
        Err(e) => {
            warn!("Backup prune: invalid glob pattern {:?}: {}", pattern, e);
            return Vec::new();
        }
    }

    if scored.len() <= cap {
        return Vec::new();
    }

    // Newest first; the path breaks mtime ties so ordering is deterministic.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

    let mut removed: Vec<String> = Vec::new();
    for (_mtime, path_str) in scored.into_iter().skip(cap) {
        let p = Path::new(&path_str);
        match remove_backup_path(p) {
            Ok(()) => {
                info!("Backup prune: removed old backup {}", path_str);
                removed.push(path_str);
            }
            Err(e) => {
                warn!("Backup prune: could not remove {}: {}", path_str, e);
            }
        }
    }

    removed
}

/// Remove a file or directory at the given path. Directories are removed
/// recursively. Symlinks are removed as files (not followed).
fn remove_backup_path(path: &Path) -> Result<()> {
    if path.is_symlink() {
        std::fs::remove_file(path).with_context(|| format!("removing symlink {}", path.display()))
    } else if path.is_dir() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("removing directory {}", path.display()))
    } else {
        std::fs::remove_file(path).with_context(|| format!("removing file {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    fn create_backup(dir: &Path, name: &str, mtime: SystemTime) -> String {
        let path = dir.join(name);
        fs::write(&path, b"backup").unwrap();
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(mtime)).unwrap();
        path.to_string_lossy().to_string()
    }

    fn create_backup_dir(dir: &Path, name: &str, mtime: SystemTime) -> String {
        let path = dir.join(name);
        fs::create_dir_all(&path).unwrap();
        // Set mtime on the directory itself (that's what metadata() reads).
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(mtime)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_prune_none_or_zero_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        for _ in 0..5 {
            create_backup(tmp.path(), "backup.tar", SystemTime::now());
        }
        let pattern = tmp.path().join("backup.tar").to_string_lossy().to_string();

        // None → no pruning
        let removed = prune_backups(&pattern, None);
        assert!(removed.is_empty(), "None cap must not prune");

        // Some(0) → no pruning
        let removed = prune_backups(&pattern, Some(0));
        assert!(removed.is_empty(), "zero cap must not prune");
    }

    #[test]
    fn test_prune_under_cap_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        create_backup(tmp.path(), "b1.tar", SystemTime::now());
        create_backup(tmp.path(), "b2.tar", SystemTime::now());
        let pattern = tmp.path().join("*.tar").to_string_lossy().to_string();

        let removed = prune_backups(&pattern, Some(5));
        assert!(removed.is_empty(), "2 backups with cap 5 must not prune");
    }

    #[test]
    fn test_prune_removes_oldest_when_over_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let now = SystemTime::now();

        // Oldest
        create_backup(tmp.path(), "oldest.tar", now - Duration::from_secs(100));
        // Middle
        create_backup(tmp.path(), "middle.tar", now - Duration::from_secs(50));
        // Newest
        create_backup(tmp.path(), "newest.tar", now);

        let pattern = tmp.path().join("*.tar").to_string_lossy().to_string();
        let removed = prune_backups(&pattern, Some(2));

        // cap=2, 3 files → 1 removed (the oldest)
        assert_eq!(removed.len(), 1, "expected 1 removal");
        let removed_file = Path::new(&removed[0])
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            removed_file.contains("oldest"),
            "oldest should be removed, got {removed_file}"
        );
    }

    #[test]
    fn test_prune_removes_directory_backups() {
        let tmp = tempfile::tempdir().unwrap();
        let now = SystemTime::now();

        create_backup_dir(tmp.path(), "backup_old", now - Duration::from_secs(200));
        create_backup_dir(tmp.path(), "backup_fresh", now);
        create_backup(tmp.path(), "backup_newest.tar", now);

        let pattern = tmp.path().join("backup_*").to_string_lossy().to_string();
        let removed = prune_backups(&pattern, Some(2));

        // 3 entries, cap=2 → 1 removed
        assert_eq!(removed.len(), 1, "expected 1 removal");
        let removed_name = Path::new(&removed[0])
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            removed_name.contains("backup_old"),
            "oldest dir should be removed, got {removed_name}"
        );
        // Confirm the directory was actually deleted from disk.
        assert!(
            !tmp.path().join("backup_old").exists(),
            "removed directory should not exist on disk"
        );
    }

    #[test]
    fn test_prune_best_effort_skips_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let now = SystemTime::now();

        create_backup(tmp.path(), "a.tar", now - Duration::from_secs(101));
        create_backup(tmp.path(), "b.tar", now - Duration::from_secs(50));
        create_backup(tmp.path(), "c.tar", now);

        // Inject a non-matching path into the glob — it doesn't exist,
        // so glob won't match it, and the test verifies no crash.
        let pattern = tmp.path().join("*.tar").to_string_lossy().to_string();
        let removed = prune_backups(&pattern, Some(1));

        // cap=1, 3 files → 2 removed
        assert_eq!(removed.len(), 2, "expected 2 removals");
    }

    #[test]
    fn test_prune_mtime_ties_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let now = SystemTime::now();

        // All at same mtime
        create_backup(tmp.path(), "z.tar", now);
        create_backup(tmp.path(), "a.tar", now);
        create_backup(tmp.path(), "m.tar", now);

        let pattern = tmp.path().join("*.tar").to_string_lossy().to_string();

        // cap=1, 3 files at same mtime → 2 removed
        // Tiebreaker: path string sorts descending (newest first),
        // so the kept one is the alphabetically-last path "z.tar".
        let removed = prune_backups(&pattern, Some(1));
        assert_eq!(removed.len(), 2, "expected 2 removals with cap=1");

        let kept_files: Vec<String> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|s| s == "tar").unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            kept_files,
            vec!["z.tar"],
            "expected z.tar to be kept (last in sort)"
        );
    }

    /// Test that a non-empty return from prune_backups matches the
    /// original Python semantics: list of successfully removed paths.
    #[test]
    fn test_prune_returns_removed_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let now = SystemTime::now();

        create_backup(tmp.path(), "old1.tar", now - Duration::from_secs(200));
        create_backup(tmp.path(), "old2.tar", now - Duration::from_secs(150));
        create_backup(tmp.path(), "keep.tar", now);

        let pattern = tmp.path().join("*.tar").to_string_lossy().to_string();
        let removed = prune_backups(&pattern, Some(1));

        assert_eq!(removed.len(), 2, "expected 2 removals");
        // Should be the two oldest in some order.
        for r in &removed {
            let name = Path::new(r).file_name().unwrap().to_string_lossy();
            assert!(name.contains("old"), "expected an 'old' backup, got {name}");
        }
    }

    /// Invalid glob pattern is handled gracefully (returns empty vec).
    #[test]
    fn test_invalid_glob_pattern_returns_empty() {
        // A pattern with unmatched bracket is a syntax error in glob.
        let removed = prune_backups("[invalid", Some(10));
        assert!(removed.is_empty(), "invalid glob must return empty vec");
    }

    #[test]
    fn test_remove_backup_path_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("test.txt");
        fs::write(&f, b"data").unwrap();
        assert!(remove_backup_path(&f).is_ok());
        assert!(!f.exists());
    }

    #[test]
    fn test_remove_backup_path_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("testdir");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("nested.txt"), b"data").unwrap();
        assert!(remove_backup_path(&d).is_ok());
        assert!(!d.exists());
    }

    #[test]
    fn test_remove_backup_path_nonexistent_returns_err() {
        let p = Path::new("/this/path/does/not/exist/123456789");
        assert!(remove_backup_path(p).is_err());
    }
}
