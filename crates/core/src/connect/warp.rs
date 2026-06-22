//! Warp adapter.
//!
//! Warp stores MCP server config at `~/.warp/.mcp.json` with the canonical
//! `mcpServers` wrapper — identical schema to Claude Code.

use std::path::PathBuf;

use crate::connect::json_mcp::write_mcp_config;
use crate::connect::types::{ConnectOptions, ConnectResult};
use crate::connect::ConnectAdapter;

pub struct WarpAdapter;

impl ConnectAdapter for WarpAdapter {
    fn name(&self) -> &'static str {
        "warp"
    }

    fn config_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|p| p.join(".warp/.mcp.json"))
            .unwrap_or_else(|| PathBuf::from("~/.warp/.mcp.json"))
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|p| p.join(".warp").exists())
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
