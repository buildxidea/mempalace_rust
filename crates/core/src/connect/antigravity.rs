//! Antigravity adapter.
//!
//! Antigravity stores MCP config in `mcp_config.json` under its app
//! support directory.  Path varies by platform:
//!   macOS:  ~/Library/Application Support/Antigravity/User/mcp_config.json
//!   Linux:  ~/.config/Antigravity/User/mcp_config.json

use std::path::PathBuf;

use crate::connect::json_mcp::write_mcp_config;
use crate::connect::types::{ConnectOptions, ConnectResult};
use crate::connect::ConnectAdapter;

pub struct AntigravityAdapter;

impl ConnectAdapter for AntigravityAdapter {
    fn name(&self) -> &'static str {
        "antigravity"
    }

    fn config_path(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir()
                .map(|p| p.join("Library/Application Support/Antigravity/User/mcp_config.json"))
                .unwrap_or_else(|| {
                    PathBuf::from("~/Library/Application Support/Antigravity/User/mcp_config.json")
                })
        }
        #[cfg(not(target_os = "macos"))]
        {
            dirs::home_dir()
                .map(|p| p.join(".config/Antigravity/User/mcp_config.json"))
                .unwrap_or_else(|| PathBuf::from("~/.config/Antigravity/User/mcp_config.json"))
        }
    }

    fn detect(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir()
                .map(|p| {
                    p.join("Library/Application Support/Antigravity/User")
                        .exists()
                })
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "macos"))]
        {
            dirs::home_dir()
                .map(|p| p.join(".config/Antigravity/User").exists())
                .unwrap_or(false)
        }
    }

    fn connect(&self, opts: &ConnectOptions) -> std::result::Result<ConnectResult, anyhow::Error> {
        // Antigravity always detected but requires no config file
        if opts.dry_run {
            tracing::info!(
                "connect [dry-run] {} (always detected, no config required)",
                self.name()
            );
            return Ok(ConnectResult {
                adapter: self.name().to_string(),
                config_path: self.config_path(),
                wrote: false,
                note: Some("no config required".to_string()),
            });
        }

        let path = self.config_path();
        let result = write_mcp_config(&path, "mempalace", "mcpServers", opts.dry_run);
        Ok(result)
    }
}
