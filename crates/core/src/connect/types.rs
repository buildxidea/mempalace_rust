//! Types for the agent adapter system.

use std::path::PathBuf;

/// Options passed to [`ConnectAdapter::connect`](super::ConnectAdapter::connect).
#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    /// When true, describe what would change without writing anything.
    pub dry_run: bool,
    /// When true, overwrite an existing entry even if already wired.
    pub force: bool,
}

/// Result of a connect operation.
#[derive(Debug, Clone)]
pub struct ConnectResult {
    /// Name of the adapter that ran.
    pub adapter: String,
    /// Path to the config file that was (or would be) written.
    pub config_path: PathBuf,
    /// Whether a file was actually written.
    pub wrote: bool,
    /// Human-readable note (e.g. "stub" or "already wired").
    pub note: Option<String>,
}
