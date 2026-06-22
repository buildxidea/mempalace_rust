//! Qwen Code adapter.
//!
//! Qwen Code stores its settings (mcpServers + hooks) in
//! `~/.qwen/settings.json`.  Schema matches the standard MCP shape.

use std::path::PathBuf;

use crate::connect::json_mcp::write_mcp_config;
use crate::connect::types::{ConnectOptions, ConnectResult};
use crate::connect::ConnectAdapter;

pub struct QwenAdapter;

impl ConnectAdapter for QwenAdapter {
    fn name(&self) -> &'static str {
        "qwen"
    }

    fn config_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|p| p.join(".qwen/settings.json"))
            .unwrap_or_else(|| PathBuf::from("~/.qwen/settings.json"))
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|p| p.join(".qwen").exists())
            .unwrap_or(false)
    }

    fn connect(&self, opts: &ConnectOptions) -> std::result::Result<ConnectResult, anyhow::Error> {
        let path = self.config_path();
        let result = write_mcp_config(&path, "mempalace", "mcpServers", opts.dry_run);
        if opts.dry_run {
            tracing::info!(
                "connect [dry-run] {} → {:?} (wrote={})",
                self.name(),
                path,
                result.wrote
            );
        }
        Ok(result)
    }
}
