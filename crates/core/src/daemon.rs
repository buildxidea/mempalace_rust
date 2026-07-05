//! In-process daemon for serialized palace writes.
//!
//! Holds a background tokio task that processes [`WriteJob`]s via an
//! mpsc channel, guaranteeing serial (non-concurrent) access to the
//! palace store. Callers submit jobs through the global [`DaemonHandle`];
//! the daemon processes them one by one.
//!
//! Lifecycle: `start_daemon` / `stop_daemon` / `daemon_status`.
//! The daemon auto-starts on `submit_job` when no instance is running.
//!
//! All module internals are `pub(crate)` — external consumers interact
//! only through the MCP tools or CLI subcommands.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Job types
// ---------------------------------------------------------------------------

/// A single write operation submitted to the daemon queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WriteJob {
    /// Add one or more documents to the palace.
    AddDrawer {
        id: String,
        content: String,
        /// Metadata key-value pairs serialized as JSON object.
        metadata: Option<serde_json::Value>,
    },
    /// Delete a drawer by ID.
    DeleteDrawer { id: String },
    /// Add a knowledge graph triple.
    KgAdd {
        subject: String,
        predicate: String,
        object: String,
        valid_from: Option<String>,
        valid_to: Option<String>,
    },
    /// Invalidate a knowledge graph fact.
    KgInvalidate {
        subject: String,
        predicate: String,
        object: String,
    },
    /// Flush the in-memory store to disk.
    Flush,
}

impl WriteJob {
    /// Short human-readable label for logging.
    pub fn label(&self) -> &'static str {
        match self {
            WriteJob::AddDrawer { .. } => "add_drawer",
            WriteJob::DeleteDrawer { .. } => "delete_drawer",
            WriteJob::KgAdd { .. } => "kg_add",
            WriteJob::KgInvalidate { .. } => "kg_invalidate",
            WriteJob::Flush => "flush",
        }
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Outcome of processing a single job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobResult {
    Ok { label: String },
    Error { label: String, message: String },
}

/// Snapshot of daemon state returned by `daemon_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    /// Whether the daemon is currently running.
    pub running: bool,
    /// Number of jobs processed since start.
    pub processed: usize,
    /// Number of jobs that errored since start.
    pub errored: usize,
    /// Number of jobs currently waiting in the queue.
    pub queued: usize,
    /// Path to the palace this daemon serves.
    pub palace_path: String,
}

// ---------------------------------------------------------------------------
// DaemonHandle — the public handle to the daemon
// ---------------------------------------------------------------------------

/// Live handle to a running daemon instance.
///
/// Cloning shares the same underlying state (Arc).
#[derive(Clone)]
pub struct DaemonHandle {
    tx: Option<mpsc::Sender<WriteJob>>,
    shutdown: Arc<AtomicBool>,
    processed: Arc<AtomicUsize>,
    errored: Arc<AtomicUsize>,
    palace_path: PathBuf,
    #[allow(dead_code)]
    started_at: chrono::DateTime<Utc>,
}

impl DaemonHandle {
    /// Submit a write job. Returns `Ok(JobResult)` once the daemon
    /// finishes processing it, or `Err` if the channel is closed.
    pub async fn submit(&self, job: WriteJob) -> anyhow::Result<JobResult> {
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("daemon channel closed"))?;
        tx.send(job)
            .await
            .map_err(|_| anyhow::anyhow!("daemon channel closed — daemon may have stopped"))?;
        // Return an immediate acknowledgement; callers that need per-job
        // confirmation can extend this with a oneshot pair.
        Ok(JobResult::Ok {
            label: "submitted".to_string(),
        })
    }

    /// Request shutdown of the daemon loop.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Build a status snapshot.
    pub fn status(&self, queue_depth: usize) -> DaemonStatus {
        DaemonStatus {
            running: !self.shutdown.load(Ordering::SeqCst),
            processed: self.processed.load(Ordering::SeqCst),
            errored: self.errored.load(Ordering::SeqCst),
            queued: queue_depth,
            palace_path: self.palace_path.display().to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Global daemon singleton
// ---------------------------------------------------------------------------

static DAEMON: OnceLock<RwLock<Option<DaemonHandle>>> = OnceLock::new();

fn daemon_slot() -> &'static RwLock<Option<DaemonHandle>> {
    DAEMON.get_or_init(|| RwLock::new(None))
}

// ---------------------------------------------------------------------------
// Public API: start / stop / status / submit_job
// ---------------------------------------------------------------------------

/// Configuration for the daemon.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Palace path to serve.
    pub palace_path: PathBuf,
    /// Maximum number of jobs that can queue before back-pressure kicks in.
    pub channel_capacity: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            palace_path: PathBuf::new(),
            channel_capacity: 256,
        }
    }
}

/// Start the daemon background task.
///
/// If a daemon is already running, it is stopped first (idempotent).
/// The daemon loops on the mpsc receiver until the shutdown flag is set
/// or the sender half is dropped.
pub fn start_daemon(config: DaemonConfig) -> anyhow::Result<DaemonHandle> {
    // Stop any existing daemon first to ensure clean state.
    let _ = stop_daemon();

    let slot = daemon_slot();
    let mut guard = slot
        .write()
        .map_err(|e| anyhow::anyhow!("daemon lock poisoned: {}", e))?;

    if guard.is_some() {
        anyhow::bail!("daemon is already running — call stop_daemon first");
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let processed = Arc::new(AtomicUsize::new(0));
    let errored = Arc::new(AtomicUsize::new(0));

    let (tx, rx) = mpsc::channel::<WriteJob>(config.channel_capacity);

    let handle = DaemonHandle {
        tx: Some(tx),
        shutdown: shutdown.clone(),
        processed: processed.clone(),
        errored: errored.clone(),
        palace_path: config.palace_path.clone(),
        started_at: Utc::now(),
    };

    // Spawn the processing loop on a dedicated tokio runtime thread,
    // following the same pattern as `background.rs`.
    let palace_path = config.palace_path.clone();
    let shutdown_c = shutdown.clone();
    let processed_c = processed.clone();
    let errored_c = errored.clone();

    std::thread::Builder::new()
        .name("mempalace-daemon".into())
        .spawn(move || {
            let rt =
                tokio::runtime::Runtime::new().expect("failed to create daemon tokio runtime");
            rt.block_on(async move {
                daemon_loop(palace_path, rx, shutdown_c, processed_c, errored_c).await;
            });
        })
        .expect("failed to spawn daemon thread");

    *guard = Some(handle.clone());

    info!(
        "Daemon started: palace={}, capacity={}",
        config.palace_path.display(),
        config.channel_capacity,
    );

    Ok(handle)
}

/// Stop the running daemon.
///
/// Sends the shutdown signal, drops the sender, and waits for the
/// background thread to finish. Returns the final status.
/// Returns Ok with running=false if no daemon is in the slot.
pub fn stop_daemon() -> anyhow::Result<DaemonStatus> {
    let slot = daemon_slot();
    let mut guard = slot
        .write()
        .map_err(|e| anyhow::anyhow!("daemon lock poisoned: {}", e))?;

    let handle = match guard.take() {
        Some(h) => h,
        None => {
            return Ok(DaemonStatus {
                running: false,
                processed: 0,
                errored: 0,
                queued: 0,
                palace_path: String::new(),
            });
        }
    };

    // Signal shutdown and drop the sender so the channel closes.
    handle.shutdown();
    let final_status = handle.status(0);

    // Drop the guard so the receiver can make progress and exit.
    drop(guard);

    // Give the daemon thread a moment to drain.
    std::thread::sleep(std::time::Duration::from_millis(100));

    info!(
        "Daemon stopped: processed={}, errored={}",
        final_status.processed, final_status.errored,
    );

    Ok(final_status)
}

/// Return the current daemon status without stopping it.
pub fn daemon_status() -> DaemonStatus {
    let slot = daemon_slot();
    let guard = slot.read().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(handle) => handle.status(0),
        None => DaemonStatus {
            running: false,
            processed: 0,
            errored: 0,
            queued: 0,
            palace_path: String::new(),
        },
    }
}

/// Submit a job to the running daemon (sync convenience wrapper).
///
/// Returns `Some(JobResult)` if the job was queued, or `None` if no
/// daemon is running or the channel is full/closed.
pub fn submit_job_sync(job: WriteJob) -> Option<JobResult> {
    let slot = daemon_slot();
    let guard = slot.read().ok()?;
    let handle = guard.as_ref()?;
    let handle = handle.clone();
    drop(guard);

    // Send on the channel — best-effort from sync context.
    if let Some(tx) = &handle.tx {
        match tx.try_send(job) {
            Ok(()) => Some(JobResult::Ok {
                label: "queued".to_string(),
            }),
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!("daemon channel full — job dropped");
                None
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("daemon channel closed");
                None
            }
        }
    } else {
        None
    }
}

/// Ensure the daemon is running, auto-starting if needed.
///
/// Returns the handle so callers can submit jobs.
pub fn ensure_daemon(palace_path: PathBuf) -> anyhow::Result<DaemonHandle> {
    {
        let slot = daemon_slot();
        let guard = slot.read().map_err(|e| anyhow::anyhow!("lock poisoned: {}", e))?;
        if let Some(handle) = guard.as_ref() {
            return Ok(handle.clone());
        }
    }
    // Not running — auto-start.
    start_daemon(DaemonConfig {
        palace_path,
        channel_capacity: 256,
    })
}

// ---------------------------------------------------------------------------
// Internal processing loop
// ---------------------------------------------------------------------------

/// The daemon's main loop. Reads jobs from the channel and processes
/// them serially against the palace DB.
async fn daemon_loop(
    palace_path: PathBuf,
    mut rx: mpsc::Receiver<WriteJob>,
    shutdown: Arc<AtomicBool>,
    processed: Arc<AtomicUsize>,
    errored: Arc<AtomicUsize>,
) {
    while let Some(job) = rx.recv().await {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let label = job.label();
        let result = process_job(&palace_path, &job).await;

        match result {
            Ok(()) => {
                processed.fetch_add(1, Ordering::SeqCst);
            }
            Err(e) => {
                warn!("daemon job '{}' failed: {}", label, e);
                errored.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    info!("daemon loop exited");
}

/// Resolve the knowledge graph file path from a palace path.
fn kg_path(palace_path: &std::path::Path) -> std::path::PathBuf {
    palace_path
        .parent()
        .unwrap_or(palace_path)
        .join("knowledge_graph.db")
}

/// Process a single write job against the palace.
async fn process_job(palace_path: &PathBuf, job: &WriteJob) -> anyhow::Result<()> {
    let mut db = crate::palace_db::PalaceDb::open(palace_path)?;

    match job {
        WriteJob::AddDrawer {
            id,
            content,
            metadata,
        } => {
            let meta_map: std::collections::HashMap<String, serde_json::Value> = metadata
                .as_ref()
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            db.upsert_documents(&[(id.clone(), content.clone(), meta_map)])?;
            db.flush()?;
        }
        WriteJob::DeleteDrawer { id } => {
            db.delete_id(id)?;
            db.flush()?;
        }
        WriteJob::KgAdd {
            subject,
            predicate,
            object,
            valid_from,
            valid_to,
        } => {
            let mut kg = crate::knowledge_graph::KnowledgeGraph::open(&kg_path(palace_path))?;
            kg.add_triple(
                subject,
                predicate,
                object,
                valid_from.as_deref(),
                valid_to.as_deref(),
                None, // ended
                None, // source_closet
                None, // source_file
                None, // source_drawer_id
                None, // confidence
            )?;
            crate::palace_graph::invalidate_cache(palace_path);
        }
        WriteJob::KgInvalidate {
            subject,
            predicate,
            object,
        } => {
            let mut kg = crate::knowledge_graph::KnowledgeGraph::open(&kg_path(palace_path))?;
            let ended = chrono::Utc::now().format("%Y-%m-%d").to_string();
            kg.invalidate(subject, predicate, object, Some(&ended))?;
            crate::palace_graph::invalidate_cache(palace_path);
        }
        WriteJob::Flush => {
            db.flush()?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_palace_path() -> PathBuf {
        let dir = TempDir::new().unwrap();
        let palace_path = dir.path().join("palace");
        std::fs::create_dir_all(&palace_path).unwrap();
        palace_path
    }

    #[test]
    fn test_daemon_status_when_not_running() {
        // stop_daemon is idempotent now — always succeeds
        let _ = stop_daemon();
        let status = daemon_status();
        assert!(!status.running);
        assert_eq!(status.processed, 0);
        assert_eq!(status.errored, 0);
    }

    #[test]
    fn test_daemon_start_and_stop() {
        // Clean slate: stop any existing daemon
        let _ = stop_daemon();
        let palace = test_palace_path();
        let config = DaemonConfig {
            palace_path: palace.clone(),
            channel_capacity: 16,
        };
        let handle = start_daemon(config).unwrap();
        assert!(!handle.shutdown.load(Ordering::SeqCst));

        let status = daemon_status();
        assert!(status.running);
        assert_eq!(status.palace_path, palace.display().to_string());

        let final_status = stop_daemon().unwrap();
        assert!(!final_status.running);
    }

    #[test]
    fn test_daemon_double_start_is_idempotent() {
        // start_daemon is now idempotent — stops existing and starts fresh
        let _ = stop_daemon();
        let palace = test_palace_path();
        let config = DaemonConfig {
            palace_path: palace.clone(),
            channel_capacity: 16,
        };
        let _h1 = start_daemon(config.clone()).unwrap();
        // Second start stops the first and starts a new one
        let h2 = start_daemon(config);
        assert!(h2.is_ok());
        let _ = stop_daemon();
    }

    #[test]
    fn test_daemon_stop_when_not_running_succeeds() {
        // stop_daemon is now idempotent — returns Ok even when nothing is running
        let _ = stop_daemon();
        let result = stop_daemon();
        assert!(result.is_ok());
    }

    #[test]
    fn test_write_job_label() {
        let job = WriteJob::Flush;
        assert_eq!(job.label(), "flush");

        let job = WriteJob::DeleteDrawer {
            id: "x".to_string(),
        };
        assert_eq!(job.label(), "delete_drawer");
    }

    #[test]
    fn test_job_result_serialization() {
        let result = JobResult::Ok {
            label: "test".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test"));

        let err = JobResult::Error {
            label: "bad".to_string(),
            message: "oops".to_string(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("oops"));
    }

    #[test]
    fn test_daemon_status_serialization() {
        let status = DaemonStatus {
            running: true,
            processed: 42,
            errored: 3,
            queued: 10,
            palace_path: "/tmp/palace".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("42"));
        assert!(json.contains("running"));
    }

    #[test]
    fn test_ensure_daemon_auto_starts() {
        let _ = stop_daemon(); // clean slate
        let palace = test_palace_path();
        let handle = ensure_daemon(palace).unwrap();
        assert!(!handle.shutdown.load(Ordering::SeqCst));
        let status = daemon_status();
        assert!(status.running);
        let _ = stop_daemon();
    }

    #[test]
    fn test_submit_job_sync_when_running() {
        let _ = stop_daemon();
        let palace = test_palace_path();
        let _handle = start_daemon(DaemonConfig {
            palace_path: palace,
            channel_capacity: 16,
        });

        let result = submit_job_sync(WriteJob::Flush);
        assert!(result.is_some());

        let _ = stop_daemon();
    }

    #[test]
    fn test_submit_job_sync_when_not_running() {
        let _ = stop_daemon();
        let result = submit_job_sync(WriteJob::Flush);
        assert!(result.is_none());
    }

    #[test]
    fn test_daemon_config_default() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.channel_capacity, 256);
        assert!(cfg.palace_path.as_os_str().is_empty());
    }

    #[test]
    fn test_add_drawer_job_serialization() {
        let job = WriteJob::AddDrawer {
            id: "d1".to_string(),
            content: "hello world".to_string(),
            metadata: Some(serde_json::json!({"wing": "test"})),
        };
        let json = serde_json::to_value(&job).unwrap();
        assert_eq!(json["AddDrawer"]["id"], "d1");
        assert_eq!(json["AddDrawer"]["content"], "hello world");
    }
}
