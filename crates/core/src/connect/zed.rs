//! Zed adapter.
//!
//! Zed stores its settings (including MCP servers) under `"context_servers"`
//! in `settings.json` — NOT `"mcpServers"`.  Config lives at
//! `~/.config/zed/settings.json` (Zed uses the XDG path even on macOS).

use std::path::PathBuf;

use crate::connect::json_mcp::write_mcp_config;
use crate::connect::types::{ConnectOptions, ConnectResult};
use crate::connect::ConnectAdapter;

pub struct ZedAdapter;

impl ConnectAdapter for ZedAdapter {
    fn name(&self) -> &'static str {
        "zed"
    }

    fn config_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|p| p.join(".config/zed/settings.json"))
            .unwrap_or_else(|| PathBuf::from("~/.config/zed/settings.json"))
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|p| p.join(".config/zed").exists())
            .unwrap_or(false)
    }

    fn connect(&self, opts: &ConnectOptions) -> std::result::Result<ConnectResult, anyhow::Error> {
        let path = self.config_path();
        // Zed uses "context_servers" not "mcpServers"
        let result = write_mcp_config(&path, "mempalace", "context_servers", opts.dry_run);
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
