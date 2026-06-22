//! Cline adapter.
//!
//! Cline CLI stores MCP server config at `~/.cline/mcp.json` with the
//! canonical `mcpServers` wrapper.

use std::path::PathBuf;

use crate::connect::json_mcp::write_mcp_config;
use crate::connect::types::{ConnectOptions, ConnectResult};
use crate::connect::ConnectAdapter;

pub struct ClineAdapter;

impl ConnectAdapter for ClineAdapter {
    fn name(&self) -> &'static str {
        "cline"
    }

    fn config_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|p| p.join(".cline/mcp.json"))
            .unwrap_or_else(|| PathBuf::from("~/.cline/mcp.json"))
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|p| p.join(".cline").exists())
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
